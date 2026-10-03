use anyhow::{Context, Result, bail};
use pub_model::{CanonicalId, NodeId, NodeKind, PageId, Sha256Digest};
#[cfg(feature = "cmo-slot-compose")]
use chaptera_scene_instance::SceneProjectionKindV1;
use pub_reader::{
    PubPageRoleObservationReceipt, PubSourceGraph, analyze_mature_0x2c_page_roles,
    build_mature_0x2c_source_graph,
};
use pub_viewer::{open_mature_0x2c_geometry, viewer_geometry_environment_v0_1};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    fs,
    io::Cursor,
};

const SCHEMA: &str = "chaptera.mature-033-object-ownership-crosswalk.v1";

fn fingerprint_page(page_id: PageId) -> String {
    Sha256::digest(page_id.as_canonical().as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn node_kind_name(kind: NodeKind) -> &'static str {
    match kind {
        NodeKind::Shape => "shape",
        NodeKind::TextFrame => "text_frame",
        NodeKind::ImageFrame => "image_frame",
        NodeKind::VectorPath => "vector_path",
        NodeKind::Group => "group",
        NodeKind::Connector => "connector",
        NodeKind::Table => "table",
        NodeKind::PlacedArtifact => "placed_artifact",
        NodeKind::Unsupported => "unsupported",
    }
}

fn source_page_for_node(graph: &PubSourceGraph, node_id: NodeId) -> Option<PageId> {
    let mut current = graph.nodes.get(&node_id)?.header.parent_id;
    let mut seen = BTreeSet::<CanonicalId>::new();
    loop {
        if !seen.insert(current) {
            return None;
        }

        let page_id = PageId::from_canonical(current);
        if graph.pages.contains_key(&page_id) {
            return Some(page_id);
        }

        let parent_node_id = NodeId::from_canonical(current);
        current = graph.nodes.get(&parent_node_id)?.header.parent_id;
    }
}

fn increment(map: &mut BTreeMap<String, usize>, key: impl Into<String>) {
    *map.entry(key.into()).or_insert(0) += 1;
}

fn raw_type_histogram(
    values: &BTreeMap<u16, usize>,
) -> BTreeMap<String, usize> {
    values
        .iter()
        .map(|(raw_type, count)| (format!("0x{raw_type:02X}"), *count))
        .collect()
}

fn role_maps(
    receipt: &PubPageRoleObservationReceipt,
) -> Result<(
    BTreeMap<u32, usize>,
    BTreeMap<usize, usize>,
)> {
    let mut seq_to_ordinal = BTreeMap::new();
    for page in &receipt.pages {
        if seq_to_ordinal
            .insert(page.contents_seq_num, page.document_ordinal)
            .is_some()
        {
            bail!("duplicate PAGE Contents seqNum in role receipt");
        }
    }

    let mut incoming_master_refs = BTreeMap::<usize, usize>::new();
    for page in &receipt.pages {
        let Some(master_seq) = page.applied_master_seq_num else {
            continue;
        };
        let Some(master_ordinal) = seq_to_ordinal.get(&master_seq).copied() else {
            continue;
        };
        *incoming_master_refs.entry(master_ordinal).or_insert(0) += 1;
    }

    Ok((seq_to_ordinal, incoming_master_refs))
}

fn main() -> Result<()> {
    let mut args = env::args_os().skip(1);
    let source_path = args
        .next()
        .context("usage: page-object-ownership-receipt SOURCE.pub OUTPUT.json")?;
    let output_path = args
        .next()
        .context("usage: page-object-ownership-receipt SOURCE.pub OUTPUT.json")?;
    if args.next().is_some() {
        bail!("unexpected extra arguments");
    }

    let bytes = fs::read(&source_path)
        .with_context(|| format!("read {:?}", source_path))?;
    let digest = Sha256::digest(&bytes);
    let mut source_hash_bytes = [0_u8; 32];
    source_hash_bytes.copy_from_slice(&digest);
    let source_hash = Sha256Digest::from_bytes(source_hash_bytes);

    let roles = analyze_mature_0x2c_page_roles(Cursor::new(bytes.as_slice()))
        .context("build existing PAGE-role observation")?;
    let source = build_mature_0x2c_source_graph(
        Cursor::new(bytes.as_slice()),
        source_hash,
    )
    .context("build existing mature SourceGraph")?;
    let viewer = open_mature_0x2c_geometry(
        &bytes,
        viewer_geometry_environment_v0_1(),
    )
    .context("open through current Viewer scene")?;

    let page_ids = source.graph.document.pages.clone();
    if page_ids.len() != roles.pages.len() {
        bail!(
            "SourceGraph PAGE count {} does not match role receipt {}",
            page_ids.len(),
            roles.pages.len()
        );
    }
    let selected_viewer_page_ids = viewer
        .document
        .pages
        .iter()
        .map(|page| page.id)
        .collect::<BTreeSet<_>>();
    if !selected_viewer_page_ids
        .iter()
        .all(|page_id| source.graph.pages.contains_key(page_id))
    {
        bail!("Viewer selected PAGE identity is absent from SourceGraph");
    }

    let page_to_ordinal = page_ids
        .iter()
        .enumerate()
        .map(|(ordinal, page_id)| (*page_id, ordinal))
        .collect::<BTreeMap<_, _>>();
    let (seq_to_ordinal, incoming_master_refs) = role_maps(&roles)?;

    let mut source_nodes_by_page = BTreeMap::<usize, BTreeSet<NodeId>>::new();
    let mut source_kind_counts = BTreeMap::<usize, BTreeMap<String, usize>>::new();
    let mut source_payload_counts = BTreeMap::<usize, BTreeMap<String, usize>>::new();
    let mut source_unowned_node_count = 0_usize;

    for (node_id, node) in &source.graph.nodes {
        let Some(page_id) = source_page_for_node(&source.graph, *node_id) else {
            source_unowned_node_count += 1;
            continue;
        };
        let Some(ordinal) = page_to_ordinal.get(&page_id).copied() else {
            source_unowned_node_count += 1;
            continue;
        };

        source_nodes_by_page.entry(ordinal).or_default().insert(*node_id);
        increment(
            source_kind_counts.entry(ordinal).or_default(),
            node_kind_name(node.kind),
        );
        let payload = source_payload_counts.entry(ordinal).or_default();
        if node.payload.story_frame.is_some() {
            increment(payload, "story_frame");
        }
        if node.payload.image_slot.is_some() {
            increment(payload, "image_slot");
        }
        if node.payload.table.is_some() {
            increment(payload, "table");
        }
        if node.payload.legacy_ole.is_some() {
            increment(payload, "legacy_ole");
        }
        if node.payload.effective_paint.is_some() {
            increment(payload, "effective_paint");
        }
    }

    let paint_order_counts = source
        .source_page_paint_orders
        .iter()
        .filter_map(|order| {
            page_to_ordinal
                .get(&order.page_id)
                .copied()
                .map(|ordinal| (ordinal, order.node_ids.len()))
        })
        .collect::<BTreeMap<_, _>>();

    let scene_node_ids = viewer
        .scene
        .nodes
        .iter()
        .map(|node| node.origin)
        .collect::<BTreeSet<_>>();

    #[cfg(feature = "cmo-slot-compose")]
    let mut projected_instance_counts_by_origin = BTreeMap::<NodeId, usize>::new();
    #[cfg(feature = "cmo-slot-compose")]
    let mut inherited_instance_counts_by_origin = BTreeMap::<NodeId, usize>::new();
    #[cfg(feature = "cmo-slot-compose")]
    for instance in &viewer.projected_instances {
        let canonical: CanonicalId = instance
            .scene_instance
            .origin_node_id
            .parse()
            .context("parse projected SceneInstance origin_node_id")?;
        let origin_node_id = NodeId::from_canonical(canonical);
        *projected_instance_counts_by_origin
            .entry(origin_node_id)
            .or_insert(0) += 1;
        if instance.scene_instance.projection_kind == SceneProjectionKindV1::InheritedMaster {
            *inherited_instance_counts_by_origin
                .entry(origin_node_id)
                .or_insert(0) += 1;
        }
    }
    #[cfg(not(feature = "cmo-slot-compose"))]
    let projected_instance_counts_by_origin = BTreeMap::<NodeId, usize>::new();
    #[cfg(not(feature = "cmo-slot-compose"))]
    let inherited_instance_counts_by_origin = BTreeMap::<NodeId, usize>::new();

    let projected_origin_ids = projected_instance_counts_by_origin
        .keys()
        .copied()
        .collect::<BTreeSet<_>>();
    let projected_unknown_origin_count = projected_origin_ids
        .iter()
        .filter(|node_id| !source.graph.nodes.contains_key(*node_id))
        .count();

    let viewer_paint_ids = viewer
        .paints
        .iter()
        .map(|paint| paint.node_id)
        .collect::<BTreeSet<_>>();
    let viewer_story_frame_ids = viewer
        .story_frames
        .iter()
        .map(|frame| frame.frame_id)
        .collect::<BTreeSet<_>>();
    let viewer_text_fragment_ids = viewer
        .text_fragments
        .iter()
        .map(|fragment| fragment.frame_id)
        .collect::<BTreeSet<_>>();
    let viewer_table_ids = viewer
        .tables
        .iter()
        .map(|table| table.node_id)
        .collect::<BTreeSet<_>>();
    let viewer_image_ids = viewer
        .images
        .iter()
        .flat_map(|image| image.node_ids.iter().copied())
        .collect::<BTreeSet<_>>();

    let mut unknown_scene_origin_count = 0_usize;
    for node in &viewer.scene.nodes {
        if !source.graph.nodes.contains_key(&node.origin) {
            unknown_scene_origin_count += 1;
        }
    }

    let role_by_ordinal = roles
        .pages
        .iter()
        .map(|page| (page.document_ordinal, page))
        .collect::<BTreeMap<_, _>>();

    #[cfg(feature = "cmo-slot-compose")]
    let projected_instance_count = viewer.projected_instances.len();
    #[cfg(not(feature = "cmo-slot-compose"))]
    let projected_instance_count = 0_usize;

    #[cfg(feature = "cmo-slot-compose")]
    let inherited_master_projected_instance_count = viewer
        .projected_instances
        .iter()
        .filter(|instance| {
            instance.scene_instance.projection_kind == SceneProjectionKindV1::InheritedMaster
        })
        .count();
    #[cfg(not(feature = "cmo-slot-compose"))]
    let inherited_master_projected_instance_count = 0_usize;

    let mut page_rows = Vec::<Value>::new();
    for (ordinal, page_id) in page_ids.iter().copied().enumerate() {
        let role = role_by_ordinal
            .get(&ordinal)
            .copied()
            .context("missing PAGE role row for SourceGraph ordinal")?;
        let master_relation = role
            .applied_master_seq_num
            .map(|seq| {
                seq_to_ordinal
                    .get(&seq)
                    .copied()
                    .map(|target| format!("page_ordinal:{target}"))
                    .unwrap_or_else(|| "external_or_unresolved".to_owned())
            })
            .unwrap_or_else(|| "none".to_owned());
        let oid_class = match (role.oid_dword0, role.oid_dword1) {
            (Some(0), Some(0)) => "zero",
            (Some(_), Some(_)) => "nonzero",
            _ => "unknown",
        };
        let master_ordinal = role
            .applied_master_seq_num
            .and_then(|seq| seq_to_ordinal.get(&seq).copied());
        let master_source_node_count = master_ordinal
            .and_then(|master| source_nodes_by_page.get(&master).map(BTreeSet::len))
            .unwrap_or(0);
        let master_source_kind_counts = master_ordinal
            .and_then(|master| source_kind_counts.get(&master).cloned())
            .unwrap_or_default();

        #[cfg(feature = "cmo-slot-compose")]
        let inherited_instances_targeting_page = viewer
            .projected_instances
            .iter()
            .filter(|instance| {
                instance.scene_instance.projection_kind == SceneProjectionKindV1::InheritedMaster
                    && instance.scene_instance.target_page_id
                        == page_id.as_canonical().to_string()
            })
            .count();
        #[cfg(not(feature = "cmo-slot-compose"))]
        let inherited_instances_targeting_page = 0_usize;

        let source_ids = source_nodes_by_page
            .get(&ordinal)
            .cloned()
            .unwrap_or_default();
        let scene_ids = source_ids
            .iter()
            .copied()
            .filter(|node_id| scene_node_ids.contains(node_id))
            .collect::<BTreeSet<_>>();
        let projected_origin_ids_for_page = source_ids
            .iter()
            .copied()
            .filter(|node_id| projected_origin_ids.contains(node_id))
            .collect::<BTreeSet<_>>();
        let represented_source_ids = scene_ids
            .union(&projected_origin_ids_for_page)
            .copied()
            .collect::<BTreeSet<_>>();
        let unrepresented_source_ids = source_ids
            .difference(&represented_source_ids)
            .copied()
            .collect::<BTreeSet<_>>();
        let projected_instance_count_from_source_page = source_ids
            .iter()
            .map(|node_id| {
                projected_instance_counts_by_origin
                    .get(node_id)
                    .copied()
                    .unwrap_or(0)
            })
            .sum::<usize>();
        let inherited_instance_count_from_source_page = source_ids
            .iter()
            .map(|node_id| {
                inherited_instance_counts_by_origin
                    .get(node_id)
                    .copied()
                    .unwrap_or(0)
            })
            .sum::<usize>();

        let scene_kind_counts = scene_ids
            .iter()
            .filter_map(|node_id| source.graph.nodes.get(node_id))
            .fold(BTreeMap::<String, usize>::new(), |mut counts, node| {
                increment(&mut counts, node_kind_name(node.kind));
                counts
            });

        let count_ids = |allowed: &BTreeSet<NodeId>| -> usize {
            source_ids.intersection(allowed).count()
        };

        page_rows.push(json!({
            "source_document_ordinal": ordinal,
            "page_identity_fingerprint_sha256": fingerprint_page(page_id),
            "selected_by_current_viewer": selected_viewer_page_ids.contains(&page_id),
            "role": {
                "oid_class": oid_class,
                "pgt_type": role.pgt_type,
                "master_relation": master_relation,
                "master_in_degree": incoming_master_refs.get(&ordinal).copied().unwrap_or(0),
                "direct_child_raw_type_counts": raw_type_histogram(&role.child_raw_type_counts),
                "master_source_node_count": master_source_node_count,
                "master_source_node_kind_counts": master_source_kind_counts,
            },
            "source_graph": {
                "node_count": source_ids.len(),
                "node_kind_counts": source_kind_counts.get(&ordinal).cloned().unwrap_or_default(),
                "payload_feature_counts": source_payload_counts.get(&ordinal).cloned().unwrap_or_default(),
                "paint_order_available": paint_order_counts.contains_key(&ordinal),
                "paint_order_node_count": paint_order_counts.get(&ordinal).copied().unwrap_or(0),
            },
            "viewer_projection": {
                "canonical_scene_node_count": scene_ids.len(),
                "projected_origin_node_count": projected_origin_ids_for_page.len(),
                "projected_instance_count_from_source_page": projected_instance_count_from_source_page,
                "inherited_master_instance_count_from_source_page": inherited_instance_count_from_source_page,
                "represented_source_node_count": represented_source_ids.len(),
                "unrepresented_source_node_count": unrepresented_source_ids.len(),
                "scene_node_kind_counts": scene_kind_counts,
                "paint_node_count": count_ids(&viewer_paint_ids),
                "story_frame_node_count": count_ids(&viewer_story_frame_ids),
                "text_fragment_frame_count": count_ids(&viewer_text_fragment_ids),
                "image_node_count": count_ids(&viewer_image_ids),
                "table_node_count": count_ids(&viewer_table_ids),
                "inherited_master_instance_count": inherited_instances_targeting_page,
            },
        }));
    }

    let total_source_nodes = source_nodes_by_page
        .values()
        .map(BTreeSet::len)
        .sum::<usize>();
    let total_source_scene_nodes = source_nodes_by_page
        .values()
        .flat_map(|nodes| nodes.iter())
        .filter(|node_id| scene_node_ids.contains(node_id))
        .count();

    let receipt = json!({
        "schema": SCHEMA,
        "source_sha256": source_hash.to_string(),
        "page_count": page_ids.len(),
        "viewer_selected_page_count": selected_viewer_page_ids.len(),
        "source_graph_total_node_count": source.graph.nodes.len(),
        "source_graph_page_owned_node_count": total_source_nodes,
        "source_graph_unowned_node_count": source_unowned_node_count,
        "viewer_scene_total_node_count": viewer.scene.nodes.len(),
        "viewer_scene_known_source_node_count": total_source_scene_nodes,
        "viewer_scene_unknown_origin_count": unknown_scene_origin_count,
        "viewer_projected_known_source_origin_node_count": projected_origin_ids
            .iter()
            .filter(|node_id| source.graph.nodes.contains_key(*node_id))
            .count(),
        "viewer_projected_unknown_origin_node_count": projected_unknown_origin_count,
        "projected_instance_count": projected_instance_count,
        "inherited_master_projected_instance_count": inherited_master_projected_instance_count,
        "pages": page_rows,
        "claims": {
            "measurement_only": true,
            "new_source_parser_semantics_added": false,
            "source_graph_mutated": false,
            "viewer_semantics_changed": false,
            "publisher_pdf_or_raster_used": false,
            "raw_page_id_emitted": false,
            "raw_node_id_emitted": false,
            "raw_contents_seq_num_emitted": false,
            "raw_oid_values_emitted": false,
            "story_text_emitted": false,
            "page_and_node_identity_join_uses_canonical_ids_in_memory_only": true,
            "source_page_universe_is_not_collapsed_to_current_viewer_selection": true,
            "source_node_representation_checks_canonical_and_projected_instances": true,
        },
    });

    fs::write(
        &output_path,
        serde_json::to_vec_pretty(&receipt).context("serialize object-ownership receipt")?,
    )
    .with_context(|| format!("write {:?}", output_path))?;

    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "page_count": page_ids.len(),
            "viewer_selected_page_count": selected_viewer_page_ids.len(),
            "source_graph_total_node_count": source.graph.nodes.len(),
            "source_graph_page_owned_node_count": total_source_nodes,
            "viewer_scene_total_node_count": viewer.scene.nodes.len(),
            "viewer_scene_known_source_node_count": total_source_scene_nodes,
            "viewer_scene_unknown_origin_count": unknown_scene_origin_count,
            "viewer_projected_known_source_origin_node_count": projected_origin_ids
                .iter()
                .filter(|node_id| source.graph.nodes.contains_key(*node_id))
                .count(),
            "viewer_projected_unknown_origin_node_count": projected_unknown_origin_count,
            "projected_instance_count": projected_instance_count,
            "inherited_master_projected_instance_count": inherited_master_projected_instance_count,
            "pages": receipt["pages"],
        }))?
    );

    Ok(())
}
