use anyhow::{Context, Result};
use pub_contents::{parse_legacy_0x22_directory, Legacy0x22Directory, Legacy0x22DirectoryEntry};
use pub_core::StreamPath;
use pub_model::Sha256Digest;
use pub_reader::{
    build_legacy_0x22_noquill_source_graph, rasterize_wmf_preview, validate_wmf_metafile,
    CONTENTS_STREAM_PATH,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
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
const WMF_PLACEABLE_KEY: u32 = 0x9ac6_cdd7;
const WMF_PLACEABLE_HEADER_BYTES: usize = 22;
const WMF_META_HEADER_BYTES: usize = 18;
const WMF_MAX_RECORDS: usize = 1_000_000;
const META_EOF_FUNCTION: u16 = 0x0000;
const META_REALIZEPALETTE_FUNCTION: u16 = 0x0035;
const META_SETPALENTRIES_FUNCTION: u16 = 0x0037;
const META_CREATEPALETTE_FUNCTION: u16 = 0x00f7;
const META_RESIZEPALETTE_FUNCTION: u16 = 0x0139;
const META_SELECTOBJECT_FUNCTION: u16 = 0x012d;
const META_SELECTPALETTE_FUNCTION: u16 = 0x0234;
const META_DIBCREATEPATTERNBRUSH_FUNCTION: u16 = 0x0142;
const META_CREATEPATTERNBRUSH_FUNCTION: u16 = 0x01f9;
const META_DELETEOBJECT_FUNCTION: u16 = 0x01f0;
const META_SETBKCOLOR_FUNCTION: u16 = 0x0201;
const META_SETTEXTCOLOR_FUNCTION: u16 = 0x0209;
const META_CREATEPENINDIRECT_FUNCTION: u16 = 0x02fa;
const META_CREATEFONTINDIRECT_FUNCTION: u16 = 0x02fb;
const META_CREATEBRUSHINDIRECT_FUNCTION: u16 = 0x02fc;
const META_POLYGON_FUNCTION: u16 = 0x0324;
const META_POLYLINE_FUNCTION: u16 = 0x0325;
const META_INTERSECTCLIPRECT_FUNCTION: u16 = 0x0416;
const META_CREATEBITMAPINDIRECT_FUNCTION: u16 = 0x02fd;
const META_ELLIPSE_FUNCTION: u16 = 0x0418;
const META_RECTANGLE_FUNCTION: u16 = 0x041b;
const META_ANIMATEPALETTE_FUNCTION: u16 = 0x0436;
const META_POLYPOLYGON_FUNCTION: u16 = 0x0538;
const META_TEXTOUT_FUNCTION: u16 = 0x0521;
const META_ESCAPE_FUNCTION: u16 = 0x0626;
const META_CREATEBITMAP_FUNCTION: u16 = 0x06fe;
const META_CREATEREGION_FUNCTION: u16 = 0x06ff;
const META_EXTTEXTOUT_FUNCTION: u16 = 0x0a32;
const META_STRETCHDIB_FUNCTION: u16 = 0x0f43;
const POSTSCRIPT_IGNORE_ESCAPE: u16 = 0x0026;

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

fn read_i16(bytes: &[u8], offset: usize) -> Option<i16> {
    let raw = bytes.get(offset..offset.checked_add(2)?)?;
    Some(i16::from_le_bytes([raw[0], raw[1]]))
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

fn write_u32(bytes: &mut [u8], offset: usize, value: u32) -> bool {
    let Some(dst) = bytes.get_mut(offset..offset.saturating_add(4)) else {
        return false;
    };
    dst.copy_from_slice(&value.to_le_bytes());
    true
}

fn wmf_header_offset(payload: &[u8]) -> usize {
    if read_u32(payload, 0) == Some(WMF_PLACEABLE_KEY) {
        WMF_PLACEABLE_HEADER_BYTES
    } else {
        0
    }
}

fn wmf_outer_size_rewrite_valid(payload: &[u8]) -> bool {
    let header_offset = wmf_header_offset(payload);
    let Some(meta_len) = payload.len().checked_sub(header_offset) else {
        return false;
    };
    if meta_len < 18 || meta_len % 2 != 0 {
        return false;
    }
    let Ok(words) = u32::try_from(meta_len / 2) else {
        return false;
    };
    let mut patched = payload.to_vec();
    if !write_u32(&mut patched, header_offset + 6, words) {
        return false;
    }
    validate_wmf_metafile(&patched).is_ok()
}

fn wmf_first_eof_profile(payload: &[u8]) -> Value {
    const META_HEADER_BYTES: usize = 18;
    const MIN_RECORD_WORDS: u32 = 3;
    const MAX_RECORDS: usize = 1_000_000;

    let header_offset = wmf_header_offset(payload);
    let Some(mut offset) = header_offset.checked_add(META_HEADER_BYTES) else {
        return json!({"found": false, "prefix_valid": false, "failure": "header_overflow"});
    };
    if offset > payload.len() {
        return json!({"found": false, "prefix_valid": false, "failure": "truncated_header"});
    }

    for _ in 0..MAX_RECORDS {
        if offset == payload.len() {
            return json!({"found": false, "prefix_valid": false, "failure": "no_eof"});
        }
        let Some(record_words) = read_u32(payload, offset) else {
            return json!({"found": false, "prefix_valid": false, "failure": "truncated_record_size"});
        };
        if record_words < MIN_RECORD_WORDS {
            return json!({"found": false, "prefix_valid": false, "failure": "invalid_record_size"});
        }
        let Ok(record_words) = usize::try_from(record_words) else {
            return json!({"found": false, "prefix_valid": false, "failure": "record_size_overflow"});
        };
        let Some(record_bytes) = record_words.checked_mul(2) else {
            return json!({"found": false, "prefix_valid": false, "failure": "record_size_overflow"});
        };
        let Some(record_end) = offset.checked_add(record_bytes) else {
            return json!({"found": false, "prefix_valid": false, "failure": "record_range_overflow"});
        };
        if record_end > payload.len() {
            return json!({"found": false, "prefix_valid": false, "failure": "record_out_of_bounds"});
        }
        let Some(function) = read_u16(payload, offset + 4) else {
            return json!({"found": false, "prefix_valid": false, "failure": "truncated_function"});
        };
        offset = record_end;
        if function == 0 {
            let Some(meta_len) = offset.checked_sub(header_offset) else {
                return json!({"found": true, "prefix_valid": false, "failure": "meta_range_underflow"});
            };
            if meta_len % 2 != 0 {
                return json!({"found": true, "prefix_valid": false, "failure": "odd_meta_length"});
            }
            let Ok(words) = u32::try_from(meta_len / 2) else {
                return json!({"found": true, "prefix_valid": false, "failure": "meta_size_overflow"});
            };
            let mut prefix = payload[..offset].to_vec();
            if !write_u32(&mut prefix, header_offset + 6, words) {
                return json!({"found": true, "prefix_valid": false, "failure": "patch_out_of_bounds"});
            }
            let validation = validate_wmf_metafile(&prefix);
            let prefix_valid = validation.is_ok();
            return json!({
                "found": true,
                "prefix_valid": prefix_valid,
                "suffix_len": payload.len() - offset,
                "validation_error_class": validation
                    .as_ref()
                    .err()
                    .map(|error| wmf_validation_error_class(&error.to_string())),
                "prefix_sha256": prefix_valid.then(|| sha256_hex(&prefix)),
            });
        }
    }

    json!({"found": false, "prefix_valid": false, "failure": "record_limit"})
}

fn wmf_bounded_prefix_profile(payload: &[u8]) -> Value {
    let placeable = read_u32(payload, 0) == Some(WMF_PLACEABLE_KEY);
    let header_offset = if placeable {
        WMF_PLACEABLE_HEADER_BYTES
    } else {
        0
    };
    let Some(declared_words) = read_u32(payload, header_offset + 6) else {
        return json!({
            "available": false,
            "placeable": placeable,
            "prefix_fits_payload": false,
            "prefix_valid": false,
        });
    };
    let Ok(declared_bytes) = usize::try_from(declared_words) else {
        return json!({
            "available": true,
            "placeable": placeable,
            "prefix_fits_payload": false,
            "prefix_valid": false,
        });
    };
    let Some(prefix_end) = header_offset.checked_add(declared_bytes.saturating_mul(2)) else {
        return json!({
            "available": true,
            "placeable": placeable,
            "prefix_fits_payload": false,
            "prefix_valid": false,
        });
    };
    let Some(prefix) = payload.get(..prefix_end) else {
        return json!({
            "available": true,
            "placeable": placeable,
            "prefix_fits_payload": false,
            "prefix_valid": false,
            "internal_declared_prefix_len": prefix_end,
        });
    };
    let validation = validate_wmf_metafile(prefix);
    let (prefix_valid, validation_error_class) = match &validation {
        Ok(_) => (true, None),
        Err(error) => (false, Some(wmf_validation_error_class(&error.to_string()))),
    };
    json!({
        "available": true,
        "placeable": placeable,
        "prefix_fits_payload": true,
        "prefix_valid": prefix_valid,
        "internal_declared_prefix_len": prefix_end,
        "suffix_len": payload.len() - prefix_end,
        "validation_error_class": validation_error_class,
        "prefix_sha256": prefix_valid.then(|| sha256_hex(prefix)),
    })
}

fn first_eof_wmf_candidate(payload: &[u8]) -> Option<Vec<u8>> {
    const META_HEADER_BYTES: usize = 18;
    const MIN_RECORD_WORDS: u32 = 3;
    const MAX_RECORDS: usize = 1_000_000;

    let header_offset = wmf_header_offset(payload);
    let mut offset = header_offset.checked_add(META_HEADER_BYTES)?;
    if offset > payload.len() {
        return None;
    }

    for _ in 0..MAX_RECORDS {
        if offset >= payload.len() {
            return None;
        }
        let record_words = usize::try_from(read_u32(payload, offset)?).ok()?;
        if record_words < usize::try_from(MIN_RECORD_WORDS).ok()? {
            return None;
        }
        let record_bytes = record_words.checked_mul(2)?;
        let record_end = offset.checked_add(record_bytes)?;
        if record_end > payload.len() {
            return None;
        }
        let function = read_u16(payload, offset + 4)?;
        offset = record_end;
        if function == 0 {
            let meta_len = offset.checked_sub(header_offset)?;
            if meta_len % 2 != 0 {
                return None;
            }
            let words = u32::try_from(meta_len / 2).ok()?;
            let mut prefix = payload.get(..offset)?.to_vec();
            if !write_u32(&mut prefix, header_offset + 6, words) {
                return None;
            }
            return validate_wmf_metafile(&prefix).is_ok().then_some(prefix);
        }
    }
    None
}

fn recovered_wmf_candidate(chunk: &[u8]) -> Option<(&'static str, Vec<u8>)> {
    let declared_len = usize::try_from(read_u32(chunk, 0x04)?).ok()?;
    let end = 0x08_usize.checked_add(declared_len)?;
    let payload = chunk.get(0x08..end)?;

    if validate_wmf_metafile(payload).is_ok() {
        return Some(("strict", payload.to_vec()));
    }

    let header_offset = wmf_header_offset(payload);
    if let Some(declared_words) = read_u32(payload, header_offset + 6) {
        if let Ok(declared_words) = usize::try_from(declared_words) {
            if let Some(meta_bytes) = declared_words.checked_mul(2) {
                if let Some(prefix_end) = header_offset.checked_add(meta_bytes) {
                    if let Some(prefix) = payload.get(..prefix_end) {
                        if validate_wmf_metafile(prefix).is_ok() {
                            return Some(("internal_declared_prefix", prefix.to_vec()));
                        }
                    }
                }
            }
        }
    }

    first_eof_wmf_candidate(payload).map(|bytes| ("first_eof_prefix", bytes))
}

fn wmf_raster_error_class(message: &str) -> &'static str {
    if message.starts_with("unsupported WMF record function") {
        "unsupported_record_function"
    } else if message.starts_with("unsupported WMF raster profile")
        || message.starts_with("unsupported WMF header")
    {
        "unsupported_raster_profile"
    } else if message.starts_with("unsupported WMF pen style") {
        "unsupported_pen_style"
    } else if message.starts_with("unsupported WMF brush style") {
        "unsupported_brush_style"
    } else if message.starts_with("unsupported WMF map mode") {
        "unsupported_map_mode"
    } else if message.starts_with("unsupported WMF ROP2 mode") {
        "unsupported_rop2_mode"
    } else if message.starts_with("unsupported WMF relative/absolute mode") {
        "unsupported_relative_mode"
    } else if message.starts_with("unsupported WMF polygon fill mode") {
        "unsupported_polygon_fill_mode"
    } else if message.starts_with("unsupported WMF stretch mode") {
        "unsupported_stretch_mode"
    } else if message.starts_with("unsupported WMF RESTOREDC value") {
        "unsupported_restore_dc"
    } else if message.starts_with("unsupported WMF escape") {
        "unsupported_escape"
    } else if message.starts_with("unsupported palette-dependent WMF COLORREF") {
        "palette_dependent_colorref"
    } else if message.starts_with("unsupported WMF Palette compatibility") {
        "unsupported_palette_profile"
    } else if message.starts_with("WMF Palette requires META_SELECTPALETTE") {
        "palette_select_api_mismatch"
    } else if message.starts_with("WMF SELECTPALETTE") {
        "select_palette_lifecycle"
    } else if message.starts_with("WMF REALIZEPALETTE") {
        "realize_palette_lifecycle"
    } else if message.starts_with("WMF DELETEOBJECT targets the selected Palette") {
        "delete_selected_palette"
    } else if message.starts_with("unsupported WMF ELLIPSE") {
        "unsupported_ellipse_profile"
    } else if message.starts_with("unsupported WMF POSTSCRIPT_IGNORE") {
        "unsupported_postscript_ignore_profile"
    } else if message.contains("object table") || message.contains("graphics object") {
        "object_table_or_object_kind"
    } else if message.contains("raster work") {
        "raster_work_limit"
    } else if message.contains("output") || message.contains("zero output extent") {
        "output_bound"
    } else if message.contains("coordinate transform") || message.contains("window extent") {
        "coordinate_or_window_transform"
    } else if message.contains("point count") || message.contains("polygon count") {
        "point_or_polygon_bound"
    } else if message.contains("truncated")
        || message.contains("overflow")
        || message.contains("declared")
        || message.contains("META_EOF")
        || message.contains("record size")
        || message.contains("missing WMF")
    {
        "malformed_or_structural"
    } else {
        "other_fail_closed"
    }
}

fn wmf_raster_error_detail(message: &str) -> String {
    if let Some(code) = message.strip_prefix("unsupported WMF record function ") {
        return format!("record_function_{code}");
    }
    if let Some(code) = message.strip_prefix("unsupported WMF escape function ") {
        return format!("escape_function_{code}");
    }
    if message == "WMF selects an unsupported pattern brush graphics object" {
        return "select_unsupported_pattern_brush".to_owned();
    }
    if message == "WMF selects an unsupported region graphics object" {
        return "select_unsupported_region".to_owned();
    }
    if message == "WMF selects an unsupported graphics object" {
        return "select_unsupported_object".to_owned();
    }
    if message.starts_with("unsupported palette-dependent WMF COLORREF mode ") {
        return "palette_dependent_colorref".to_owned();
    }
    if message.starts_with("unsupported WMF Palette compatibility payload length ") {
        return "palette_payload_length".to_owned();
    }
    if message.starts_with("unsupported WMF Palette compatibility profile ") {
        return "palette_profile".to_owned();
    }
    if message == "WMF Palette requires META_SELECTPALETTE" {
        return "palette_select_api_mismatch".to_owned();
    }
    if message == "WMF SELECTPALETTE has unsupported payload length" {
        return "select_palette_payload_length".to_owned();
    }
    if message == "WMF SELECTPALETTE refers to a non-Palette object" {
        return "select_palette_non_palette_object".to_owned();
    }
    if message == "WMF SELECTPALETTE refers to an empty or invalid slot" {
        return "select_palette_empty_slot".to_owned();
    }
    if message == "WMF REALIZEPALETTE has unsupported payload length" {
        return "realize_palette_payload_length".to_owned();
    }
    if message == "WMF REALIZEPALETTE without selected Palette" {
        return "realize_palette_without_selection".to_owned();
    }
    if message == "WMF REALIZEPALETTE selected Palette is unavailable" {
        return "realize_palette_unavailable".to_owned();
    }
    if message == "WMF DELETEOBJECT targets the selected Palette" {
        return "delete_selected_palette".to_owned();
    }
    if message.starts_with("unsupported WMF ELLIPSE ") {
        return wmf_raster_error_class(message).to_owned();
    }
    if message.starts_with("unsupported WMF POSTSCRIPT_IGNORE ") {
        return "postscript_ignore_profile".to_owned();
    }
    // Remaining rasterizer errors are generated by our own bounded parser and
    // contain no source strings or source identities. Preserve the short
    // generated message only when the broad classifier would otherwise erase it.
    if wmf_raster_error_class(message) == "other_fail_closed"
        && message.len() <= 160
        && (message.starts_with("WMF ") || message.starts_with("unsupported WMF "))
    {
        return format!("generated:{message}");
    }
    wmf_raster_error_class(message).to_owned()
}

fn sign_i16(value: Option<i16>) -> &'static str {
    match value {
        Some(value) if value < 0 => "negative",
        Some(0) => "zero",
        Some(_) => "positive",
        None => "missing",
    }
}

fn sign_i32(value: Option<i32>) -> &'static str {
    match value {
        Some(value) if value < 0 => "negative",
        Some(0) => "zero",
        Some(_) => "positive",
        None => "missing",
    }
}

fn wmf_records<'a>(wmf: &'a [u8]) -> Option<Vec<(u16, &'a [u8])>> {
    let header_offset = wmf_header_offset(wmf);
    let mut offset = header_offset.checked_add(WMF_META_HEADER_BYTES)?;
    if offset > wmf.len() {
        return None;
    }
    let mut records = Vec::new();
    for _ in 0..WMF_MAX_RECORDS {
        if offset >= wmf.len() {
            return None;
        }
        let record_words = usize::try_from(read_u32(wmf, offset)?).ok()?;
        if record_words < 3 {
            return None;
        }
        let record_bytes = record_words.checked_mul(2)?;
        let record_end = offset.checked_add(record_bytes)?;
        if record_end > wmf.len() {
            return None;
        }
        let function = read_u16(wmf, offset.checked_add(4)?)?;
        let params = wmf.get(offset.checked_add(6)?..record_end)?;
        records.push((function, params));
        offset = record_end;
        if function == META_EOF_FUNCTION {
            return Some(records);
        }
    }
    None
}

fn font_payload_profile(params: &[u8]) -> Value {
    let facename_32 = params.get(18..50);
    let facename_tail = params.get(18..).unwrap_or(&[]);
    let nul_index = facename_tail.iter().position(|byte| *byte == 0);
    let nonzero_after_nul = nul_index.is_some_and(|index| {
        facename_tail[index.saturating_add(1)..]
            .iter()
            .any(|byte| *byte != 0)
    });
    json!({
        "param_len": params.len(),
        "height": read_i16(params, 0),
        "width": read_i16(params, 2),
        "escapement": read_i16(params, 4),
        "orientation": read_i16(params, 6),
        "weight": read_u16(params, 8),
        "italic": params.get(10).copied(),
        "underline": params.get(11).copied(),
        "strikeout": params.get(12).copied(),
        "charset": params.get(13).copied(),
        "out_precision": params.get(14).copied(),
        "clip_precision": params.get(15).copied(),
        "quality": params.get(16).copied(),
        "pitch_and_family": params.get(17).copied(),
        "facename_tail_len": facename_tail.len(),
        "facename_32_present": facename_32.is_some(),
        "facename_nul_index": nul_index,
        "facename_nonzero_after_nul": nonzero_after_nul,
    })
}

fn object_creator(function: u16) -> bool {
    matches!(
        function,
        META_CREATEPALETTE_FUNCTION
            | META_DIBCREATEPATTERNBRUSH_FUNCTION
            | META_CREATEPATTERNBRUSH_FUNCTION
            | META_CREATEPENINDIRECT_FUNCTION
            | META_CREATEFONTINDIRECT_FUNCTION
            | META_CREATEBRUSHINDIRECT_FUNCTION
            | META_CREATEBITMAPINDIRECT_FUNCTION
            | META_CREATEBITMAP_FUNCTION
            | META_CREATEREGION_FUNCTION
    )
}

fn bump_function(counts: &mut BTreeMap<String, usize>, function: u16) {
    *counts.entry(format!("0x{function:04x}")).or_default() += 1;
}

fn font_blocker_profile(records: &[(u16, &[u8])]) -> Value {
    let first_font = records
        .iter()
        .find(|(function, _)| *function == META_CREATEFONTINDIRECT_FUNCTION)
        .map(|(_, params)| font_payload_profile(params));
    let all_font_records = records
        .iter()
        .filter(|(function, _)| *function == META_CREATEFONTINDIRECT_FUNCTION)
        .map(|(_, params)| font_payload_profile(params))
        .collect::<Vec<_>>();

    let mut objects = Vec::<Option<(u16, usize)>>::new();
    let mut first_font_record_index = None;
    let mut font_creation_count = 0usize;
    let mut font_select_count = 0usize;
    let mut font_delete_count = 0usize;
    let mut font_delete_while_selected_count = 0usize;
    let mut font_slot_reuse_count = 0usize;
    let mut text_output_record_count = 0usize;
    let mut text_output_after_first_font_count = 0usize;
    let mut text_output_with_selected_font_count = 0usize;
    let mut selected_font_slot = None::<usize>;
    let mut font_slots = BTreeSet::<usize>::new();
    let mut deleted_font_slots = BTreeSet::<usize>::new();
    let mut reused_font_slots = BTreeSet::<usize>::new();
    let mut immediate_after_font_create_counts = BTreeMap::<String, usize>::new();
    let mut immediate_after_font_select_counts = BTreeMap::<String, usize>::new();
    let mut downstream_function_counts = BTreeMap::<String, usize>::new();
    let mut font_slot_reuse_creator_counts = BTreeMap::<String, usize>::new();
    let mut unknown_select_count = 0usize;
    let mut unknown_delete_count = 0usize;

    for (record_index, (function, params)) in records.iter().enumerate() {
        if first_font_record_index.is_some_and(|first| record_index > first)
            && *function != META_EOF_FUNCTION
        {
            bump_function(&mut downstream_function_counts, *function);
        }

        if object_creator(*function) {
            let slot = allocate_probe_object(&mut objects, (*function, record_index));
            if deleted_font_slots.remove(&slot) {
                font_slot_reuse_count += 1;
                reused_font_slots.insert(slot);
                bump_function(&mut font_slot_reuse_creator_counts, *function);
            }
            if *function == META_CREATEFONTINDIRECT_FUNCTION {
                first_font_record_index.get_or_insert(record_index);
                font_creation_count += 1;
                font_slots.insert(slot);
                if let Some((next_function, _)) = records.get(record_index + 1) {
                    bump_function(&mut immediate_after_font_create_counts, *next_function);
                }
            }
        }

        match *function {
            META_SELECTOBJECT_FUNCTION => {
                let Some(slot) = read_u16(params, 0).map(usize::from) else {
                    unknown_select_count += 1;
                    continue;
                };
                match objects.get(slot).and_then(|entry| *entry) {
                    Some((META_CREATEFONTINDIRECT_FUNCTION, _)) => {
                        font_select_count += 1;
                        selected_font_slot = Some(slot);
                        if let Some((next_function, _)) = records.get(record_index + 1) {
                            bump_function(&mut immediate_after_font_select_counts, *next_function);
                        }
                    }
                    Some(_) => {}
                    None => unknown_select_count += 1,
                }
            }
            META_DELETEOBJECT_FUNCTION => {
                let Some(slot) = read_u16(params, 0).map(usize::from) else {
                    unknown_delete_count += 1;
                    continue;
                };
                let Some(entry) = objects.get_mut(slot) else {
                    unknown_delete_count += 1;
                    continue;
                };
                match *entry {
                    Some((META_CREATEFONTINDIRECT_FUNCTION, _)) => {
                        font_delete_count += 1;
                        if selected_font_slot == Some(slot) {
                            font_delete_while_selected_count += 1;
                            selected_font_slot = None;
                        }
                        deleted_font_slots.insert(slot);
                        *entry = None;
                    }
                    Some(_) => *entry = None,
                    None => unknown_delete_count += 1,
                }
            }
            META_TEXTOUT_FUNCTION | META_EXTTEXTOUT_FUNCTION => {
                text_output_record_count += 1;
                if first_font_record_index.is_some() {
                    text_output_after_first_font_count += 1;
                }
                if selected_font_slot.is_some() {
                    text_output_with_selected_font_count += 1;
                }
            }
            _ => {}
        }
    }

    json!({
        "kind": "createfontindirect",
        "font_record": first_font,
        "all_font_records": all_font_records,
        "lifecycle": {
            "first_font_record_index": first_font_record_index,
            "font_creation_count": font_creation_count,
            "font_slots": font_slots,
            "font_select_count": font_select_count,
            "font_delete_count": font_delete_count,
            "font_delete_while_selected_count": font_delete_while_selected_count,
            "font_slot_reuse_count": font_slot_reuse_count,
            "reused_font_slots": reused_font_slots,
            "font_slot_reuse_creator_counts": font_slot_reuse_creator_counts,
            "text_output_record_count": text_output_record_count,
            "text_output_after_first_font_count": text_output_after_first_font_count,
            "text_output_with_selected_font_count": text_output_with_selected_font_count,
            "immediate_after_font_create_counts": immediate_after_font_create_counts,
            "immediate_after_font_select_counts": immediate_after_font_select_counts,
            "downstream_function_counts": downstream_function_counts,
            "unknown_select_count": unknown_select_count,
            "unknown_delete_count": unknown_delete_count,
        }
    })
}

fn palette_payload_profile(params: &[u8]) -> Value {
    let start = read_u16(params, 0);
    let entry_count = read_u16(params, 2).map(usize::from);
    let expected_len = entry_count.and_then(|count| count.checked_mul(4)?.checked_add(4));
    let entry_flags_nonzero_count = entry_count
        .filter(|_| expected_len == Some(params.len()))
        .map(|count| {
            (0..count)
                .filter(|index| {
                    params
                        .get(4 + index * 4 + 3)
                        .is_some_and(|flags| *flags != 0)
                })
                .count()
        });

    json!({
        "param_len": params.len(),
        "start": start.map(|value| format!("0x{value:04x}")),
        "entry_count": entry_count,
        "expected_len": expected_len,
        "exact_length": expected_len == Some(params.len()),
        "entry_flags_nonzero_count": entry_flags_nonzero_count,
    })
}

fn colorref_mode(value: Option<u32>) -> &'static str {
    match value {
        Some(value) if value & 0xff00_0000 == 0 => "rgb",
        Some(value) if value & 0xff00_0000 == 0x0100_0000 => "palette_index",
        Some(value) if value & 0xff00_0000 == 0x0200_0000 => "palette_rgb",
        Some(_) => "other",
        None => "missing",
    }
}

fn palette_blocker_profile(records: &[(u16, &[u8])]) -> Value {
    let first_palette = records
        .iter()
        .find(|(function, _)| *function == META_CREATEPALETTE_FUNCTION)
        .map(|(_, params)| palette_payload_profile(params));

    let mut objects = Vec::<Option<(u16, usize)>>::new();
    let mut first_palette_record_index = None;
    let mut palette_creation_count = 0usize;
    let mut palette_select_count = 0usize;
    let mut palette_delete_count = 0usize;
    let mut palette_delete_while_selected_count = 0usize;
    let mut palette_slot_reuse_count = 0usize;
    let mut selected_palette_slot = None::<usize>;
    let mut palette_slots = BTreeSet::<usize>::new();
    let mut deleted_palette_slots = BTreeSet::<usize>::new();
    let mut reused_palette_slots = BTreeSet::<usize>::new();
    let mut immediate_after_palette_create_counts = BTreeMap::<String, usize>::new();
    let mut immediate_after_palette_select_counts = BTreeMap::<String, usize>::new();
    let mut selected_palette_function_counts = BTreeMap::<String, usize>::new();
    let mut palette_slot_reuse_creator_counts = BTreeMap::<String, usize>::new();
    let mut realize_palette_count = 0usize;
    let mut set_palette_entries_count = 0usize;
    let mut resize_palette_count = 0usize;
    let mut animate_palette_count = 0usize;
    let mut unknown_select_palette_count = 0usize;
    let mut unknown_delete_count = 0usize;
    let mut all_colorref_mode_counts = BTreeMap::<String, usize>::new();
    let mut dib_color_usage_counts = BTreeMap::<String, usize>::new();
    let mut select_palette_param_len_counts = BTreeMap::<String, usize>::new();
    let mut realize_palette_param_len_counts = BTreeMap::<String, usize>::new();
    let mut palette_param_len_counts = BTreeMap::<String, usize>::new();
    let mut palette_start_counts = BTreeMap::<String, usize>::new();
    let mut palette_entry_count_counts = BTreeMap::<String, usize>::new();
    let mut palette_exact_length_counts = BTreeMap::<String, usize>::new();

    for (record_index, (function, params)) in records.iter().enumerate() {
        let colorref = match *function {
            META_SETBKCOLOR_FUNCTION | META_SETTEXTCOLOR_FUNCTION => read_u32(params, 0),
            META_CREATEPENINDIRECT_FUNCTION => read_u32(params, 6),
            META_CREATEBRUSHINDIRECT_FUNCTION => read_u32(params, 2),
            _ => None,
        };
        if matches!(
            *function,
            META_SETBKCOLOR_FUNCTION
                | META_SETTEXTCOLOR_FUNCTION
                | META_CREATEPENINDIRECT_FUNCTION
                | META_CREATEBRUSHINDIRECT_FUNCTION
        ) {
            *all_colorref_mode_counts
                .entry(colorref_mode(colorref).to_owned())
                .or_default() += 1;
        }
        match *function {
            META_DIBCREATEPATTERNBRUSH_FUNCTION => {
                let usage = read_u16(params, 2)
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "missing".to_owned());
                *dib_color_usage_counts
                    .entry(format!("pattern:{usage}"))
                    .or_default() += 1;
            }
            META_STRETCHDIB_FUNCTION => {
                let usage = read_u16(params, 4)
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "missing".to_owned());
                *dib_color_usage_counts
                    .entry(format!("stretchdib:{usage}"))
                    .or_default() += 1;
            }
            _ => {}
        }

        if selected_palette_slot.is_some()
            && !matches!(
                *function,
                META_CREATEPALETTE_FUNCTION | META_SELECTPALETTE_FUNCTION | META_EOF_FUNCTION
            )
        {
            bump_function(&mut selected_palette_function_counts, *function);
        }

        if object_creator(*function) {
            let slot = allocate_probe_object(&mut objects, (*function, record_index));
            if deleted_palette_slots.remove(&slot) {
                palette_slot_reuse_count += 1;
                reused_palette_slots.insert(slot);
                bump_function(&mut palette_slot_reuse_creator_counts, *function);
            }
            if *function == META_CREATEPALETTE_FUNCTION {
                first_palette_record_index.get_or_insert(record_index);
                palette_creation_count += 1;
                palette_slots.insert(slot);
                *palette_param_len_counts
                    .entry(params.len().to_string())
                    .or_default() += 1;
                let start = read_u16(params, 0)
                    .map(|value| format!("0x{value:04x}"))
                    .unwrap_or_else(|| "missing".to_owned());
                *palette_start_counts.entry(start).or_default() += 1;
                let entry_count = read_u16(params, 2).map(usize::from);
                *palette_entry_count_counts
                    .entry(
                        entry_count
                            .map(|value| value.to_string())
                            .unwrap_or_else(|| "missing".to_owned()),
                    )
                    .or_default() += 1;
                let exact_length = entry_count
                    .and_then(|count| count.checked_mul(4)?.checked_add(4))
                    == Some(params.len());
                *palette_exact_length_counts
                    .entry(exact_length.to_string())
                    .or_default() += 1;
                if let Some((next_function, _)) = records.get(record_index + 1) {
                    bump_function(&mut immediate_after_palette_create_counts, *next_function);
                }
            }
        }

        match *function {
            META_SELECTPALETTE_FUNCTION => {
                *select_palette_param_len_counts
                    .entry(params.len().to_string())
                    .or_default() += 1;
                let Some(slot) = read_u16(params, 0).map(usize::from) else {
                    unknown_select_palette_count += 1;
                    continue;
                };
                match objects.get(slot).and_then(|entry| *entry) {
                    Some((META_CREATEPALETTE_FUNCTION, _)) => {
                        palette_select_count += 1;
                        selected_palette_slot = Some(slot);
                        if let Some((next_function, _)) = records.get(record_index + 1) {
                            bump_function(
                                &mut immediate_after_palette_select_counts,
                                *next_function,
                            );
                        }
                    }
                    Some(_) | None => unknown_select_palette_count += 1,
                }
            }
            META_DELETEOBJECT_FUNCTION => {
                let Some(slot) = read_u16(params, 0).map(usize::from) else {
                    unknown_delete_count += 1;
                    continue;
                };
                let Some(entry) = objects.get_mut(slot) else {
                    unknown_delete_count += 1;
                    continue;
                };
                match *entry {
                    Some((META_CREATEPALETTE_FUNCTION, _)) => {
                        palette_delete_count += 1;
                        if selected_palette_slot == Some(slot) {
                            palette_delete_while_selected_count += 1;
                            selected_palette_slot = None;
                        }
                        deleted_palette_slots.insert(slot);
                        *entry = None;
                    }
                    Some(_) => *entry = None,
                    None => unknown_delete_count += 1,
                }
            }
            META_REALIZEPALETTE_FUNCTION if selected_palette_slot.is_some() => {
                realize_palette_count += 1;
                *realize_palette_param_len_counts
                    .entry(params.len().to_string())
                    .or_default() += 1;
            }
            META_SETPALENTRIES_FUNCTION if selected_palette_slot.is_some() => {
                set_palette_entries_count += 1;
            }
            META_RESIZEPALETTE_FUNCTION if selected_palette_slot.is_some() => {
                resize_palette_count += 1;
            }
            META_ANIMATEPALETTE_FUNCTION if selected_palette_slot.is_some() => {
                animate_palette_count += 1;
            }
            _ => {}
        }
    }

    json!({
        "kind": "createpalette",
        "palette_record": first_palette,
        "lifecycle": {
            "first_palette_record_index": first_palette_record_index,
            "palette_creation_count": palette_creation_count,
            "palette_slots": palette_slots,
            "palette_select_count": palette_select_count,
            "palette_delete_count": palette_delete_count,
            "palette_delete_while_selected_count": palette_delete_while_selected_count,
            "palette_slot_reuse_count": palette_slot_reuse_count,
            "reused_palette_slots": reused_palette_slots,
            "palette_slot_reuse_creator_counts": palette_slot_reuse_creator_counts,
            "realize_palette_count": realize_palette_count,
            "set_palette_entries_count": set_palette_entries_count,
            "resize_palette_count": resize_palette_count,
            "animate_palette_count": animate_palette_count,
            "immediate_after_palette_create_counts": immediate_after_palette_create_counts,
            "immediate_after_palette_select_counts": immediate_after_palette_select_counts,
            "selected_palette_function_counts": selected_palette_function_counts,
            "unknown_select_palette_count": unknown_select_palette_count,
            "unknown_delete_count": unknown_delete_count,
            "all_colorref_mode_counts": all_colorref_mode_counts,
            "dib_color_usage_counts": dib_color_usage_counts,
            "select_palette_param_len_counts": select_palette_param_len_counts,
            "realize_palette_param_len_counts": realize_palette_param_len_counts,
            "palette_param_len_counts": palette_param_len_counts,
            "palette_start_counts": palette_start_counts,
            "palette_entry_count_counts": palette_entry_count_counts,
            "palette_exact_length_counts": palette_exact_length_counts,
        }
    })
}

fn dib_pattern_brush_blocker_profile(params: &[u8]) -> Value {
    json!({
        "kind": "dib_pattern_brush",
        "param_len": params.len(),
        "style": read_u16(params, 0),
        "color_usage": read_u16(params, 2),
        "header_size": read_u32(params, 4),
        "width": read_i32(params, 8),
        "height": read_i32(params, 12),
        "planes": read_u16(params, 16),
        "bit_count": read_u16(params, 18),
        "compression": read_u32(params, 20),
        "image_size": read_u32(params, 24),
        "colors_used": read_u32(params, 36),
    })
}

fn creator_style_profile(function: u16, params: &[u8]) -> Value {
    match function {
        META_CREATEPENINDIRECT_FUNCTION => json!({
            "creator": "pen",
            "style": read_u16(params, 0),
            "width_x_sign": sign_i16(read_i16(params, 2)),
            "param_len": params.len(),
        }),
        META_CREATEBRUSHINDIRECT_FUNCTION => json!({
            "creator": "brush",
            "style": read_u16(params, 0),
            "param_len": params.len(),
        }),
        META_DIBCREATEPATTERNBRUSH_FUNCTION => json!({
            "creator": "dib_pattern_brush",
            "param_len": params.len(),
        }),
        other => json!({
            "creator_function": format!("0x{other:04x}"),
            "param_len": params.len(),
        }),
    }
}

fn ellipse_blocker_profile(records: &[(u16, &[u8])]) -> Value {
    let mut objects = Vec::<Option<(u16, usize)>>::new();
    let mut active_pen = None::<(u16, usize)>;
    let mut active_brush = None::<(u16, usize)>;
    let mut clip_intersection_count = 0usize;

    for (record_index, (function, params)) in records.iter().enumerate() {
        if object_creator(*function) {
            allocate_probe_object(&mut objects, (*function, record_index));
        }

        match *function {
            META_DELETEOBJECT_FUNCTION => {
                if let Some(slot) = read_u16(params, 0).map(usize::from) {
                    if let Some(entry) = objects.get_mut(slot) {
                        *entry = None;
                    }
                }
            }
            META_SELECTOBJECT_FUNCTION => {
                let Some(slot) = read_u16(params, 0).map(usize::from) else {
                    continue;
                };
                let Some((creator, creator_index)) = objects.get(slot).and_then(|entry| *entry)
                else {
                    continue;
                };
                if creator == META_CREATEPENINDIRECT_FUNCTION {
                    active_pen = Some((creator, creator_index));
                } else if matches!(
                    creator,
                    META_CREATEBRUSHINDIRECT_FUNCTION
                        | META_DIBCREATEPATTERNBRUSH_FUNCTION
                        | META_CREATEPATTERNBRUSH_FUNCTION
                ) {
                    active_brush = Some((creator, creator_index));
                }
            }
            0x0416 => {
                clip_intersection_count += 1;
            }
            META_ELLIPSE_FUNCTION => {
                let bottom = read_i16(params, 0);
                let right = read_i16(params, 2);
                let top = read_i16(params, 4);
                let left = read_i16(params, 6);
                let width_sign = match (right, left) {
                    (Some(r), Some(l)) => sign_i32(Some(i32::from(r) - i32::from(l))),
                    _ => "missing",
                };
                let height_sign = match (bottom, top) {
                    (Some(b), Some(t)) => sign_i32(Some(i32::from(b) - i32::from(t))),
                    _ => "missing",
                };
                let pen = active_pen.and_then(|(creator, creator_index)| {
                    records
                        .get(creator_index)
                        .map(|(_, creator_params)| creator_style_profile(creator, creator_params))
                });
                let brush = active_brush.and_then(|(creator, creator_index)| {
                    records
                        .get(creator_index)
                        .map(|(_, creator_params)| creator_style_profile(creator, creator_params))
                });
                return json!({
                    "kind": "ellipse",
                    "record_index": record_index,
                    "param_len": params.len(),
                    "width_sign": width_sign,
                    "height_sign": height_sign,
                    "nondegenerate": !matches!(width_sign, "zero" | "missing")
                        && !matches!(height_sign, "zero" | "missing"),
                    "active_pen": pen.unwrap_or_else(|| json!({"creator": "stock"})),
                    "active_brush": brush.unwrap_or_else(|| json!({"creator": "stock"})),
                    "clip_intersection_count_before": clip_intersection_count,
                    "next_function": records
                        .get(record_index + 1)
                        .map(|(next, _)| format!("0x{next:04x}")),
                });
            }
            _ => {}
        }
    }

    json!({"kind": "ellipse", "record": "missing"})
}

fn stretchdib_blocker_profile(params: &[u8]) -> Value {
    const FIXED_BYTES: usize = 22;
    let dib = params.get(FIXED_BYTES..).unwrap_or(&[]);
    let header_size = read_u32(dib, 0);
    json!({
        "kind": "stretchdib",
        "param_len": params.len(),
        "raster_operation": read_u32(params, 0).map(|value| format!("0x{value:08x}")),
        "color_usage": read_u16(params, 4),
        "src_height": read_i16(params, 6),
        "src_width": read_i16(params, 8),
        "src_y": read_i16(params, 10),
        "src_x": read_i16(params, 12),
        "src_height_sign": sign_i16(read_i16(params, 6)),
        "src_width_sign": sign_i16(read_i16(params, 8)),
        "dest_height": read_i16(params, 14),
        "dest_width": read_i16(params, 16),
        "dest_y": read_i16(params, 18),
        "dest_x": read_i16(params, 20),
        "dest_height_sign": sign_i16(read_i16(params, 14)),
        "dest_width_sign": sign_i16(read_i16(params, 16)),
        "dib_len": dib.len(),
        "dib_header_size": header_size,
        "dib_width": read_i32(dib, 4),
        "dib_height": read_i32(dib, 8),
        "dib_width_sign": sign_i32(read_i32(dib, 4)),
        "dib_height_sign": sign_i32(read_i32(dib, 8)),
        "dib_planes": read_u16(dib, 12),
        "dib_bit_count": read_u16(dib, 14),
        "dib_compression": read_u32(dib, 16),
        "dib_image_size": read_u32(dib, 20),
        "dib_colors_used": read_u32(dib, 32),
        "dib_is_bitmapinfoheader_or_later": header_size.is_some_and(|size| size >= 40),
    })
}

fn stretchdib_context_profile(records: &[(u16, &[u8])]) -> Value {
    let Some((target_index, (_, params))) =
        records.iter().enumerate().find(|(_, (function, params))| {
            *function == META_STRETCHDIB_FUNCTION && read_u32(params, 0) == Some(0x0088_00c6)
        })
    else {
        return json!({"kind": "stretchdib", "record": "missing"});
    };

    let mut profile = stretchdib_blocker_profile(params);
    let mut prior_drawing_counts = BTreeMap::<String, usize>::new();
    let mut prior_canvas_mutation_count = 0usize;
    let mut prior_clip_count = 0usize;
    for (function, _) in &records[..target_index] {
        if matches!(
            *function,
            META_POLYGON_FUNCTION
                | META_POLYLINE_FUNCTION
                | META_ELLIPSE_FUNCTION
                | META_RECTANGLE_FUNCTION
                | META_POLYPOLYGON_FUNCTION
                | META_STRETCHDIB_FUNCTION
        ) {
            bump_function(&mut prior_drawing_counts, *function);
            prior_canvas_mutation_count += 1;
        }
        if *function == META_INTERSECTCLIPRECT_FUNCTION {
            prior_clip_count += 1;
        }
    }

    let previous_function = target_index
        .checked_sub(1)
        .and_then(|index| records.get(index))
        .map(|(function, _)| format!("0x{function:04x}"));
    let next_function = records
        .get(target_index + 1)
        .map(|(function, _)| format!("0x{function:04x}"));

    let mut following_drawing_counts = BTreeMap::<String, usize>::new();
    let mut following_stretchdib_rop_counts = BTreeMap::<String, usize>::new();
    let mut first_following_canvas_mutation = None::<String>;
    for (function, following_params) in records.iter().skip(target_index + 1) {
        if matches!(
            *function,
            META_POLYGON_FUNCTION
                | META_POLYLINE_FUNCTION
                | META_ELLIPSE_FUNCTION
                | META_RECTANGLE_FUNCTION
                | META_POLYPOLYGON_FUNCTION
                | META_STRETCHDIB_FUNCTION
        ) {
            bump_function(&mut following_drawing_counts, *function);
            first_following_canvas_mutation.get_or_insert_with(|| format!("0x{function:04x}"));
        }
        if *function == META_STRETCHDIB_FUNCTION {
            let rop = read_u32(following_params, 0)
                .map(|value| format!("0x{value:08x}"))
                .unwrap_or_else(|| "missing".to_owned());
            *following_stretchdib_rop_counts.entry(rop).or_default() += 1;
        }
    }

    if let Some(object) = profile.as_object_mut() {
        object.insert("target_record_index".to_owned(), json!(target_index));
        object.insert("previous_function".to_owned(), json!(previous_function));
        object.insert("next_function".to_owned(), json!(next_function));
        object.insert(
            "prior_canvas_mutation_count".to_owned(),
            json!(prior_canvas_mutation_count),
        );
        object.insert(
            "prior_drawing_function_counts".to_owned(),
            json!(prior_drawing_counts),
        );
        object.insert(
            "prior_intersect_cliprect_count".to_owned(),
            json!(prior_clip_count),
        );
        object.insert(
            "following_drawing_function_counts".to_owned(),
            json!(following_drawing_counts),
        );
        object.insert(
            "following_stretchdib_rop_counts".to_owned(),
            json!(following_stretchdib_rop_counts),
        );
        object.insert(
            "first_following_canvas_mutation".to_owned(),
            json!(first_following_canvas_mutation),
        );
    }
    profile
}

fn postscript_ignore_blocker_profile(params: &[u8]) -> Value {
    let byte_count = read_u16(params, 2).map(usize::from);
    let payload_len = params.len().saturating_sub(4);
    json!({
        "kind": "postscript_ignore",
        "param_len": params.len(),
        "escape_function": read_u16(params, 0).map(|value| format!("0x{value:04x}")),
        "byte_count": byte_count,
        "payload_len": payload_len,
        "byte_count_matches_payload": byte_count == Some(payload_len),
    })
}

fn postscript_data_blocker_profile(records: &[(u16, &[u8])]) -> Value {
    let Some((record_index, (_, params))) =
        records.iter().enumerate().find(|(_, (function, params))| {
            *function == META_ESCAPE_FUNCTION && read_u16(params, 0) == Some(0x0025)
        })
    else {
        return json!({"kind": "postscript_data", "record": "missing"});
    };

    let byte_count = read_u16(params, 2).map(usize::from);
    let payload = params.get(4..).unwrap_or(&[]);
    let declared_payload = byte_count
        .and_then(|count| payload.get(..count))
        .unwrap_or(&[]);
    let printable_ascii_count = declared_payload
        .iter()
        .filter(|byte| matches!(**byte, 0x20..=0x7e | b'\n' | b'\r' | b'\t'))
        .count();
    let high_bit_count = declared_payload
        .iter()
        .filter(|byte| **byte >= 0x80)
        .count();
    let nul_count = declared_payload.iter().filter(|byte| **byte == 0).count();
    let lowercase = declared_payload
        .iter()
        .map(|byte| byte.to_ascii_lowercase())
        .collect::<Vec<_>>();
    let has_token = |needle: &[u8]| {
        lowercase
            .windows(needle.len())
            .any(|window| window == needle)
    };

    let previous_function = record_index
        .checked_sub(1)
        .and_then(|index| records.get(index))
        .map(|(function, _)| format!("0x{function:04x}"));
    let next_function = records
        .get(record_index + 1)
        .map(|(function, _)| format!("0x{function:04x}"));

    json!({
        "kind": "postscript_data",
        "record_index": record_index,
        "param_len": params.len(),
        "escape_function": read_u16(params, 0).map(|value| format!("0x{value:04x}")),
        "byte_count": byte_count,
        "payload_len": payload.len(),
        "byte_count_matches_payload": byte_count == Some(payload.len()),
        "declared_payload_sha256": (!declared_payload.is_empty()).then(|| sha256_hex(declared_payload)),
        "printable_ascii_count": printable_ascii_count,
        "high_bit_count": high_bit_count,
        "nul_count": nul_count,
        "starts_percent_bang": declared_payload.starts_with(b"%!"),
        "contains_image_operator": has_token(b"image"),
        "contains_imagemask_operator": has_token(b"imagemask"),
        "contains_show_operator": has_token(b"show"),
        "contains_stroke_operator": has_token(b"stroke"),
        "contains_fill_operator": has_token(b"fill"),
        "contains_moveto_operator": has_token(b"moveto"),
        "contains_lineto_operator": has_token(b"lineto"),
        "previous_function": previous_function,
        "next_function": next_function,
    })
}

fn allocate_probe_object(objects: &mut Vec<Option<(u16, usize)>>, object: (u16, usize)) -> usize {
    if let Some((index, slot)) = objects
        .iter_mut()
        .enumerate()
        .find(|(_, slot)| slot.is_none())
    {
        *slot = Some(object);
        index
    } else {
        let index = objects.len();
        objects.push(Some(object));
        index
    }
}

fn selected_unsupported_blocker_profile(records: &[(u16, &[u8])]) -> Value {
    let mut objects = Vec::<Option<(u16, usize)>>::new();
    for (function, params) in records {
        match *function {
            META_CREATEPENINDIRECT_FUNCTION
            | META_CREATEBRUSHINDIRECT_FUNCTION
            | META_DIBCREATEPATTERNBRUSH_FUNCTION
            | META_CREATEREGION_FUNCTION => {
                allocate_probe_object(&mut objects, (*function, params.len()));
            }
            META_DELETEOBJECT_FUNCTION => {
                let Some(index) = read_u16(params, 0).map(usize::from) else {
                    continue;
                };
                if let Some(slot) = objects.get_mut(index) {
                    *slot = None;
                }
            }
            META_SELECTOBJECT_FUNCTION => {
                let Some(index) = read_u16(params, 0).map(usize::from) else {
                    continue;
                };
                let Some((creator, creator_param_len)) = objects.get(index).and_then(|slot| *slot)
                else {
                    continue;
                };
                if matches!(
                    creator,
                    META_DIBCREATEPATTERNBRUSH_FUNCTION | META_CREATEREGION_FUNCTION
                ) {
                    return json!({
                        "kind": "selected_unsupported_object",
                        "selected_slot": index,
                        "creator_function": format!("0x{creator:04x}"),
                        "creator_param_len": creator_param_len,
                    });
                }
            }
            _ => {}
        }
    }
    json!({
        "kind": "selected_unsupported_object",
        "creator_function": "unresolved",
    })
}

fn wmf_blocker_profile(wmf: &[u8], detail: &str) -> Value {
    let Some(records) = wmf_records(wmf) else {
        return json!({"kind": "record_walk_failed"});
    };
    match detail {
        "record_function_0x00f7"
        | "palette_payload_length"
        | "palette_profile"
        | "palette_select_api_mismatch"
        | "select_palette_payload_length"
        | "select_palette_non_palette_object"
        | "select_palette_empty_slot"
        | "realize_palette_payload_length"
        | "realize_palette_without_selection"
        | "realize_palette_unavailable"
        | "delete_selected_palette"
        | "palette_dependent_colorref" => palette_blocker_profile(&records),
        "record_function_0x02fb"
        | "generated:unsupported WMF Font compatibility profile"
        | "generated:unsupported WMF Font compatibility payload length 36" => {
            font_blocker_profile(&records)
        }
        "record_function_0x0418" => ellipse_blocker_profile(&records),
        "record_function_0x0f43" | "generated:unsupported WMF STRETCHDIB profile" => {
            stretchdib_context_profile(&records)
        }
        "generated:unsupported WMF DIB pattern brush payload length" => records
            .iter()
            .find(|(function, _)| *function == META_DIBCREATEPATTERNBRUSH_FUNCTION)
            .map(|(_, params)| dib_pattern_brush_blocker_profile(params))
            .unwrap_or_else(|| json!({"kind": "dib_pattern_brush", "record": "missing"})),
        "escape_function_0x0026" => records
            .iter()
            .find(|(function, params)| {
                *function == META_ESCAPE_FUNCTION
                    && read_u16(params, 0) == Some(POSTSCRIPT_IGNORE_ESCAPE)
            })
            .map(|(_, params)| postscript_ignore_blocker_profile(params))
            .unwrap_or_else(|| json!({"kind": "postscript_ignore", "record": "missing"})),
        "escape_function_0x0025" => postscript_data_blocker_profile(&records),
        "select_unsupported_object" => selected_unsupported_blocker_profile(&records),
        _ => json!({"kind": "other", "detail": detail}),
    }
}

fn wmf_raster_profile(chunk: &[u8]) -> Value {
    let Some((recovery_class, wmf)) = recovered_wmf_candidate(chunk) else {
        return json!({
            "candidate": false,
            "recovery_class": null,
            "raster_success": false,
            "raster_error_class": "no_bounded_wmf_candidate",
        });
    };

    match rasterize_wmf_preview(&wmf, 512, 512) {
        Ok(preview) => json!({
            "candidate": true,
            "recovery_class": recovery_class,
            "raster_success": true,
            "raster_error_class": null,
            "raster_width": preview.width,
            "raster_height": preview.height,
        }),
        Err(error) => {
            let message = error.to_string();
            let detail = wmf_raster_error_detail(&message);
            let blocker_profile = wmf_blocker_profile(&wmf, &detail);
            json!({
                "candidate": true,
                "recovery_class": recovery_class,
                "raster_success": false,
                "raster_error_class": wmf_raster_error_class(&message),
                "raster_error_detail": detail,
                "raster_blocker_profile": blocker_profile,
            })
        }
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
    let wmf_prefix = wmf_bounded_prefix_profile(payload);
    let outer_size_rewrite_valid = wmf_outer_size_rewrite_valid(payload);
    let first_eof = wmf_first_eof_profile(payload);
    json!({
        "length_present": true,
        "declared_len": declared_len,
        "declared_fits_chunk": true,
        "exact_chunk_end": end == chunk.len(),
        "trailing_len": chunk.len() - end,
        "wmf_valid": wmf_valid,
        "validation_error_class": validation_error_class,
        "payload_sha256": wmf_valid.then(|| sha256_hex(payload)),
        "wmf_prefix": wmf_prefix,
        "outer_size_rewrite_valid": outer_size_rewrite_valid,
        "first_eof": first_eof,
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
        let direct_native_raster_profile = direct_native_chunk.map(wmf_raster_profile);

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
            "direct_native_raster_profile": direct_native_raster_profile,
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
    fn colorref_mode_distinguishes_palette_index_from_truecolor() {
        assert_eq!(colorref_mode(Some(0x0011_2233)), "rgb");
        assert_eq!(colorref_mode(Some(0x0100_0007)), "palette_index");
        assert_eq!(colorref_mode(Some(0x0211_2233)), "palette_rgb");
        assert_eq!(colorref_mode(Some(0x0311_2233)), "other");
        assert_eq!(colorref_mode(None), "missing");
    }

    #[test]
    fn palette_lifecycle_tracks_select_realize_delete_and_reuse() {
        let palette = [0x00_u8, 0x03, 0x01, 0x00, 10, 20, 30, 0];
        let select_zero = 0_u16.to_le_bytes();
        let delete_zero = 0_u16.to_le_bytes();
        let records = vec![
            (META_CREATEPALETTE_FUNCTION, palette.as_slice()),
            (META_SELECTPALETTE_FUNCTION, select_zero.as_slice()),
            (META_REALIZEPALETTE_FUNCTION, &[][..]),
            (META_DELETEOBJECT_FUNCTION, delete_zero.as_slice()),
            (META_CREATEPENINDIRECT_FUNCTION, &[][..]),
            (META_EOF_FUNCTION, &[][..]),
        ];
        let profile = palette_blocker_profile(&records);
        assert_eq!(profile["palette_record"]["start"], "0x0300");
        assert_eq!(profile["palette_record"]["entry_count"], 1);
        assert_eq!(profile["palette_record"]["exact_length"], true);
        assert_eq!(profile["lifecycle"]["palette_creation_count"], 1);
        assert_eq!(profile["lifecycle"]["palette_select_count"], 1);
        assert_eq!(profile["lifecycle"]["realize_palette_count"], 1);
        assert_eq!(profile["lifecycle"]["palette_delete_count"], 1);
        assert_eq!(profile["lifecycle"]["palette_slot_reuse_count"], 1);
    }

    #[test]
    fn ellipse_profile_tracks_bounded_geometry_and_selected_objects() {
        let mut pen = [0_u8; 10];
        pen[0..2].copy_from_slice(&0_u16.to_le_bytes());
        pen[2..4].copy_from_slice(&1_i16.to_le_bytes());
        let mut brush = [0_u8; 8];
        brush[0..2].copy_from_slice(&0_u16.to_le_bytes());
        let select_zero = 0_u16.to_le_bytes();
        let select_one = 1_u16.to_le_bytes();
        let mut ellipse = [0_u8; 8];
        ellipse[0..2].copy_from_slice(&80_i16.to_le_bytes());
        ellipse[2..4].copy_from_slice(&90_i16.to_le_bytes());
        ellipse[4..6].copy_from_slice(&20_i16.to_le_bytes());
        ellipse[6..8].copy_from_slice(&10_i16.to_le_bytes());
        let records = vec![
            (META_CREATEPENINDIRECT_FUNCTION, pen.as_slice()),
            (META_CREATEBRUSHINDIRECT_FUNCTION, brush.as_slice()),
            (META_SELECTOBJECT_FUNCTION, select_zero.as_slice()),
            (META_SELECTOBJECT_FUNCTION, select_one.as_slice()),
            (META_ELLIPSE_FUNCTION, ellipse.as_slice()),
            (META_EOF_FUNCTION, &[][..]),
        ];
        let profile = ellipse_blocker_profile(&records);
        assert_eq!(profile["param_len"], 8);
        assert_eq!(profile["width_sign"], "positive");
        assert_eq!(profile["height_sign"], "positive");
        assert_eq!(profile["nondegenerate"], true);
        assert_eq!(profile["active_pen"]["creator"], "pen");
        assert_eq!(profile["active_brush"]["creator"], "brush");
    }

    #[test]
    fn font_lifecycle_tracks_slot_select_delete_reuse_and_text() {
        let font = [0_u8; 26];
        let select_zero = 0_u16.to_le_bytes();
        let delete_zero = 0_u16.to_le_bytes();
        let records = vec![
            (META_CREATEFONTINDIRECT_FUNCTION, font.as_slice()),
            (META_SELECTOBJECT_FUNCTION, select_zero.as_slice()),
            (META_TEXTOUT_FUNCTION, &[][..]),
            (META_DELETEOBJECT_FUNCTION, delete_zero.as_slice()),
            (META_CREATEPENINDIRECT_FUNCTION, &[][..]),
            (META_EOF_FUNCTION, &[][..]),
        ];
        let profile = font_blocker_profile(&records);
        assert_eq!(profile["lifecycle"]["font_creation_count"], 1);
        assert_eq!(profile["lifecycle"]["font_select_count"], 1);
        assert_eq!(profile["lifecycle"]["font_delete_count"], 1);
        assert_eq!(profile["lifecycle"]["font_slot_reuse_count"], 1);
        assert_eq!(profile["lifecycle"]["text_output_record_count"], 1);
        assert_eq!(
            profile["lifecycle"]["text_output_with_selected_font_count"],
            1
        );
    }

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
