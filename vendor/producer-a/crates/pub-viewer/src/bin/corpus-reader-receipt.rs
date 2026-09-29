use anyhow::{Context, Result};
use pub_model::Sha256Digest;
use pub_reader::{build_legacy_0x22_noquill_source_graph, PubBridgeDiagnostic};
use pub_viewer::{open_pub_geometry, viewer_geometry_environment_v0_1};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, env, fs, io::Cursor, path::PathBuf};

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn legacy_object_not_materialized_residuals(
    bytes: &[u8],
    source_sha256: &str,
) -> (
    BTreeMap<String, usize>,
    BTreeMap<String, BTreeMap<String, usize>>,
) {
    let Ok(source_hash) = source_sha256.parse::<Sha256Digest>() else {
        return (BTreeMap::new(), BTreeMap::new());
    };
    let Ok(build) = build_legacy_0x22_noquill_source_graph(Cursor::new(bytes), source_hash) else {
        return (BTreeMap::new(), BTreeMap::new());
    };

    let mut raw_type_counts = BTreeMap::new();
    let mut raw_type_reason_counts = BTreeMap::<String, BTreeMap<String, usize>>::new();
    for diagnostic in build.diagnostics {
        if let PubBridgeDiagnostic::LegacyObjectNotMaterialized {
            raw_type, reason, ..
        } = diagnostic
        {
            let raw_type_key = raw_type
                .map(|value| format!("0x{value:04x}"))
                .unwrap_or_else(|| "unknown".to_owned());
            *raw_type_counts.entry(raw_type_key.clone()).or_insert(0) += 1;
            *raw_type_reason_counts
                .entry(raw_type_key)
                .or_default()
                .entry(reason)
                .or_insert(0) += 1;
        }
    }
    (raw_type_counts, raw_type_reason_counts)
}

fn main() -> Result<()> {
    let mut args = env::args_os().skip(1);
    let source = PathBuf::from(
        args.next()
            .context("usage: corpus-reader-receipt SOURCE.pub OUTPUT.json")?,
    );
    let output = PathBuf::from(
        args.next()
            .context("usage: corpus-reader-receipt SOURCE.pub OUTPUT.json")?,
    );
    if args.next().is_some() {
        anyhow::bail!("corpus-reader-receipt accepts exactly SOURCE.pub OUTPUT.json");
    }

    let bytes = fs::read(&source).with_context(|| format!("read {}", source.display()))?;
    let source_sha256 = sha256_hex(&bytes);

    let receipt = match open_pub_geometry(&bytes, viewer_geometry_environment_v0_1()) {
        Ok(visual) => {
            let mut diagnostic_codes = visual
                .document
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.code.clone())
                .collect::<Vec<_>>();
            diagnostic_codes.sort();
            diagnostic_codes.dedup();

            let (
                legacy_object_not_materialized_raw_type_counts,
                legacy_object_not_materialized_raw_type_reason_counts,
            ) = if visual.document.source.format_version.as_deref() == Some("0x22-noquill") {
                legacy_object_not_materialized_residuals(&bytes, &source_sha256)
            } else {
                (BTreeMap::new(), BTreeMap::new())
            };

            let inherited_typography_run_count = visual
                .typography_runs
                .iter()
                .filter(|run| run.font_inherited || run.size_inherited)
                .count();
            let image_placement_count = visual
                .images
                .iter()
                .map(|image| image.node_ids.len())
                .sum::<usize>();

            json!({
                "schema": "chaptera.reader-corpus-structural-receipt.v1",
                "opened": true,
                "source_sha256": source_sha256,
                "byte_len": bytes.len(),
                "format": visual.document.source.format,
                "format_version": visual.document.source.format_version,
                "fidelity_status": visual.document.fidelity_status(),
                "viewer_page_count": visual.document.pages.len(),
                "scene_surface_count": visual.scene.surfaces.len(),
                "scene_node_count": visual.scene.nodes.len(),
                "story_count": visual.document.stories.len(),
                "story_frame_count": visual.story_frames.len(),
                "text_fragment_count": visual.text_fragments.len(),
                "typography_run_count": visual.typography_runs.len(),
                "inherited_typography_run_count": inherited_typography_run_count,
                "image_resource_count": visual.images.len(),
                "image_placement_count": image_placement_count,
                "paint_node_count": visual.paints.len(),
                "solid_fill_count": visual.paints.iter().filter(|paint| paint.solid_fill_rgb.is_some()).count(),
                "solid_line_count": visual.paints.iter().filter(|paint| paint.solid_line.is_some()).count(),
                "diagnostic_codes": diagnostic_codes,
                "legacy_object_not_materialized_raw_type_counts": legacy_object_not_materialized_raw_type_counts,
                "legacy_object_not_materialized_raw_type_reason_counts": legacy_object_not_materialized_raw_type_reason_counts,
                "visual_fidelity_proven": false,
            })
        }
        Err(error) => {
            let error_text = format!("{error:#}");
            json!({
                "schema": "chaptera.reader-corpus-structural-receipt.v1",
                "opened": false,
                "source_sha256": source_sha256,
                "byte_len": bytes.len(),
                "open_error_kind": "viewer_open_failed",
                "open_error_signature_sha256": sha256_hex(error_text.as_bytes()),
                "visual_fidelity_proven": false,
            })
        }
    };

    fs::write(
        &output,
        serde_json::to_vec_pretty(&receipt).context("serialize Reader corpus receipt")?,
    )
    .with_context(|| format!("write {}", output.display()))?;
    Ok(())
}
