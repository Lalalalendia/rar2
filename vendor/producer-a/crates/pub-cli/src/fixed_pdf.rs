use anyhow::{Context, Result, bail};
use chaptera_suite_handoff::AdmittedSource;
use pub_export::{ConversionProfile, EnvironmentFence, TargetProfile};
use pub_layout::{
    BoundedLayoutEnvironment, BoundedShapedFlowRuntime, BoundedShapedText, BoundedShapingRuntime,
    font_fingerprint_sha256, project_bounded, resolve_bounded_shaped_flow,
};
use pub_model::{
    AuthorityClass, EMU_PER_POINT, LengthEmu, NodeId, ReadConfidence, SourceDescriptor, SourceRef,
    SourceRole,
};
use pub_output::{
    ExplicitFontResource, FixedOutputFontProfile, FontIdentity, OutputFontRequest,
    PreferredEmbedding, plan_output_fonts, read_opentype_embedding_flags,
};
use pub_pdf::{
    FixedFontResource, FixedImagePlacement, FixedImageResource, FixedNodePaint, FixedPdfResources,
    FixedStroke, FixedTextRun, PdfPaintTraceKindV1, PdfTargetProfile, render_bounded_pdf,
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

const PDF_PRODUCT_SCHEMA: &str = "free-pub-pdf-v0.1";
const FALLBACK_FONT_SIZE_PT: i64 = 8;
const FALLBACK_LINE_HEIGHT_MULTIPLIER: i64 = 2;
const FIXED_FLOW_RECEIPT_VERSION: &str = "chaptera.fixed-pdf-shaped-flow-receipt.v1";

pub struct PdfConversionResult {
    pub report: Value,
    pub report_json_path: PathBuf,
    pub report_text_path: PathBuf,
}

fn sha256_json_id(value: &Value) -> Result<String> {
    let bytes = serde_json::to_vec(value).context("serialize fixed-flow hash payload")?;
    let digest = Sha256::digest(bytes);
    Ok(format!(
        "sha256:{}",
        digest
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    ))
}

fn shaped_glyph_sequence_hash(glyphs: &[pub_layout::BoundedShapedGlyph]) -> Result<String> {
    let normalized = glyphs
        .iter()
        .map(|glyph| {
            serde_json::json!({
                "glyph_id": glyph.glyph_id,
                "cluster": glyph.cluster,
                "x_advance": glyph.x_advance.get(),
                "y_advance": glyph.y_advance.get(),
                "x_offset": glyph.x_offset.get(),
                "y_offset": glyph.y_offset.get(),
            })
        })
        .collect::<Vec<_>>();
    sha256_json_id(&serde_json::json!(normalized))
}

fn authoritative_source_refs<'a>(
    references: &'a [SourceRef],
    primary_source: &SourceDescriptor,
) -> Vec<&'a SourceRef> {
    references
        .iter()
        .filter(|reference| {
            reference.validate_primary_source(primary_source).is_ok()
                && reference.authority == AuthorityClass::Authoritative
                && reference.confidence == Some(ReadConfidence::Exact)
                && matches!(
                    reference.role,
                    SourceRole::Semantic | SourceRole::Projection
                )
        })
        .collect()
}

fn report_path_label(path: &Path, fallback: &str) -> String {
    path.file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or(fallback)
        .to_owned()
}

fn fixed_image_placement_has_output_semantics(
    mime: &str,
    placement: &pub_viewer::ViewerImagePlacementV1,
) -> bool {
    placement.content_rotation_degrees.is_some()
        || (mime == "image/gif"
            && (placement.source_window.is_some() || placement.recolor.is_some()))
}

fn image_placement_transfer_status(
    viewer: Option<&pub_viewer::ViewerImagePlacementV1>,
    output: Option<&FixedImagePlacement>,
) -> &'static str {
    let Some(viewer) = viewer else {
        return if output.is_some() {
            "pdf_has_unmatched_placement"
        } else {
            "no_explicit_viewer_placement"
        };
    };
    let source_crop = viewer.source_window.is_some();
    let output_crop = output.is_some_and(|value| value.source_window_present);
    if source_crop && !output_crop {
        return "viewer_crop_not_forwarded";
    }
    if !source_crop && output_crop {
        return "pdf_crop_not_in_viewer";
    }
    let source_recolor = viewer.recolor.is_some();
    let output_recolor = output.is_some_and(|value| value.recolor_present);
    if source_recolor && !output_recolor {
        return "viewer_recolor_not_forwarded";
    }
    if !source_recolor && output_recolor {
        return "pdf_recolor_not_in_viewer";
    }
    if viewer.content_rotation_degrees != output.and_then(|value| value.content_rotation_degrees) {
        return "viewer_rotation_not_forwarded";
    }
    "placement_fields_forwarded"
}

fn retain_scene_node_ids(
    node_ids: &[NodeId],
    scene_node_ids: &BTreeSet<NodeId>,
) -> (Vec<NodeId>, usize) {
    let mut filtered_count = 0usize;
    let kept = node_ids
        .iter()
        .copied()
        .filter(|node_id| {
            let keep = scene_node_ids.contains(node_id);
            if !keep {
                filtered_count += 1;
            }
            keep
        })
        .collect();
    (kept, filtered_count)
}

pub fn convert_pdf(
    input: &Path,
    output: &Path,
    fallback_font: &Path,
) -> Result<PdfConversionResult> {
    let admitted = AdmittedSource::open(input).map_err(anyhow::Error::msg)?;
    let pub_bytes = admitted.bytes();
    let font_bytes = fs::read(fallback_font)
        .with_context(|| format!("read explicit fallback font {}", fallback_font.display()))?;
    // Explicit opt-in; no provenance geometry in default public receipts.
    let trace_requested = std::env::var("CHAPTERA_PDF_PAINT_TRACE").as_deref() == Ok("1");
    let (artifact, report, summary, paint_trace_receipt) = build_pdf_artifact(
        &pub_bytes,
        report_path_label(input, "input.pub"),
        &font_bytes,
        report_path_label(fallback_font, "fallback-font.ttf"),
        trace_requested,
    )?;

    if let Some(parent) = output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)
            .with_context(|| format!("create output directory {}", parent.display()))?;
    }
    admitted
        .commit_derived_output(output, &artifact)
        .with_context(|| format!("commit PDF {}", output.display()))?;

    let report_json_path = sidecar_path(output, ".loss.json");
    let report_text_path = sidecar_path(output, ".loss.txt");
    admitted
        .commit_derived_output(
            &report_json_path,
            &serde_json::to_vec_pretty(&report).context("serialize PDF loss report")?,
        )
        .with_context(|| format!("commit PDF loss report {}", report_json_path.display()))?;
    admitted
        .commit_derived_output(&report_text_path, summary.as_bytes())
        .with_context(|| format!("commit PDF loss summary {}", report_text_path.display()))?;

    if let Some(receipt) = paint_trace_receipt {
        // This diagnostic exposes NodeId and page geometry, which may reveal
        // private publication structure. Never upload it as a public CI artifact.
        let trace_path = sidecar_path(output, ".paint-trace.json");
        admitted
            .commit_derived_output(
                &trace_path,
                &serde_json::to_vec_pretty(&receipt).context("serialize private paint trace")?,
            )
            .with_context(|| format!("commit private paint trace {}", trace_path.display()))?;
    }

    Ok(PdfConversionResult {
        report,
        report_json_path,
        report_text_path,
    })
}

fn build_pdf_artifact(
    pub_bytes: &[u8],
    source_label: String,
    fallback_font_bytes: &[u8],
    fallback_label: String,
    trace_requested: bool,
) -> Result<(Vec<u8>, Value, String, Option<Value>)> {
    let classification = pub_viewer::classify_pub_family(pub_bytes);
    let source_profile_id = match classification.route {
        pub_viewer::PubReaderRoute::Mature2c => "pub-mature-0x2c-v0.1",
        pub_viewer::PubReaderRoute::Legacy22LowText => "pub-legacy-0x22-low-text-v0.1",
        pub_viewer::PubReaderRoute::Legacy22Quill => "pub-legacy-0x22-quill-v0.1",
        _ => {
            bail!(
                "bounded PDF conversion currently requires mature 0x2C, legacy 0x22 low-text, or legacy 0x22 Quill PUB input"
            )
        }
    };
    let route_uses_viewer_scene = matches!(
        classification.route,
        pub_viewer::PubReaderRoute::Legacy22LowText | pub_viewer::PubReaderRoute::Legacy22Quill
    );
    let bundle =
        pub_viewer::open_pub_bundle(pub_bytes, pub_viewer::viewer_geometry_environment_v0_1())
            .context("open supported PUB for bounded PDF conversion")?;
    let effective_page_ids = bundle
        .geometry
        .document
        .pages
        .iter()
        .map(|page| page.id)
        .collect::<Vec<_>>();
    let authoring = pub_viewer::bounded_authoring_slice_from_resolved_pages(
        &bundle.resolved_graph,
        &effective_page_ids,
    )
    .context("project effective PUB pages for bounded PDF conversion")?;
    let projection = project_bounded(authoring);
    let visual = bundle.geometry;
    let viewer_scene = visual.scene.clone();

    let embedding = read_opentype_embedding_flags(fallback_font_bytes, 0)
        .context("read fallback-font OpenType embedding flags")?;
    let fallback_fingerprint = font_fingerprint_sha256(fallback_font_bytes);
    let fallback_identity = FontIdentity {
        fingerprint_sha256: fallback_fingerprint.clone(),
        face_index: 0,
    };
    let mut conversion_profile = ConversionProfile::from_registry(
        source_profile_id,
        "pdf-basic-fixed-v0.1",
        "viewer-geometry-v0.1",
        visual.document.source.source_hash.to_string(),
        EnvironmentFence {
            policy_version: PDF_PRODUCT_SCHEMA.into(),
            renderer_version: Some("fixed-pdf-v0.1".into()),
            pinned_environment: BTreeMap::from([
                ("font_face_index".into(), "0".into()),
                (
                    "fallback_font_size_pt".into(),
                    FALLBACK_FONT_SIZE_PT.to_string(),
                ),
                ("shaping_mode".into(), "bounded-ltr".into()),
                (
                    "fallback_line_height_multiplier".into(),
                    FALLBACK_LINE_HEIGHT_MULTIPLIER.to_string(),
                ),
            ]),
        },
    )
    .context("compose deterministic PDF conversion profile")?;
    conversion_profile
        .engine_versions
        .insert("pub-viewer".into(), "viewer-geometry-v0.1".into());
    conversion_profile
        .engine_versions
        .insert("pub-output".into(), "fixed-output-font-plan-v0.1".into());
    conversion_profile.resources.insert(
        "font.fallback".into(),
        format!("sha256:{fallback_fingerprint}"),
    );
    conversion_profile
        .validate_target(&TargetProfile {
            format: "pdf".into(),
            adapter_version: "fixed-pdf-v0.1".into(),
            profile: "basic-fixed".into(),
            schema_fence: Some("PDF-1.7-bounded".into()),
        })
        .context("validate PDF conversion fence against target")?;
    let conversion_fence = conversion_profile
        .identity()
        .context("derive deterministic PDF conversion fence")?;
    let font_size_emu = LengthEmu::new(FALLBACK_FONT_SIZE_PT * EMU_PER_POINT);
    let shaping_runtime = BoundedShapingRuntime {
        layout: BoundedLayoutEnvironment {
            engine_revision: PDF_PRODUCT_SCHEMA.into(),
            font_set_fingerprint: fallback_fingerprint.clone(),
            resource_fingerprint: format!("explicit-user-fallback:{fallback_fingerprint}"),
        },
        face_index: 0,
        font_size_emu,
        font_bytes: fallback_font_bytes,
    };

    let line_height_emu = font_size_emu
        .get()
        .checked_mul(FALLBACK_LINE_HEIGHT_MULTIPLIER)
        .context("bounded PDF fallback line-height overflow")?;
    let shaped_flow = resolve_bounded_shaped_flow(
        &projection,
        &BoundedShapedFlowRuntime {
            shaping: shaping_runtime.clone(),
            line_height: LengthEmu::new(line_height_emu),
        },
    )
    .context("resolve bounded shaped text flow for fixed PDF")?;
    // Legacy no-Quill Viewer owns additional source-backed geometry laws
    // (structural point-group suppression and bounded grouped-image placement).
    // Reuse that already-resolved scene rather than duplicating its private
    // projector in the CLI. Mature 0x2C retains the established shaped-flow
    // geometry path byte-for-byte.
    let pdf_scene = if route_uses_viewer_scene {
        viewer_scene
    } else {
        shaped_flow.geometry_scene()
    };
    let scene_node_ids = pdf_scene
        .nodes
        .iter()
        .map(|node| node.origin)
        .collect::<BTreeSet<_>>();

    let mut text_runs = Vec::new();
    let mut used_glyph_ids = BTreeSet::new();
    let mut materialized = Vec::new();
    let mut skipped = Vec::new();
    let mut receipt_lines = Vec::new();
    let mut receipt_runs = Vec::new();

    for (line_index, line) in shaped_flow.lines.iter().enumerate() {
        if line.text.is_empty() {
            continue;
        }
        if line.glyphs.iter().any(|glyph| glyph.glyph_id == 0) {
            skipped.push(serde_json::json!({
                "story_id": line.story_origin,
                "frame_id": line.frame_origin,
                "scalar_start": line.scalar_start,
                "scalar_end": line.scalar_end,
                "code": "pdf.text.fallback_missing_glyph",
            }));
            continue;
        }

        let row_offset = i64::from(line.frame_line_index)
            .checked_mul(shaped_flow.environment.line_height.get())
            .context("bounded PDF shaped-flow row offset overflow")?;
        let baseline_y = row_offset
            .checked_add(font_size_emu.get())
            .context("bounded PDF shaped-flow baseline overflow")?;

        used_glyph_ids.extend(line.glyphs.iter().map(|glyph| glyph.glyph_id));
        materialized.push(serde_json::json!({
            "line_index": line_index,
            "story_id": line.story_origin,
            "frame_id": line.frame_origin,
            "frame_line_index": line.frame_line_index,
            "scalar_start": line.scalar_start,
            "scalar_end": line.scalar_end,
            "logical_text": line.text,
            "reshaped_for_break": line.reshaped_for_break,
        }));

        text_runs.push(FixedTextRun {
            node_id: line.frame_origin,
            scalar_base: line.scalar_start,
            logical_text: line.text.clone(),
            shaped: BoundedShapedText {
                environment: shaped_flow.environment.shaping.clone(),
                units_per_em: line.units_per_em,
                glyphs: line.glyphs.clone(),
                total_x_advance: line.measured_width,
            },
            baseline_x: LengthEmu::ZERO,
            baseline_y: LengthEmu::new(baseline_y),
            fill_rgb: [0, 0, 0],
        });

        let glyph_sequence_hash = shaped_glyph_sequence_hash(&line.glyphs)?;
        receipt_lines.push(serde_json::json!({
            "line_index": receipt_lines.len(),
            "frame_node_id": line.frame_origin,
            "story_id": line.story_origin,
            "scalar_start": line.scalar_start,
            "scalar_end": line.scalar_end,
            "glyph_count": line.glyphs.len(),
            "glyph_sequence_hash": glyph_sequence_hash,
            "units_per_em": line.units_per_em,
            "measured_width": line.measured_width.get(),
        }));
        receipt_runs.push(serde_json::json!({
            "run_index": receipt_runs.len(),
            "frame_node_id": line.frame_origin,
            "story_id": line.story_origin,
            "scalar_base": line.scalar_start,
            "scalar_end": line.scalar_end,
            "glyph_count": line.glyphs.len(),
            "glyph_sequence_hash": glyph_sequence_hash,
            "baseline_x": 0,
            "baseline_y": baseline_y,
        }));
    }

    let shaped_flow_diagnostics = shaped_flow
        .diagnostics
        .iter()
        .map(|diagnostic| {
            serde_json::json!({
                "code": diagnostic.code,
                "origin": diagnostic.origin,
            })
        })
        .collect::<Vec<_>>();
    let story_overset = shaped_flow
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "story_overset");
    let reshaped_line_count = shaped_flow
        .lines
        .iter()
        .filter(|line| line.reshaped_for_break)
        .count();

    let receipt_flow_id = sha256_json_id(&serde_json::json!({
        "source_hash": visual.document.source.source_hash.to_string(),
        "font_size_emu": font_size_emu.get(),
        "line_height_emu": shaped_flow.environment.line_height.get(),
        "story_overset": story_overset,
        "lines": &receipt_lines,
    }))?;
    let fixed_flow_receipt = serde_json::json!({
        "receipt_version": FIXED_FLOW_RECEIPT_VERSION,
        "producer": {
            "implementation": "chaptera-pub-cli-fixed-flow",
            "commit_or_build": format!("conversion:{}", conversion_fence.digest_sha256),
            "core_integration": true,
        },
        "source_hash": visual.document.source.source_hash.to_string(),
        "flow_id": receipt_flow_id,
        "lines": receipt_lines,
        "runs": receipt_runs,
        "story_overset": story_overset,
        "invariants": {
            "reshaping_calls": 0,
            "raw_text_emitted": false,
            "ascii_gate_applied": false,
            "overset_tail_painted": false,
            "line_order_preserved": true,
            "story_global_clusters_preserved": true,
        },
    });

    let (font_plan, fonts) = if text_runs.is_empty() {
        (None, Vec::new())
    } else {
        let mut profile = FixedOutputFontProfile::basic_pdf_v0_1();
        profile.preferred_embedding = PreferredEmbedding::Full;
        let plan = plan_output_fonts(
            &profile,
            vec![OutputFontRequest {
                source: fallback_identity.clone(),
                source_resource: Some(ExplicitFontResource {
                    identity: fallback_identity.clone(),
                    bytes: fallback_font_bytes,
                    embedding,
                }),
                fallback_resource: None,
                used_glyph_ids,
            }],
        );
        if !plan.can_serialize() {
            let codes = plan
                .blockers
                .iter()
                .map(|item| item.code.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            bail!("fallback font cannot be embedded under fixed PDF policy: {codes}");
        }
        (
            Some(plan),
            vec![FixedFontResource {
                identity: fallback_identity.clone(),
                bytes: fallback_font_bytes.to_vec(),
            }],
        )
    };

    let mut filtered_paint_node_count = 0usize;
    let node_paints = visual
        .paints
        .iter()
        .filter_map(|paint| {
            if !scene_node_ids.contains(&paint.node_id) {
                filtered_paint_node_count += 1;
                return None;
            }
            Some(FixedNodePaint {
                node_id: paint.node_id,
                fill_rgb: paint.solid_fill_rgb,
                stroke: paint.solid_line.as_ref().map(|line| FixedStroke {
                    rgb: line.rgb,
                    width_emu: line.width_emu,
                }),
            })
        })
        .collect();

    let mut filtered_image_use_count = 0usize;
    let mut filtered_image_resource_count = 0usize;
    let images = visual
        .images
        .iter()
        .filter_map(|image| {
            let (node_ids, filtered_count) =
                retain_scene_node_ids(&image.node_ids, &scene_node_ids);
            filtered_image_use_count += filtered_count;
            if node_ids.is_empty() {
                filtered_image_resource_count += 1;
                return None;
            }
            let placements = image
                .placements
                .iter()
                .filter(|placement| {
                    node_ids.contains(&placement.node_id)
                        && fixed_image_placement_has_output_semantics(&image.mime, placement)
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
        page_order: visual.document.pages.iter().map(|page| page.id).collect(),
        node_paints,
        images,
        font_plan,
        fonts,
        text_runs,
    };

    let rendered = render_bounded_pdf(
        &pdf_scene,
        &resources,
        &PdfTargetProfile::basic_geometry_v0_1(),
    )
    .context("render bounded deterministic PDF")?;

    let unsupported_pdf_nodes = rendered
        .report
        .nodes
        .iter()
        .filter(|node| node.disposition != pub_pdf::PdfRenderDisposition::Painted)
        .count();

    let mut report = serde_json::json!({
        "schema_version": PDF_PRODUCT_SCHEMA,
        "source": {
            "label": source_label,
            "descriptor": &visual.document.source,
        },
        "target": {
            "format": "pdf",
            "profile": "basic-fixed",
            "adapter_version": "fixed-pdf-v0.1",
            "schema_fence": "PDF-1.7-bounded",
        },
        "conversion_profile": &conversion_profile,
        "conversion_fence": &conversion_fence,
        "typography": {
            "source_font_identity_available": false,
            "disposition": "explicit_user_fallback_not_source_font",
            "font_size_pt": FALLBACK_FONT_SIZE_PT,
            "fallback_font": {
                "label": fallback_label,
                "fingerprint_sha256": fallback_fingerprint,
                "face_index": fallback_identity.face_index,
                "technical_embedding": embedding,
            },
            "fixed_flow_receipt": fixed_flow_receipt,
            "shaped_flow": {
                "line_height_emu": shaped_flow.environment.line_height.get(),
                "visible_line_count": shaped_flow.lines.len(),
                "reshaped_line_count": reshaped_line_count,
                "story_overset": story_overset,
                "output_adapter_reshaping_calls": 0,
                "diagnostics": shaped_flow_diagnostics,
            },
            "materialized_runs": materialized,
            "skipped": skipped,
        },
        "pdf": &rendered.report,
    });

    if filtered_paint_node_count > 0
        || filtered_image_use_count > 0
        || filtered_image_resource_count > 0
    {
        report
            .as_object_mut()
            .expect("fixed-PDF loss report must be an object")
            .insert(
                "resource_projection".into(),
                serde_json::json!({
                    "filtered_paint_node_count": filtered_paint_node_count,
                    "filtered_image_use_count": filtered_image_use_count,
                    "filtered_image_resource_count": filtered_image_resource_count,
                }),
            );
    }

    let summary = format!(
        "source: {}\ntarget: pdf / basic-fixed\nresult: ready_with_losses\nconversion_fence_sha256: {}\ntypography: explicit_user_fallback_not_source_font\nfallback_font_sha256: {}\ntext_runs_materialized: {}\ntext_runs_skipped: {}\npdf_nodes_unsupported_or_partial: {}\npdf_diagnostics: {}\n",
        report["source"]["label"].as_str().unwrap_or("-"),
        conversion_fence.digest_sha256,
        fallback_identity.fingerprint_sha256,
        report["typography"]["materialized_runs"]
            .as_array()
            .map_or(0, Vec::len),
        report["typography"]["skipped"]
            .as_array()
            .map_or(0, Vec::len),
        unsupported_pdf_nodes,
        rendered.report.diagnostics.len(),
    );
    let summary = if filtered_paint_node_count > 0
        || filtered_image_use_count > 0
        || filtered_image_resource_count > 0
    {
        format!(
            "{summary}pdf_resources_filtered_paint_nodes: {filtered_paint_node_count}\npdf_resources_filtered_image_uses: {filtered_image_use_count}\npdf_resources_filtered_image_resources: {filtered_image_resource_count}\n"
        )
    } else {
        summary
    };

    let paint_trace_receipt = trace_requested.then(|| {
        let pdf_sha256 = Sha256::digest(&rendered.bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        // The converter already owns the same resolved graph used to build
        // the Viewer scene. Never reparse PUB or guess by object coordinates.
        let painted = rendered
            .paint_trace
            .iter()
            .map(|event| event.node_origin)
            .collect::<BTreeSet<_>>();

        // Compare the exact Viewer image-placement evidence used for this
        // conversion with the *actual* FixedPdfResources handed to the PDF
        // backend. A PNG/JPEG crop may be omitted by the bounded adapter:
        // that omission is reported, not silently described as uncropped.
        let image_inputs = rendered
            .paint_trace
            .iter()
            .filter(|event| event.kind == PdfPaintTraceKindV1::Image)
            .map(|event| {
                let source_images = visual
                    .images
                    .iter()
                    .filter(|image| {
                        Some(image.resource_id) == event.resource_origin
                            && image.node_ids.contains(&event.node_origin)
                    })
                    .collect::<Vec<_>>();
                let output_images = resources
                    .images
                    .iter()
                    .filter(|image| {
                        Some(image.resource_id) == event.resource_origin
                            && image.node_ids.contains(&event.node_origin)
                    })
                    .collect::<Vec<_>>();
                if source_images.len() != 1 || output_images.len() != 1 {
                    return serde_json::json!({
                        "node_origin": event.node_origin,
                        "resource_origin": event.resource_origin,
                        "transfer_status": "unmatched_or_ambiguous_image_identity",
                    });
                }
                let viewer_image = source_images[0];
                let output_image = output_images[0];
                let viewer_placements = viewer_image
                    .placements
                    .iter()
                    .filter(|placement| placement.node_id == event.node_origin)
                    .collect::<Vec<_>>();
                let output_placements = output_image
                    .placements
                    .iter()
                    .filter(|placement| placement.node_id == event.node_origin)
                    .collect::<Vec<_>>();
                if viewer_placements.len() > 1 || output_placements.len() > 1 {
                    return serde_json::json!({
                        "node_origin": event.node_origin,
                        "resource_origin": event.resource_origin,
                        "transfer_status": "ambiguous_placement_records",
                    });
                }
                let viewer_placement = viewer_placements.first().copied();
                let output_placement = output_placements.first().copied();
                let source_window_q16 = viewer_placement
                    .and_then(|placement| placement.source_window.as_ref())
                    .map(|window| {
                        [
                            window.left_q16,
                            window.top_q16,
                            window.right_q16,
                            window.bottom_q16,
                        ]
                    });
                serde_json::json!({
                    "node_origin": event.node_origin,
                    "resource_origin": event.resource_origin,
                    "source_exact": viewer_image.source_exact,
                    "mime": &viewer_image.mime,
                    "viewer_placement_present": viewer_placement.is_some(),
                    "viewer_source_window_q16": source_window_q16,
                    "viewer_recolor_present": viewer_placement.is_some_and(|p| p.recolor.is_some()),
                    "viewer_rotation_degrees": viewer_placement.and_then(|p| p.content_rotation_degrees),
                    "pdf_placement_present": output_placement.is_some(),
                    "pdf_source_window_present": output_placement.is_some_and(|p| p.source_window_present),
                    "pdf_recolor_present": output_placement.is_some_and(|p| p.recolor_present),
                    "pdf_rotation_degrees": output_placement.and_then(|p| p.content_rotation_degrees),
                    "transfer_status": image_placement_transfer_status(viewer_placement, output_placement),
                })
            })
            .collect::<Vec<_>>();
        let source_nodes = painted
            .iter()
            .map(|node_id| {
                let source_node = bundle.resolved_graph.nodes.get(node_id);
                let references = source_node.map_or_else(Vec::new, |node| {
                    authoritative_source_refs(
                        &node.header.source_refs,
                        &bundle.resolved_graph.source,
                    )
                });
                let status = if source_node.is_none() {
                    "unmapped_projected_node"
                } else if references.is_empty() {
                    "no_exact_authoritative_source_ref"
                } else if references
                    .iter()
                    .any(|reference| reference.byte_range.is_some())
                {
                    "exact_source_byte_range"
                } else {
                    "exact_source_object_only"
                };
                serde_json::json!({
                    "node_origin": node_id,
                    "status": status,
                    "source_refs": references,
                })
            })
            .collect::<Vec<_>>();
        serde_json::json!({
            "schema_version": "chaptera.pdf-paint-trace.v1",
            "source_sha256": &bundle.resolved_graph.source.source_hash,
            "candidate_pdf_sha256": pdf_sha256,
            "pages": &rendered.report.pages,
            "paint_trace": &rendered.paint_trace,
            "source_nodes": source_nodes,
            "image_inputs": image_inputs,
        })
    });

    Ok((rendered.bytes, report, summary, paint_trace_receipt))
}

fn sidecar_path(output: &Path, suffix: &str) -> PathBuf {
    let mut value = OsString::from(output.as_os_str());
    value.push(suffix);
    PathBuf::from(value)
}

#[cfg(test)]
mod tests {
    use super::{
        authoritative_source_refs, fixed_image_placement_has_output_semantics,
        image_placement_transfer_status, report_path_label, retain_scene_node_ids,
    };
    use pub_model::{CanonicalId, NodeId};
    use pub_viewer::{ViewerImagePlacementV1, ViewerImageRecolorV1, ViewerImageSourceWindowV1};
    use std::collections::BTreeSet;
    use std::path::Path;

    #[test]
    fn pdf_source_refs_require_validated_exact_primary_authority() {
        use pub_model::{
            AuthorityClass, ByteRange, ReadConfidence, Sha256Digest, SourceDescriptor, SourceRef,
            SourceRole,
        };
        let source = SourceDescriptor {
            format: "pub".into(),
            format_version: Some("2c".into()),
            adapter_version: "synthetic-v1".into(),
            source_hash: Sha256Digest::from_bytes([7; 32]),
        };
        let exact = SourceRef {
            format: source.format.clone(),
            adapter_version: source.adapter_version.clone(),
            source_hash: source.source_hash,
            carrier: "PUB/Contents".into(),
            object_key: Some("synthetic-seq".into()),
            path: Some("SpContainer/FOPT".into()),
            byte_range: Some(ByteRange::new(30, 12)),
            role: SourceRole::Semantic,
            authority: AuthorityClass::Authoritative,
            confidence: Some(ReadConfidence::Exact),
        };
        let mut stale = exact.clone();
        stale.source_hash = Sha256Digest::from_bytes([8; 32]);
        let mut derived = exact.clone();
        derived.authority = AuthorityClass::Derived;
        let mut unknown = exact.clone();
        unknown.confidence = Some(ReadConfidence::Inferred);
        let mut cache = exact.clone();
        cache.role = SourceRole::Cache;
        let mut overflowing = exact.clone();
        overflowing.byte_range = Some(ByteRange::new(u64::MAX, 1));
        let candidates = vec![exact, stale, derived, unknown, cache, overflowing];
        let collected = authoritative_source_refs(&candidates, &source);
        assert_eq!(collected, vec![&candidates[0]]);
        assert!(candidates[0].validate_primary_source(&source).is_ok());
    }

    #[test]
    fn trace_image_placement_does_not_silently_hide_png_crop_loss() {
        use pub_pdf::FixedImagePlacement;
        use pub_viewer::ViewerImagePlacementV1;

        let node = NodeId::from_canonical(CanonicalId::from_bytes([8; 16]));
        let source = ViewerImagePlacementV1 {
            node_id: node,
            source_window: Some(pub_viewer::ViewerImageSourceWindowV1 {
                left_q16: 8_192,
                top_q16: 0,
                right_q16: 57_344,
                bottom_q16: 65_536,
            }),
            content_rotation_degrees: None,
            recolor: None,
        };
        assert!(
            !fixed_image_placement_has_output_semantics("image/png", &source),
            "current bounded PDF adapter drops standalone PNG crop metadata"
        );
        assert_eq!(
            image_placement_transfer_status(Some(&source), None),
            "viewer_crop_not_forwarded"
        );
        let passed = FixedImagePlacement {
            node_id: node,
            content_rotation_degrees: None,
            source_window_present: true,
            recolor_present: false,
        };
        assert_eq!(
            image_placement_transfer_status(Some(&source), Some(&passed)),
            "placement_fields_forwarded"
        );
        let pure_rotation = ViewerImagePlacementV1 {
            source_window: None,
            content_rotation_degrees: Some(90),
            ..source.clone()
        };
        assert_eq!(
            image_placement_transfer_status(Some(&pure_rotation), None),
            "viewer_rotation_not_forwarded"
        );
        assert_eq!(
            image_placement_transfer_status(None, None),
            "no_explicit_viewer_placement"
        );
    }

    #[test]
    fn report_label_drops_parent_directories() {
        let first = Path::new("runner-a")
            .join("nested")
            .join("SampleNewsletter.pub");
        let second = Path::new("runner-b")
            .join("other")
            .join("SampleNewsletter.pub");

        assert_eq!(
            report_path_label(&first, "input.pub"),
            "SampleNewsletter.pub"
        );
        assert_eq!(
            report_path_label(&second, "input.pub"),
            "SampleNewsletter.pub"
        );
    }

    #[test]
    fn report_label_falls_back_without_filename() {
        assert_eq!(report_path_label(Path::new(""), "input.pub"), "input.pub");
    }

    #[test]
    fn fixed_image_placement_keeps_crop_recolor_and_rotation_semantics() {
        let node_id = NodeId::from_canonical(CanonicalId::from_bytes([3; 16]));
        let empty = ViewerImagePlacementV1 {
            node_id,
            source_window: None,
            content_rotation_degrees: None,
            recolor: None,
        };
        assert!(!fixed_image_placement_has_output_semantics(
            "image/png",
            &empty
        ));
        assert!(!fixed_image_placement_has_output_semantics(
            "image/gif",
            &empty
        ));

        let mut rotation = empty.clone();
        rotation.content_rotation_degrees = Some(90);
        assert!(fixed_image_placement_has_output_semantics(
            "image/png",
            &rotation
        ));

        let mut crop = empty.clone();
        crop.source_window = Some(ViewerImageSourceWindowV1 {
            left_q16: 0,
            top_q16: 0,
            right_q16: 1 << 15,
            bottom_q16: 1 << 16,
        });
        assert!(!fixed_image_placement_has_output_semantics(
            "image/png",
            &crop
        ));
        assert!(fixed_image_placement_has_output_semantics(
            "image/gif",
            &crop
        ));

        let mut recolor = empty;
        recolor.recolor = Some(ViewerImageRecolorV1 {
            target_rgb: [1, 2, 3],
            preserve_grays: true,
        });
        assert!(!fixed_image_placement_has_output_semantics(
            "image/jpeg",
            &recolor
        ));
        assert!(fixed_image_placement_has_output_semantics(
            "image/gif",
            &recolor
        ));
    }

    #[test]
    fn resource_scene_projection_filters_only_absent_node_ids() {
        let present = NodeId::from_canonical(CanonicalId::from_bytes([1; 16]));
        let absent = NodeId::from_canonical(CanonicalId::from_bytes([2; 16]));
        let scene = BTreeSet::from([present]);

        let (kept, filtered_count) = retain_scene_node_ids(&[present, absent], &scene);

        assert_eq!(kept, vec![present]);
        assert_eq!(filtered_count, 1);
    }
}
