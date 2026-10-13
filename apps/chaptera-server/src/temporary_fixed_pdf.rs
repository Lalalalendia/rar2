//! Temporary guest fixed-output adapter. This is not a PUB renderer.
//!
//! Replays are performed by product_replay_worker before this function is called,
//! under filesystem/network seccomp deny. We combine the *current* EditorSession
//! with the existing Viewer resolved resources and the source-neutral pub-pdf
//! renderer. The original PUB remains immutable.
use std::{collections::BTreeSet, str::FromStr};

use pub_editor::{EditOperation, EditorSession, LengthEmu, NodeId, Sha256Digest};
use pub_layout::{
    BoundedLayoutEnvironment, BoundedShapedFlowRuntime, BoundedShapedText, BoundedShapingRuntime,
    font_fingerprint_sha256, project_bounded, resolve_bounded_shaped_flow,
};
use pub_output::{
    ExplicitFontResource, FixedOutputFontProfile, FontIdentity, OutputFontRequest,
    PreferredEmbedding, plan_output_fonts, read_opentype_embedding_flags,
};
use pub_pdf::{
    FixedFontResource, FixedImagePlacement, FixedImageResource, FixedNodePaint, FixedPdfResources,
    FixedStroke, FixedTextRun, PdfRenderDisposition, PdfRenderError, PdfTargetProfile, render_bounded_pdf,
    render_bounded_pdf_a4_card_9up,
};
use serde_json::{Value, json};

const MAX_PRIVATE_PDF_BYTES: usize = 16 * 1024 * 1024;
const LOSS_REPORT_SCHEMA: &str = "chaptera.temporary-edited-fixed-pdf-loss.v1";

pub(crate) struct CurrentFixedPdf {
    pub bytes: Vec<u8>,
    pub loss_report: Value,
}

/// The only source-derived authority is the already-replayed EditorSession.
/// In particular, never call the PUB-to-PDF source CLI on original source bytes.
pub(crate) fn render_current_fixed_pdf(
    original: &[u8],
    expected_source_hash: &str,
    session: &EditorSession,
) -> Result<CurrentFixedPdf, &'static str> {
    render_current_fixed_pdf_with_sheet(original, expected_source_hash, session, false)
}

/// The one existing guest EditorSession current-revision PDF pipeline, with
/// nine vector copies on a physical A4 sheet. No new authoring or PUB parsing.
pub(crate) fn render_current_fixed_pdf_a4_card_9up(
    original: &[u8],
    expected_source_hash: &str,
    session: &EditorSession,
) -> Result<CurrentFixedPdf, &'static str> {
    render_current_fixed_pdf_with_sheet(original, expected_source_hash, session, true)
}

fn render_current_fixed_pdf_with_sheet(
    original: &[u8],
    expected_source_hash: &str,
    session: &EditorSession,
    nine_up: bool,
) -> Result<CurrentFixedPdf, &'static str> {
    let source_hash =
        Sha256Digest::from_str(expected_source_hash).map_err(|_| "invalid_source_hash")?;
    if session.source_hash() != source_hash {
        return Err("edited_pdf_source_identity_mismatch");
    }
    let mut bundle =
        pub_viewer::open_pub_bundle(original, pub_viewer::viewer_geometry_environment_v0_1())
            .map_err(|_| "edited_pdf_source_unsupported")?;
    if bundle.geometry.document.source.source_hash != source_hash
        || bundle.resolved_graph.source.source_hash != source_hash
    {
        return Err("edited_pdf_source_identity_mismatch");
    }

    // The guest contract only admits MoveNode and (when enabled) StoryRange.
    // Unknown operations must never be silently discarded by a fixed exporter.
    let mut moved = BTreeSet::<NodeId>::new();
    let mut edited_stories = 0usize;
    for operation in session.operations() {
        match operation {
            EditOperation::MoveNode { node_id, .. } => {
                moved.insert(*node_id);
            }
            EditOperation::ReplaceStoryRange { .. } => edited_stories += 1,
            _ => return Err("edited_pdf_operation_unsupported"),
        }
    }

    // Preserve the Viewer page-role, paints and exact image-resource authority.
    // Only direct page-local MoveNode geometry may be overridden from authoring.
    for scene_node in &mut bundle.geometry.scene.nodes {
        if !moved.contains(&scene_node.origin) {
            continue;
        }
        let authored = session
            .graph()
            .nodes
            .get(&scene_node.origin)
            .ok_or("edited_pdf_node_missing")?;
        if authored.header.parent_id != scene_node.parent_origin
            || session
                .can_move_node_to(
                    scene_node.origin,
                    authored.header.bounds.x,
                    authored.header.bounds.y,
                )
                .is_err()
        {
            return Err("edited_pdf_node_not_page_local");
        }
        scene_node.bounds = authored.header.bounds;
    }
    for node_id in &moved {
        let authored = session
            .graph()
            .nodes
            .get(node_id)
            .ok_or("edited_pdf_node_missing")?;
        if !bundle.geometry.scene.nodes.iter().any(|node| {
            node.origin == *node_id
                && node.parent_origin == authored.header.parent_id
                && node.bounds == authored.header.bounds
        }) {
            return Err("edited_pdf_changed_geometry_not_representable");
        }
    }

    let pages = bundle
        .geometry
        .document
        .pages
        .iter()
        .map(|page| page.id)
        .collect::<Vec<_>>();
    if pages.is_empty() {
        return Err("edited_pdf_no_customer_pages");
    }
    let authoring =
        pub_viewer::bounded_authoring_slice_from_resolved_pages(session.graph(), &pages)
            .map_err(|_| "edited_pdf_current_layout_unavailable")?;
    let projection = project_bounded(authoring);

    // The same pinned, license-disclosed fallback used by the Chaptera Reader.
    // Never discover or copy host/customer source fonts.
    chaptera_desktop_fallback_font_resource::validate()
        .map_err(|_| "edited_pdf_fallback_font_invalid")?;
    let font = chaptera_desktop_fallback_font_resource::bytes();
    let fingerprint = font_fingerprint_sha256(font);
    let font_size = LengthEmu::new(chaptera_desktop_fallback_font_resource::FONT_SIZE_EMU);
    let shaping = BoundedShapingRuntime {
        layout: BoundedLayoutEnvironment {
            engine_revision: LOSS_REPORT_SCHEMA.into(),
            font_set_fingerprint: fingerprint.clone(),
            resource_fingerprint: format!("pinned-ubuntu-light:{fingerprint}"),
        },
        face_index: 0,
        font_size_emu: font_size,
        font_bytes: font,
    };
    let flow = resolve_bounded_shaped_flow(
        &projection,
        &BoundedShapedFlowRuntime {
            shaping,
            line_height: LengthEmu::new(chaptera_desktop_fallback_font_resource::LINE_HEIGHT_EMU),
        },
    )
    .map_err(|_| "edited_pdf_current_text_layout_unavailable")?;

    let scene_ids = bundle
        .geometry
        .scene
        .nodes
        .iter()
        .map(|node| node.origin)
        .collect::<BTreeSet<_>>();
    let mut text_runs = Vec::new();
    let mut glyphs = BTreeSet::new();
    let mut unavailable_glyph_lines = 0usize;
    for line in &flow.lines {
        if line.text.is_empty() {
            continue;
        }
        if !scene_ids.contains(&line.frame_origin) {
            return Err("edited_pdf_current_text_frame_missing");
        }
        if line.glyphs.iter().any(|glyph| glyph.glyph_id == 0) {
            // The standard source-PDF CLI likewise omits unshapable runs with
            // an explicit side-channel loss. Never silently replace glyph 0.
            unavailable_glyph_lines += 1;
            continue;
        }
        let row = i64::from(line.frame_line_index)
            .checked_mul(chaptera_desktop_fallback_font_resource::LINE_HEIGHT_EMU)
            .ok_or("edited_pdf_text_coordinate_overflow")?;
        let baseline_y = row
            .checked_add(font_size.get())
            .ok_or("edited_pdf_text_coordinate_overflow")?;
        glyphs.extend(line.glyphs.iter().map(|glyph| glyph.glyph_id));
        text_runs.push(FixedTextRun {
            node_id: line.frame_origin,
            scalar_base: line.scalar_start,
            logical_text: line.text.clone(),
            shaped: BoundedShapedText {
                environment: flow.environment.shaping.clone(),
                units_per_em: line.units_per_em,
                glyphs: line.glyphs.clone(),
                total_x_advance: line.measured_width,
            },
            baseline_x: LengthEmu::ZERO,
            baseline_y: LengthEmu::new(baseline_y),
            fill_rgb: [0, 0, 0],
        });
    }

    let (font_plan, fonts) = if text_runs.is_empty() {
        (None, Vec::new())
    } else {
        let embedding = read_opentype_embedding_flags(font, 0)
            .map_err(|_| "edited_pdf_fallback_font_embedding_unknown")?;
        let identity = FontIdentity {
            fingerprint_sha256: fingerprint.clone(),
            face_index: 0,
        };
        let mut profile = FixedOutputFontProfile::basic_pdf_v0_1();
        profile.preferred_embedding = PreferredEmbedding::Full;
        let plan = plan_output_fonts(
            &profile,
            vec![OutputFontRequest {
                source: identity.clone(),
                source_resource: Some(ExplicitFontResource {
                    identity: identity.clone(),
                    bytes: font,
                    embedding,
                }),
                fallback_resource: None,
                used_glyph_ids: glyphs,
            }],
        );
        if !plan.can_serialize() {
            return Err("edited_pdf_font_embedding_blocked");
        }
        (
            Some(plan),
            vec![FixedFontResource {
                identity,
                bytes: font.to_vec(),
            }],
        )
    };

    let paints = bundle
        .geometry
        .paints
        .iter()
        .filter(|paint| scene_ids.contains(&paint.node_id))
        .map(|paint| FixedNodePaint {
            node_id: paint.node_id,
            fill_rgb: paint.solid_fill_rgb,
            stroke: paint.solid_line.as_ref().map(|line| FixedStroke {
                rgb: line.rgb,
                width_emu: line.width_emu,
            }),
        })
        .collect();
    let images = bundle
        .geometry
        .images
        .iter()
        .filter_map(|image| {
            let node_ids = image
                .node_ids
                .iter()
                .copied()
                .filter(|id| scene_ids.contains(id))
                .collect::<Vec<_>>();
            if node_ids.is_empty() {
                return None;
            }
            let placements = image
                .placements
                .iter()
                .filter(|placement| {
                    node_ids.contains(&placement.node_id)
                        && (placement.content_rotation_degrees.is_some()
                            || (image.mime == "image/gif"
                                && (placement.source_window.is_some()
                                    || placement.recolor.is_some())))
                })
                .map(|placement| FixedImagePlacement {
                    node_id: placement.node_id,
                    content_rotation_degrees: placement.content_rotation_degrees,
                    source_window_present: placement.source_window.is_some(),
                    recolor_present: placement.recolor.is_some(),
                })
                .collect();
            Some(FixedImageResource {
                resource_id: image.resource_id,
                mime: image.mime.clone(),
                source_exact: image.source_exact,
                node_ids,
                placements,
                bytes: image.bytes.clone(),
            })
        })
        .collect();
    let resources = FixedPdfResources {
        page_order: pages,
        node_paints: paints,
        images,
        font_plan,
        fonts,
        text_runs,
    };
    let rendered = if nine_up {
        render_bounded_pdf_a4_card_9up(
            &bundle.geometry.scene,
            &resources,
            &PdfTargetProfile::basic_geometry_v0_1(),
        )
    } else {
        render_bounded_pdf(
            &bundle.geometry.scene,
            &resources,
            &PdfTargetProfile::basic_geometry_v0_1(),
        )
    }
    .map_err(|error| match error {
        PdfRenderError::NineUpCardGeometryUnsupported => {
            "edited_pdf_nine_up_geometry_unsupported"
        }
        _ => "edited_pdf_render_failed",
    })?;
    if rendered.bytes.len() <= 5
        || rendered.bytes.len() > MAX_PRIVATE_PDF_BYTES
        || !rendered.bytes.starts_with(b"%PDF-")
    {
        return Err("edited_pdf_output_invalid");
    }

    let painted = rendered
        .report
        .nodes
        .iter()
        .filter(|node| node.disposition == PdfRenderDisposition::Painted)
        .count();
    let partial = rendered
        .report
        .nodes
        .iter()
        .filter(|node| node.disposition == PdfRenderDisposition::Partial)
        .count();
    let unsupported = rendered
        .report
        .nodes
        .iter()
        .filter(|node| node.disposition == PdfRenderDisposition::Unsupported)
        .count();
    let layout_warnings = flow.diagnostics.len();
    let pdf_warnings = rendered.report.diagnostics.len();
    // Repeating a partial render nine times is not valid physical print
    // acceptance. Ordinary single-page PDF retains its existing explicit
    // LossReport, while 9-up refuses lossy or unsupported content.
    if nine_up
        && (rendered.report.pages.len() != 1
            || partial != 0
            || unsupported != 0
            || unavailable_glyph_lines != 0
            || layout_warnings != 0
            || pdf_warnings != 0)
    {
        return Err("edited_pdf_nine_up_visual_loss_unresolved");
    }
    // No original Story text or private font/PUB bytes appear in this report.
    let loss_report = json!({
        "schema_version": LOSS_REPORT_SCHEMA,
        "target": if nine_up { "pdf9up" } else { "pdf" },
        "renderer": "pub-pdf-v0.1",
        "current_editor_state": true,
        "can_serialize": true,
        "source_hash": expected_source_hash,
        "font": {
            "family": chaptera_desktop_fallback_font_resource::FAMILY_NAME,
            "disposition": chaptera_desktop_fallback_font_resource::DISPOSITION,
            "fingerprint_sha256": fingerprint,
            "license": chaptera_desktop_fallback_font_resource::LICENSE_ID,
            "embedding_policy": "full_if_technically_permitted"
        },
        "counts": {
            "blocking": 0,
            "pages": rendered.report.pages.len(),
            "painted": painted,
            "partial": partial,
            "unsupported": unsupported,
            "shaped_lines": resources.text_runs.len(),
            "font_missing_glyph_lines": unavailable_glyph_lines,
            "moved_nodes": moved.len(),
            "edited_stories": edited_stories,
            "layout_warnings": layout_warnings,
            "pdf_warnings": pdf_warnings
        },
        "fidelity": if unavailable_glyph_lines > 0 || partial > 0 || unsupported > 0 || layout_warnings > 0
            || pdf_warnings > 0 || !resources.text_runs.is_empty() {
                "partial"
            } else {
                "bounded"
            },
        "limitations": [
            "fixed_pdf_bounded_visual_subset_not_publisher_parity",
            "fallback_ubuntu_light_not_original_source_font",
            "missing_fallback_glyph_lines_disclosed_not_serialized",
            "publisher_rich_vectors_and_effects_not_fully_supported"
        ]
    });
    Ok(CurrentFixedPdf {
        bytes: rendered.bytes,
        loss_report,
    })
}
