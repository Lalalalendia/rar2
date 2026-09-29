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

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn source_hash(bytes: &[u8]) -> Sha256Digest {
    Sha256Digest::from_bytes(Sha256::digest(bytes).into())
}

fn type_hex(value: u16) -> String {
    format!("0x{value:04x}")
}

fn read_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    let raw = bytes.get(offset..offset.checked_add(2)?)?;
    Some(u16::from_le_bytes([raw[0], raw[1]]))
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

fn type_counts<'a>(
    entries: impl Iterator<Item = &'a Legacy0x22DirectoryEntry>,
) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for entry in entries {
        *counts.entry(type_hex(entry.chunk_type)).or_insert(0) += 1;
    }
    counts
}

fn geometry_profile(chunk: &[u8]) -> Value {
    if chunk.len() < GEOMETRY_END {
        return json!({
            "carrier_present": false,
            "positive_rect": null,
            "width": null,
            "height": null,
            "normalized_chunk_sha256": null,
        });
    }

    let Some(xs) = read_i32(chunk, 0x06) else {
        return json!({"carrier_present": false});
    };
    let Some(ys) = read_i32(chunk, 0x0a) else {
        return json!({"carrier_present": false});
    };
    let Some(xe) = read_i32(chunk, 0x0e) else {
        return json!({"carrier_present": false});
    };
    let Some(ye) = read_i32(chunk, 0x12) else {
        return json!({"carrier_present": false});
    };

    let width = i64::from(xe) - i64::from(xs);
    let height = i64::from(ye) - i64::from(ys);
    let mut normalized = chunk.to_vec();
    normalized[GEOMETRY_START..GEOMETRY_END].fill(0);

    json!({
        "carrier_present": true,
        "positive_rect": width > 0 && height > 0,
        "width": width,
        "height": height,
        "normalized_chunk_sha256": sha256_hex(&normalized),
    })
}

fn edge_fingerprint(chunk: &[u8], from_start: bool) -> String {
    let len = chunk.len().min(EDGE_FINGERPRINT_BYTES);
    if from_start {
        sha256_hex(&chunk[..len])
    } else {
        sha256_hex(&chunk[chunk.len() - len..])
    }
}

fn child_0022_profile(
    contents: &[u8],
    directory: &Legacy0x22Directory,
    parent_object_id: u16,
) -> Option<Value> {
    let mut children = directory
        .entries_by_parent_id(parent_object_id)
        .filter(|entry| entry.chunk_type == RAW_TYPE_0022);
    let child = children.next()?;
    if children.next().is_some() {
        return None;
    }
    let chunk = chunk_bytes(contents, child)?;
    Some(json!({
        "object_id": child.object_id,
        "service_word": child.service_word,
        "chunk_len": chunk.len(),
        "chunk_sha256": sha256_hex(chunk),
        "prefix32_sha256": edge_fingerprint(chunk, true),
        "suffix32_sha256": edge_fingerprint(chunk, false),
    }))
}

fn main() -> Result<()> {
    let mut args = env::args_os().skip(1);
    let source = PathBuf::from(
        args.next()
            .context("usage: legacy22_raw0003_profile SOURCE.pub OUTPUT.json")?,
    );
    let output = PathBuf::from(
        args.next()
            .context("usage: legacy22_raw0003_profile SOURCE.pub OUTPUT.json")?,
    );
    if args.next().is_some() {
        anyhow::bail!("legacy22_raw0003_profile accepts exactly SOURCE.pub OUTPUT.json");
    }

    let bytes = fs::read(&source).with_context(|| format!("read {}", source.display()))?;
    let source_sha256 = sha256_hex(&bytes);
    let digest = source_hash(&bytes);

    if build_legacy_0x22_noquill_source_graph(Cursor::new(bytes.as_slice()), digest).is_err() {
        let receipt = json!({
            "schema": "chaptera.legacy22-raw0003-profile.v1",
            "source_sha256": source_sha256,
            "eligible_reader_open": false,
            "raw0003_instance_count": 0,
            "instances": [],
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
    let document_page_ids = page_child_ids(document_chunk).context("parse DOCUMENT PageList")?;
    let document_page_ids = document_page_ids.into_iter().collect::<BTreeSet<_>>();

    let mut page_memberships = BTreeMap::<u16, Vec<(u16, usize, Option<u16>, Option<u16>)>>::new();
    for page_entry in directory.entries.iter().filter(|entry| {
        entry.chunk_type == LEGACY_PAGE_TYPE && document_page_ids.contains(&entry.object_id)
    }) {
        let Some(page_chunk) = chunk_bytes(&contents, page_entry) else {
            continue;
        };
        let Some(child_ids) = page_child_ids(page_chunk) else {
            continue;
        };
        for (position, child_id) in child_ids.iter().copied().enumerate() {
            let previous_type = position
                .checked_sub(1)
                .and_then(|previous| child_ids.get(previous))
                .and_then(|object_id| directory.entry_by_object_id(*object_id))
                .map(|entry| entry.chunk_type);
            let next_type = child_ids
                .get(position + 1)
                .and_then(|object_id| directory.entry_by_object_id(*object_id))
                .map(|entry| entry.chunk_type);
            page_memberships.entry(child_id).or_default().push((
                page_entry.object_id,
                position,
                previous_type,
                next_type,
            ));
        }
    }

    let mut instances = Vec::new();
    for (index, entry) in directory.entries.iter().enumerate() {
        if entry.chunk_type != RAW_TYPE_0003 {
            continue;
        }

        let chunk = chunk_bytes(&contents, entry)
            .with_context(|| format!("bounded chunk for object {}", entry.object_id))?;
        let parent_type = directory
            .entry_by_object_id(entry.parent_id)
            .map(|parent| parent.chunk_type);
        let previous_type = index
            .checked_sub(1)
            .and_then(|previous| directory.entries.get(previous))
            .map(|previous| previous.chunk_type);
        let next_type = directory.entries.get(index + 1).map(|next| next.chunk_type);

        let sibling_counts = type_counts(
            directory
                .entries_by_parent_id(entry.parent_id)
                .filter(|sibling| sibling.object_id != entry.object_id),
        );
        let child_counts = type_counts(directory.entries_by_parent_id(entry.object_id));
        let memberships = page_memberships
            .get(&entry.object_id)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let matching_membership = memberships
            .iter()
            .find(|(page_object_id, _, _, _)| *page_object_id == entry.parent_id);
        let matching_parent_page_membership = matching_membership.is_some();
        let other_page_membership_count = memberships
            .iter()
            .filter(|(page_object_id, _, _, _)| *page_object_id != entry.parent_id)
            .count();

        instances.push(json!({
            "object_id": entry.object_id,
            "parent_id": entry.parent_id,
            "parent_raw_type": parent_type.map(type_hex),
            "parent_is_page": parent_type == Some(LEGACY_PAGE_TYPE),
            "parent_is_document_page": parent_type == Some(LEGACY_PAGE_TYPE)
                && document_page_ids.contains(&entry.parent_id),
            "page_child_membership_count": memberships.len(),
            "matching_parent_page_membership": matching_parent_page_membership,
            "other_page_membership_count": other_page_membership_count,
            "matching_page_child_position": matching_membership.map(|(_, position, _, _)| *position),
            "matching_page_previous_raw_type": matching_membership
                .and_then(|(_, _, previous, _)| previous.map(type_hex)),
            "matching_page_next_raw_type": matching_membership
                .and_then(|(_, _, _, next)| next.map(type_hex)),
            "service_word": entry.service_word,
            "chunk_len": chunk.len(),
            "chunk_sha256": sha256_hex(chunk),
            "prefix32_sha256": edge_fingerprint(chunk, true),
            "suffix32_sha256": edge_fingerprint(chunk, false),
            "geometry": geometry_profile(chunk),
            "previous_directory_raw_type": previous_type.map(type_hex),
            "next_directory_raw_type": next_type.map(type_hex),
            "sibling_raw_type_counts": sibling_counts,
            "child_raw_type_counts": child_counts,
            "child_0x0022": child_0022_profile(&contents, &directory, entry.object_id),
        }));
    }

    let receipt = json!({
        "schema": "chaptera.legacy22-raw0003-profile.v1",
        "source_sha256": source_sha256,
        "eligible_reader_open": true,
        "raw0003_instance_count": instances.len(),
        "instances": instances,
    });
    fs::write(&output, serde_json::to_vec_pretty(&receipt)?)
        .with_context(|| format!("write {}", output.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn geometry_profile_requires_complete_positive_rectangle() {
        let mut chunk = vec![0_u8; GEOMETRY_END];
        chunk[0x06..0x0a].copy_from_slice(&(-10_i32).to_le_bytes());
        chunk[0x0a..0x0e].copy_from_slice(&(-20_i32).to_le_bytes());
        chunk[0x0e..0x12].copy_from_slice(&(30_i32).to_le_bytes());
        chunk[0x12..0x16].copy_from_slice(&(40_i32).to_le_bytes());

        let profile = geometry_profile(&chunk);
        assert_eq!(profile["carrier_present"], true);
        assert_eq!(profile["positive_rect"], true);
        assert_eq!(profile["width"], 40);
        assert_eq!(profile["height"], 60);
        assert!(profile["normalized_chunk_sha256"].is_string());
    }

    #[test]
    fn geometry_profile_rejects_short_carrier() {
        let profile = geometry_profile(&[0_u8; GEOMETRY_END - 1]);
        assert_eq!(profile["carrier_present"], false);
        assert!(profile["positive_rect"].is_null());
    }
}
