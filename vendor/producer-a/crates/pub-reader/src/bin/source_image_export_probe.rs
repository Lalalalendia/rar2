use pub_escher::{inspect_bstore, inspect_delayed_blips, resolve_delayed_blip};
use pub_model::Sha256Digest;
use pub_reader::{
    ESCHER_DELAY_STREAM_PATH, ESCHER_STREAM_PATH, build_mature_0x2c_asset_export_bundle_from_bytes,
    build_mature_0x2c_source_graph, build_pub_asset_manifest, build_pub_image_resource_catalog,
};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{env, error::Error, fs, io::Cursor};

fn source_hash(bytes: &[u8]) -> Sha256Digest {
    let digest = Sha256::digest(bytes);
    let mut raw = [0_u8; 32];
    raw.copy_from_slice(&digest);
    Sha256Digest::from_bytes(raw)
}

fn main() -> Result<(), Box<dyn Error>> {
    let input = env::args()
        .nth(1)
        .ok_or("usage: source_image_export_probe INPUT.pub")?;
    let bytes = fs::read(&input)?;
    let hash = source_hash(&bytes);
    let source = build_mature_0x2c_source_graph(Cursor::new(&bytes), hash)?;

    let escher = pub_cfb::read_stream_reader(Cursor::new(&bytes), ESCHER_STREAM_PATH)?;
    let inventory = pub_cfb::inspect_reader(Cursor::new(&bytes))?;
    let delayed = if inventory
        .entries
        .iter()
        .any(|entry| entry.path == ESCHER_DELAY_STREAM_PATH)
    {
        pub_cfb::read_stream_reader(Cursor::new(&bytes), ESCHER_DELAY_STREAM_PATH)?
    } else {
        Vec::new()
    };
    let bstore = inspect_bstore(pub_core::StreamPath(ESCHER_STREAM_PATH.into()), &escher)?;
    let delayed_inventory = inspect_delayed_blips(
        pub_core::StreamPath(ESCHER_DELAY_STREAM_PATH.into()),
        &delayed,
    )?;
    let asset_manifest = build_pub_asset_manifest(&source.graph, &escher, &delayed)?;
    let asset_catalog = build_pub_image_resource_catalog(&source.graph, &asset_manifest)?;
    let asset_states = asset_manifest
        .assets
        .iter()
        .map(|asset| {
            let signature_probe = asset.blip_record_source.as_ref().and_then(|span| {
                let stream = if span.stream.0 == ESCHER_DELAY_STREAM_PATH {
                    delayed.as_slice()
                } else if span.stream.0 == ESCHER_STREAM_PATH {
                    escher.as_slice()
                } else {
                    return None;
                };
                let start = usize::try_from(span.offset).ok()?;
                let len = usize::try_from(span.len).ok()?;
                let end = start.checked_add(len)?;
                let record = stream.get(start..end)?;
                let png = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
                let jpeg = [0xFF, 0xD8, 0xFF];
                Some(json!({
                    "record_len": record.len(),
                    "png_signature_offset": record.windows(png.len()).position(|window| window == png),
                    "jpeg_signature_offset": record.windows(jpeg.len()).position(|window| window == jpeg),
                }))
            });
            let bstore_slot = bstore.slots.iter().find(|slot| slot.slot == asset.slot);
            let delayed_blip = resolve_delayed_blip(&bstore, &delayed_inventory, asset.slot)
                .ok()
                .flatten();
            json!({
                "slot": asset.slot,
                "use_count": asset.uses.len(),
                "blip_kind": asset.blip_kind,
                "blip_record_present": asset.blip_record_source.is_some(),
                "image_payload_present": asset.image_payload_source.is_some(),
                "payload_sha256_present": asset.payload_sha256.is_some(),
                "payload_len": asset.payload_len,
                "signature_probe": signature_probe,
                "bstore": bstore_slot.map(|slot| json!({
                    "bt_win32": slot.bt_win32,
                    "bt_macos": slot.bt_macos,
                    "size": slot.size,
                    "c_ref": slot.c_ref,
                    "fo_delay": slot.fo_delay,
                    "cb_name": slot.cb_name,
                    "has_delayed_blip": slot.has_delayed_blip(),
                    "embedded_blip_present": slot.embedded_blip_source.is_some(),
                })),
                "delayed_blip": delayed_blip.map(|blip| json!({
                    "rec_type": blip.rec_type,
                    "rec_instance": blip.rec_instance,
                    "record_len": blip.record_source.len,
                    "payload_record_len": blip.payload_source.len,
                    "kind": blip.kind,
                    "image_payload_present": blip.image_payload_source.is_some(),
                })),
            })
        })
        .collect::<Vec<_>>();

    let bundle = build_mature_0x2c_asset_export_bundle_from_bytes(&bytes, &source.graph)?;

    let mut resources = bundle
        .manifest
        .assets
        .into_iter()
        .filter(|entry| matches!(entry.mime.as_str(), "image/png" | "image/jpeg"))
        .map(|entry| {
            let mut uses = entry
                .uses
                .into_iter()
                .map(|usage| {
                    json!({
                        "page_id": usage.page_id.as_canonical().to_string(),
                        "node_id": usage.node_id.as_canonical().to_string(),
                    })
                })
                .collect::<Vec<_>>();
            uses.sort_by(|left, right| {
                left["node_id"]
                    .as_str()
                    .cmp(&right["node_id"].as_str())
                    .then_with(|| left["page_id"].as_str().cmp(&right["page_id"].as_str()))
            });
            json!({
                "resource_id": entry.resource_id.as_canonical().to_string(),
                "mime": entry.mime,
                "sha256": entry.sha256.to_string(),
                "byte_len": entry.byte_len,
                "uses": uses,
            })
        })
        .collect::<Vec<_>>();
    resources.sort_by(|left, right| {
        left["resource_id"]
            .as_str()
            .cmp(&right["resource_id"].as_str())
    });

    let use_count = resources
        .iter()
        .map(|entry| entry["uses"].as_array().map_or(0, Vec::len))
        .sum::<usize>();

    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "schema": "chaptera.editable-source-image-reader-probe.v1",
            "source_sha256": hash.to_string(),
            "source_bytes": bytes.len(),
            "resource_count": resources.len(),
            "use_count": use_count,
            "resources": resources,
            "asset_states": asset_states,
            "asset_manifest_diagnostics": asset_manifest.diagnostics,
            "asset_catalog_diagnostics": asset_catalog.diagnostics,
            "asset_export_diagnostics": bundle.manifest.diagnostics,
        }))?
    );
    Ok(())
}
