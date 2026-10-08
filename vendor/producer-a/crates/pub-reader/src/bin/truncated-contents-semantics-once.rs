use anyhow::{Context, Result};
use pub_cfb::{
    RootRegularStreamPrefixStatus, inspect_truncated_cfb_raw_directory_reader,
    recover_truncated_regular_stream_prefix_by_sid_reader_with_expected_sha,
};
use pub_reader::{
    ReaderPartialRootStreamEvidence, READER_PARTIAL_ROOT_STREAM_EVIDENCE_SCHEMA_V1,
    analyze_reader_partial_contents_prefix,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    env, fs,
    io::Cursor,
    path::{Path, PathBuf},
};

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect()
}

fn collect_pub_paths(root: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(root).with_context(|| format!("read {}", root.display()))? {
        let path = entry?.path();
        if path.is_dir() {
            collect_pub_paths(&path, out)?;
        } else if path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("pub"))
        {
            out.push(path);
        }
    }
    Ok(())
}

fn main() -> Result<()> {
    let mut args = env::args_os().skip(1);
    let root = PathBuf::from(args.next().context("usage: truncated-contents-semantics-once CORPUS_DIR OUTPUT.json")?);
    let output = PathBuf::from(args.next().context("usage: truncated-contents-semantics-once CORPUS_DIR OUTPUT.json")?);
    anyhow::ensure!(args.next().is_none(), "exactly two arguments required");
    let mut paths = Vec::new();
    collect_pub_paths(&root, &mut paths)?;
    paths.sort();

    let mut rows: Vec<Value> = Vec::new();
    let mut decisions: BTreeMap<String, usize> = BTreeMap::new();
    let mut classes: BTreeMap<String, usize> = BTreeMap::new();
    let mut useful_count = 0usize;
    let mut total_complete_chunks = 0usize;
    let mut total_fully_decoded_chunks = 0usize;

    for path in paths.iter() {
        let bytes = fs::read(path).with_context(|| format!("read {}", path.display()))?;
        let source_sha256 = sha256_hex(&bytes);
        let mut row = json!({
            "source_sha256": source_sha256,
            "logical_path_proven": false,
            "research_only_not_product_admission": true
        });
        let decision;
        if bytes.len() > 256 * 1024 * 1024 {
            decision = "source_over_limit";
        } else if let Ok(inv) = inspect_truncated_cfb_raw_directory_reader(Cursor::new(&bytes)) {
            if inv.source_sha256 != source_sha256
                || inv.source_byte_len != bytes.len() as u64
                || inv.entries.iter().any(|entry| {
                    matches!(entry.object_type, 1 | 2) && entry.descriptive_name.is_none()
                })
            {
                decision = "identity_or_opaque_entry";
            } else {
                let candidates = inv.entries.iter().filter(|entry| {
                    entry.object_type == 2 &&
                    entry.descriptive_name.as_deref().is_some_and(|n| n.eq_ignore_ascii_case("Contents"))
                }).collect::<Vec<_>>();
                row["raw_contents_candidates"] = json!(candidates.len());
                if candidates.len() != 1 || candidates[0].descriptive_name.as_deref() != Some("Contents") {
                    decision = if candidates.is_empty() { "contents_not_found" } else { "ambiguous_raw_contents" };
                } else {
                    let entry = candidates[0];
                    match recover_truncated_regular_stream_prefix_by_sid_reader_with_expected_sha(
                        Cursor::new(&bytes), entry.sid, &source_sha256
                    ) {
                        Ok(recovered) => {
                            if recovered.source_modified
                                || recovered.source_sha256 != source_sha256
                                || recovered.source_byte_len != bytes.len() as u64
                                || recovered.stream_sid != entry.sid
                                || recovered.declared_len != entry.declared_len
                                || recovered.available_prefix_len != recovered.bytes.len() as u64
                                || recovered.available_prefix_len > 64 * 1024 * 1024
                                || sha256_hex(&recovered.bytes) != recovered.prefix_sha256
                            {
                                decision = "recovered_identity_mismatch";
                            } else if recovered.status != RootRegularStreamPrefixStatus::Partial {
                                decision = "not_partial";
                            } else {
                                let evidence = ReaderPartialRootStreamEvidence {
                                    schema_version: READER_PARTIAL_ROOT_STREAM_EVIDENCE_SCHEMA_V1.to_string(),
                                    source_sha256: source_sha256.clone(),
                                    stream_sid: recovered.stream_sid,
                                    // Parser discriminator label only: NOT a proven root CFB path.
                                    stream_identity: "/Contents".to_owned(),
                                    declared_len: recovered.declared_len,
                                    available_prefix_len: recovered.available_prefix_len,
                                    prefix_sha256: recovered.prefix_sha256,
                                    status: recovered.status,
                                    truncation_reason: recovered.truncation_reason,
                                    source_ranges: recovered.source_ranges,
                                    prefix_bytes: recovered.bytes,
                                    source_modified: false,
                                };
                                let semantic = analyze_reader_partial_contents_prefix(&evidence);
                                let class = format!("{:?}", semantic.class);
                                *classes.entry(class.clone()).or_default() += 1;
                                if class == "UsefulSemanticPrefix" { useful_count += 1; }
                                let complete = semantic.complete_chunk_facts.len();
                                let fully_decoded = semantic.complete_chunk_facts.iter()
                                    .filter(|fact| fact.fully_decoded).count();
                                total_complete_chunks += complete;
                                total_fully_decoded_chunks += fully_decoded;
                                let mut raw_type_counts: BTreeMap<String, usize> = BTreeMap::new();
                                for fact in &semantic.complete_chunk_facts {
                                    *raw_type_counts.entry(format!("0x{:04x}", fact.raw_type)).or_default() += 1;
                                }
                                row["stream_sid"] = json!(evidence.stream_sid);
                                row["prefix_sha256"] = json!(evidence.prefix_sha256);
                                row["declared_len"] = json!(evidence.declared_len);
                                row["available_prefix_len"] = json!(evidence.available_prefix_len);
                                row["family"] = json!(semantic.family);
                                row["class"] = json!(class);
                                row["boundary"] = json!(format!("{:?}", semantic.boundary));
                                row["complete_chunk_fact_count"] = json!(complete);
                                row["fully_decoded_chunk_fact_count"] = json!(fully_decoded);
                                row["raw_type_counts"] = json!(raw_type_counts);
                                row["ambiguous_reference_count"] = json!(semantic.ambiguous_reference_count);
                                row["referenced_chunk_unavailable_count"] = json!(semantic.referenced_chunk_unavailable_count);
                                decision = "classified_partial_candidate";
                            }
                        }
                        Err(_) => decision = "physical_recovery_unavailable",
                    }
                }
            }
        } else {
            decision = "truncated_inventory_unavailable";
        }
        row["decision"] = json!(decision);
        *decisions.entry(decision.to_owned()).or_default() += 1;
        rows.push(row);
    }
    let result = json!({
        "schema": "chaptera.exploratory-truncated-contents-semantics.v1",
        "research_only_not_product_admission": true,
        "source_count": rows.len(),
        "decision_counts": decisions,
        "semantic_class_counts": classes,
        "useful_semantic_prefix_candidates": useful_count,
        "complete_chunk_fact_count": total_complete_chunks,
        "fully_decoded_chunk_fact_count": total_fully_decoded_chunks,
        "rows": rows
    });
    if let Some(parent) = output.parent() { fs::create_dir_all(parent)?; }
    fs::write(&output, serde_json::to_vec_pretty(&result)?)?;
    Ok(())
}
