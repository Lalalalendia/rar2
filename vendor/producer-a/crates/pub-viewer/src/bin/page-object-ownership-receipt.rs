use anyhow::{Context, Result};
use pub_viewer::{open_pub_bundle, viewer_geometry_environment_v0_1};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::{BTreeMap, BTreeSet}, env, fs};

const SCHEMA: &str = "chaptera.mature-033-page-object-ownership.v1";

fn id_string<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_default()
}

fn page_fingerprint(page_id: &pub_model::PageId) -> String {
    Sha256::digest(page_id.as_canonical().as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn node_kind<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| "unclassified".to_owned())
}

fn main() -> Result<()> {
    let mut args = env::args_os().skip(1);
    let source = args
        .next()
        .context("usage: page-object-ownership-receipt SOURCE.pub OUTPUT.json")?;
    let output = args.next().context("missing output path")?;
    if args.next().is_some() {
        anyhow::bail!("unexpected extra arguments");
    }

    let bytes = fs::read(&source).with_context(|| format!("read {:?}", source))?;
    let source_sha256 = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let bundle = open_pub_bundle(&bytes, viewer_geometry_environment_v0_1())
        .context("open exact PUB through current Viewer bundle")?;
    let graph = &bundle.resolved_graph;

    let document_pages = graph.document.pages.clone();
    let page_ids = document_pages
        .iter()
        .map(|page_id| (id_string(page_id), page_id))
        .collect::<BTreeMap<_, _>>();
    let selected_pages = bundle
        .geometry
        .document
        .pages
        .iter()
        .map(|page| id_string(&page.id))
        .collect::<BTreeSet<_>>();
    let scene_nodes = bundle
        .geometry
        .scene
        .nodes
        .iter()
        .map(|node| id_string(&node.origin))
        .collect::<BTreeSet<_>>();
    let paint_nodes = bundle
        .geometry
        .paints
        .iter()
        .map(|paint| id_string(&paint.node_id))
        .collect::<BTreeSet<_>>();

    let mut node_parent = BTreeMap::<String, String>::new();
    for node in graph.nodes.values() {
        node_parent.insert(
            id_string(&node.header.id),
            id_string(&node.header.parent_id),
        );
    }

    let owner_page = |node_id: &str| -> Option<String> {
        let mut current = node_parent.get(node_id)?.clone();
        let mut seen = BTreeSet::new();
        for _ in 0..=node_parent.len() {
            if page_ids.contains_key(&current) {
                return Some(current);
            }
            if !seen.insert(current.clone()) {
                return None;
            }
            current = node_parent.get(&current)?.clone();
        }
        None
    };

    let mut rows = Vec::new();
    for (document_ordinal, page_id) in document_pages.iter().enumerate() {
        let page_key = id_string(page_id);
        let mut direct_node_count = 0_u64;
        let mut descendant_node_count = 0_u64;
        let mut positive_bounds_count = 0_u64;
        let mut non_positive_bounds_count = 0_u64;
        let mut scene_node_count = 0_u64;
        let mut paint_node_count = 0_u64;
        let mut story_frame_count = 0_u64;
        let mut resolved_story_frame_count = 0_u64;
        let mut table_node_count = 0_u64;
        let mut image_bound_node_count = 0_u64;
        let mut effective_paint_node_count = 0_u64;
        let mut source_ref_count = 0_u64;
        let mut projection_ref_count = 0_u64;
        let mut kind_histogram = BTreeMap::<String, u64>::new();

        for node in graph.nodes.values() {
            let node_id = id_string(&node.header.id);
            if owner_page(&node_id).as_deref() != Some(page_key.as_str()) {
                continue;
            }
            descendant_node_count += 1;
            if id_string(&node.header.parent_id) == page_key {
                direct_node_count += 1;
            }
            let width = node.header.bounds.width.get();
            let height = node.header.bounds.height.get();
            if width > 0 && height > 0 {
                positive_bounds_count += 1;
            } else {
                non_positive_bounds_count += 1;
            }
            if scene_nodes.contains(&node_id) {
                scene_node_count += 1;
            }
            if paint_nodes.contains(&node_id) {
                paint_node_count += 1;
            }
            if let Some(frame) = node.payload.story_frame.as_ref() {
                story_frame_count += 1;
                if frame.story_id.is_some() {
                    resolved_story_frame_count += 1;
                }
            }
            table_node_count += u64::from(node.payload.table.is_some());
            image_bound_node_count += u64::from(node.payload.image_slot.is_some());
            effective_paint_node_count += u64::from(node.payload.effective_paint.is_some());
            source_ref_count += u64::try_from(node.header.source_refs.len()).unwrap_or(u64::MAX);
            projection_ref_count += node
                .header
                .source_refs
                .iter()
                .filter(|reference| {
                    serde_json::to_value(&reference.role)
                        .ok()
                        .as_ref()
                        .and_then(Value::as_str)
                        == Some("projection")
                })
                .count() as u64;
            *kind_histogram.entry(node_kind(&node.kind)).or_default() += 1;
        }

        rows.push(json!({
            "document_ordinal": document_ordinal,
            "page_identity_fingerprint_sha256": page_fingerprint(page_id),
            "selected_by_viewer": selected_pages.contains(&page_key),
            "direct_node_count": direct_node_count,
            "descendant_node_count": descendant_node_count,
            "positive_bounds_count": positive_bounds_count,
            "non_positive_bounds_count": non_positive_bounds_count,
            "scene_node_count": scene_node_count,
            "paint_node_count": paint_node_count,
            "story_frame_count": story_frame_count,
            "resolved_story_frame_count": resolved_story_frame_count,
            "table_node_count": table_node_count,
            "image_bound_node_count": image_bound_node_count,
            "effective_paint_node_count": effective_paint_node_count,
            "source_ref_count": source_ref_count,
            "projection_ref_count": projection_ref_count,
            "node_kind_histogram": kind_histogram,
        }));
    }

    let receipt = json!({
        "schema": SCHEMA,
        "source_sha256": source_sha256,
        "source_byte_len": bytes.len(),
        "document_page_count": document_pages.len(),
        "viewer_page_count": bundle.geometry.document.pages.len(),
        "scene_node_count": bundle.geometry.scene.nodes.len(),
        "paint_node_count": bundle.geometry.paints.len(),
        "pages": rows,
        "claims": {
            "measurement_only": true,
            "raw_page_identity_emitted": false,
            "page_identity_fingerprint_emitted": true,
            "raw_node_identity_emitted": false,
            "raw_contents_seq_num_emitted": false,
            "story_text_emitted": false,
            "source_coordinates_emitted": false,
            "publisher_or_pdf_reference_used": false,
            "raster_reference_used": false,
            "source_graph_mutated": false,
            "viewer_semantics_changed": false,
        },
    });

    fs::write(&output, serde_json::to_vec_pretty(&receipt)?)
        .with_context(|| format!("write {:?}", output))?;
    Ok(())
}
