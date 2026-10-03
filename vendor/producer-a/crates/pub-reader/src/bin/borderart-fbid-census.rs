use anyhow::{Context, Result};
use pub_contents::{
    detect_family, parse_0x2c_header, parse_confirmed_0x2c_chunk,
    decode_packed_field_tag, parse_confirmed_0x2c_trailer_root, parse_confirmed_block,
    parse_confirmed_chunk_reference, ContentsCursor, ContentsFamily, RawContentsBlock,
    RawContentsBlockBody, BLOCK_TYPE_CONTAINER_88, BLOCK_TYPE_CONTAINER_A0, BLOCK_TYPE_U16,
    BLOCK_TYPE_U32,
};
use pub_core::StreamPath;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    env, fs,
    io::Cursor,
    path::{Path, PathBuf},
};

const CONTENTS_STREAM_PATH: &str = "/Contents";
const RAW_TYPE_SHAPE: u16 = 0x01;
const RAW_TYPE_FANCY_BORDERS: u16 = 0x46;
const OPLPO_FBID_FIELD: u16 = 0x09;
const OPLPLBFB_IFBMAX_FIELD: u16 = 0x01;
const OPLPLBFB_RGFB_FIELD: u16 = 0x02;
const OPLFB_SZ_FBRD_NAME_FIELD: u16 = 0x03;
const BLOCK_TYPE_UTF16_Z: u8 = 0xC0;

#[derive(Debug, Serialize)]
struct ScalarObservation {
    seq_num: usize,
    value: u32,
    source_offset: u64,
}

#[derive(Debug, Clone, Serialize)]
struct CatalogNameObservation {
    ordinal: usize,
    name: String,
    child_source_offset: u64,
    name_source_offset: u64,
}

#[derive(Debug, Serialize)]
struct ResolvedFbidObservation {
    seq_num: usize,
    value: u32,
    source_offset: u64,
    catalog_name: Option<String>,
}

#[derive(Debug, Serialize)]
struct FileReceipt {
    source_sha256: String,
    file_name: String,
    status: String,
    contents_family: Option<String>,
    shape_chunk_count: usize,
    shape_chunks_with_opaque_tail: usize,
    shape_fbid_wrong_wire_count: usize,
    shape_fbid_observations: Vec<ScalarObservation>,
    fancy_borders_object_count: usize,
    fancy_borders_chunks_with_opaque_tail: usize,
    ifbmax_wrong_wire_count: usize,
    ifbmax_observations: Vec<ScalarObservation>,
    catalog_name_wrong_wire_count: usize,
    catalog_names: Vec<CatalogNameObservation>,
    single_ifbmax: Option<u32>,
    catalog_name_count_matches_single_ifbmax: Option<bool>,
    all_fbid_in_range_when_single_ifbmax: Option<bool>,
    resolved_shape_fbid_observations: Vec<ResolvedFbidObservation>,
    all_fbid_names_resolved: Option<bool>,
    error: Option<String>,
}

#[derive(Debug, Serialize)]
struct CorpusReceipt {
    schema: &'static str,
    corpus_file_count: usize,
    mature_0x2c_file_count: usize,
    skipped_non_0x2c_count: usize,
    error_count: usize,
    files_with_shape_fbid: usize,
    files_with_nonzero_shape_fbid: usize,
    max_shape_fbid: Option<u32>,
    shape_fbid_value_counts: BTreeMap<u32, usize>,
    files_with_fancy_borders_object: usize,
    files_with_ifbmax: usize,
    files_with_ifbmax_gt1: usize,
    max_ifbmax: Option<u32>,
    ifbmax_value_counts: BTreeMap<u32, usize>,
    files_with_fbid_and_single_ifbmax: usize,
    files_with_fbid_out_of_range_when_single_ifbmax: usize,
    files_with_catalog_names: usize,
    files_with_catalog_name_count_mismatch: usize,
    files_with_unresolved_fbid_names: usize,
    catalog_name_counts: BTreeMap<String, usize>,
    shape_chunk_count: usize,
    shape_chunks_with_opaque_tail: usize,
    fancy_borders_object_count: usize,
    fancy_borders_chunks_with_opaque_tail: usize,
    rows: Vec<FileReceipt>,
    evidence_boundary: &'static str,
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn exact_raw_type(reference: &pub_contents::Contents0x2cChunkReference) -> Option<u16> {
    match reference.raw_types.as_slice() {
        [field] => Some(field.value),
        _ => None,
    }
}

fn decode_utf16_z_field(
    contents: &[u8],
    offset: usize,
    limit: usize,
) -> Result<(String, usize, u64)> {
    let tag_end = offset
        .checked_add(2)
        .filter(|end| *end <= limit)
        .context("UTF-16 field tag crosses parent boundary")?;
    let (field_id, wire_type) =
        decode_packed_field_tag([contents[offset], contents[offset + 1]]);
    if field_id != OPLFB_SZ_FBRD_NAME_FIELD || wire_type != BLOCK_TYPE_UTF16_Z {
        anyhow::bail!(
            "expected OplFb.SzFBrdName field0x{OPLFB_SZ_FBRD_NAME_FIELD:02X}/wire0x{BLOCK_TYPE_UTF16_Z:02X} at 0x{offset:X}, got field0x{field_id:02X}/wire0x{wire_type:02X}"
        );
    }

    let length_end = tag_end
        .checked_add(4)
        .filter(|end| *end <= limit)
        .context("UTF-16 field length crosses parent boundary")?;
    let declared_length = u32::from_le_bytes(
        contents[tag_end..length_end]
            .try_into()
            .expect("four-byte UTF-16 length slice"),
    );
    if declared_length < 4 {
        anyhow::bail!("invalid UTF-16 declared length {declared_length} at 0x{offset:X}");
    }
    let payload_len = usize::try_from(declared_length - 4).context("UTF-16 payload length")?;
    if payload_len % 2 != 0 {
        anyhow::bail!("odd UTF-16 payload length {payload_len} at 0x{offset:X}");
    }
    let payload_end = length_end
        .checked_add(payload_len)
        .filter(|end| *end <= limit)
        .context("UTF-16 payload crosses parent boundary")?;

    let mut units = contents[length_end..payload_end]
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect::<Vec<_>>();
    if units.last() == Some(&0) {
        units.pop();
    }
    if units.iter().any(|unit| *unit == 0) {
        anyhow::bail!("embedded NUL in OplFb.SzFBrdName at 0x{offset:X}");
    }
    let name = String::from_utf16(&units).context("decode OplFb.SzFBrdName UTF-16")?;
    if name.is_empty() {
        anyhow::bail!("empty OplFb.SzFBrdName at 0x{offset:X}");
    }
    Ok((name, payload_end, length_end as u64))
}

fn parse_oplfb_name(
    contents: &[u8],
    stream: StreamPath,
    child: &RawContentsBlock,
) -> Result<CatalogNameObservation> {
    let RawContentsBlockBody::Container { content_source, .. } = &child.body else {
        anyhow::bail!("OplFb collection child is not a container");
    };
    let start = usize::try_from(content_source.offset).context("OplFb content start")?;
    let len = usize::try_from(content_source.len).context("OplFb content length")?;
    let limit = start.checked_add(len).context("OplFb content end overflow")?;

    let mut cursor = ContentsCursor::bounded(stream, contents, start, len)?;
    if cursor.remaining() < 2 {
        anyhow::bail!("empty OplFb child at 0x{start:X}");
    }

    let (first_id, first_wire) =
        decode_packed_field_tag([contents[cursor.position()], contents[cursor.position() + 1]]);
    if first_id == 0x02 && first_wire == BLOCK_TYPE_U32 {
        parse_confirmed_block(&mut cursor).context("parse optional OplFb field0x02")?;
    }

    let name_offset = cursor.position();
    let (name, end, name_source_offset) =
        decode_utf16_z_field(contents, name_offset, limit)?;
    if end > limit {
        anyhow::bail!("OplFb name crosses child boundary");
    }

    Ok(CatalogNameObservation {
        ordinal: 0,
        name,
        child_source_offset: child.source.offset,
        name_source_offset,
    })
}

fn parse_catalog_names(
    contents: &[u8],
    stream: StreamPath,
    rgfb: &RawContentsBlock,
) -> Result<Vec<CatalogNameObservation>> {
    if rgfb.id != OPLPLBFB_RGFB_FIELD || rgfb.block_type != BLOCK_TYPE_CONTAINER_A0 {
        anyhow::bail!(
            "expected OplPlbFb.Rgfb field0x{OPLPLBFB_RGFB_FIELD:02X}/wire0x{BLOCK_TYPE_CONTAINER_A0:02X}"
        );
    }
    let RawContentsBlockBody::Container { content_source, .. } = &rgfb.body else {
        anyhow::bail!("OplPlbFb.Rgfb is not a container");
    };
    let start = usize::try_from(content_source.offset).context("Rgfb content start")?;
    let len = usize::try_from(content_source.len).context("Rgfb content length")?;
    let mut cursor = ContentsCursor::bounded(stream.clone(), contents, start, len)?;
    let mut names = Vec::new();

    while cursor.remaining() > 0 {
        let child = parse_confirmed_block(&mut cursor).context("parse OplFb catalog child")?;
        if child.block_type != BLOCK_TYPE_CONTAINER_88 {
            anyhow::bail!(
                "unexpected Rgfb child wire 0x{:02X} at 0x{:X}",
                child.block_type,
                child.source.offset
            );
        }
        let mut observation = parse_oplfb_name(contents, stream.clone(), &child)?;
        observation.ordinal = names.len();
        names.push(observation);
    }
    Ok(names)
}

fn scan_file(path: &Path) -> FileReceipt {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("")
        .to_owned();

    match scan_file_inner(path, file_name.clone()) {
        Ok(row) => row,
        Err(error) => {
            let source_sha256 = fs::read(path)
                .map(|bytes| sha256_hex(&bytes))
                .unwrap_or_else(|_| String::new());
            FileReceipt {
                source_sha256,
                file_name,
                status: "error".into(),
                contents_family: None,
                shape_chunk_count: 0,
                shape_chunks_with_opaque_tail: 0,
                shape_fbid_wrong_wire_count: 0,
                shape_fbid_observations: Vec::new(),
                fancy_borders_object_count: 0,
                fancy_borders_chunks_with_opaque_tail: 0,
                ifbmax_wrong_wire_count: 0,
                ifbmax_observations: Vec::new(),
                catalog_name_wrong_wire_count: 0,
                catalog_names: Vec::new(),
                single_ifbmax: None,
                catalog_name_count_matches_single_ifbmax: None,
                all_fbid_in_range_when_single_ifbmax: None,
                resolved_shape_fbid_observations: Vec::new(),
                all_fbid_names_resolved: None,
                error: Some(format!("{error:#}")),
            }
        }
    }
}

fn scan_file_inner(path: &Path, file_name: String) -> Result<FileReceipt> {
    let pub_bytes = fs::read(path).with_context(|| format!("read {}", path.display()))?;
    let source_sha256 = sha256_hex(&pub_bytes);
    let contents =
        pub_cfb::read_stream_reader(Cursor::new(pub_bytes.as_slice()), CONTENTS_STREAM_PATH)
            .with_context(|| format!("read {CONTENTS_STREAM_PATH}"))?;

    let family = detect_family(&contents).context("detect Contents family")?;
    if family != ContentsFamily::Family0x2c {
        return Ok(FileReceipt {
            source_sha256,
            file_name,
            status: "skipped_non_0x2c".into(),
            contents_family: Some("0x22".into()),
            shape_chunk_count: 0,
            shape_chunks_with_opaque_tail: 0,
            shape_fbid_wrong_wire_count: 0,
            shape_fbid_observations: Vec::new(),
            fancy_borders_object_count: 0,
            fancy_borders_chunks_with_opaque_tail: 0,
            ifbmax_wrong_wire_count: 0,
            ifbmax_observations: Vec::new(),
            catalog_name_wrong_wire_count: 0,
            catalog_names: Vec::new(),
            single_ifbmax: None,
            catalog_name_count_matches_single_ifbmax: None,
            all_fbid_in_range_when_single_ifbmax: None,
            resolved_shape_fbid_observations: Vec::new(),
            all_fbid_names_resolved: None,
            error: None,
        });
    }

    let stream = StreamPath(CONTENTS_STREAM_PATH.into());
    let header = parse_0x2c_header(stream.clone(), &contents).context("parse 0x2C header")?;
    let trailer =
        parse_confirmed_0x2c_trailer_root(&contents, &header).context("parse 0x2C trailer")?;

    let mut shape_chunk_count = 0usize;
    let mut shape_chunks_with_opaque_tail = 0usize;
    let mut shape_fbid_wrong_wire_count = 0usize;
    let mut shape_fbid_observations = Vec::new();
    let mut fancy_borders_object_count = 0usize;
    let mut fancy_borders_chunks_with_opaque_tail = 0usize;
    let mut ifbmax_wrong_wire_count = 0usize;
    let mut ifbmax_observations = Vec::new();
    let mut catalog_name_wrong_wire_count = 0usize;
    let mut catalog_names = Vec::new();

    for seq_num in 0..trailer.directory.slots.len() {
        let Some(reference) =
            parse_confirmed_chunk_reference(&contents, &trailer.directory, seq_num)
                .with_context(|| format!("parse directory reference seq {seq_num}"))?
        else {
            continue;
        };
        let Some(raw_type) = exact_raw_type(&reference) else {
            continue;
        };
        if raw_type != RAW_TYPE_SHAPE && raw_type != RAW_TYPE_FANCY_BORDERS {
            continue;
        }
        let [offset] = reference.chunk_offsets.as_slice() else {
            continue;
        };
        let chunk = parse_confirmed_0x2c_chunk(stream.clone(), &contents, offset.value)
            .with_context(|| format!("parse chunk seq {seq_num} raw type 0x{raw_type:02X}"))?;

        match raw_type {
            RAW_TYPE_SHAPE => {
                shape_chunk_count += 1;
                if chunk.unsupported_tail.is_some() {
                    shape_chunks_with_opaque_tail += 1;
                }
                for field in chunk
                    .fields
                    .iter()
                    .filter(|field| field.id == OPLPO_FBID_FIELD)
                {
                    if field.block_type != BLOCK_TYPE_U16 {
                        shape_fbid_wrong_wire_count += 1;
                        continue;
                    }
                    let RawContentsBlockBody::U16 {
                        value,
                        value_source,
                    } = &field.body
                    else {
                        shape_fbid_wrong_wire_count += 1;
                        continue;
                    };
                    shape_fbid_observations.push(ScalarObservation {
                        seq_num,
                        value: u32::from(*value),
                        source_offset: value_source.offset,
                    });
                }
            }
            RAW_TYPE_FANCY_BORDERS => {
                fancy_borders_object_count += 1;
                if chunk.unsupported_tail.is_some() {
                    fancy_borders_chunks_with_opaque_tail += 1;
                }
                for field in chunk
                    .fields
                    .iter()
                    .filter(|field| field.id == OPLPLBFB_IFBMAX_FIELD)
                {
                    if field.block_type != BLOCK_TYPE_U32 {
                        ifbmax_wrong_wire_count += 1;
                        continue;
                    }
                    let RawContentsBlockBody::U32 {
                        value,
                        value_source,
                    } = &field.body
                    else {
                        ifbmax_wrong_wire_count += 1;
                        continue;
                    };
                    ifbmax_observations.push(ScalarObservation {
                        seq_num,
                        value: *value,
                        source_offset: value_source.offset,
                    });
                }
                for field in chunk
                    .fields
                    .iter()
                    .filter(|field| field.id == OPLPLBFB_RGFB_FIELD)
                {
                    if field.block_type != BLOCK_TYPE_CONTAINER_A0 {
                        catalog_name_wrong_wire_count += 1;
                        continue;
                    }
                    let names = parse_catalog_names(&contents, stream.clone(), field)
                        .with_context(|| format!("parse FancyBorders catalog names seq {seq_num}"))?;
                    catalog_names.extend(names);
                }
            }
            _ => unreachable!(),
        }
    }

    let single_ifbmax = match ifbmax_observations.as_slice() {
        [observation] => Some(observation.value),
        _ => None,
    };
    let all_fbid_in_range_when_single_ifbmax = single_ifbmax.map(|max| {
        shape_fbid_observations
            .iter()
            .all(|observation| observation.value < max)
    });
    let catalog_name_count_matches_single_ifbmax =
        single_ifbmax.map(|max| catalog_names.len() == max as usize);
    let resolved_shape_fbid_observations = shape_fbid_observations
        .iter()
        .map(|observation| ResolvedFbidObservation {
            seq_num: observation.seq_num,
            value: observation.value,
            source_offset: observation.source_offset,
            catalog_name: catalog_names
                .get(observation.value as usize)
                .map(|item| item.name.clone()),
        })
        .collect::<Vec<_>>();
    let all_fbid_names_resolved = (!shape_fbid_observations.is_empty()).then(|| {
        resolved_shape_fbid_observations
            .iter()
            .all(|observation| observation.catalog_name.is_some())
    });

    Ok(FileReceipt {
        source_sha256,
        file_name,
        status: "mature_scanned".into(),
        contents_family: Some("0x2c".into()),
        shape_chunk_count,
        shape_chunks_with_opaque_tail,
        shape_fbid_wrong_wire_count,
        shape_fbid_observations,
        fancy_borders_object_count,
        fancy_borders_chunks_with_opaque_tail,
        ifbmax_wrong_wire_count,
        ifbmax_observations,
        catalog_name_wrong_wire_count,
        catalog_names,
        single_ifbmax,
        catalog_name_count_matches_single_ifbmax,
        all_fbid_in_range_when_single_ifbmax,
        resolved_shape_fbid_observations,
        all_fbid_names_resolved,
        error: None,
    })
}

fn main() -> Result<()> {
    let args = env::args().collect::<Vec<_>>();
    if args.len() != 3 {
        anyhow::bail!("usage: borderart-fbid-census <corpus-dir> <out.json>");
    }
    let corpus_dir = PathBuf::from(&args[1]);
    let out_path = PathBuf::from(&args[2]);

    let mut paths = fs::read_dir(&corpus_dir)
        .with_context(|| format!("read corpus dir {}", corpus_dir.display()))?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            path.extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("pub"))
        })
        .collect::<Vec<_>>();
    paths.sort();

    let mut rows = paths.iter().map(|path| scan_file(path)).collect::<Vec<_>>();
    rows.sort_by(|left, right| left.source_sha256.cmp(&right.source_sha256));

    let mature_0x2c_file_count = rows
        .iter()
        .filter(|row| row.status == "mature_scanned")
        .count();
    let skipped_non_0x2c_count = rows
        .iter()
        .filter(|row| row.status == "skipped_non_0x2c")
        .count();
    let error_count = rows.iter().filter(|row| row.status == "error").count();

    let mut shape_fbid_value_counts = BTreeMap::<u32, usize>::new();
    let mut ifbmax_value_counts = BTreeMap::<u32, usize>::new();
    for row in &rows {
        for observation in &row.shape_fbid_observations {
            *shape_fbid_value_counts
                .entry(observation.value)
                .or_default() += 1;
        }
        for observation in &row.ifbmax_observations {
            *ifbmax_value_counts.entry(observation.value).or_default() += 1;
        }
    }

    let files_with_shape_fbid = rows
        .iter()
        .filter(|row| !row.shape_fbid_observations.is_empty())
        .count();
    let files_with_nonzero_shape_fbid = rows
        .iter()
        .filter(|row| row.shape_fbid_observations.iter().any(|obs| obs.value > 0))
        .count();
    let max_shape_fbid = rows
        .iter()
        .flat_map(|row| row.shape_fbid_observations.iter().map(|obs| obs.value))
        .max();

    let files_with_fancy_borders_object = rows
        .iter()
        .filter(|row| row.fancy_borders_object_count > 0)
        .count();
    let files_with_ifbmax = rows
        .iter()
        .filter(|row| !row.ifbmax_observations.is_empty())
        .count();
    let files_with_ifbmax_gt1 = rows
        .iter()
        .filter(|row| row.ifbmax_observations.iter().any(|obs| obs.value > 1))
        .count();
    let max_ifbmax = rows
        .iter()
        .flat_map(|row| row.ifbmax_observations.iter().map(|obs| obs.value))
        .max();

    let files_with_fbid_and_single_ifbmax = rows
        .iter()
        .filter(|row| !row.shape_fbid_observations.is_empty() && row.single_ifbmax.is_some())
        .count();
    let files_with_fbid_out_of_range_when_single_ifbmax = rows
        .iter()
        .filter(|row| {
            !row.shape_fbid_observations.is_empty()
                && row.all_fbid_in_range_when_single_ifbmax == Some(false)
        })
        .count();
    let files_with_catalog_names = rows
        .iter()
        .filter(|row| !row.catalog_names.is_empty())
        .count();
    let files_with_catalog_name_count_mismatch = rows
        .iter()
        .filter(|row| row.catalog_name_count_matches_single_ifbmax == Some(false))
        .count();
    let files_with_unresolved_fbid_names = rows
        .iter()
        .filter(|row| row.all_fbid_names_resolved == Some(false))
        .count();
    let mut catalog_name_counts = BTreeMap::<String, usize>::new();
    for row in &rows {
        for item in &row.catalog_names {
            *catalog_name_counts.entry(item.name.clone()).or_default() += 1;
        }
    }

    let receipt = CorpusReceipt {
        schema: "chaptera.borderart-fbid-corpus-census.v2",
        corpus_file_count: rows.len(),
        mature_0x2c_file_count,
        skipped_non_0x2c_count,
        error_count,
        files_with_shape_fbid,
        files_with_nonzero_shape_fbid,
        max_shape_fbid,
        shape_fbid_value_counts,
        files_with_fancy_borders_object,
        files_with_ifbmax,
        files_with_ifbmax_gt1,
        max_ifbmax,
        ifbmax_value_counts,
        files_with_fbid_and_single_ifbmax,
        files_with_fbid_out_of_range_when_single_ifbmax,
        files_with_catalog_names,
        files_with_catalog_name_count_mismatch,
        files_with_unresolved_fbid_names,
        catalog_name_counts,
        shape_chunk_count: rows.iter().map(|row| row.shape_chunk_count).sum(),
        shape_chunks_with_opaque_tail: rows
            .iter()
            .map(|row| row.shape_chunks_with_opaque_tail)
            .sum(),
        fancy_borders_object_count: rows
            .iter()
            .map(|row| row.fancy_borders_object_count)
            .sum(),
        fancy_borders_chunks_with_opaque_tail: rows
            .iter()
            .map(|row| row.fancy_borders_chunks_with_opaque_tail)
            .sum(),
        rows,
        evidence_boundary: "Exact mature-0x2C directory/raw-type/bounded-chunk census only. OplPo field0x09/wire0x18 is the physically exact Fbid coordinate; OplPlbFb field0x01/wire0x20 is IfbMax; OplPlbFb field0x02/wire0xA0 is traversed as the bounded Rgfb catalog, and each sequential OplFb child is required to expose field0x03/wire0xC0 as a bounded UTF-16 SzFBrdName. Shape Fbid values are resolved only by zero-based ordinal into that same-document bounded catalog. This does not establish rendering/materialization or native Delete semantics.",
    };

    if let Some(parent) = out_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&out_path, serde_json::to_vec_pretty(&receipt)?)?;
    println!("{}", serde_json::to_string_pretty(&receipt)?);
    Ok(())
}
