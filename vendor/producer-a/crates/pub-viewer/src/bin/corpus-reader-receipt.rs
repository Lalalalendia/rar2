use anyhow::{Context, Result};
use pub_reader::{build_legacy_0x22_noquill_source_graph, PubBridgeDiagnostic};
use pub_viewer::{open_pub_bundle, viewer_geometry_environment_v0_1};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{collections::{BTreeMap, BTreeSet}, env, fs, io::Cursor, path::PathBuf};

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}


fn legacy_object_residual_census(
    bytes: &[u8],
    source_hash: pub_model::Sha256Digest,
    format_version: Option<&str>,
) -> Result<Vec<Value>> {
    if format_version != Some("0x22-noquill") {
        return Ok(Vec::new());
    }

    let source = build_legacy_0x22_noquill_source_graph(Cursor::new(bytes), source_hash)
        .context("rebuild legacy no-Quill source graph for source-free residual census")?;
    let mut counts = BTreeMap::<(Option<u16>, String), usize>::new();

    for diagnostic in source.diagnostics {
        if let PubBridgeDiagnostic::LegacyObjectNotMaterialized {
            raw_type, reason, ..
        } = diagnostic
        {
            *counts.entry((raw_type, reason)).or_default() += 1;
        }
    }

    Ok(counts
        .into_iter()
        .map(|((raw_type, reason), count)| {
            json!({
                "raw_type": raw_type,
                "raw_type_hex": raw_type.map(|value| format!("0x{value:04x}")),
                "reason": reason,
                "count": count,
            })
        })
        .collect())
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

    let receipt = match open_pub_bundle(&bytes, viewer_geometry_environment_v0_1()) {
        Ok(bundle) => {
            let visual = bundle.geometry;
            let resolved_graph = bundle.resolved_graph;
            let mut diagnostic_codes = visual
                .document
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.code.clone())
                .collect::<Vec<_>>();
            diagnostic_codes.sort();
            diagnostic_codes.dedup();

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

            let legacy_ole_node_ids = resolved_graph
                .nodes
                .values()
                .filter(|node| node.payload.legacy_ole.is_some())
                .map(|node| node.header.id)
                .collect::<BTreeSet<_>>();
            let renderable_node_ids = visual
                .scene
                .nodes
                .iter()
                .map(|node| node.origin)
                .collect::<BTreeSet<_>>();
            let legacy_ole_renderable_node_count = legacy_ole_node_ids
                .intersection(&renderable_node_ids)
                .count();
            let legacy_ole_preview_node_ids = visual
                .images
                .iter()
                .flat_map(|image| image.node_ids.iter().copied())
                .filter(|node_id| legacy_ole_node_ids.contains(node_id))
                .collect::<BTreeSet<_>>();
            let legacy_ole_preview_resource_count = visual
                .images
                .iter()
                .filter(|image| {
                    image
                        .node_ids
                        .iter()
                        .any(|node_id| legacy_ole_node_ids.contains(node_id))
                })
                .count();
            let mut legacy_ole_preview_diagnostic_counts = BTreeMap::<String, usize>::new();
            for diagnostic in &visual.document.diagnostics {
                if diagnostic.code.starts_with("viewer.legacy_ole.") {
                    *legacy_ole_preview_diagnostic_counts
                        .entry(diagnostic.code.clone())
                        .or_default() += 1;
                }
            }

            let legacy_object_residuals = legacy_object_residual_census(
                &bytes,
                visual.document.source.source_hash,
                visual.document.source.format_version.as_deref(),
            )?;

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
                "legacy_ole_node_count": legacy_ole_node_ids.len(),
                "legacy_ole_renderable_node_count": legacy_ole_renderable_node_count,
                "legacy_ole_preview_resource_count": legacy_ole_preview_resource_count,
                "legacy_ole_preview_node_count": legacy_ole_preview_node_ids.len(),
                "legacy_ole_preview_diagnostic_counts": legacy_ole_preview_diagnostic_counts,
                "paint_node_count": visual.paints.len(),
                "solid_fill_count": visual.paints.iter().filter(|paint| paint.solid_fill_rgb.is_some()).count(),
                "solid_line_count": visual.paints.iter().filter(|paint| paint.solid_line.is_some()).count(),
                "diagnostic_codes": diagnostic_codes,
                "legacy_object_residuals": legacy_object_residuals,
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
