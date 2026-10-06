use pub_contents::{
    parse_0x2c_header, parse_confirmed_0x2c_trailer_root, parse_confirmed_chunk_reference,
};
use pub_core::StreamPath;
use pub_escher::{
    inspect_sp_containers, PUBLISHER_FIELD_SHAPE_ID, PUBLISHER_FIELD_XE, PUBLISHER_FIELD_XS,
    PUBLISHER_FIELD_YE, PUBLISHER_FIELD_YS,
};
use pub_model::Sha256Digest;
use pub_reader::{
    build_mature_0x2c_source_graph, PubBridgeDiagnostic, CONTENTS_STREAM_PATH,
    ESCHER_STREAM_PATH,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, env, error::Error, fs, io::Cursor};

fn source_hash(bytes: &[u8]) -> Sha256Digest {
    let digest = Sha256::digest(bytes);
    let mut raw = [0_u8; 32];
    raw.copy_from_slice(&digest);
    Sha256Digest::from_bytes(raw)
}

fn diagnostic_summary(diagnostic: &PubBridgeDiagnostic) -> Option<Value> {
    use PubBridgeDiagnostic::*;

    match diagnostic {
        MissingEscherGeometry { seq_num } => {
            Some(json!({"code": "missing_escher_geometry", "seq_num": seq_num}))
        }
        AmbiguousEscherGeometry { seq_num, matches } => Some(json!({
            "code": "ambiguous_escher_geometry",
            "seq_num": seq_num,
            "matches": matches,
        })),
        AmbiguousImageSlot { seq_num, slots } => Some(json!({
            "code": "ambiguous_image_slot",
            "seq_num": seq_num,
            "slot_count": slots.len(),
        })),
        IncompleteEscherAnchor { seq_num } => {
            Some(json!({"code": "incomplete_escher_anchor", "seq_num": seq_num}))
        }
        InvalidEscherAnchor { seq_num } => {
            Some(json!({"code": "invalid_escher_anchor", "seq_num": seq_num}))
        }
        GroupedImageProjected { seq_num, depth } => Some(json!({
            "code": "grouped_image_projected",
            "seq_num": seq_num,
            "depth": depth,
        })),
        GroupedImageProjectionUnavailable { seq_num, .. } => Some(json!({
            "code": "grouped_image_projection_unavailable",
            "seq_num": seq_num,
        })),
        GroupedStoryProjected { seq_num, depth } => Some(json!({
            "code": "grouped_story_projected",
            "seq_num": seq_num,
            "depth": depth,
        })),
        GroupedStoryProjectionUnavailable { seq_num, .. } => Some(json!({
            "code": "grouped_story_projection_unavailable",
            "seq_num": seq_num,
        })),
        GroupedTableProjected { seq_num, depth } => Some(json!({
            "code": "grouped_table_projected",
            "seq_num": seq_num,
            "depth": depth,
        })),
        GroupedTableProjectionUnavailable { seq_num, .. } => Some(json!({
            "code": "grouped_table_projection_unavailable",
            "seq_num": seq_num,
        })),
        _ => None,
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let input = env::args()
        .nth(1)
        .ok_or("usage: residual_object_probe INPUT.pub")?;
    let bytes = fs::read(&input)?;
    let hash = source_hash(&bytes);
    let source = build_mature_0x2c_source_graph(Cursor::new(&bytes), hash)?;

    let contents_bytes =
        pub_cfb::read_stream_reader(Cursor::new(&bytes), CONTENTS_STREAM_PATH)?;
    let contents_stream = StreamPath(CONTENTS_STREAM_PATH.into());
    let contents_header = parse_0x2c_header(contents_stream.clone(), &contents_bytes)?;
    let contents_trailer =
        parse_confirmed_0x2c_trailer_root(&contents_bytes, &contents_header)?;
    let mut contents_references = Vec::new();
    for seq_num in 0..contents_trailer.directory.slots.len() {
        let Some(reference) = parse_confirmed_chunk_reference(
            &contents_bytes,
            &contents_trailer.directory,
            seq_num,
        )? else {
            continue;
        };
        contents_references.push(json!({
            "seq_num": seq_num,
            "raw_types": reference
                .raw_types
                .iter()
                .map(|field| field.value)
                .collect::<Vec<_>>(),
            "parent_seq_nums": reference
                .parent_seq_nums
                .iter()
                .map(|field| field.value)
                .collect::<Vec<_>>(),
        }));
    }

    let escher_bytes =
        pub_cfb::read_stream_reader(Cursor::new(&bytes), ESCHER_STREAM_PATH)?;
    let escher_inventory = inspect_sp_containers(
        StreamPath(ESCHER_STREAM_PATH.into()),
        &escher_bytes,
    )?;
    let mut raw_escher_shapes = escher_inventory
        .shapes
        .iter()
        .map(|shape| {
            let publisher_shape_ids = shape
                .client_data
                .as_ref()
                .map(|record| record.values(PUBLISHER_FIELD_SHAPE_ID).collect::<Vec<_>>())
                .unwrap_or_default();
            let rotation_properties = shape
                .fopts
                .iter()
                .flat_map(|record| record.properties.iter())
                .filter(|property| property.property_id() == 0x0004)
                .map(|property| {
                    json!({
                        "op_u32": property.op,
                        "signed_16_16_raw": property.op as i32,
                        "f_bid": property.f_bid(),
                        "f_complex": property.f_complex(),
                    })
                })
                .collect::<Vec<_>>();
            let blip_properties = shape
                .fopts
                .iter()
                .flat_map(|record| record.properties.iter())
                .filter(|property| property.op_is_blip_id())
                .map(|property| {
                    json!({
                        "property_id": property.property_id(),
                        "slot": property.op,
                    })
                })
                .collect::<Vec<_>>();
            let anchor = shape.client_anchor.as_ref().map(|record| {
                json!({
                    "xs": record.values(PUBLISHER_FIELD_XS).collect::<Vec<_>>(),
                    "ys": record.values(PUBLISHER_FIELD_YS).collect::<Vec<_>>(),
                    "xe": record.values(PUBLISHER_FIELD_XE).collect::<Vec<_>>(),
                    "ye": record.values(PUBLISHER_FIELD_YE).collect::<Vec<_>>(),
                })
            });

            json!({
                "publisher_shape_ids": publisher_shape_ids,
                "officeart_spid": shape.fsp.as_ref().map(|fsp| fsp.spid),
                "officeart_shape_type": shape.fsp.as_ref().map(|fsp| fsp.shape_type),
                "fsp_flags": shape.fsp.as_ref().map(|fsp| fsp.flags),
                "grouped": shape.parent_group_shape_source.is_some(),
                "client_textbox_present": shape.client_textbox.is_some(),
                "client_anchor": anchor,
                "child_anchor": shape.child_anchor.as_ref().map(|anchor| json!({
                    "x_left": anchor.x_left,
                    "y_top": anchor.y_top,
                    "x_right": anchor.x_right,
                    "y_bottom": anchor.y_bottom,
                })),
                "rotation_properties": rotation_properties,
                "blip_properties": blip_properties,
            })
        })
        .collect::<Vec<_>>();
    raw_escher_shapes.sort_by(|left, right| {
        left["publisher_shape_ids"]
            .to_string()
            .cmp(&right["publisher_shape_ids"].to_string())
            .then_with(|| left["officeart_spid"].as_u64().cmp(&right["officeart_spid"].as_u64()))
    });

    let mut page_membership = BTreeMap::<String, Vec<String>>::new();
    for (page_id, page) in &source.graph.pages {
        for node_id in &page.children {
            page_membership
                .entry(node_id.as_canonical().to_string())
                .or_default()
                .push(page_id.as_canonical().to_string());
        }
    }

    let mut nodes = source
        .graph
        .nodes
        .iter()
        .map(|(node_id, node)| {
            let canonical_node_id = node_id.as_canonical().to_string();
            json!({
                "node_id": canonical_node_id,
                "page_membership": page_membership
                    .get(&node_id.as_canonical().to_string())
                    .cloned()
                    .unwrap_or_default(),
                "parent_id": node.header.parent_id.to_string(),
                "kind": &node.kind,
                "bounds": &node.header.bounds,
                "transform": &node.header.transform,
                "contents_seq_num": node.payload.contents_seq_num,
                "officeart_shape_type": node.payload.officeart_shape_type,
                "officeart_spid": node.payload.officeart_spid,
                "image_slot": node.payload.image_slot,
                "image_crop_present": node.payload.explicit_image_crop.is_some(),
                "image_crop_ambiguous": node
                    .payload
                    .explicit_image_crop
                    .as_ref()
                    .is_some_and(|crop| crop.ambiguous),
                "image_cardinal_rotation_degrees":
                    node.payload.explicit_image_cardinal_rotation_degrees,
                "image_recolor_present": node.payload.explicit_image_recolor.is_some(),
                "explicit_paint": &node.payload.explicit_paint,
                "effective_paint_present": node.payload.effective_paint.is_some(),
                "story_frame_present": node.payload.story_frame.is_some(),
                "story_id": node
                    .payload
                    .story_frame
                    .as_ref()
                    .and_then(|frame| frame.story_id)
                    .map(|story_id| story_id.as_canonical().to_string()),
                "text_frame_inset_present": node.payload.text_frame_inset.is_some(),
                "table_present": node.payload.table.is_some(),
                "table_story_present": node.payload.table_story.is_some(),
            })
        })
        .collect::<Vec<_>>();
    nodes.sort_by(|left, right| {
        left["contents_seq_num"]
            .as_u64()
            .cmp(&right["contents_seq_num"].as_u64())
            .then_with(|| left["node_id"].as_str().cmp(&right["node_id"].as_str()))
    });

    let mut pages = source
        .graph
        .pages
        .iter()
        .map(|(page_id, page)| {
            json!({
                "page_id": page_id.as_canonical().to_string(),
                "size": &page.size,
                "child_node_ids": page
                    .children
                    .iter()
                    .map(|node_id| node_id.as_canonical().to_string())
                    .collect::<Vec<_>>(),
            })
        })
        .collect::<Vec<_>>();
    pages.sort_by(|left, right| left["page_id"].as_str().cmp(&right["page_id"].as_str()));

    let diagnostics = source
        .diagnostics
        .iter()
        .filter_map(diagnostic_summary)
        .collect::<Vec<_>>();

    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "schema": "chaptera.manual-residual-object-probe.v1",
            "source_sha256": hash.to_string(),
            "source_bytes": bytes.len(),
            "page_count": pages.len(),
            "node_count": nodes.len(),
            "pages": pages,
            "contents_references": contents_references,
            "nodes": nodes,
            "raw_escher_shapes": raw_escher_shapes,
            "relevant_diagnostics": diagnostics,
            "source_page_paint_orders": source.source_page_paint_orders,
            "claims": {
                "story_text_emitted": false,
                "asset_bytes_emitted": false,
                "source_paths_emitted": false,
                "raw_stream_offsets_emitted": false,
            }
        }))?
    );

    Ok(())
}
