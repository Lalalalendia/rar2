use anyhow::{bail, Context, Result};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::io::Cursor;
use std::path::Path;

const QUILL_STREAM: &str = "/Quill/QuillSub/CONTENTS";
const DESCRIPTOR_ROOT: u32 = 0x18;
const DESCRIPTOR_END: u32 = 0xffff_ffff;
const DESCRIPTOR_SIZE: usize = 24;
const DESCRIPTOR_PRESENT: u16 = 0x0018;

const EXPECTED_SOURCE_SHA256: &str =
    "6b5d5b269be7ca74b03d47423aec985676c45be7033e007792fcc3eb35ad929a";
const EXPECTED_VARIANT_SHA256: &str =
    "e8c360c97f4604e60fd84fbc106f59723b03838e05c74b74f4d56aef8d40088e";

#[derive(Debug, Clone, PartialEq, Eq)]
struct Descriptor {
    name: [u8; 4],
    data_offset: u32,
    data_length: u32,
}

#[derive(Debug, Serialize)]
struct Report {
    schema: String,
    source_sha256: String,
    variant_sha256: String,
    source_quill_len: usize,
    variant_quill_len: usize,
    descriptor_count: usize,
    descriptor_layout_equal: bool,
    masked_quill_equal: bool,
    source_masked_quill_sha256: String,
    variant_masked_quill_sha256: String,
    differing_non_payload_byte_count: usize,
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
            let data_offset =
                u32_at(bytes, offset + 16).context("descriptor data offset truncated")?;
            let data_length =
                u32_at(bytes, offset + 20).context("descriptor data length truncated")?;
            out.push(Descriptor {
                name: [name_raw[0], name_raw[1], name_raw[2], name_raw[3]],
                data_offset,
                data_length,
            });
        }

        current = next;
    }

    Ok(out)
}

fn mask_payloads(bytes: &[u8], descriptors: &[Descriptor]) -> Result<Vec<u8>> {
    let mut masked = bytes.to_vec();
    for descriptor in descriptors {
        let start =
            usize::try_from(descriptor.data_offset).context("descriptor offset too large")?;
        let len = usize::try_from(descriptor.data_length).context("descriptor length too large")?;
        let end = start
            .checked_add(len)
            .context("descriptor payload range overflow")?;
        let range = masked
            .get_mut(start..end)
            .context("descriptor payload outside Quill")?;
        range.fill(0);
    }
    Ok(masked)
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

    let source_quill = pub_cfb::read_stream_reader(Cursor::new(&source), QUILL_STREAM)?;
    let variant_quill = pub_cfb::read_stream_reader(Cursor::new(&variant), QUILL_STREAM)?;
    let source_descriptors = parse_descriptor_directory(&source_quill)?;
    let variant_descriptors = parse_descriptor_directory(&variant_quill)?;

    let descriptor_layout_equal = source_descriptors == variant_descriptors;
    let source_masked = mask_payloads(&source_quill, &source_descriptors)?;
    let variant_masked = mask_payloads(&variant_quill, &variant_descriptors)?;

    let differing_non_payload_byte_count = if source_masked.len() == variant_masked.len() {
        source_masked
            .iter()
            .zip(&variant_masked)
            .filter(|(left, right)| left != right)
            .count()
    } else {
        source_masked.len().max(variant_masked.len())
    };

    let report = Report {
        schema: "chaptera.quill-story-same-document-skeleton-diff.v1".to_owned(),
        source_sha256,
        variant_sha256,
        source_quill_len: source_quill.len(),
        variant_quill_len: variant_quill.len(),
        descriptor_count: source_descriptors.len(),
        descriptor_layout_equal,
        masked_quill_equal: source_masked == variant_masked,
        source_masked_quill_sha256: sha256_hex(&source_masked),
        variant_masked_quill_sha256: sha256_hex(&variant_masked),
        differing_non_payload_byte_count,
        evidence_boundary: "exact same-document public pair only; every declared Quill descriptor payload is masked before comparison; receipt retains only source identities, lengths, descriptor count/layout equality, masked hashes/equality and non-payload difference count; no document text, raw payloads, descriptor offsets, physical offsets or PUB artifacts".to_owned(),
    };

    if let Some(parent) = Path::new(&output_path)
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)?;
    }
    fs::write(output_path, serde_json::to_vec_pretty(&report)?)?;
    Ok(())
}
