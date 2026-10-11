use flate2::bufread::ZlibDecoder;
use pub_core::StreamPath;
use pub_escher::{BlipKind, BlipMetafileCompression, inspect_validated_delayed_blips_prefix};
use pub_model::Sha256Digest;
use pub_reader::{
    ESCHER_DELAY_STREAM_PATH, ESCHER_STREAM_PATH, build_mature_0x2c_source_graph,
    build_pub_asset_manifest,
};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    env,
    error::Error,
    fs,
    io::{Cursor, Read},
};

fn source_hash(bytes: &[u8]) -> Sha256Digest {
    let digest = Sha256::digest(bytes);
    let mut raw = [0_u8; 32];
    raw.copy_from_slice(&digest);
    Sha256Digest::from_bytes(raw)
}

fn slice_span(bytes: &[u8], offset: u64, len: u64) -> Result<&[u8], Box<dyn Error>> {
    let start = usize::try_from(offset)?;
    let len = usize::try_from(len)?;
    let end = start.checked_add(len).ok_or("span end overflow")?;
    bytes
        .get(start..end)
        .ok_or_else(|| "span out of bounds".into())
}

fn logical_metafile_bytes(
    delayed: &[u8],
    record: &pub_escher::ValidatedBlip,
) -> Result<Vec<u8>, Box<dyn Error>> {
    let stored = slice_span(
        delayed,
        record.payload_source.offset,
        record.payload_source.len,
    )?;
    match record.metafile_compression {
        Some(BlipMetafileCompression::Uncompressed) => {
            let expected =
                usize::try_from(record.logical_payload_len.ok_or("missing logical len")?)?;
            if expected > 64 * 1024 * 1024 || stored.len() != expected {
                return Err("uncompressed EMF length out of bounds".into());
            }
            Ok(stored.to_vec())
        }
        Some(BlipMetafileCompression::Deflate) => {
            let decoder = ZlibDecoder::new(Cursor::new(stored));
            let mut logical = Vec::new();
            let expected =
                usize::try_from(record.logical_payload_len.ok_or("missing logical len")?)?;
            if expected > 64 * 1024 * 1024 {
                return Err("EMF logical payload exceeds 64MiB".into());
            }
            decoder
                .take(u64::try_from(expected)?.saturating_add(1))
                .read_to_end(&mut logical)?;
            if logical.len() != expected {
                return Err(format!(
                    "logical EMF length mismatch: expected {expected}, got {}",
                    logical.len()
                )
                .into());
            }
            Ok(logical)
        }
        None => Err("validated metafile lacks compression class".into()),
    }
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}

// EMR_STRETCHDIBITS (MS-EMF section 2.3.1.7, type 81).
// Reads only bounded record metadata; never copies or emits embedded pixel bytes.
fn bounded_record_field(bytes: &[u8], offset: u32, len: u32) -> Option<&[u8]> {
    let start = usize::try_from(offset).ok()?;
    let len = usize::try_from(len).ok()?;
    let end = start.checked_add(len)?;
    bytes.get(start..end)
}

fn stretchdibits_profile(record: &[u8]) -> serde_json::Value {
    if record.len() < 80 {
        return json!({"status": "short_fixed_header"});
    }
    let bmi_offset = read_u32(record, 48);
    let bmi_len = read_u32(record, 52);
    let bits_offset = read_u32(record, 56);
    let bits_len = read_u32(record, 60);
    let Some(bmi) = bounded_record_field(record, bmi_offset, bmi_len) else {
        return json!({"status": "bitmap_header_out_of_range"});
    };
    let Some(bits) = bounded_record_field(record, bits_offset, bits_len) else {
        return json!({"status": "bitmap_bits_out_of_range"});
    };
    if bmi.len() < 40 {
        return json!({"status": "unsupported_bitmap_header_short"});
    }
    let bi_size = read_u32(bmi, 0);
    let bi_width = read_u32(bmi, 4) as i32;
    let bi_height = read_u32(bmi, 8) as i32;
    let bi_planes = u16::from_le_bytes([bmi[12], bmi[13]]);
    let bi_bit_count = u16::from_le_bytes([bmi[14], bmi[15]]);
    let bi_compression = read_u32(bmi, 16);
    let x_src = read_u32(record, 32) as i32;
    let y_src = read_u32(record, 36) as i32;
    let cx_src = read_u32(record, 40) as i32;
    let cy_src = read_u32(record, 44) as i32;
    let cx_dest = read_u32(record, 72) as i32;
    let cy_dest = read_u32(record, 76) as i32;
    let usage = read_u32(record, 64);
    let rop = read_u32(record, 68);
    let src_whole_bitmap = x_src == 0
        && y_src == 0
        && bi_width.checked_abs() == Some(cx_src)
        && bi_height.checked_abs() == Some(cy_src);
    json!({
        "status": "bounded_bitmap_fields",
        "bi_size": bi_size,
        "bi_planes": bi_planes,
        "bi_bit_count": bi_bit_count,
        "bi_compression": bi_compression,
        "bmi_len": bmi.len(),
        "bits_len": bits.len(),
        "usage": usage,
        "rop": rop,
        "rop_is_srccopy": rop == 0x00cc0020,
        "src_whole_bitmap": src_whole_bitmap,
        "src_dimensions_positive": cx_src > 0 && cy_src > 0,
        "dest_dimensions_positive": cx_dest > 0 && cy_dest > 0,
        "bitmap_width_abs": bi_width.checked_abs(),
        "bitmap_height_abs": bi_height.checked_abs(),
    })
}

fn emf_record_census(logical: &[u8]) -> Result<serde_json::Value, Box<dyn Error>> {
    let mut offset = 0usize;
    let mut count = 0usize;
    let mut max_record_size = 0usize;
    let mut histogram = BTreeMap::<u32, usize>::new();
    let mut ordered_unique = Vec::<u32>::new();
    let mut first_type = None;
    let mut last_type = None;
    let mut stretchdibits_profiles = Vec::new();

    while offset < logical.len() {
        if logical.len() - offset < 8 {
            return Err(format!("short EMF record header at {offset}").into());
        }
        let record_type = read_u32(logical, offset);
        let record_size = usize::try_from(read_u32(logical, offset + 4))?;
        if record_size < 8 || record_size % 4 != 0 {
            return Err(format!("invalid EMF record size {record_size} at {offset}").into());
        }
        let end = offset
            .checked_add(record_size)
            .filter(|end| *end <= logical.len())
            .ok_or_else(|| format!("EMF record {record_type} crosses logical payload"))?;
        if record_type == 81 {
            stretchdibits_profiles.push(stretchdibits_profile(&logical[offset..end]));
        }

        first_type.get_or_insert(record_type);
        last_type = Some(record_type);
        count += 1;
        max_record_size = max_record_size.max(record_size);
        if !histogram.contains_key(&record_type) {
            ordered_unique.push(record_type);
        }
        *histogram.entry(record_type).or_default() += 1;
        offset = end;
    }

    let non_header_eof_record_count = count
        .saturating_sub(histogram.get(&1).copied().unwrap_or(0))
        .saturating_sub(histogram.get(&14).copied().unwrap_or(0));
    Ok(json!({
        "stretchdibits_profiles": stretchdibits_profiles,
        "non_header_eof_record_count": non_header_eof_record_count,
        "record_count": count,
        "max_record_size": max_record_size,
        "first_type": first_type,
        "last_type": last_type,
        "header_is_emr_header": first_type == Some(1),
        "last_is_emr_eof": last_type == Some(14),
        "ordered_unique_record_types": ordered_unique,
        "record_type_histogram": histogram,
        "full_consumption": offset == logical.len(),
    }))
}

fn main() -> Result<(), Box<dyn Error>> {
    let input = env::args()
        .nth(1)
        .ok_or("usage: emf_record_census_probe INPUT.pub")?;
    let bytes = fs::read(&input)?;
    if bytes.len() > 16 * 1024 * 1024 {
        return Err("PUB exceeds 16MiB research scope".into());
    }
    let hash = source_hash(&bytes);
    let source = build_mature_0x2c_source_graph(Cursor::new(bytes.as_slice()), hash)?;
    let escher = pub_cfb::read_stream_reader(Cursor::new(bytes.as_slice()), ESCHER_STREAM_PATH)?;
    let delayed =
        pub_cfb::read_stream_reader(Cursor::new(bytes.as_slice()), ESCHER_DELAY_STREAM_PATH)?;
    let manifest = build_pub_asset_manifest(&source.graph, &escher, &delayed)?;
    let validated = inspect_validated_delayed_blips_prefix(
        StreamPath(ESCHER_DELAY_STREAM_PATH.to_owned()),
        &delayed,
    );
    let validated_by_offset = validated
        .records
        .iter()
        .map(|record| (record.record_source.offset, record))
        .collect::<BTreeMap<_, _>>();

    let mut rows = Vec::new();
    let mut logical_clusters = BTreeMap::<String, Vec<u32>>::new();

    for asset in manifest
        .assets
        .iter()
        .filter(|asset| asset.blip_kind == Some(BlipKind::Emf))
    {
        let record_source = asset
            .blip_record_source
            .as_ref()
            .ok_or("EMF asset missing BLIP record source")?;
        let record = validated_by_offset
            .get(&record_source.offset)
            .copied()
            .ok_or_else(|| format!("EMF slot {} missing strict validated record", asset.slot))?;
        let logical = logical_metafile_bytes(&delayed, record)?;
        let census = emf_record_census(&logical)?;
        let logical_sha = record
            .logical_payload_sha256
            .clone()
            .ok_or("validated EMF missing logical SHA")?;
        logical_clusters
            .entry(logical_sha.clone())
            .or_default()
            .push(asset.slot);

        rows.push(json!({
            "slot": asset.slot,
            "use_count": asset.uses.len(),
            "rec_instance": record.rec_instance,
            "compression": format!("{:?}", record.metafile_compression),
            "stored_len": record.payload_source.len,
            "logical_len": logical.len(),
            "logical_sha256": logical_sha,
            "census": census,
        }));
    }

    rows.sort_by_key(|row| row["slot"].as_u64().unwrap_or_default());
    if rows.is_empty() {
        return Err("no strict EMF asset inventory found".into());
    }
    let duplicate_clusters = logical_clusters
        .into_iter()
        .filter(|(_, slots)| slots.len() > 1)
        .map(|(sha256, slots)| json!({"sha256": sha256, "slots": slots}))
        .collect::<Vec<_>>();

    let public_rows = rows
        .iter()
        .map(|row| {
            json!({
                "use_count": row["use_count"],
                "logical_len": row["logical_len"],
                "census": row["census"],
            })
        })
        .collect::<Vec<_>>();
    let nontrivial_emf_slots = rows
        .iter()
        .filter(|row| {
            row["census"]["non_header_eof_record_count"]
                .as_u64()
                .unwrap_or(0)
                > 0
        })
        .count();

    println!(
        "EMF_RECORD_CENSUS {}",
        serde_json::to_string(&json!({
            "source_sha256": hash.to_string(),
            "emf_slot_count": rows.len(),
            "rows": public_rows,
            "emf_slots_with_non_header_eof_records": nontrivial_emf_slots,
            "duplicate_logical_cluster_count": duplicate_clusters.len(),
            "strict_scanned_record_count": validated.scanned_record_count,
            "strict_rejected_complete_blip_count": validated.rejected_complete_blips.len(),
            "strict_terminal_gap_present": validated.terminal_gap.is_some(),
        }))?
    );
    Ok(())
}
