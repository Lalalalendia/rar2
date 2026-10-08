use anyhow::{Context, Result, bail};
use pub_export::{ConversionProfile, EnvironmentFence, TargetProfile};
use pub_layout::{
    BoundedLayoutEnvironment, BoundedShapedFlowRuntime, BoundedShapedText, BoundedShapingRuntime,
    font_fingerprint_sha256, project_bounded, resolve_bounded_shaped_flow,
};
use pub_model::{EMU_PER_POINT, LengthEmu};
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
        input.display().to_string(),
        &font_bytes,
        fallback_font.display().to_string(),
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
    if classification.route != pub_viewer::PubReaderRoute::Mature2c {
        bail!("bounded PDF conversion currently requires mature 0x2C PUB input");
    }
    let bundle = pub_viewer::open_pub_bundle(
        pub_bytes,
        pub_viewer::viewer_geometry_environment_v0_1(),
    )
    .context("open mature-0x2C PUB for bounded PDF conversion")?;
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
    .context("project effective mature pages for bounded PDF conversion")?;
    let projection = project_bounded(authoring);
    let visual = bundle.geometry;

    let embedding = read_opentype_embedding_flags(fallback_font_bytes, 0)
        .context("read fallback-font OpenType embedding flags")?;
    let fallback_fingerprint = font_fingerprint_sha256(fallback_font_bytes);
    let fallback_identity = FontIdentity {
        fingerprint_sha256: fallback_fingerprint.clone(),
        face_index: 0,
    };
    let mut conversion_profile = ConversionProfile::from_registry(
        "pub-mature-0x2c-v0.1",
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
    let pdf_scene = shaped_flow.geometry_scene();

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

    let resources = FixedPdfResources {
        node_paints: visual
            .paints
            .iter()
            .map(|paint| FixedNodePaint {
                node_id: paint.node_id,
                fill_rgb: paint.solid_fill_rgb,
                stroke: paint.solid_line.as_ref().map(|line| FixedStroke {
                    rgb: line.rgb,
                    width_emu: line.width_emu,
                }),
            })
            .collect(),
        images: visual
            .images
            .iter()
            .map(|image| FixedImageResource {
                resource_id: image.resource_id,
                mime: image.mime.clone(),
                node_ids: image.node_ids.clone(),
                bytes: image.bytes.clone(),
            })
            .collect(),
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

    let report = serde_json::json!({
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

    Ok((rendered.bytes, report, summary))
}

fn sidecar_path(output: &Path, suffix: &str) -> PathBuf {
    let mut value = OsString::from(output.as_os_str());
    value.push(suffix);
    PathBuf::from(value)
}
