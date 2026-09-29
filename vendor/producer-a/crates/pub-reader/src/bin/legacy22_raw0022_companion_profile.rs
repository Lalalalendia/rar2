use anyhow::{Context, Result};
use pub_contents::{parse_legacy_0x22_directory, Legacy0x22Directory, Legacy0x22DirectoryEntry};
use pub_core::StreamPath;
use pub_model::Sha256Digest;
use pub_reader::{build_legacy_0x22_noquill_source_graph, CONTENTS_STREAM_PATH};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::io::Cursor;
use std::path::PathBuf;

const RAW_TYPE_0003: u16 = 0x0003;
const RAW_TYPE_0022: u16 = 0x0022;
const LEGACY_PAGE_TYPE: u16 = 0x0014;
const LEGACY_DOCUMENT_TYPE: u16 = 0x0015;
const GEOMETRY_START: usize = 0x06;
const GEOMETRY_END: usize = 0x16;
const EDGE_FINGERPRINT_BYTES: usize = 32;
const SCALAR_WINDOW_BYTES: usize = 64;

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn source_hash(bytes: &[u8]) -> Sha256Digest {
    Sha256Digest::from_bytes(Sha256::digest(bytes).into())
}

fn read_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    let raw = bytes.get(offset..offset.checked_add(2)?)?;
    Some(u16::from_le_bytes([raw[0], raw[1]]))
}

fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    let raw = bytes.get(offset..offset.checked_add(4)?)?;
    Some(u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]))
}

fn read_i32(bytes: &[u8], offset: usize) -> Option<i32> {
    let raw = bytes.get(offset..offset.checked_add(4)?)?;
    Some(i32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]))
}

fn page_child_ids(chunk: &[u8]) -> Option<Vec<u16>> {
    let list_start = usize::from(*chunk.get(3)?);
    let header_end = list_start.checked_add(10)?;
    if header_end > chunk.len() {
        return None;
    }
    let count = usize::from(read_u16(chunk, list_start)?);
    let max_count = usize::from(read_u16(chunk, list_start + 2)?);
    let record_size = usize::from(read_u16(chunk, list_start + 4)?);
    if max_count < count || record_size != 2 {
        return None;
    }
    let payload_end = header_end.checked_add(count.checked_mul(record_size)?)?;
    if payload_end > chunk.len() {
        return None;
    }
    (0..count)
        .map(|index| read_u16(chunk, header_end + index * record_size))
        .collect::<Option<Vec<_>>>()
}

fn chunk_bytes<'a>(contents: &'a [u8], entry: &Legacy0x22DirectoryEntry) -> Option<&'a [u8]> {
    let start = usize::try_from(entry.chunk_source.offset).ok()?;
    let len = usize::try_from(entry.chunk_source.len).ok()?;
    let end = start.checked_add(len)?;
    contents.get(start..end)
}

fn positive_geometry(chunk: &[u8]) -> bool {
    if chunk.len() < GEOMETRY_END {
        return false;
    }
    let Some(xs) = read_i32(chunk, 0x06) else {
        return false;
    };
    let Some(ys) = read_i32(chunk, 0x0a) else {
        return false;
    };
    let Some(xe) = read_i32(chunk, 0x0e) else {
        return false;
    };
    let Some(ye) = read_i32(chunk, 0x12) else {
        return false;
    };
    i64::from(xe) > i64::from(xs) && i64::from(ye) > i64::from(ys)
}

fn normalized_parent_sha256(chunk: &[u8]) -> Option<String> {
    if chunk.len() < GEOMETRY_END {
        return None;
    }
    let mut normalized = chunk.to_vec();
    normalized[GEOMETRY_START..GEOMETRY_END].fill(0);
    Some(sha256_hex(&normalized))
}

fn edge_fingerprint(chunk: &[u8], from_start: bool) -> String {
    let len = chunk.len().min(EDGE_FINGERPRINT_BYTES);
    if from_start {
        sha256_hex(&chunk[..len])
    } else {
        sha256_hex(&chunk[chunk.len() - len..])
    }
}

fn scalar_u16_profile(chunk: &[u8]) -> BTreeMap<String, u16> {
    let mut values = BTreeMap::new();
    let limit = chunk.len().min(SCALAR_WINDOW_BYTES);
    for offset in (0..limit.saturating_sub(1)).step_by(2) {
        if let Some(value) = read_u16(chunk, offset) {
            values.insert(format!("0x{offset:02x}"), value);
        }
    }
    values
}

fn scalar_u32_profile(chunk: &[u8]) -> BTreeMap<String, u32> {
    let mut values = BTreeMap::new();
    let limit = chunk.len().min(SCALAR_WINDOW_BYTES);
    for offset in (0..limit.saturating_sub(3)).step_by(4) {
        if let Some(value) = read_u32(chunk, offset) {
            values.insert(format!("0x{offset:02x}"), value);
        }
    }
    values
}

fn single_child_0022<'a>(
    directory: &'a Legacy0x22Directory,
    parent_object_id: u16,
) -> Option<&'a Legacy0x22DirectoryEntry> {
    let mut children = directory
        .entries_by_parent_id(parent_object_id)
        .filter(|entry| entry.chunk_type == RAW_TYPE_0022);
    let child = children.next()?;
    if children.next().is_some() {
        return None;
    }
    Some(child)
}

fn main() -> Result<()> {
    let mut args = env::args_os().skip(1);
    let source = PathBuf::from(
        args.next()
            .context("usage: legacy22_raw0022_companion_profile SOURCE.pub OUTPUT.json")?,
    );
    let output = PathBuf::from(
        args.next()
            .context("usage: legacy22_raw0022_companion_profile SOURCE.pub OUTPUT.json")?,
    );
    if args.next().is_some() {
        anyhow::bail!("legacy22_raw0022_companion_profile accepts exactly SOURCE.pub OUTPUT.json");
    }

    let bytes = fs::read(&source).with_context(|| format!("read {}", source.display()))?;
    let source_sha256 = sha256_hex(&bytes);
    let digest = source_hash(&bytes);

    if build_legacy_0x22_noquill_source_graph(Cursor::new(bytes.as_slice()), digest).is_err() {
        let receipt = json!({
            "schema": "chaptera.legacy22-raw0022-companion-profile.v1",
            "source_sha256": source_sha256,
            "eligible_reader_open": false,
            "candidate_count": 0,
            "candidates": [],
        });
        fs::write(&output, serde_json::to_vec_pretty(&receipt)?)
            .with_context(|| format!("write {}", output.display()))?;
        return Ok(());
    }

    let contents = pub_cfb::read_stream_reader(Cursor::new(bytes.as_slice()), CONTENTS_STREAM_PATH)
        .context("read legacy Contents")?;
    let directory = parse_legacy_0x22_directory(StreamPath(CONTENTS_STREAM_PATH.into()), &contents)
        .context("parse legacy 0x22 directory")?;

    let document_entry = directory
        .entries
        .iter()
        .find(|entry| entry.chunk_type == LEGACY_DOCUMENT_TYPE)
        .context("missing legacy DOCUMENT 0x0015")?;
    let document_chunk =
        chunk_bytes(&contents, document_entry).context("bounded legacy DOCUMENT chunk")?;
    let document_page_ids = page_child_ids(document_chunk)
        .context("parse DOCUMENT PageList")?
        .into_iter()
        .collect::<BTreeSet<_>>();

    let mut page_memberships = BTreeMap::<u16, BTreeSet<u16>>::new();
    for page_entry in directory.entries.iter().filter(|entry| {
        entry.chunk_type == LEGACY_PAGE_TYPE && document_page_ids.contains(&entry.object_id)
    }) {
        let Some(page_chunk) = chunk_bytes(&contents, page_entry) else {
            continue;
        };
        let Some(child_ids) = page_child_ids(page_chunk) else {
            continue;
        };
        for child_id in child_ids {
            page_memberships
                .entry(child_id)
                .or_default()
                .insert(page_entry.object_id);
        }
    }

    let mut candidates = Vec::<Value>::new();
    for parent in directory
        .entries
        .iter()
        .filter(|entry| entry.chunk_type == RAW_TYPE_0003)
    {
        if !document_page_ids.contains(&parent.parent_id) {
            continue;
        }
        let membership_matches_parent = page_memberships
            .get(&parent.object_id)
            .is_some_and(|parents| parents.contains(&parent.parent_id));
        if !membership_matches_parent {
            continue;
        }

        let Some(parent_chunk) = chunk_bytes(&contents, parent) else {
            continue;
        };
        if !positive_geometry(parent_chunk) {
            continue;
        }

        let Some(child) = single_child_0022(&directory, parent.object_id) else {
            continue;
        };
        let child_chunk = chunk_bytes(&contents, child)
            .with_context(|| format!("bounded 0x0022 child {}", child.object_id))?;

        candidates.push(json!({
            "parent_object_id": parent.object_id,
            "parent_id": parent.parent_id,
            "parent_service_word": parent.service_word,
            "parent_chunk_len": parent_chunk.len(),
            "parent_normalized_chunk_sha256": normalized_parent_sha256(parent_chunk),
            "child_object_id": child.object_id,
            "child_parent_id": child.parent_id,
            "child_service_word": child.service_word,
            "child_chunk_len": child_chunk.len(),
            "child_chunk_sha256": sha256_hex(child_chunk),
            "child_prefix32_sha256": edge_fingerprint(child_chunk, true),
            "child_suffix32_sha256": edge_fingerprint(child_chunk, false),
            "child_u16_first64": scalar_u16_profile(child_chunk),
            "child_u32_first64": scalar_u32_profile(child_chunk),
        }));
    }

    let receipt = json!({
        "schema": "chaptera.legacy22-raw0022-companion-profile.v1",
        "source_sha256": source_sha256,
        "eligible_reader_open": true,
        "candidate_count": candidates.len(),
        "candidates": candidates,
    });
    fs::write(&output, serde_json::to_vec_pretty(&receipt)?)
        .with_context(|| format!("write {}", output.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scalar_profiles_are_bounded_and_little_endian() {
        let bytes = [1_u8, 2, 3, 4, 5, 6, 7, 8];
        let u16s = scalar_u16_profile(&bytes);
        let u32s = scalar_u32_profile(&bytes);
        assert_eq!(u16s["0x00"], 0x0201);
        assert_eq!(u16s["0x06"], 0x0807);
        assert_eq!(u32s["0x00"], 0x04030201);
        assert_eq!(u32s["0x04"], 0x08070605);
    }

    #[test]
    fn normalized_parent_hash_zeroes_only_geometry_carrier() {
        let mut chunk = vec![0x5a_u8; 0x20];
        let before = sha256_hex(&chunk);
        let normalized = normalized_parent_sha256(&chunk).unwrap();
        chunk[GEOMETRY_START..GEOMETRY_END].fill(0);
        assert_ne!(before, normalized);
        assert_eq!(normalized, sha256_hex(&chunk));
    }
}
