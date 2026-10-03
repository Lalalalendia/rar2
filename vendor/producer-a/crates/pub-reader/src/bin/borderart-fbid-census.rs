use anyhow::{Context, Result};
use pub_contents::{
    detect_family, parse_0x2c_header, parse_confirmed_0x2c_chunk,
    parse_confirmed_0x2c_trailer_root, parse_confirmed_chunk_reference, ContentsFamily,
    RawContentsBlockBody, BLOCK_TYPE_U16, BLOCK_TYPE_U32,
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
const OPLPO_DONT_STRETCH_BORDERART_FIELD: u16 = 0x07;
const OPLPO_FBID_FIELD: u16 = 0x09;
const OPLPLBFB_IFBMAX_FIELD: u16 = 0x01;

#[derive(Debug, Serialize)]
struct ScalarObservation {
    seq_num: usize,
    value: u32,
    source_offset: u64,
}

#[derive(Debug, Serialize)]
struct ShapeFieldObservation {
    seq_num: usize,
    wire_type: u8,
    value_u32: Option<u32>,
    source_offset: u64,
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
    shape_fbid_field07_observations: Vec<ShapeFieldObservation>,
    fancy_borders_object_count: usize,
    fancy_borders_chunks_with_opaque_tail: usize,
    ifbmax_wrong_wire_count: usize,
    ifbmax_observations: Vec<ScalarObservation>,
    single_ifbmax: Option<u32>,
    all_fbid_in_range_when_single_ifbmax: Option<bool>,
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
    fbid_shape_field07_observation_count: usize,
    files_with_fbid_shape_field07: usize,
    shape_fbid_field07_wire_counts: BTreeMap<u8, usize>,
    files_with_fancy_borders_object: usize,
    files_with_ifbmax: usize,
    files_with_ifbmax_gt1: usize,
    max_ifbmax: Option<u32>,
    ifbmax_value_counts: BTreeMap<u32, usize>,
    files_with_fbid_and_single_ifbmax: usize,
    files_with_fbid_out_of_range_when_single_ifbmax: usize,
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
                shape_fbid_field07_observations: Vec::new(),
                fancy_borders_object_count: 0,
                fancy_borders_chunks_with_opaque_tail: 0,
                ifbmax_wrong_wire_count: 0,
                ifbmax_observations: Vec::new(),
                single_ifbmax: None,
                all_fbid_in_range_when_single_ifbmax: None,
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
            shape_fbid_field07_observations: Vec::new(),
            fancy_borders_object_count: 0,
            fancy_borders_chunks_with_opaque_tail: 0,
            ifbmax_wrong_wire_count: 0,
            ifbmax_observations: Vec::new(),
            single_ifbmax: None,
            all_fbid_in_range_when_single_ifbmax: None,
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
    let mut shape_fbid_field07_observations = Vec::new();
    let mut fancy_borders_object_count = 0usize;
    let mut fancy_borders_chunks_with_opaque_tail = 0usize;
    let mut ifbmax_wrong_wire_count = 0usize;
    let mut ifbmax_observations = Vec::new();

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
                let mut exact_fbid_seen = false;
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
                    exact_fbid_seen = true;
                    shape_fbid_observations.push(ScalarObservation {
                        seq_num,
                        value: u32::from(*value),
                        source_offset: value_source.offset,
                    });
                }
                if exact_fbid_seen {
                    for field in chunk
                        .fields
                        .iter()
                        .filter(|field| field.id == OPLPO_DONT_STRETCH_BORDERART_FIELD)
                    {
                        let value_u32 = match &field.body {
                            RawContentsBlockBody::U16 { value, .. } => Some(u32::from(*value)),
                            RawContentsBlockBody::U32 { value, .. } => Some(*value),
                            _ => None,
                        };
                        shape_fbid_field07_observations.push(ShapeFieldObservation {
                            seq_num,
                            wire_type: field.block_type,
                            value_u32,
                            source_offset: field.source.offset,
                        });
                    }
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

    Ok(FileReceipt {
        source_sha256,
        file_name,
        status: "mature_scanned".into(),
        contents_family: Some("0x2c".into()),
        shape_chunk_count,
        shape_chunks_with_opaque_tail,
        shape_fbid_wrong_wire_count,
        shape_fbid_observations,
        shape_fbid_field07_observations,
        fancy_borders_object_count,
        fancy_borders_chunks_with_opaque_tail,
        ifbmax_wrong_wire_count,
        ifbmax_observations,
        single_ifbmax,
        all_fbid_in_range_when_single_ifbmax,
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
    let mut shape_fbid_field07_wire_counts = BTreeMap::<u8, usize>::new();
    for row in &rows {
        for observation in &row.shape_fbid_observations {
            *shape_fbid_value_counts
                .entry(observation.value)
                .or_default() += 1;
        }
        for observation in &row.ifbmax_observations {
            *ifbmax_value_counts.entry(observation.value).or_default() += 1;
        }
        for observation in &row.shape_fbid_field07_observations {
            *shape_fbid_field07_wire_counts
                .entry(observation.wire_type)
                .or_default() += 1;
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
    let fbid_shape_field07_observation_count = rows
        .iter()
        .map(|row| row.shape_fbid_field07_observations.len())
        .sum();
    let files_with_fbid_shape_field07 = rows
        .iter()
        .filter(|row| !row.shape_fbid_field07_observations.is_empty())
        .count();

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

    let receipt = CorpusReceipt {
        schema: "chaptera.borderart-fbid-corpus-census.v1",
        corpus_file_count: rows.len(),
        mature_0x2c_file_count,
        skipped_non_0x2c_count,
        error_count,
        files_with_shape_fbid,
        files_with_nonzero_shape_fbid,
        max_shape_fbid,
        shape_fbid_value_counts,
        fbid_shape_field07_observation_count,
        files_with_fbid_shape_field07,
        shape_fbid_field07_wire_counts,
        files_with_fancy_borders_object,
        files_with_ifbmax,
        files_with_ifbmax_gt1,
        max_ifbmax,
        ifbmax_value_counts,
        files_with_fbid_and_single_ifbmax,
        files_with_fbid_out_of_range_when_single_ifbmax,
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
        evidence_boundary: "Exact mature-0x2C directory/raw-type/bounded-chunk census only. OplPo field0x09/wire0x18 is counted as the physically exact candidate corresponding to Publisher11 XML Fbid priv=0903; for those exact Fbid-bearing shapes, field0x07 is inventoried without yet assigning StretchPictures semantics. OplPlbFb field0x01/wire0x20 is the exact IfbMax coordinate. This census does not scan opaque tails for byte patterns and does not infer mutation causality.",
    };

    if let Some(parent) = out_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&out_path, serde_json::to_vec_pretty(&receipt)?)?;
    println!("{}", serde_json::to_string_pretty(&receipt)?);
    Ok(())
}
