use pub_model::Sha256Digest;
use pub_reader::{
    PubBridgeDiagnostic, build_mature_0x2c_asset_export_bundle_from_bytes,
    build_mature_0x2c_source_graph, derive_pub_node_id,
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
    let bundle = build_mature_0x2c_asset_export_bundle_from_bytes(&bytes, &source.graph)?;

    let page_number_by_id = source
        .effective_pages
        .page_ids
        .iter()
        .enumerate()
        .map(|(index, page_id)| (*page_id, index + 1))
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut grouped_image_projection_failures = source
        .diagnostics
        .iter()
        .filter_map(|diagnostic| match diagnostic {
            PubBridgeDiagnostic::GroupedImageProjectionUnavailable {
                seq_num,
                target_page_id,
                image_slot,
                group_ancestry,
                reason,
            } => Some(json!({
                "node_id": derive_pub_node_id(&hash, *seq_num).ok()?.as_canonical().to_string(),
                "seq_num": seq_num,
                "target_page_id": target_page_id.map(|page| page.as_canonical().to_string()),
                "target_page_number": target_page_id
                    .as_ref()
                    .and_then(|page| page_number_by_id.get(page).copied()),
                "image_slot": image_slot,
                "group_depth_to_page": group_ancestry.len(),
                "group_ancestry": group_ancestry,
                "reason": reason,
            })),
            _ => None,
        })
        .collect::<Vec<_>>();
    grouped_image_projection_failures.sort_by(|left, right| {
        left["target_page_number"]
            .as_u64()
            .cmp(&right["target_page_number"].as_u64())
            .then_with(|| left["seq_num"].as_u64().cmp(&right["seq_num"].as_u64()))
    });

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
            "grouped_image_projection_failures": grouped_image_projection_failures,
        }))?
    );
    Ok(())
}
