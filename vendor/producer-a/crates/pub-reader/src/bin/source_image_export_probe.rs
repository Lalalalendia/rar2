use pub_model::Sha256Digest;
use pub_reader::{
    ESCHER_DELAY_STREAM_PATH, ESCHER_STREAM_PATH, PubAssetManifestDiagnostic,
    build_mature_0x2c_asset_export_bundle_from_bytes, build_mature_0x2c_source_graph,
    build_mature_0x2c_wmf_preview_bundle_from_bytes, build_pub_asset_manifest,
};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, env, error::Error, fs, io::Cursor};

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
    if env::var_os("READER_CORPUS_DIAGNOSTIC").is_some() {
        let wmf = build_mature_0x2c_wmf_preview_bundle_from_bytes(&bytes, &source.graph)?;
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
        let source_manifest = build_pub_asset_manifest(&source.graph, &escher, &delayed)?;
        let exact_nodes = bundle
            .manifest
            .assets
            .iter()
            .flat_map(|entry| entry.uses.iter().map(|usage| usage.node_id))
            .collect::<BTreeSet<_>>();
        let wmf_nodes = wmf
            .sources
            .iter()
            .flat_map(|entry| entry.uses.iter().map(|usage| usage.node_id))
            .collect::<BTreeSet<_>>();
        let mut source_uses = source
            .graph
            .nodes
            .values()
            .filter_map(|node| Some((node.header.id, node.payload.image_slot?)))
            .collect::<Vec<_>>();
        source_uses.sort();

        let remainder = source_uses
            .iter()
            .filter(|(node_id, _)| !exact_nodes.contains(node_id) && !wmf_nodes.contains(node_id))
            .map(|(node_id, slot)| {
                json!({
                    "node_id": node_id.as_canonical().to_string(),
                    "slot": slot,
                })
            })
            .collect::<Vec<_>>();
        let remainder_slots = source_uses
            .iter()
            .filter(|(node_id, _)| !exact_nodes.contains(node_id) && !wmf_nodes.contains(node_id))
            .map(|(_, slot)| *slot)
            .collect::<BTreeSet<_>>();
        let remainder_slot_profiles = remainder_slots
            .iter()
            .map(|slot| {
                let asset = source_manifest.assets.iter().find(|asset| asset.slot == *slot);
                let diagnostics = source_manifest
                    .diagnostics
                    .iter()
                    .filter_map(|diagnostic| {
                        let (diagnostic_slot, label) = match diagnostic {
                            PubAssetManifestDiagnostic::MissingBStoreSlot { slot } => {
                                (*slot, "missing_bstore_slot")
                            }
                            PubAssetManifestDiagnostic::EmptyBStoreSlot { slot } => {
                                (*slot, "empty_bstore_slot")
                            }
                            PubAssetManifestDiagnostic::DelayedBlipUnresolved { slot, .. } => {
                                (*slot, "delayed_blip_unresolved")
                            }
                            PubAssetManifestDiagnostic::EmbeddedBlipNotExtracted { slot } => {
                                (*slot, "embedded_blip_not_extracted")
                            }
                            PubAssetManifestDiagnostic::StandardPayloadUnavailable { slot, .. } => {
                                (*slot, "standard_payload_unavailable")
                            }
                        };
                        (diagnostic_slot == *slot).then_some(label)
                    })
                    .collect::<Vec<_>>();
                json!({
                    "slot": slot,
                    "blip_kind": asset
                        .and_then(|asset| asset.blip_kind)
                        .map(|kind| format!("{kind:?}")),
                    "payload_exact": asset.is_some_and(|asset| {
                        asset.payload_sha256.is_some() && asset.image_payload_source.is_some()
                    }),
                    "blip_record_present": asset.is_some_and(|asset| asset.blip_record_source.is_some()),
                    "diagnostics": diagnostics,
                })
            })
            .collect::<Vec<_>>();
        let exact_only = source_uses
            .iter()
            .filter(|(node_id, _)| exact_nodes.contains(node_id))
            .count();
        let wmf_only = source_uses
            .iter()
            .filter(|(node_id, _)| wmf_nodes.contains(node_id))
            .count();
        eprintln!(
            "IMAGE_RESOURCE_ADMISSION {}",
            serde_json::to_string(&json!({
                "source_image_uses": source_uses.len(),
                "exact_resource_uses": exact_only,
                "wmf_preview_uses": wmf_only,
                "wmf_physical_records": wmf.physical_wmf_record_count,
                "wmf_live_slots": wmf.live_bstore_wmf_slot_count,
                "wmf_rejected_sources": wmf.rejected_source_count,
                "remainder_count": remainder.len(),
                "remainder": remainder,
                "remainder_slot_profiles": remainder_slot_profiles,
            }))?
        );
    }

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
        }))?
    );
    Ok(())
}
