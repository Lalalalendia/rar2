// Exact-seven #315 research probe; output is source-safe aggregate evidence only.
use anyhow::{Context, Result};
use pub_reader::{
    PubMatureContentsStoryDemand, QuillStoryFailureEvidence,
    probe_mature_0x2c_contents_serialization_revision,
    probe_mature_0x2c_contents_story_count,
    probe_mature_0x2c_contents_story_demand,
    probe_mature_0x2c_quill_story_error_kind,
    probe_mature_0x2c_quill_story_failure_evidence,
    probe_mature_0x2c_quill_story_failure_stage,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, env, fs, path::{Path, PathBuf}};

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[derive(Debug, Serialize)]
struct SentinelRow {
    source_sha256: String,
    byte_len: usize,
    contents_serialization_revision: Option<u16>,
    contents_story_count: Option<u32>,
    contents_story_demand: Option<PubMatureContentsStoryDemand>,
    quill_error_kind: Option<&'static str>,
    quill_failure_stage: Option<&'static str>,
    quill_failure_evidence: Option<QuillStoryFailureEvidence>,
}

fn pub_paths(root: &Path) -> Result<Vec<PathBuf>> {
    let mut out = fs::read_dir(root)
        .with_context(|| format!("read witness dir {}", root.display()))?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            path.extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("pub"))
        })
        .collect::<Vec<_>>();
    out.sort();
    Ok(out)
}

fn main() -> Result<()> {
    let mut args = env::args_os().skip(1);
    let root = PathBuf::from(
        args.next()
            .context("usage: quill-story-sentinel-probe WITNESS_DIR OUTPUT.json")?,
    );
    let output = PathBuf::from(
        args.next()
            .context("usage: quill-story-sentinel-probe WITNESS_DIR OUTPUT.json")?,
    );
    if args.next().is_some() {
        anyhow::bail!("quill-story-sentinel-probe accepts exactly WITNESS_DIR OUTPUT.json");
    }

    let mut rows = Vec::new();
    for path in pub_paths(&root)? {
        let bytes = fs::read(&path).with_context(|| format!("read {}", path.display()))?;
        rows.push(SentinelRow {
            source_sha256: sha256_hex(&bytes),
            byte_len: bytes.len(),
            contents_serialization_revision:
                probe_mature_0x2c_contents_serialization_revision(&bytes),
            contents_story_count: probe_mature_0x2c_contents_story_count(&bytes),
            contents_story_demand: probe_mature_0x2c_contents_story_demand(&bytes),
            quill_error_kind: probe_mature_0x2c_quill_story_error_kind(&bytes),
            quill_failure_stage: probe_mature_0x2c_quill_story_failure_stage(&bytes),
            quill_failure_evidence: probe_mature_0x2c_quill_story_failure_evidence(&bytes),
        });
    }

    let mut story_count_clusters = BTreeMap::<String, usize>::new();
    let mut demand_clusters = BTreeMap::<String, usize>::new();
    for row in &rows {
        *story_count_clusters
            .entry(
                row.contents_story_count
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "none".to_owned()),
            )
            .or_default() += 1;
        let demand_key = row
            .contents_story_demand
            .as_ref()
            .map(|demand| {
                format!(
                    "shape_occ={};table_occ={};distinct={};shape_chunks={};table_chunks={}",
                    demand.live_shape_field_27_occurrence_count,
                    demand.live_table_field_27_occurrence_count,
                    demand.live_distinct_field_27_value_count,
                    demand.live_shape_chunks_with_field_27,
                    demand.live_table_chunks_with_field_27,
                )
            })
            .unwrap_or_else(|| "none".to_owned());
        *demand_clusters.entry(demand_key).or_default() += 1;
    }

    let report = serde_json::json!({
        "schema": "chaptera.quill-story-sentinel-ffffffff.v1",
        "witness_count": rows.len(),
        "contents_story_count_clusters": story_count_clusters,
        "contents_story_demand_clusters": demand_clusters,
        "rows": rows,
        "evidence_boundary": "exact witness SHA plus source-safe counts/booleans/lengths only; no filenames, paths, document text, Story IDs, raw stream bytes, offsets or raw parser error text",
    });

    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&output, serde_json::to_vec_pretty(&report)?)?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}
