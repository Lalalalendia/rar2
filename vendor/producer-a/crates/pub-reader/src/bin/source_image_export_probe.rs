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
    let asset_manifest = build_pub_asset_manifest(&source.graph, &escher, &delayed)?;
    let asset_catalog = build_pub_image_resource_catalog(&source.graph, &asset_manifest)?;
    let asset_states = asset_manifest
        .assets
        .iter()
        .map(|asset| {
            json!({
                "slot": asset.slot,
                "use_count": asset.uses.len(),
                "blip_kind": asset.blip_kind,
                "blip_record_present": asset.blip_record_source.is_some(),
                "image_payload_present": asset.image_payload_source.is_some(),
                "payload_sha256_present": asset.payload_sha256.is_some(),
                "payload_len": asset.payload_len,
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
