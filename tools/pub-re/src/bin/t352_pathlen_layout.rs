//! T352-PATHLEN-01: source-safe physical relocation evidence for a Publisher Save.
//!
//! Compare the immutable native-normalized base to one saved no-geometry arm.
//! Only numeric offsets, lengths, counts, and hashes leave the private runner.
//! No path text, document bytes, or stream bytes are serialized into receipts.

use anyhow::{Context, Result, bail};
use pub_contents::{
    parse_0x2c_header, parse_confirmed_0x2c_trailer_root, parse_confirmed_chunk_reference,
};
use pub_core::StreamPath;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, env, fs, path::Path};

const STREAM: &str = "/Contents";
const MAX_BYTES: usize = 32 * 1024 * 1024;
const MAX_SLOTS: usize = 40_000;

fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>()
}

struct Snapshot {
    full_sha256: String,
    contents: Vec<u8>,
    root: u64,
    root_len: u64,
    root_declared_len: u32,
    directory: u64,
    directory_len: u64,
    slots: Vec<(u64, u64)>,
    chunk_refs: BTreeMap<usize, Vec<u32>>,
}

fn snapshot(path: &Path) -> Result<Snapshot> {
    let raw = fs::read(path).context("read pinned Publisher source")?;
    if !(512..=MAX_BYTES).contains(&raw.len()) {
        bail!("PUB size exceeds bounded T352 policy");
    }
    let contents = pub_cfb::read_stream_path(path, STREAM).context("read canonical /Contents")?;
    let header = parse_0x2c_header(StreamPath(STREAM.into()), &contents)
        .context("parse confirmed 0x2C Contents header")?;
    let root = parse_confirmed_0x2c_trailer_root(&contents, &header)
        .context("parse confirmed 0x2C trailer/directory")?;
    if root.directory.slots.len() > MAX_SLOTS {
        bail!("Contents directory too large");
    }
    if !root.observed_slot_count_matches_directory() {
        bail!("Contents trailer slot count does not match parsed directory");
    }
    let mut chunk_refs = BTreeMap::new();
    let mut slots = Vec::new();
    for (ordinal, slot) in root.directory.slots.iter().enumerate() {
        slots.push((slot.source().offset, slot.source().len));
        if let Some(reference) =
            parse_confirmed_chunk_reference(&contents, &root.directory, ordinal)?
        {
            let offsets = reference
                .chunk_offsets
                .iter()
                .map(|item| item.value)
                .collect();
            chunk_refs.insert(ordinal, offsets);
        }
    }
    Ok(Snapshot {
        full_sha256: sha256(&raw),
        contents,
        root: u64::from(header.trailer_offset),
        root_len: root.source.len,
        root_declared_len: root.declared_length,
        directory: root.directory.source.offset,
        directory_len: root.directory.source.len,
        slots,
        chunk_refs,
    })
}

fn utf16_hits(bytes: &[u8], name: &str) -> Result<Vec<usize>> {
    if name.is_empty() || !name.is_ascii() || (name.contains('/') || name.contains('\\')) {
        bail!("filename must be a nonempty ASCII leaf name");
    }
    let needle: Vec<u8> = name.encode_utf16().flat_map(u16::to_le_bytes).collect();
    Ok(bytes
        .windows(needle.len())
        .enumerate()
        .filter_map(|(offset, window)| (window == needle).then_some(offset))
        .collect())
}

fn increment(map: &mut BTreeMap<i64, usize>, delta: i64) {
    *map.entry(delta).or_default() += 1;
}

fn main() -> Result<()> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    if args.len() != 5 {
        bail!("usage: t352_pathlen_layout BASE.pub SAVED.pub OLD_LEAF NEW_LEAF OUT.json");
    }
    let before_path = Path::new(&args[0]);
    let after_path = Path::new(&args[1]);
    let old_leaf = args[2].to_str().context("old leaf not UTF-8")?;
    let new_leaf = args[3].to_str().context("new leaf not UTF-8")?;
    let out = Path::new(&args[4]);

    let before = snapshot(before_path)?;
    let after = snapshot(after_path)?;

    let old_hits = utf16_hits(&before.contents, old_leaf)?;
    let new_hits = utf16_hits(&after.contents, new_leaf)?;
    let before_path_units = before_path.to_string_lossy().encode_utf16().count();
    let after_path_units = after_path.to_string_lossy().encode_utf16().count();
    let expected_delta = (after_path_units as i64 - before_path_units as i64) * 2;
    let actual_delta = after.contents.len() as i64 - before.contents.len() as i64;

    let mut slot_delta_counts = BTreeMap::new();
    let mut slot_lengths_equal = 0_usize;
    for ((start_a, len_a), (start_b, len_b)) in before.slots.iter().zip(after.slots.iter()) {
        increment(&mut slot_delta_counts, *start_b as i64 - *start_a as i64);
        if len_a == len_b {
            slot_lengths_equal += 1;
        }
    }

    let mut chunk_delta_counts = BTreeMap::new();
    let mut reference_pairs = 0_usize;
    let mut mismatched_reference_cardinality = 0_usize;
    for (index, offsets_a) in &before.chunk_refs {
        if let Some(offsets_b) = after.chunk_refs.get(index) {
            if offsets_a.len() != offsets_b.len() {
                mismatched_reference_cardinality += 1;
                continue;
            }
            for (offset_a, offset_b) in offsets_a.iter().zip(offsets_b) {
                increment(
                    &mut chunk_delta_counts,
                    i64::from(*offset_b) - i64::from(*offset_a),
                );
                reference_pairs += 1;
            }
        }
    }

    let earliest_change = before
        .contents
        .iter()
        .zip(after.contents.iter())
        .position(|(a, b)| a != b);
    let receipt = json!({
        "schema": "chaptera.t352-pathlen-layout-diff.v1",
        "before_sha256": before.full_sha256,
        "after_sha256": after.full_sha256,
        "before_contents_sha256": sha256(&before.contents),
        "after_contents_sha256": sha256(&after.contents),
        "before_contents_len": before.contents.len(),
        "after_contents_len": after.contents.len(),
        "predicted_contents_len_delta": expected_delta,
        "observed_contents_len_delta": actual_delta,
        "path_length_model_matches": expected_delta == actual_delta,
        "path_units_delta": after_path_units as i64 - before_path_units as i64,
        "before_trailer_offset": before.root,
        "after_trailer_offset": after.root,
        "trailer_offset_delta": after.root as i64 - before.root as i64,
        "before_trailer_root_span_len": before.root_len,
        "after_trailer_root_span_len": after.root_len,
        "before_trailer_declared_len": before.root_declared_len,
        "after_trailer_declared_len": after.root_declared_len,
        "before_directory_offset": before.directory,
        "after_directory_offset": after.directory,
        "directory_offset_delta": after.directory as i64 - before.directory as i64,
        "before_directory_len": before.directory_len,
        "after_directory_len": after.directory_len,
        "before_slot_count": before.slots.len(),
        "after_slot_count": after.slots.len(),
        "paired_slot_count": before.slots.len().min(after.slots.len()),
        "paired_slot_lengths_equal": slot_lengths_equal,
        "slot_source_offset_delta_histogram": slot_delta_counts,
        "before_occupied_reference_count": before.chunk_refs.len(),
        "after_occupied_reference_count": after.chunk_refs.len(),
        "paired_chunk_offset_field_count": reference_pairs,
        "chunk_offset_field_delta_histogram": chunk_delta_counts,
        "reference_cardinality_mismatches": mismatched_reference_cardinality,
        "first_changed_stream_byte_offset": earliest_change,
        "before_utf16_leaf_match_count": old_hits.len(),
        "after_utf16_leaf_match_count": new_hits.len(),
        "before_utf16_leaf_offsets": old_hits.iter().take(8).collect::<Vec<_>>(),
        "after_utf16_leaf_offsets": new_hits.iter().take(8).collect::<Vec<_>>(),
        "observed_leaf_localization": old_hits.len() == 1 && new_hits.len() == 1,
        "raw_document_bytes_emitted": false,
        "raw_stream_bytes_emitted": false
    });
    fs::write(out, serde_json::to_vec_pretty(&receipt)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_ascii_utf16_leaves_without_disclosing_contents() {
        let bytes = [
            0xff, 0x01, b'a', 0, b'.', 0, b'p', 0, b'u', 0, b'b', 0, 0xee,
        ];
        assert_eq!(utf16_hits(&bytes, "a.pub").unwrap(), vec![2]);
        assert!(utf16_hits(&bytes, "../a.pub").is_err());
    }

    #[test]
    fn delta_histogram_keeps_signed_changes() {
        let mut deltas = BTreeMap::new();
        increment(&mut deltas, 0);
        increment(&mut deltas, 10);
        increment(&mut deltas, 10);
        assert_eq!(deltas.get(&0), Some(&1));
        assert_eq!(deltas.get(&10), Some(&2));
    }
}
