use anyhow::{Context, Result};
use pub_contents::{parse_legacy_0x22_directory, Legacy0x22Directory, Legacy0x22DirectoryEntry};
use pub_core::StreamPath;
use pub_model::Sha256Digest;
use pub_reader::{
    build_legacy_0x22_noquill_source_graph, validate_wmf_metafile, CONTENTS_STREAM_PATH,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, VecDeque};
use std::env;
use std::fs;
use std::io::Cursor;
use std::path::PathBuf;

const RAW_IMAGE: u16 = 0x0002;
const RAW_IMAGE_DATA: u16 = 0x0021;
const RAW_FILENAME: u16 = 0x0054;
const RAW_GROUP: u16 = 0x000f;
const RAW_PAGE: u16 = 0x0014;
const RAW_DOCUMENT: u16 = 0x0015;

const XS: usize = 0x06;
const YS: usize = 0x0a;
const XE: usize = 0x0e;
const YE: usize = 0x12;
const NATIVE_REF: usize = 0x72;
const REPL_REF: usize = 0x8a;
const GIF_PAYLOAD: usize = 0x10;
const WMF_OFFSETS: [usize; 5] = [0x08, 0x0c, 0x10, 0x14, 0x18];

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

fn chunk_bytes<'a>(contents: &'a [u8], entry: &Legacy0x22DirectoryEntry) -> Option<&'a [u8]> {
    let start = usize::try_from(entry.chunk_source.offset).ok()?;
    let len = usize::try_from(entry.chunk_source.len).ok()?;
    contents.get(start..start.checked_add(len)?)
}

fn list_ids(chunk: &[u8]) -> Option<Vec<u16>> {
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
    let end = header_end.checked_add(count.checked_mul(record_size)?)?;
    if end > chunk.len() {
        return None;
    }
    (0..count)
        .map(|index| read_u16(chunk, header_end + index * 2))
        .collect()
}

fn active_page_ids(contents: &[u8], directory: &Legacy0x22Directory) -> Result<BTreeSet<u16>> {
    let document = directory
        .entries
        .iter()
        .find(|entry| entry.chunk_type == RAW_DOCUMENT)
        .context("missing legacy DOCUMENT")?;
    let chunk = chunk_bytes(contents, document).context("bounded DOCUMENT chunk")?;
    Ok(list_ids(chunk)
        .context("parse DOCUMENT PageList")?
        .into_iter()
        .collect())
}

fn reader_reachable_ids(contents: &[u8], directory: &Legacy0x22Directory) -> Result<BTreeSet<u16>> {
    let active_pages = active_page_ids(contents, directory)?;
    let mut reachable = BTreeSet::new();
    let mut queue = VecDeque::new();

    for page in directory
        .entries
        .iter()
        .filter(|entry| entry.chunk_type == RAW_PAGE && active_pages.contains(&entry.object_id))
    {
        let Some(chunk) = chunk_bytes(contents, page) else {
            continue;
        };
        let Some(children) = list_ids(chunk) else {
            continue;
        };
        for child_id in children {
            let Some(child) = directory.entry_by_object_id(child_id) else {
                continue;
            };
            if child.parent_id != page.object_id {
                continue;
            }
            if reachable.insert(child_id) {
                queue.push_back(child_id);
            }
        }
    }

    while let Some(object_id) = queue.pop_front() {
        let Some(entry) = directory.entry_by_object_id(object_id) else {
            continue;
        };
        if entry.chunk_type != RAW_GROUP {
            continue;
        }
        for child in directory.entries_by_parent_id(object_id) {
            if reachable.insert(child.object_id) {
                queue.push_back(child.object_id);
            }
        }
    }

    Ok(reachable)
}

fn geometry(chunk: &[u8]) -> Value {
    let values = (
        read_i32(chunk, XS),
        read_i32(chunk, YS),
        read_i32(chunk, XE),
        read_i32(chunk, YE),
    );
    let (Some(xs), Some(ys), Some(xe), Some(ye)) = values else {
        return json!({
            "carrier_present": false,
            "positive_rect": false,
            "nondegenerate": false,
        });
    };
    let dx = i64::from(xe) - i64::from(xs);
    let dy = i64::from(ye) - i64::from(ys);
    json!({
        "carrier_present": true,
        "positive_rect": dx > 0 && dy > 0,
        "nondegenerate": dx != 0 || dy != 0,
        "delta_x_sign": if dx < 0 { "negative" } else if dx > 0 { "positive" } else { "zero" },
        "delta_y_sign": if dy < 0 { "negative" } else if dy > 0 { "positive" } else { "zero" },
    })
}

fn wmf_payload_offsets(chunk: &[u8]) -> Vec<usize> {
    WMF_OFFSETS
        .iter()
        .copied()
        .filter(|offset| {
            chunk
                .get(*offset..)
                .is_some_and(|payload| validate_wmf_metafile(payload).is_ok())
        })
        .collect()
}

fn wmf_validation_error_class(message: &str) -> &'static str {
    if message.contains("truncated placeable WMF header") {
        "truncated_placeable_header"
    } else if message.contains("Reserved field must be zero") {
        "placeable_reserved_nonzero"
    } else if message.contains("placeable WMF checksum mismatch") {
        "placeable_checksum_mismatch"
    } else if message.contains("truncated WMF META_HEADER") {
        "truncated_meta_header"
    } else if message.contains("unsupported WMF metafile type") {
        "unsupported_metafile_type"
    } else if message.contains("invalid WMF HeaderSize") {
        "invalid_header_size"
    } else if message.contains("unsupported WMF version") {
        "unsupported_version"
    } else if message.contains("declared size is smaller than META_HEADER") {
        "declared_size_too_small"
    } else if message.contains("WMF declared size mismatch") {
        "declared_size_mismatch"
    } else if message.contains("WMF MaxRecord is smaller than a record header") {
        "max_record_too_small"
    } else if message.contains("truncated WMF record size") {
        "truncated_record_size"
    } else if message.contains("invalid WMF record size") {
        "invalid_record_size"
    } else if message.contains("WMF record exceeds META_HEADER MaxRecord") {
        "record_exceeds_max_record"
    } else if message.contains("WMF record exceeds declared metafile size") {
        "record_exceeds_declared_size"
    } else if message.contains("WMF META_EOF is not the final record") {
        "eof_not_final"
    } else if message.contains("WMF META_EOF record is missing") {
        "eof_missing"
    } else {
        "other"
    }
}

fn wmf_declared_profile(chunk: &[u8]) -> Value {
    let Some(declared_u32) = read_u32(chunk, 0x04) else {
        return json!({
            "length_present": false,
            "declared_fits_chunk": false,
            "exact_chunk_end": false,
            "wmf_valid": false,
            "validation_error_class": "missing_length",
        });
    };
    let Ok(declared_len) = usize::try_from(declared_u32) else {
        return json!({
            "length_present": true,
            "declared_fits_chunk": false,
            "exact_chunk_end": false,
            "wmf_valid": false,
        });
    };
    let Some(end) = 0x08_usize.checked_add(declared_len) else {
        return json!({
            "length_present": true,
            "declared_fits_chunk": false,
            "exact_chunk_end": false,
            "wmf_valid": false,
        });
    };
    let Some(payload) = chunk.get(0x08..end) else {
        return json!({
            "length_present": true,
            "declared_len": declared_len,
            "declared_fits_chunk": false,
            "exact_chunk_end": false,
            "wmf_valid": false,
        });
    };
    let validation = validate_wmf_metafile(payload);
    let (wmf_valid, validation_error_class) = match &validation {
        Ok(_) => (true, None),
        Err(error) => (false, Some(wmf_validation_error_class(&error.to_string()))),
    };
    json!({
        "length_present": true,
        "declared_len": declared_len,
        "declared_fits_chunk": true,
        "exact_chunk_end": end == chunk.len(),
        "trailing_len": chunk.len() - end,
        "wmf_valid": wmf_valid,
        "validation_error_class": validation_error_class,
        "payload_sha256": wmf_valid.then(|| sha256_hex(payload)),
    })
}

fn read_contents(bytes: &[u8]) -> Result<Vec<u8>> {
    let strict_error = match pub_cfb::read_stream_reader(Cursor::new(bytes), CONTENTS_STREAM_PATH) {
        Ok(contents) => return Ok(contents),
        Err(error) => error,
    };
    let recovered = pub_cfb::recover_root_regular_stream_reader(
        Cursor::new(bytes),
        CONTENTS_STREAM_PATH,
    )
    .with_context(|| {
        format!("strict legacy Contents read failed ({strict_error}); bounded root recovery failed")
    })?;
    Ok(recovered.bytes)
}

fn gif_profile(chunk: &[u8]) -> Value {
    if chunk.len() < GIF_PAYLOAD {
        return json!({"header_present": false, "gif89a": false});
    }
    let payload = &chunk[GIF_PAYLOAD..];
    let length0 = read_u32(chunk, 0x08);
    let length1 = read_u32(chunk, 0x0c);
    json!({
        "header_present": true,
        "length0": length0,
        "length1": length1,
        "payload_len": payload.len(),
        "lengths_equal_payload": length0 == Some(payload.len() as u32)
            && length1 == Some(payload.len() as u32),
        "gif89a": payload.starts_with(b"GIF89a"),
        "payload_sha256": sha256_hex(payload),
    })
}

fn main() -> Result<()> {
    let mut args = env::args_os().skip(1);
    let source = PathBuf::from(
        args.next()
            .context("usage: legacy22_image0002_profile SOURCE.pub OUTPUT.json")?,
    );
    let output = PathBuf::from(
        args.next()
            .context("usage: legacy22_image0002_profile SOURCE.pub OUTPUT.json")?,
    );
    if args.next().is_some() {
        anyhow::bail!("legacy22_image0002_profile accepts exactly SOURCE.pub OUTPUT.json");
    }

    let bytes = fs::read(&source).with_context(|| format!("read {}", source.display()))?;
    let source_sha256 = sha256_hex(&bytes);
    let digest = source_hash(&bytes);

    if build_legacy_0x22_noquill_source_graph(Cursor::new(bytes.as_slice()), digest).is_err() {
        let receipt = json!({
            "schema": "chaptera.legacy22-image0002-profile.v1",
            "source_sha256": source_sha256,
            "eligible_reader_open": false,
            "physical_image_count": 0,
            "reachable_image_count": 0,
            "images": [],
        });
        fs::write(&output, serde_json::to_vec_pretty(&receipt)?)?;
        return Ok(());
    }

    let contents = read_contents(&bytes)?;
    let directory = parse_legacy_0x22_directory(StreamPath(CONTENTS_STREAM_PATH.into()), &contents)
        .context("parse legacy 0x22 directory")?;
    let reachable = reader_reachable_ids(&contents, &directory)?;

    let physical_image_count = directory
        .entries
        .iter()
        .filter(|entry| entry.chunk_type == RAW_IMAGE)
        .count();

    let mut images = Vec::new();
    for image in directory
        .entries
        .iter()
        .filter(|entry| entry.chunk_type == RAW_IMAGE && reachable.contains(&entry.object_id))
    {
        let chunk = chunk_bytes(&contents, image)
            .with_context(|| format!("bounded image chunk {}", image.object_id))?;
        let parent_type = directory
            .entry_by_object_id(image.parent_id)
            .map(|entry| entry.chunk_type);

        let field_native_ref = read_u16(chunk, NATIVE_REF);
        let field_native_entry = field_native_ref.and_then(|id| directory.entry_by_object_id(id));

        let direct_image_data = directory
            .entries_by_parent_id(image.object_id)
            .filter(|entry| entry.chunk_type == RAW_IMAGE_DATA)
            .collect::<Vec<_>>();
        let direct_native_entry = if direct_image_data.len() == 1 {
            direct_image_data.first().copied()
        } else {
            None
        };
        let direct_native_chunk =
            direct_native_entry.and_then(|entry| chunk_bytes(&contents, entry));
        let direct_native_wmf_offsets = direct_native_chunk
            .map(wmf_payload_offsets)
            .unwrap_or_default();
        let direct_native_payload_hashes = direct_native_chunk
            .map(|chunk| {
                direct_native_wmf_offsets
                    .iter()
                    .filter_map(|offset| chunk.get(*offset..).map(sha256_hex))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let direct_native_declared_profile = direct_native_chunk.map(wmf_declared_profile);

        let previous_entry = image
            .object_id
            .checked_sub(1)
            .and_then(|id| directory.entry_by_object_id(id));
        let next_entry = image
            .object_id
            .checked_add(1)
            .and_then(|id| directory.entry_by_object_id(id));

        let field_repl_ref = read_u16(chunk, REPL_REF);
        let field_repl_entry = field_repl_ref
            .filter(|id| *id != 0)
            .and_then(|id| directory.entry_by_object_id(id));
        let field_repl_chunk = field_repl_entry.and_then(|entry| chunk_bytes(&contents, entry));

        let child_types = directory
            .entries_by_parent_id(image.object_id)
            .map(|entry| format!("0x{:04x}", entry.chunk_type))
            .collect::<Vec<_>>();
        let filename_children = directory
            .entries_by_parent_id(image.object_id)
            .filter(|entry| entry.chunk_type == RAW_FILENAME)
            .count();

        images.push(json!({
            "object_id": image.object_id,
            "parent_id": image.parent_id,
            "parent_raw_type": parent_type.map(|value| format!("0x{value:04x}")),
            "service_word": image.service_word,
            "chunk_len": chunk.len(),
            "geometry": geometry(chunk),
            "child_raw_types": child_types,
            "filename_child_count": filename_children,
            "field_native_ref_at_0x72": field_native_ref,
            "field_native_target_raw_type": field_native_entry
                .map(|entry| format!("0x{:04x}", entry.chunk_type)),
            "direct_image_data_child_count": direct_image_data.len(),
            "direct_native_object_id": direct_native_entry.map(|entry| entry.object_id),
            "direct_native_chunk_len": direct_native_chunk.map(|chunk| chunk.len()),
            "direct_native_len_u32_at_0x04": direct_native_chunk
                .and_then(|chunk| read_u32(chunk, 0x04)),
            "direct_native_len_after_0x08": direct_native_chunk
                .and_then(|chunk| chunk.len().checked_sub(0x08)),
            "direct_native_wmf_valid_offsets": direct_native_wmf_offsets,
            "direct_native_wmf_payload_sha256": direct_native_payload_hashes,
            "direct_native_declared_profile": direct_native_declared_profile,
            "previous_object_raw_type": previous_entry
                .map(|entry| format!("0x{:04x}", entry.chunk_type)),
            "previous_object_parent_matches_image": previous_entry
                .is_some_and(|entry| entry.parent_id == image.object_id),
            "next_object_raw_type": next_entry
                .map(|entry| format!("0x{:04x}", entry.chunk_type)),
            "next_object_parent_matches_image": next_entry
                .is_some_and(|entry| entry.parent_id == image.object_id),
            "direct_native_len_u32_at_0x08": direct_native_chunk
                .and_then(|chunk| read_u32(chunk, 0x08)),
            "direct_native_len_u32_at_0x0c": direct_native_chunk
                .and_then(|chunk| read_u32(chunk, 0x0c)),
            "direct_native_len_after_0x10": direct_native_chunk
                .and_then(|chunk| chunk.len().checked_sub(0x10)),
            "field_replacement_ref_at_0x8a": field_repl_ref,
            "field_replacement_target_raw_type": field_repl_entry
                .map(|entry| format!("0x{:04x}", entry.chunk_type)),
            "field_replacement_target_parent_matches": field_repl_entry
                .is_some_and(|entry| entry.parent_id == image.object_id),
            "field_replacement": field_repl_chunk.map(gif_profile),
        }));
    }

    let receipt = json!({
        "schema": "chaptera.legacy22-image0002-profile.v1",
        "source_sha256": source_sha256,
        "eligible_reader_open": true,
        "physical_image_count": physical_image_count,
        "reachable_image_count": images.len(),
        "images": images,
    });
    fs::write(&output, serde_json::to_vec_pretty(&receipt)?)
        .with_context(|| format!("write {}", output.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gif_profile_requires_exact_header_lengths() {
        let mut chunk = vec![0_u8; 16];
        let payload = b"GIF89aexample";
        chunk[8..12].copy_from_slice(&(payload.len() as u32).to_le_bytes());
        chunk[12..16].copy_from_slice(&(payload.len() as u32).to_le_bytes());
        chunk.extend_from_slice(payload);
        let profile = gif_profile(&chunk);
        assert_eq!(profile["gif89a"], true);
        assert_eq!(profile["lengths_equal_payload"], true);
    }

    #[test]
    fn geometry_preserves_signed_endpoint_classification() {
        let mut chunk = vec![0_u8; 0x20];
        chunk[XS..XS + 4].copy_from_slice(&10_i32.to_le_bytes());
        chunk[YS..YS + 4].copy_from_slice(&20_i32.to_le_bytes());
        chunk[XE..XE + 4].copy_from_slice(&5_i32.to_le_bytes());
        chunk[YE..YE + 4].copy_from_slice(&30_i32.to_le_bytes());
        let profile = geometry(&chunk);
        assert_eq!(profile["carrier_present"], true);
        assert_eq!(profile["positive_rect"], false);
        assert_eq!(profile["nondegenerate"], true);
        assert_eq!(profile["delta_x_sign"], "negative");
        assert_eq!(profile["delta_y_sign"], "positive");
    }
}
