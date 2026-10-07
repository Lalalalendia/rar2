use pub_model::Sha256Digest;
use pub_reader::{build_mature_0x2c_source_graph, derive_pub_node_id, PubBridgeDiagnostic};
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
        .ok_or("usage: grouped_image_projection_receipt INPUT.pub")?;
    let bytes = fs::read(&input)?;
    let hash = source_hash(&bytes);
    let build = build_mature_0x2c_source_graph(Cursor::new(&bytes), hash)?;

    let page_number_by_id = build
        .effective_pages
        .page_ids
        .iter()
        .enumerate()
        .map(|(index, page_id)| (*page_id, index + 1))
        .collect::<std::collections::BTreeMap<_, _>>();

    let mut rows = build
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
                "target_page_number": target_page_id.and_then(|page| page_number_by_id.get(page).copied()),
                "image_slot": image_slot,
                "group_depth_to_page": group_ancestry.len(),
                "group_ancestry": group_ancestry,
                "reason": reason,
            })),
            _ => None,
        })
        .collect::<Vec<_>>();

    rows.sort_by(|left, right| {
        left["target_page_id"]
            .as_str()
            .cmp(&right["target_page_id"].as_str())
            .then_with(|| left["seq_num"].as_u64().cmp(&right["seq_num"].as_u64()))
    });

    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "schema": "chaptera.grouped-image-projection-receipt.v1",
            "source_sha256": hash.to_string(),
            "source_bytes": bytes.len(),
            "row_count": rows.len(),
            "rows": rows,
        }))?
    );
    Ok(())
}
