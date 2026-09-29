use anyhow::{Context, Result};
use pub_model::{NodeId, NodeKind};
use pub_reader::{
    LegacyOleCachedPresentationSelection, PubBridgeDiagnostic,
    build_legacy_0x22_noquill_source_graph, rasterize_wmf_preview, resolve_pub_source_graph,
    scan_legacy_ole_cached_presentations, select_unambiguous_legacy_ole_cached_presentation,
};
use pub_viewer::{ViewerGeometryDocument, open_pub_geometry, viewer_geometry_environment_v0_1};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    io::Cursor,
    path::PathBuf,
};

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


fn legacy_ole_preview_funnel(
    bytes: &[u8],
    source_hash: pub_model::Sha256Digest,
    format_version: Option<&str>,
    visual: &ViewerGeometryDocument,
) -> Value {
    if format_version != Some("0x22-noquill") {
        return json!({
            "schema": "chaptera.legacy-ole-preview-funnel.v1",
            "applicable": false,
            "available": true,
        });
    }

    let source = match build_legacy_0x22_noquill_source_graph(Cursor::new(bytes), source_hash) {
        Ok(source) => source,
        Err(_) => {
            return json!({
                "schema": "chaptera.legacy-ole-preview-funnel.v1",
                "applicable": true,
                "available": false,
                "failure_stage": "source_graph",
            });
        }
    };
    let resolved = match resolve_pub_source_graph(&source.graph) {
        Ok(resolved) => resolved,
        Err(_) => {
            return json!({
                "schema": "chaptera.legacy-ole-preview-funnel.v1",
                "applicable": true,
                "available": false,
                "failure_stage": "resolve",
            });
        }
    };

    let renderable_node_ids = visual
        .scene
        .nodes
        .iter()
        .map(|node| node.origin)
        .collect::<BTreeSet<_>>();
    let mut uses_by_storage = BTreeMap::<u16, Vec<NodeId>>::new();

    for node in resolved.graph.nodes.values() {
        if node.kind != NodeKind::Unsupported || !renderable_node_ids.contains(&node.header.id) {
            continue;
        }
        let Some(legacy_ole) = node.payload.legacy_ole.as_ref() else {
            continue;
        };
        uses_by_storage
            .entry(legacy_ole.storage_number)
            .or_default()
            .push(node.header.id);
    }

    for node_ids in uses_by_storage.values_mut() {
        node_ids.sort();
        node_ids.dedup();
    }

    let admitted_node_ids = uses_by_storage
        .values()
        .flat_map(|node_ids| node_ids.iter().copied())
        .collect::<BTreeSet<_>>();

    let mut scan_success_node_count = 0usize;
    let mut scan_failure_node_count = 0usize;
    let mut valid_candidate_count = 0usize;
    let mut rejected_sibling_count = 0usize;
    let mut selection_none_node_count = 0usize;
    let mut selection_unique_node_count = 0usize;
    let mut selection_equivalent_node_count = 0usize;
    let mut selection_ambiguous_node_count = 0usize;
    let mut raster_success_node_count = 0usize;
    let mut raster_rejected_node_count = 0usize;

    for (storage_number, node_ids) in &uses_by_storage {
        let node_count = node_ids.len();
        let scan = match scan_legacy_ole_cached_presentations(Cursor::new(bytes), *storage_number) {
            Ok(scan) => scan,
            Err(_) => {
                scan_failure_node_count += node_count;
                continue;
            }
        };

        scan_success_node_count += node_count;
        valid_candidate_count += scan.presentations.len();
        rejected_sibling_count += scan.diagnostics.len();

        let selected = match select_unambiguous_legacy_ole_cached_presentation(&scan) {
            LegacyOleCachedPresentationSelection::None => {
                selection_none_node_count += node_count;
                continue;
            }
            LegacyOleCachedPresentationSelection::Ambiguous { .. } => {
                selection_ambiguous_node_count += node_count;
                continue;
            }
            LegacyOleCachedPresentationSelection::Selected {
                presentation,
                equivalent_candidate_count,
            } => {
                if equivalent_candidate_count > 1 {
                    selection_equivalent_node_count += node_count;
                } else {
                    selection_unique_node_count += node_count;
                }
                presentation
            }
        };

        match rasterize_wmf_preview(&selected.data, selected.width, selected.height) {
            Ok(_) => raster_success_node_count += node_count,
            Err(_) => raster_rejected_node_count += node_count,
        }
    }

    let viewer_preview_node_ids = visual
        .images
        .iter()
        .flat_map(|image| image.node_ids.iter().copied())
        .filter(|node_id| admitted_node_ids.contains(node_id))
        .collect::<BTreeSet<_>>();
    let viewer_preview_resource_count = visual
        .images
        .iter()
        .filter(|image| {
            image
                .node_ids
                .iter()
                .any(|node_id| admitted_node_ids.contains(node_id))
        })
        .count();
    let admitted_node_count = admitted_node_ids.len();
    let viewer_preview_node_count = viewer_preview_node_ids.len();

    json!({
        "schema": "chaptera.legacy-ole-preview-funnel.v1",
        "applicable": true,
        "available": true,
        "admitted_node_count": admitted_node_count,
        "admitted_storage_count": uses_by_storage.len(),
        "scan_success_node_count": scan_success_node_count,
        "scan_failure_node_count": scan_failure_node_count,
        "valid_candidate_count": valid_candidate_count,
        "rejected_sibling_count": rejected_sibling_count,
        "selection_none_node_count": selection_none_node_count,
        "selection_unique_node_count": selection_unique_node_count,
        "selection_equivalent_node_count": selection_equivalent_node_count,
        "selection_ambiguous_node_count": selection_ambiguous_node_count,
        "raster_success_node_count": raster_success_node_count,
        "raster_rejected_node_count": raster_rejected_node_count,
        "viewer_preview_resource_count": viewer_preview_resource_count,
        "viewer_preview_node_count": viewer_preview_node_count,
        "viewer_preview_unavailable_node_count": admitted_node_count.saturating_sub(viewer_preview_node_count),
    })
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
            let legacy_object_residuals = legacy_object_residual_census(
                &bytes,
                visual.document.source.source_hash,
                visual.document.source.format_version.as_deref(),
            )?;
            let legacy_ole_preview_funnel = legacy_ole_preview_funnel(
                &bytes,
                visual.document.source.source_hash,
                visual.document.source.format_version.as_deref(),
                &visual,
            );

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
                "legacy_object_residuals": legacy_object_residuals,
                "legacy_ole_preview_funnel": legacy_ole_preview_funnel,
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
