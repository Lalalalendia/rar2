use anyhow::{Context, Result, bail};
use chaptera_viewer_render_plan::{
    ExplicitRenderTextFontResourceV1, RenderResolvedShapingV1, RenderTextFragmentV1,
    RenderTextLayoutDispositionV1, build_page_render_plan_with_text_layout_v1,
};
use pub_export::{ConversionProfile, EnvironmentFence, TargetProfile};
use pub_layout::{
    BoundedLayoutEnvironment, BoundedResolvedScene, BoundedShapedFlowRuntime, BoundedShapedText,
    BoundedShapingRuntime, font_fingerprint_sha256, project_bounded, resolve_bounded_shaped_flow,
};
use pub_model::{EMU_PER_POINT, LengthEmu, NodeId};
use pub_output::{
    ExplicitFontResource, FixedOutputFontProfile, FontIdentity, OutputFontRequest,
    PreferredEmbedding, plan_output_fonts, read_opentype_embedding_flags,
};
use pub_pdf::{
    FixedFontResource, FixedImageResource, FixedNodePaint, FixedPdfResources, FixedStroke,
    FixedTextRun, PdfTargetProfile, render_bounded_pdf,
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

fn report_path_label(path: &Path, fallback: &str) -> String {
    path.file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or(fallback)
        .to_owned()
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

#[derive(Default)]
struct ViewerTextMaterialization {
    text_runs: Vec<FixedTextRun>,
    used_glyph_ids: BTreeSet<u32>,
    materialized: Vec<Value>,
    skipped: Vec<Value>,
    receipt_lines: Vec<Value>,
    receipt_runs: Vec<Value>,
    layout_fallback_counts: BTreeMap<String, usize>,
    visible_line_count: usize,
    fallback_nodes: BTreeSet<NodeId>,
}

fn resolved_text_color_for_range(
    fragment: &RenderTextFragmentV1,
    scalar_start: u32,
    scalar_end: u32,
) -> Option<[u8; 3]> {
    if scalar_start >= scalar_end {
        return None;
    }

    let mut cursor = scalar_start;
    let mut resolved = None;
    for run in &fragment.typography {
        if run.scalar_end <= scalar_start || run.scalar_start >= scalar_end {
            continue;
        }
        if run.scalar_end <= run.scalar_start {
            return None;
        }
        let start = run.scalar_start.max(scalar_start);
        let end = run.scalar_end.min(scalar_end);
        if start != cursor || end <= start {
            return None;
        }
        let color = run.color_rgb?;
        match resolved {
            None => resolved = Some(color),
            Some(existing) if existing == color => {}
            Some(_) => return None,
        }
        cursor = end;
    }

    (cursor == scalar_end).then_some(resolved).flatten()
}

#[allow(clippy::too_many_arguments)]
fn push_viewer_text_run(
    output: &mut ViewerTextMaterialization,
    node_id: NodeId,
    scalar_start: u32,
    scalar_end: u32,
    logical_text: &str,
    measured_width_emu: i64,
    shaping: &RenderResolvedShapingV1,
    baseline_x: i64,
    baseline_y: i64,
    fill_rgb: [u8; 3],
    source_kind: &str,
) -> Result<()> {
    if logical_text.is_empty() {
        return Ok(());
    }
    if measured_width_emu <= 0 || shaping.units_per_em == 0 {
        output.fallback_nodes.insert(node_id);
        output.skipped.push(serde_json::json!({
            "node_id": node_id,
            "scalar_start": scalar_start,
            "scalar_end": scalar_end,
            "code": "pdf.text.viewer_shaping_invalid",
        }));
        return Ok(());
    }
    if shaping.glyphs.iter().any(|glyph| glyph.glyph_id == 0) {
        output.fallback_nodes.insert(node_id);
        output.skipped.push(serde_json::json!({
            "node_id": node_id,
            "scalar_start": scalar_start,
            "scalar_end": scalar_end,
            "code": "pdf.text.fallback_missing_glyph",
        }));
        return Ok(());
    }

    output
        .used_glyph_ids
        .extend(shaping.glyphs.iter().map(|glyph| glyph.glyph_id));
    let glyph_sequence_hash = shaped_glyph_sequence_hash(&shaping.glyphs)?;
    let run_index = output.receipt_runs.len();

    output.materialized.push(serde_json::json!({
        "run_index": run_index,
        "frame_id": node_id,
        "scalar_start": scalar_start,
        "scalar_end": scalar_end,
        "logical_text": logical_text,
        "source_kind": source_kind,
        "baseline_x": baseline_x,
        "baseline_y": baseline_y,
        "fill_rgb": fill_rgb,
    }));
    output.receipt_lines.push(serde_json::json!({
        "line_index": output.receipt_lines.len(),
        "frame_node_id": node_id,
        "scalar_start": scalar_start,
        "scalar_end": scalar_end,
        "glyph_count": shaping.glyphs.len(),
        "glyph_sequence_hash": glyph_sequence_hash,
        "units_per_em": shaping.units_per_em,
        "measured_width": measured_width_emu,
        "source_kind": source_kind,
    }));
    output.receipt_runs.push(serde_json::json!({
        "run_index": run_index,
        "frame_node_id": node_id,
        "scalar_base": scalar_start,
        "scalar_end": scalar_end,
        "glyph_count": shaping.glyphs.len(),
        "glyph_sequence_hash": glyph_sequence_hash,
        "baseline_x": baseline_x,
        "baseline_y": baseline_y,
        "fill_rgb": fill_rgb,
        "source_kind": source_kind,
    }));
    output.text_runs.push(FixedTextRun {
        node_id,
        scalar_base: scalar_start,
        logical_text: logical_text.to_owned(),
        shaped: BoundedShapedText {
            environment: shaping.environment.clone(),
            units_per_em: shaping.units_per_em,
            glyphs: shaping.glyphs.clone(),
            total_x_advance: LengthEmu::new(measured_width_emu),
        },
        baseline_x: LengthEmu::new(baseline_x),
        baseline_y: LengthEmu::new(baseline_y),
        fill_rgb,
    });
    Ok(())
}

fn materialize_viewer_text_runs(
    visual: &pub_viewer::ViewerGeometryDocument,
    pdf_scene: &BoundedResolvedScene,
    fallback_font: &ExplicitRenderTextFontResourceV1<'_>,
) -> Result<ViewerTextMaterialization> {
    let scene_bounds = pdf_scene
        .nodes
        .iter()
        .map(|node| (node.origin, node.bounds))
        .collect::<BTreeMap<_, _>>();
    let mut output = ViewerTextMaterialization::default();

    for page_index in 0..visual.document.pages.len() {
        let plan = build_page_render_plan_with_text_layout_v1(visual, page_index, fallback_font)
            .with_context(|| format!("build Viewer resolved text layout for page {page_index}"))?;

        for node in &plan.nodes {
            let Some(fragment) = node.text.as_ref() else {
                continue;
            };
            let Some(layout) = fragment.layout.as_ref() else {
                output.skipped.push(serde_json::json!({
                    "node_id": node.node_id,
                    "code": "pdf.text.viewer_layout_missing",
                }));
                output.fallback_nodes.insert(node.node_id);
                continue;
            };

            match &layout.disposition {
                RenderTextLayoutDispositionV1::BackendFallback { reason } => {
                    *output
                        .layout_fallback_counts
                        .entry(reason.code().to_owned())
                        .or_default() += 1;
                    output.skipped.push(serde_json::json!({
                        "node_id": node.node_id,
                        "code": "pdf.text.viewer_layout_fallback",
                        "reason": reason.code(),
                    }));
                    output.fallback_nodes.insert(node.node_id);
                continue;
                }
                RenderTextLayoutDispositionV1::SharedResolved {
                    font_resource_id,
                    font_fingerprint_sha256,
                    ..
                } => {
                    if font_resource_id != fallback_font.resource_id
                        || font_fingerprint_sha256 != fallback_font.expected_sha256
                    {
                        output.skipped.push(serde_json::json!({
                            "node_id": node.node_id,
                            "code": "pdf.text.viewer_font_resource_mismatch",
                        }));
                        output.fallback_nodes.insert(node.node_id);
                continue;
                    }
                }
            }

            let Some(scene_bounds_for_node) = scene_bounds.get(&node.node_id) else {
                output.skipped.push(serde_json::json!({
                    "node_id": node.node_id,
                    "code": "pdf.text.viewer_node_missing_from_scene",
                }));
                output.fallback_nodes.insert(node.node_id);
                continue;
            };
            if scene_bounds_for_node != &node.bounds {
                output.skipped.push(serde_json::json!({
                    "node_id": node.node_id,
                    "code": "pdf.text.viewer_node_geometry_mismatch",
                }));
                output.fallback_nodes.insert(node.node_id);
                continue;
            }

            let text_bounds = node.text_bounds.unwrap_or(node.bounds);
            let base_x = text_bounds
                .x
                .get()
                .checked_sub(node.bounds.x.get())
                .context("Viewer text bounds x offset overflow")?;
            let base_y = text_bounds
                .y
                .get()
                .checked_sub(node.bounds.y.get())
                .context("Viewer text bounds y offset overflow")?;
            let mut line_top = 0_i64;

            for line in &layout.lines {
                output.visible_line_count += 1;
                let current_line_top = line_top;
                line_top = line_top
                    .checked_add(line.line_height_emu)
                    .context("Viewer text line top overflow")?;
                if line.text.is_empty() {
                    continue;
                }

                if !line.spans.is_empty() {
                    for span in &line.spans {
                        let Some(shaping) = span.shaping.as_ref() else {
                            output.skipped.push(serde_json::json!({
                                "node_id": node.node_id,
                                "scalar_start": span.scalar_start,
                                "scalar_end": span.scalar_end,
                                "code": "pdf.text.viewer_span_shaping_missing",
                            }));
                            output.fallback_nodes.insert(node.node_id);
                continue;
                        };
                        if shaping.environment.face_index != fallback_font.face_index
                            || shaping.environment.layout.font_set_fingerprint
                                != fallback_font.expected_sha256
                            || shaping.environment.layout.resource_fingerprint
                                != fallback_font.resource_id
                        {
                            output.skipped.push(serde_json::json!({
                                "node_id": node.node_id,
                                "scalar_start": span.scalar_start,
                                "scalar_end": span.scalar_end,
                                "code": "pdf.text.viewer_span_font_resource_mismatch",
                            }));
                            output.fallback_nodes.insert(node.node_id);
                continue;
                        }
                        let fill_rgb = resolved_text_color_for_range(
                            fragment,
                            span.scalar_start,
                            span.scalar_end,
                        )
                        .unwrap_or_else(|| {
                            *output
                                .layout_fallback_counts
                                .entry("color_unresolved_black".to_owned())
                                .or_default() += 1;
                            [0, 0, 0]
                        });
                        let baseline_x = base_x
                            .checked_add(line.x_offset_emu)
                            .and_then(|value| value.checked_add(span.x_offset_emu))
                            .context("Viewer text span baseline x overflow")?;
                        let baseline_y = base_y
                            .checked_add(layout.vertical_offset_emu)
                            .and_then(|value| value.checked_add(current_line_top))
                            .and_then(|value| value.checked_add(span.font_size_emu))
                            .context("Viewer text span baseline y overflow")?;
                        push_viewer_text_run(
                            &mut output,
                            node.node_id,
                            span.scalar_start,
                            span.scalar_end,
                            &span.text,
                            span.measured_width_emu,
                            shaping,
                            baseline_x,
                            baseline_y,
                            fill_rgb,
                            "viewer_shared_resolved_span",
                        )?;
                    }
                    continue;
                }

                let Some(shaping) = line.shaping.as_ref() else {
                    output.skipped.push(serde_json::json!({
                        "node_id": node.node_id,
                        "scalar_start": line.scalar_start,
                        "scalar_end": line.scalar_end,
                        "code": "pdf.text.viewer_line_shaping_missing",
                    }));
                    output.fallback_nodes.insert(node.node_id);
                continue;
                };
                if shaping.environment.face_index != fallback_font.face_index
                    || shaping.environment.layout.font_set_fingerprint
                        != fallback_font.expected_sha256
                    || shaping.environment.layout.resource_fingerprint != fallback_font.resource_id
                {
                    output.skipped.push(serde_json::json!({
                        "node_id": node.node_id,
                        "scalar_start": line.scalar_start,
                        "scalar_end": line.scalar_end,
                        "code": "pdf.text.viewer_line_font_resource_mismatch",
                    }));
                    output.fallback_nodes.insert(node.node_id);
                continue;
                }
                let fill_rgb =
                    resolved_text_color_for_range(fragment, line.scalar_start, line.scalar_end)
                        .unwrap_or_else(|| {
                            *output
                                .layout_fallback_counts
                                .entry("color_unresolved_black".to_owned())
                                .or_default() += 1;
                            [0, 0, 0]
                        });
                let baseline_x = base_x
                    .checked_add(line.x_offset_emu)
                    .context("Viewer text line baseline x overflow")?;
                let baseline_y = base_y
                    .checked_add(layout.vertical_offset_emu)
                    .and_then(|value| value.checked_add(current_line_top))
                    .and_then(|value| value.checked_add(shaping.environment.font_size_emu.get()))
                    .context("Viewer text line baseline y overflow")?;
                push_viewer_text_run(
                    &mut output,
                    node.node_id,
                    line.scalar_start,
                    line.scalar_end,
                    &line.text,
                    line.measured_width_emu,
                    shaping,
                    baseline_x,
                    baseline_y,
                    fill_rgb,
                    "viewer_shared_resolved_line",
                )?;
            }
        }
    }

    Ok(output)
}

fn json_value_has_node(
    value: &Value,
    field: &str,
    nodes: &BTreeSet<NodeId>,
) -> bool {
    let Some(candidate) = value.get(field) else {
        return false;
    };
    nodes.iter().any(|node_id| {
        serde_json::to_value(node_id)
            .ok()
            .as_ref()
            .is_some_and(|expected| expected == candidate)
    })
}

pub fn convert_pdf(
    input: &Path,
    output: &Path,
    fallback_font: &Path,
) -> Result<PdfConversionResult> {
    let pub_bytes =
        fs::read(input).with_context(|| format!("read PUB source {}", input.display()))?;
    let font_bytes = fs::read(fallback_font)
        .with_context(|| format!("read explicit fallback font {}", fallback_font.display()))?;
    let (artifact, report, summary) = build_pdf_artifact(
        &pub_bytes,
        report_path_label(input, "input.pub"),
        &font_bytes,
        report_path_label(fallback_font, "fallback-font.ttf"),
    )?;

    if let Some(parent) = output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)
            .with_context(|| format!("create output directory {}", parent.display()))?;
    }
    fs::write(output, artifact).with_context(|| format!("write PDF {}", output.display()))?;

    let report_json_path = sidecar_path(output, ".loss.json");
    let report_text_path = sidecar_path(output, ".loss.txt");
    fs::write(
        &report_json_path,
        serde_json::to_vec_pretty(&report).context("serialize PDF loss report")?,
    )
    .with_context(|| format!("write PDF loss report {}", report_json_path.display()))?;
    fs::write(&report_text_path, summary)
        .with_context(|| format!("write PDF loss summary {}", report_text_path.display()))?;

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
) -> Result<(Vec<u8>, Value, String)> {
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
    conversion_profile.engine_versions.insert(
        "chaptera-viewer-render-plan".into(),
        chaptera_viewer_render_plan::SHARED_TEXT_LAYOUT_REVISION_V1.into(),
    );
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

    let fallback_resource_id = format!("explicit-user-fallback:{fallback_fingerprint}");
    let fallback_render_font = ExplicitRenderTextFontResourceV1 {
        resource_id: &fallback_resource_id,
        expected_sha256: &fallback_fingerprint,
        face_index: 0,
        default_font_size_emu: font_size_emu.get(),
        default_line_height_emu: line_height_emu,
        bytes: fallback_font_bytes,
    };
    let mut viewer_text =
        materialize_viewer_text_runs(&visual, &pdf_scene, &fallback_render_font)?;

    // A Viewer node replaces the old bounded approximation only when the node
    // has at least one resolved run and no hard Viewer-layout failure.
    let viewer_candidate_nodes = viewer_text
        .text_runs
        .iter()
        .map(|run| run.node_id)
        .collect::<BTreeSet<_>>();
    let viewer_replacement_nodes = viewer_candidate_nodes
        .difference(&viewer_text.fallback_nodes)
        .cloned()
        .collect::<BTreeSet<_>>();

    text_runs.retain(|run| !viewer_replacement_nodes.contains(&run.node_id));
    materialized.retain(|item| {
        !json_value_has_node(item, "frame_id", &viewer_replacement_nodes)
    });
    skipped.retain(|item| {
        !json_value_has_node(item, "frame_id", &viewer_replacement_nodes)
    });
    receipt_lines.retain(|item| {
        !json_value_has_node(item, "frame_node_id", &viewer_replacement_nodes)
    });
    receipt_runs.retain(|item| {
        !json_value_has_node(item, "frame_node_id", &viewer_replacement_nodes)
    });

    viewer_text
        .text_runs
        .retain(|run| viewer_replacement_nodes.contains(&run.node_id));
    viewer_text.materialized.retain(|item| {
        !json_value_has_node(item, "frame_id", &viewer_text.fallback_nodes)
    });
    viewer_text.receipt_lines.retain(|item| {
        !json_value_has_node(item, "frame_node_id", &viewer_text.fallback_nodes)
    });
    viewer_text.receipt_runs.retain(|item| {
        !json_value_has_node(item, "frame_node_id", &viewer_text.fallback_nodes)
    });

    text_runs.extend(viewer_text.text_runs);
    materialized.extend(viewer_text.materialized);
    skipped.extend(viewer_text.skipped);
    receipt_lines.extend(viewer_text.receipt_lines);
    receipt_runs.extend(viewer_text.receipt_runs);

    used_glyph_ids.clear();
    for run in &text_runs {
        used_glyph_ids.extend(run.shaped.glyphs.iter().map(|glyph| glyph.glyph_id));
    }

    let viewer_replacement_node_count = viewer_replacement_nodes.len();
    let viewer_fallback_node_count = viewer_text.fallback_nodes.len();
    let viewer_layout_fallback_counts = viewer_text.layout_fallback_counts;

    let receipt_flow_id = sha256_json_id(&serde_json::json!({
        "source_hash": visual.document.source.source_hash.to_string(),
        "font_size_emu": font_size_emu.get(),
        "line_height_emu": shaped_flow.environment.line_height.get(),
        "story_overset": story_overset,
        "layout_authority": "viewer_resolved_with_bounded_fallback",
        "viewer_replacement_node_count": viewer_replacement_node_count,
        "viewer_fallback_node_count": viewer_fallback_node_count,
        "viewer_layout_fallback_counts": &viewer_layout_fallback_counts,
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
        "layout_authority": "viewer_resolved_with_bounded_fallback",
        "viewer_replacement_node_count": viewer_replacement_node_count,
        "viewer_fallback_node_count": viewer_fallback_node_count,
        "viewer_layout_fallback_counts": &viewer_layout_fallback_counts,
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
            Some(FixedImageResource {
                resource_id: image.resource_id,
                mime: image.mime.clone(),
                source_exact: image.source_exact,
                node_ids,
                bytes: image.bytes.clone(),
            })
        })
        .collect();

    let resources = FixedPdfResources {
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
            "layout_authority": "viewer_resolved_with_bounded_fallback",
            "viewer_replacement_node_count": viewer_replacement_node_count,
            "viewer_fallback_node_count": viewer_fallback_node_count,
            "viewer_layout_fallback_counts": &viewer_layout_fallback_counts,
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

    Ok((rendered.bytes, report, summary))
}

fn sidecar_path(output: &Path, suffix: &str) -> PathBuf {
    let mut value = OsString::from(output.as_os_str());
    value.push(suffix);
    PathBuf::from(value)
}

#[cfg(test)]
mod tests {
    use super::{report_path_label, retain_scene_node_ids};
    use pub_model::{CanonicalId, NodeId};
    use std::collections::BTreeSet;
    use std::path::Path;

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
    fn resource_scene_projection_filters_only_absent_node_ids() {
        let present = NodeId::from_canonical(CanonicalId::from_bytes([1; 16]));
        let absent = NodeId::from_canonical(CanonicalId::from_bytes([2; 16]));
        let scene = BTreeSet::from([present]);

        let (kept, filtered_count) = retain_scene_node_ids(&[present, absent], &scene);

        assert_eq!(kept, vec![present]);
        assert_eq!(filtered_count, 1);
    }
}
