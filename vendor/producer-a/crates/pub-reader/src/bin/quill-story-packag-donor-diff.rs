use anyhow::{bail, Context, Result};
use pub_cfb::EntryKind;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::io::Cursor;
use std::path::Path;

const CONTENTS_STREAM: &str = "/Contents";
const QUILL_STREAM: &str = "/Quill/QuillSub/CONTENTS";
const DESCRIPTOR_ROOT: u32 = 0x18;
const DESCRIPTOR_END: u32 = 0xffff_ffff;
const DESCRIPTOR_SIZE: usize = 24;
const DESCRIPTOR_PRESENT: u16 = 0x0018;

const EXPECTED_SOURCE_SHA256: &str =
    "6b5d5b269be7ca74b03d47423aec985676c45be7033e007792fcc3eb35ad929a";
const EXPECTED_VARIANT_SHA256: &str =
    "e8c360c97f4604e60fd84fbc106f59723b03838e05c74b74f4d56aef8d40088e";

#[derive(Debug, Clone)]
struct Descriptor {
    ordinal: usize,
    name: [u8; 4],
    data_offset: u32,
    data_length: u32,
}

#[derive(Debug, Serialize)]
struct LogicalStreamDiff {
    path: String,
    source_len: Option<usize>,
    variant_len: Option<usize>,
    source_sha256: Option<String>,
    variant_sha256: Option<String>,
    byte_equal: bool,
}

#[derive(Debug, Serialize)]
struct DescriptorDiff {
    ordinal: usize,
    source_name: Option<String>,
    variant_name: Option<String>,
    source_len: Option<u32>,
    variant_len: Option<u32>,
    source_payload_sha256: Option<String>,
    variant_payload_sha256: Option<String>,
    source_all_ff: Option<bool>,
    variant_all_ff: Option<bool>,
    payload_equal: bool,
}

#[derive(Debug, Serialize)]
struct Report {
    schema: String,
    source_sha256: String,
    variant_sha256: String,
    source_byte_len: usize,
    variant_byte_len: usize,
    logical_stream_topology_equal: bool,
    changed_logical_stream_count: usize,
    changed_logical_stream_paths: Vec<String>,
    contents_stream_equal: bool,
    quill_stream_equal: bool,
    logical_streams: Vec<LogicalStreamDiff>,
    quill_descriptor_topology_equal: bool,
    changed_quill_descriptor_count: usize,
    changed_quill_descriptor_names: Vec<String>,
    text_payloads_equal: bool,
    fdpp_payloads_equal: bool,
    fdpc_payloads_equal: bool,
    stsh_payloads_equal: bool,
    syid_payloads_equal: bool,
    strs_payloads_equal: bool,
    quill_descriptors: Vec<DescriptorDiff>,
    evidence_boundary: String,
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn u16_at(bytes: &[u8], offset: usize) -> Option<u16> {
    let raw = bytes.get(offset..offset.checked_add(2)?)?;
    Some(u16::from_le_bytes([raw[0], raw[1]]))
}

fn u32_at(bytes: &[u8], offset: usize) -> Option<u32> {
    let raw = bytes.get(offset..offset.checked_add(4)?)?;
    Some(u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]))
}

fn descriptor_name(raw: &[u8; 4]) -> String {
    String::from_utf8_lossy(raw).trim_end().to_owned()
}

fn parse_descriptor_directory(bytes: &[u8]) -> Result<Vec<Descriptor>> {
    let mut current = DESCRIPTOR_ROOT;
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();

    while current != DESCRIPTOR_END {
        if !seen.insert(current) {
            bail!("descriptor list cycle");
        }
        let start = usize::try_from(current).context("descriptor node offset too large")?;
        let count =
            usize::from(u16_at(bytes, start + 2).context("descriptor node header truncated")?);
        let next = u32_at(bytes, start + 4).context("descriptor node next truncated")?;
        let array_start = start.checked_add(8).context("descriptor array overflow")?;
        let array_end = array_start
            .checked_add(
                count
                    .checked_mul(DESCRIPTOR_SIZE)
                    .context("descriptor array size overflow")?,
            )
            .context("descriptor array end overflow")?;
        if array_end > bytes.len() {
            bail!("descriptor array outside Quill");
        }

        for index in 0..count {
            let offset = array_start + index * DESCRIPTOR_SIZE;
            if u16_at(bytes, offset) != Some(DESCRIPTOR_PRESENT) {
                bail!("unexpected descriptor presence marker");
            }
            let name_raw = bytes
                .get(offset + 2..offset + 6)
                .context("descriptor name truncated")?;
            let name = [name_raw[0], name_raw[1], name_raw[2], name_raw[3]];
            let data_offset =
                u32_at(bytes, offset + 16).context("descriptor data offset truncated")?;
            let data_length =
                u32_at(bytes, offset + 20).context("descriptor data length truncated")?;
            out.push(Descriptor {
                ordinal: out.len(),
                name,
                data_offset,
                data_length,
            });
        }

        current = next;
    }

    Ok(out)
}

fn descriptor_payload<'a>(bytes: &'a [u8], descriptor: &Descriptor) -> Result<&'a [u8]> {
    let start = usize::try_from(descriptor.data_offset).context("descriptor offset too large")?;
    let len = usize::try_from(descriptor.data_length).context("descriptor length too large")?;
    let end = start.checked_add(len).context("descriptor payload overflow")?;
    bytes
        .get(start..end)
        .with_context(|| format!("descriptor {} payload outside Quill", descriptor.ordinal))
}

fn stream_paths(bytes: &[u8]) -> Result<BTreeMap<String, usize>> {
    let inventory = pub_cfb::inspect_reader(Cursor::new(bytes))?;
    Ok(inventory
        .entries
        .into_iter()
        .filter(|entry| entry.kind == EntryKind::Stream)
        .map(|entry| (entry.path, usize::try_from(entry.len).unwrap_or(usize::MAX)))
        .collect())
}

fn read_stream(bytes: &[u8], path: &str) -> Result<Vec<u8>> {
    pub_cfb::read_stream_reader(Cursor::new(bytes), path)
}

fn compare_named_payloads(
    source_quill: &[u8],
    source_descriptors: &[Descriptor],
    variant_quill: &[u8],
    variant_descriptors: &[Descriptor],
    wanted: [u8; 4],
) -> Result<bool> {
    let source = source_descriptors
        .iter()
        .filter(|descriptor| descriptor.name == wanted)
        .map(|descriptor| descriptor_payload(source_quill, descriptor))
        .collect::<Result<Vec<_>>>()?;
    let variant = variant_descriptors
        .iter()
        .filter(|descriptor| descriptor.name == wanted)
        .map(|descriptor| descriptor_payload(variant_quill, descriptor))
        .collect::<Result<Vec<_>>>()?;
    Ok(source.len() == variant.len()
        && source
            .iter()
            .zip(&variant)
            .all(|(left, right)| *left == *right))
}

fn main() -> Result<()> {
    let mut args = env::args_os().skip(1);
    let source_path = args.next().context("missing source PUB path")?;
    let variant_path = args.next().context("missing variant PUB path")?;
    let output_path = args.next().context("missing output JSON path")?;
    if args.next().is_some() {
        bail!("unexpected extra arguments");
    }

    let source = fs::read(&source_path)
        .with_context(|| format!("read {}", Path::new(&source_path).display()))?;
    let variant = fs::read(&variant_path)
        .with_context(|| format!("read {}", Path::new(&variant_path).display()))?;

    let source_sha256 = sha256_hex(&source);
    let variant_sha256 = sha256_hex(&variant);
    if source_sha256 != EXPECTED_SOURCE_SHA256 {
        bail!("unexpected source SHA-256: {source_sha256}");
    }
    if variant_sha256 != EXPECTED_VARIANT_SHA256 {
        bail!("unexpected variant SHA-256: {variant_sha256}");
    }

    let source_paths = stream_paths(&source)?;
    let variant_paths = stream_paths(&variant)?;
    let all_paths = source_paths
        .keys()
        .chain(variant_paths.keys())
        .cloned()
        .collect::<BTreeSet<_>>();

    let mut logical_streams = Vec::new();
    for path in all_paths {
        let source_payload = source_paths
            .contains_key(&path)
            .then(|| read_stream(&source, &path))
            .transpose()?;
        let variant_payload = variant_paths
            .contains_key(&path)
            .then(|| read_stream(&variant, &path))
            .transpose()?;
        let byte_equal = source_payload.as_deref() == variant_payload.as_deref();
        logical_streams.push(LogicalStreamDiff {
            path,
            source_len: source_payload.as_ref().map(Vec::len),
            variant_len: variant_payload.as_ref().map(Vec::len),
            source_sha256: source_payload.as_deref().map(sha256_hex),
            variant_sha256: variant_payload.as_deref().map(sha256_hex),
            byte_equal,
        });
    }

    let source_quill = read_stream(&source, QUILL_STREAM)?;
    let variant_quill = read_stream(&variant, QUILL_STREAM)?;
    let source_descriptors = parse_descriptor_directory(&source_quill)?;
    let variant_descriptors = parse_descriptor_directory(&variant_quill)?;
    let max_descriptors = source_descriptors.len().max(variant_descriptors.len());

    let mut descriptor_diffs = Vec::new();
    for ordinal in 0..max_descriptors {
        let source_descriptor = source_descriptors.get(ordinal);
        let variant_descriptor = variant_descriptors.get(ordinal);
        let source_payload = source_descriptor
            .map(|descriptor| descriptor_payload(&source_quill, descriptor))
            .transpose()?;
        let variant_payload = variant_descriptor
            .map(|descriptor| descriptor_payload(&variant_quill, descriptor))
            .transpose()?;
        descriptor_diffs.push(DescriptorDiff {
            ordinal,
            source_name: source_descriptor.map(|descriptor| descriptor_name(&descriptor.name)),
            variant_name: variant_descriptor.map(|descriptor| descriptor_name(&descriptor.name)),
            source_len: source_descriptor.map(|descriptor| descriptor.data_length),
            variant_len: variant_descriptor.map(|descriptor| descriptor.data_length),
            source_payload_sha256: source_payload.map(sha256_hex),
            variant_payload_sha256: variant_payload.map(sha256_hex),
            source_all_ff: source_payload.map(|payload| payload.iter().all(|byte| *byte == 0xff)),
            variant_all_ff: variant_payload.map(|payload| payload.iter().all(|byte| *byte == 0xff)),
            payload_equal: source_payload == variant_payload,
        });
    }

    let changed_logical_stream_paths = logical_streams
        .iter()
        .filter(|row| !row.byte_equal)
        .map(|row| row.path.clone())
        .collect::<Vec<_>>();
    let changed_quill_descriptor_names = descriptor_diffs
        .iter()
        .filter(|row| !row.payload_equal)
        .filter_map(|row| row.source_name.clone().or_else(|| row.variant_name.clone()))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();

    let topology = |items: &[Descriptor]| {
        items
            .iter()
            .map(|item| (item.name, item.data_length))
            .collect::<Vec<_>>()
    };

    let contents_stream_equal = logical_streams
        .iter()
        .find(|row| row.path == CONTENTS_STREAM)
        .is_some_and(|row| row.byte_equal);
    let quill_stream_equal = logical_streams
        .iter()
        .find(|row| row.path == QUILL_STREAM)
        .is_some_and(|row| row.byte_equal);

    let report = Report {
        schema: "chaptera.quill-story-same-document-stream-diff.v1".to_owned(),
        source_sha256,
        variant_sha256,
        source_byte_len: source.len(),
        variant_byte_len: variant.len(),
        logical_stream_topology_equal: source_paths.keys().eq(variant_paths.keys()),
        changed_logical_stream_count: changed_logical_stream_paths.len(),
        changed_logical_stream_paths,
        contents_stream_equal,
        quill_stream_equal,
        logical_streams,
        quill_descriptor_topology_equal: topology(&source_descriptors) == topology(&variant_descriptors),
        changed_quill_descriptor_count: descriptor_diffs
            .iter()
            .filter(|row| !row.payload_equal)
            .count(),
        changed_quill_descriptor_names,
        text_payloads_equal: compare_named_payloads(
            &source_quill,
            &source_descriptors,
            &variant_quill,
            &variant_descriptors,
            *b"TEXT",
        )?,
        fdpp_payloads_equal: compare_named_payloads(
            &source_quill,
            &source_descriptors,
            &variant_quill,
            &variant_descriptors,
            *b"FDPP",
        )?,
        fdpc_payloads_equal: compare_named_payloads(
            &source_quill,
            &source_descriptors,
            &variant_quill,
            &variant_descriptors,
            *b"FDPC",
        )?,
        stsh_payloads_equal: compare_named_payloads(
            &source_quill,
            &source_descriptors,
            &variant_quill,
            &variant_descriptors,
            *b"STSH",
        )?,
        syid_payloads_equal: compare_named_payloads(
            &source_quill,
            &source_descriptors,
            &variant_quill,
            &variant_descriptors,
            *b"SYID",
        )?,
        strs_payloads_equal: compare_named_payloads(
            &source_quill,
            &source_descriptors,
            &variant_quill,
            &variant_descriptors,
            *b"STRS",
        )?,
        quill_descriptors: descriptor_diffs,
        evidence_boundary: "exact same-document public pair only; receipt contains CFB logical stream paths, lengths, hashes/equality and Quill descriptor names, lengths, payload hashes/all-FF/equality; no document text, raw payload bytes, physical offsets or recovered materialization".to_owned(),
    };

    let parent = Path::new(&output_path)
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty());
    if let Some(parent) = parent {
        fs::create_dir_all(parent)?;
    }
    fs::write(output_path, serde_json::to_vec_pretty(&report)?)?;
    Ok(())
}
