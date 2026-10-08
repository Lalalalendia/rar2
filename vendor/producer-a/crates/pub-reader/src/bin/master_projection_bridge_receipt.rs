use anyhow::{bail, Context, Result};
use pub_model::Sha256Digest;
use pub_reader::{build_mature_0x2c_master_projection_bridge_v1, build_mature_0x2c_source_graph};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{env, fs, io::Cursor};

const SCHEMA: &str = "chaptera.master-projection-active-reader-bridge.v1";

fn main() -> Result<()> {
    let mut args = env::args_os().skip(1);
    let source_path = args
        .next()
        .context("usage: master_projection_bridge_receipt SOURCE.pub OUTPUT.json")?;
    let output_path = args
        .next()
        .context("usage: master_projection_bridge_receipt SOURCE.pub OUTPUT.json")?;
    if args.next().is_some() {
        bail!("unexpected extra arguments");
    }

    let bytes = fs::read(&source_path).with_context(|| format!("read {:?}", source_path))?;
    let digest = Sha256::digest(&bytes);
    let mut source_hash_bytes = [0_u8; 32];
    source_hash_bytes.copy_from_slice(&digest);
    let source_hash = Sha256Digest::from_bytes(source_hash_bytes);

    let source = build_mature_0x2c_source_graph(Cursor::new(bytes.as_slice()), source_hash.clone())
        .context("build active mature SourceGraph")?;
    let bridge = build_mature_0x2c_master_projection_bridge_v1(&bytes, source_hash, &source.graph)
        .context("build active Reader master projection authority bridge")?;

    let mut relation_pairs = bridge
        .output
        .context
        .master_relations
        .iter()
        .map(|relation| format!("{}\n{}\n", relation.source_page_id, relation.master_page_id))
        .collect::<Vec<_>>();
    relation_pairs.sort();
    let relation_identity_digest = "sha256:".to_owned()
        + &Sha256::digest(relation_pairs.concat().as_bytes())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();

    let receipt = json!({
        "schema": SCHEMA,
        "source_sha256": source_hash.to_string(),
        "relation_count": bridge.output.receipt.relation_count,
        "relation_identity_digest_sha256": relation_identity_digest,
        "active_graph_identity_parity": bridge.active_graph_identity_parity,
        "invariants": {
            "exact_field_id": bridge.output.receipt.invariants.exact_field_id,
            "exact_wire": bridge.output.receipt.invariants.exact_wire,
            "target_raw_type": bridge.output.receipt.invariants.target_raw_type,
            "source_graph_mutated": bridge.output.receipt.invariants.source_graph_mutated,
            "semantic_node_clones_created": bridge.output.receipt.invariants.semantic_node_clones_created,
        },
        "claims": {
            "new_contents_parser_added": false,
            "root_semantic_gate_reused": true,
            "raw_page_id_emitted": false,
            "raw_contents_seq_num_emitted": false,
            "story_text_emitted": false,
            "root_vendor_page_identity_parity_checked": true,
        },
    });

    fs::write(
        &output_path,
        serde_json::to_vec_pretty(&receipt).context("serialize bridge receipt")?,
    )
    .with_context(|| format!("write {:?}", output_path))?;

    println!("{}", serde_json::to_string_pretty(&receipt)?);
    Ok(())
}
