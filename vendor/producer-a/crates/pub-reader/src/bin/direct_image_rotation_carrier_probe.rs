use pub_core::StreamPath;
use pub_escher::{PUBLISHER_FIELD_SHAPE_ID, inspect_sp_containers};
use pub_model::Sha256Digest;
use pub_reader::{
    ESCHER_DELAY_STREAM_PATH, ESCHER_STREAM_PATH, build_mature_0x2c_source_graph,
    build_pub_asset_manifest,
};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, env, error::Error, fs, io::Cursor};

const ROTATION_PROPERTY_ID: u16 = 0x0004;
const FULL_TURN_UNITS: i64 = 360 * 65_536;
const HALF_TURN_UNITS: i64 = 180 * 65_536;

fn source_hash(bytes: &[u8]) -> Sha256Digest {
    let digest = Sha256::digest(bytes);
    let mut raw = [0_u8; 32];
    raw.copy_from_slice(&digest);
    Sha256Digest::from_bytes(raw)
}

fn normalized_rotation_units(op: u32) -> i64 {
    let mut angle = i64::from(op as i32) % FULL_TURN_UNITS;
    if angle > HALF_TURN_UNITS {
        angle -= FULL_TURN_UNITS;
    } else if angle < -HALF_TURN_UNITS {
        angle += FULL_TURN_UNITS;
    }
    angle
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = env::args().skip(1);
    let input = args
        .next()
        .ok_or("usage: direct_image_rotation_carrier_probe INPUT.pub LABEL")?;
    let label = args
        .next()
        .ok_or("usage: direct_image_rotation_carrier_probe INPUT.pub LABEL")?;

    let bytes = fs::read(&input)?;
    let hash = source_hash(&bytes);
    let source = build_mature_0x2c_source_graph(Cursor::new(bytes.as_slice()), hash.clone())?;
    let escher = pub_cfb::read_stream_reader(Cursor::new(bytes.as_slice()), ESCHER_STREAM_PATH)?;
    let inventory = inspect_sp_containers(StreamPath(ESCHER_STREAM_PATH.to_owned()), &escher)?;

    let mut shapes_by_seq = BTreeMap::<u32, Vec<usize>>::new();
    for (index, shape) in inventory.shapes.iter().enumerate() {
        let Some(client_data) = shape.client_data.as_ref() else {
            continue;
        };
        let mut seqs = client_data
            .fields
            .iter()
            .filter(|field| field.id == PUBLISHER_FIELD_SHAPE_ID)
            .map(|field| field.value)
            .collect::<Vec<_>>();
        seqs.sort_unstable();
        seqs.dedup();
        for seq in seqs {
            shapes_by_seq.entry(seq).or_default().push(index);
        }
    }

    let cfb_inventory = pub_cfb::inspect_reader(Cursor::new(bytes.as_slice()))?;
    let delayed = if cfb_inventory
        .entries
        .iter()
        .any(|entry| entry.path == ESCHER_DELAY_STREAM_PATH)
    {
        pub_cfb::read_stream_reader(Cursor::new(bytes.as_slice()), ESCHER_DELAY_STREAM_PATH)?
    } else {
        Vec::new()
    };
    let manifest = build_pub_asset_manifest(&source.graph, &escher, &delayed)?;
    let assets_by_slot = manifest
        .assets
        .iter()
        .map(|asset| (asset.slot, asset))
        .collect::<BTreeMap<_, _>>();

    let mut rows = Vec::new();
    for node in source
        .graph
        .nodes
        .values()
        .filter(|node| node.payload.image_slot.is_some())
        .filter(|node| {
            !node.header.source_refs.iter().any(|source_ref| {
                source_ref
                    .object_key
                    .as_deref()
                    .is_some_and(|key| key.starts_with("escher/group-ancestor/"))
            })
        })
    {
        let matches = shapes_by_seq
            .get(&node.payload.contents_seq_num)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let [shape_index] = matches else {
            continue;
        };
        let shape = &inventory.shapes[*shape_index];
        let rotation_properties = shape
            .fopts
            .iter()
            .flat_map(|record| {
                record
                    .properties
                    .iter()
                    .filter(|property| property.property_id() == ROTATION_PROPERTY_ID)
                    .map(move |property| (record.rec_type, property))
            })
            .collect::<Vec<_>>();
        let [(rotation_rec_type, rotation)] = rotation_properties.as_slice() else {
            continue;
        };
        if rotation.f_bid() || rotation.f_complex() {
            continue;
        }
        let signed_rotation_units = normalized_rotation_units(rotation.op);
        if signed_rotation_units == 0
            || signed_rotation_units.abs() == 90 * 65_536
            || signed_rotation_units.abs() == 180 * 65_536
        {
            continue;
        }

        let mut fopt_profiles = shape
            .fopts
            .iter()
            .map(|record| {
                let mut property_ids = record
                    .properties
                    .iter()
                    .map(|property| property.property_id())
                    .collect::<Vec<_>>();
                property_ids.sort_unstable();
                property_ids.dedup();
                json!({
                    "rec_type": record.rec_type,
                    "property_ids": property_ids,
                })
            })
            .collect::<Vec<_>>();
        fopt_profiles.sort_by_key(|row| row["rec_type"].as_u64());

        let mut client_anchor_field_ids = shape
            .client_anchor
            .as_ref()
            .map(|anchor| anchor.fields.iter().map(|field| field.id).collect::<Vec<_>>())
            .unwrap_or_default();
        client_anchor_field_ids.sort_unstable();
        client_anchor_field_ids.dedup();

        let slot = node.payload.image_slot.expect("filtered image slot");
        let asset = assets_by_slot.get(&slot).copied();
        rows.push(json!({
            "node_id": node.header.id.as_canonical().to_string(),
            "page_id": node.header.parent_id.to_string(),
            "contents_seq_num": node.payload.contents_seq_num,
            "image_slot": slot,
            "rotation_rec_type": rotation_rec_type,
            "raw_rotation_op": rotation.op,
            "signed_rotation_units": signed_rotation_units,
            "shape_type": shape.fsp.as_ref().map(|fsp| fsp.shape_type),
            "fsp_flags": shape.fsp.as_ref().map(|fsp| fsp.flags),
            "fopt_profiles": fopt_profiles,
            "client_anchor_field_ids": client_anchor_field_ids,
            "child_anchor_present": shape.child_anchor.is_some(),
            "crop_present": node.payload.explicit_image_crop.is_some(),
            "recolor_present": node.payload.explicit_image_recolor.is_some(),
            "asset_kind": asset
                .and_then(|asset| asset.blip_kind)
                .map(|kind| format!("{kind:?}")),
            "asset_payload_exact": asset.is_some_and(|asset| {
                asset.payload_sha256.is_some() && asset.image_payload_source.is_some()
            }),
        }));
    }

    rows.sort_by_key(|row| {
        (
            row["page_id"].as_str().unwrap_or_default().to_owned(),
            row["contents_seq_num"].as_u64().unwrap_or_default(),
        )
    });
    eprintln!(
        "DIRECT_IMAGE_ROTATION_CARRIER_PROFILE {}",
        serde_json::to_string(&json!({
            "label": label,
            "source_sha256": hash.to_string(),
            "non_cardinal_direct_image_count": rows.len(),
            "rows": rows,
        }))?
    );
    Ok(())
}
