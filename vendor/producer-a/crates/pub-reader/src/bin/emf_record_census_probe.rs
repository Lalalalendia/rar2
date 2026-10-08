use flate2::bufread::ZlibDecoder;
use pub_core::StreamPath;
use pub_escher::{
    BlipKind, BlipMetafileCompression, inspect_validated_delayed_blips_prefix,
};
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

fn slice_span<'a>(bytes: &'a [u8], offset: u64, len: u64) -> Result<&'a [u8], Box<dyn Error>> {
    let start = usize::try_from(offset)?;
    let len = usize::try_from(len)?;
    let end = start.checked_add(len).ok_or("span end overflow")?;
    bytes.get(start..end).ok_or_else(|| "span out of bounds".into())
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
        Some(BlipMetafileCompression::Uncompressed) => Ok(stored.to_vec()),
        Some(BlipMetafileCompression::Deflate) => {
            let mut decoder = ZlibDecoder::new(Cursor::new(stored));
            let mut logical = Vec::new();
            decoder.read_to_end(&mut logical)?;
            let expected = usize::try_from(record.logical_payload_len.ok_or("missing logical len")?)?;
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

fn emf_record_census(logical: &[u8]) -> Result<serde_json::Value, Box<dyn Error>> {
    let mut offset = 0usize;
    let mut count = 0usize;
    let mut max_record_size = 0usize;
    let mut histogram = BTreeMap::<u32, usize>::new();
    let mut ordered_unique = Vec::<u32>::new();
    let mut first_type = None;
    let mut last_type = None;

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

    Ok(json!({
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
    let hash = source_hash(&bytes);
    let source = build_mature_0x2c_source_graph(Cursor::new(bytes.as_slice()), hash.clone())?;
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
    let duplicate_clusters = logical_clusters
        .into_iter()
        .filter(|(_, slots)| slots.len() > 1)
        .map(|(sha256, slots)| json!({"sha256": sha256, "slots": slots}))
        .collect::<Vec<_>>();

    eprintln!(
        "EMF_RECORD_CENSUS {}",
        serde_json::to_string(&json!({
            "source_sha256": hash.to_string(),
            "emf_slot_count": rows.len(),
            "rows": rows,
            "duplicate_logical_clusters": duplicate_clusters,
            "strict_scanned_record_count": validated.scanned_record_count,
            "strict_rejected_complete_blip_count": validated.rejected_complete_blips.len(),
            "strict_terminal_gap": validated.terminal_gap,
        }))?
    );
    Ok(())
}
