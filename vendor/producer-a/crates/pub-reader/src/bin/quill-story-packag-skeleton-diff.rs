use anyhow::{bail, Context, Result};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::io::Cursor;
use std::path::Path;

const QUILL_STREAM: &str = "/Quill/QuillSub/CONTENTS";
const DESCRIPTOR_ROOT: u32 = 0x18;
const DESCRIPTOR_END: u32 = 0xffff_ffff;
const DESCRIPTOR_SIZE: usize = 24;
const DESCRIPTOR_PRESENT: u16 = 0x0018;

const REGION_INTERSTITIAL: u8 = 0;
const REGION_NODE_HEADER: u8 = 1;
const REGION_DESCRIPTOR_METADATA: u8 = 2;
const REGION_PAYLOAD: u8 = 3;

const EXPECTED_SOURCE_SHA256: &str =
    "6b5d5b269be7ca74b03d47423aec985676c45be7033e007792fcc3eb35ad929a";
const EXPECTED_VARIANT_SHA256: &str =
    "e8c360c97f4604e60fd84fbc106f59723b03838e05c74b74f4d56aef8d40088e";

#[derive(Debug, Clone, PartialEq, Eq)]
struct Descriptor {
    entry_offset: usize,
    name: [u8; 4],
    data_offset: u32,
    data_length: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Directory {
    node_starts: Vec<usize>,
    descriptors: Vec<Descriptor>,
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
    region_layout_equal: bool,
    masked_quill_equal: bool,
    source_masked_quill_sha256: String,
    variant_masked_quill_sha256: String,
    differing_non_payload_byte_count: usize,
    differing_node_header_byte_count: usize,
    differing_descriptor_metadata_byte_count: usize,
    differing_interstitial_byte_count: usize,
    interstitial_diff_run_count: usize,
    interstitial_diff_run_lengths: Vec<usize>,
    source_interstitial_diff_ff_count: usize,
    variant_interstitial_diff_ff_count: usize,
    source_interstitial_diff_all_ff: bool,
    variant_interstitial_diff_all_ff: bool,
    interstitial_diff_gap_bytes: BTreeMap<String, usize>,
    descriptor_presence_diff_count: usize,
    descriptor_name_diff_count: usize,
    descriptor_opt_a_diff_count: usize,
    descriptor_opt_b_diff_count: usize,
    descriptor_opt_c_diff_count: usize,
    descriptor_bit_type_diff_count: usize,
    descriptor_data_offset_diff_count: usize,
    descriptor_data_length_diff_count: usize,
    node_prefix_u16_diff_count: usize,
    node_count_diff_count: usize,
    node_next_diff_count: usize,
    changed_descriptor_metadata_names: Vec<String>,
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

fn descriptor_name(name: &[u8; 4]) -> String {
    String::from_utf8_lossy(name).trim_end().to_owned()
}

fn parse_descriptor_directory(bytes: &[u8]) -> Result<Directory> {
    let mut current = DESCRIPTOR_ROOT;
    let mut seen = BTreeSet::new();
    let mut node_starts = Vec::new();
    let mut descriptors = Vec::new();

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

        node_starts.push(start);
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
            descriptors.push(Descriptor {
                entry_offset: offset,
                name: [name_raw[0], name_raw[1], name_raw[2], name_raw[3]],
                data_offset,
                data_length,
            });
        }

        current = next;
    }

    Ok(Directory {
        node_starts,
        descriptors,
    })
}

fn mark_range(regions: &mut [u8], start: usize, len: usize, kind: u8) -> Result<()> {
    let end = start.checked_add(len).context("region range overflow")?;
    let range = regions
        .get_mut(start..end)
        .context("region outside Quill")?;
    for byte in range {
        if *byte != REGION_INTERSTITIAL && *byte != kind {
            bail!("overlapping Quill structural regions");
        }
        *byte = kind;
    }
    Ok(())
}

fn region_map(bytes: &[u8], directory: &Directory) -> Result<Vec<u8>> {
    let mut regions = vec![REGION_INTERSTITIAL; bytes.len()];

    for start in &directory.node_starts {
        mark_range(&mut regions, *start, 8, REGION_NODE_HEADER)?;
    }
    for descriptor in &directory.descriptors {
        mark_range(
            &mut regions,
            descriptor.entry_offset,
            DESCRIPTOR_SIZE,
            REGION_DESCRIPTOR_METADATA,
        )?;
        let payload_start =
            usize::try_from(descriptor.data_offset).context("descriptor offset too large")?;
        let payload_len =
            usize::try_from(descriptor.data_length).context("descriptor length too large")?;
        mark_range(&mut regions, payload_start, payload_len, REGION_PAYLOAD)?;
    }

    Ok(regions)
}

fn mask_payloads(bytes: &[u8], regions: &[u8]) -> Vec<u8> {
    bytes
        .iter()
        .zip(regions)
        .map(
            |(byte, region)| {
                if *region == REGION_PAYLOAD {
                    0
                } else {
                    *byte
                }
            },
        )
        .collect()
}

fn interstitial_run_lengths(indices: &[usize]) -> Vec<usize> {
    if indices.is_empty() {
        return Vec::new();
    }

    let mut out = Vec::new();
    let mut run = 1usize;
    for pair in indices.windows(2) {
        if pair[1] == pair[0] + 1 {
            run += 1;
        } else {
            out.push(run);
            run = 1;
        }
    }
    out.push(run);
    out
}

fn interstitial_gap_bytes(
    indices: &[usize],
    directory: &Directory,
) -> Result<BTreeMap<String, usize>> {
    let mut payloads = directory
        .descriptors
        .iter()
        .map(|descriptor| {
            let start =
                usize::try_from(descriptor.data_offset).context("descriptor offset too large")?;
            let len =
                usize::try_from(descriptor.data_length).context("descriptor length too large")?;
            let end = start
                .checked_add(len)
                .context("descriptor payload range overflow")?;
            Ok((start, end, descriptor_name(&descriptor.name)))
        })
        .collect::<Result<Vec<_>>>()?;
    payloads.sort_by_key(|row| row.0);

    let mut out = BTreeMap::new();
    for index in indices {
        let mut previous = "START".to_owned();
        let mut next = "END".to_owned();
        for (start, end, name) in &payloads {
            if *end <= *index {
                previous = name.clone();
                continue;
            }
            if *start > *index {
                next = name.clone();
            }
            break;
        }
        *out.entry(format!("{previous}->{next}")).or_insert(0) += 1;
    }
    Ok(out)
}

fn field_diff(
    source: &[u8],
    variant: &[u8],
    source_offset: usize,
    variant_offset: usize,
    relative: usize,
    len: usize,
) -> Result<bool> {
    let source_range = source
        .get(source_offset + relative..source_offset + relative + len)
        .context("source metadata field outside Quill")?;
    let variant_range = variant
        .get(variant_offset + relative..variant_offset + relative + len)
        .context("variant metadata field outside Quill")?;
    Ok(source_range != variant_range)
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
    let source_directory = parse_descriptor_directory(&source_quill)?;
    let variant_directory = parse_descriptor_directory(&variant_quill)?;
    let source_regions = region_map(&source_quill, &source_directory)?;
    let variant_regions = region_map(&variant_quill, &variant_directory)?;

    let descriptor_layout_equal = source_directory == variant_directory;
    let region_layout_equal = source_regions == variant_regions;
    let source_masked = mask_payloads(&source_quill, &source_regions);
    let variant_masked = mask_payloads(&variant_quill, &variant_regions);

    let mut differing_node_header_byte_count = 0usize;
    let mut differing_descriptor_metadata_byte_count = 0usize;
    let mut differing_interstitial_byte_count = 0usize;
    let mut interstitial_diff_indices = Vec::new();
    if source_quill.len() == variant_quill.len() && region_layout_equal {
        for (index, ((source_byte, variant_byte), region)) in source_quill
            .iter()
            .zip(&variant_quill)
            .zip(&source_regions)
            .enumerate()
        {
            if source_byte == variant_byte || *region == REGION_PAYLOAD {
                continue;
            }
            match *region {
                REGION_NODE_HEADER => differing_node_header_byte_count += 1,
                REGION_DESCRIPTOR_METADATA => differing_descriptor_metadata_byte_count += 1,
                REGION_INTERSTITIAL => {
                    differing_interstitial_byte_count += 1;
                    interstitial_diff_indices.push(index);
                }
                _ => {}
            }
        }
    }

    let differing_non_payload_byte_count = differing_node_header_byte_count
        + differing_descriptor_metadata_byte_count
        + differing_interstitial_byte_count;
    let interstitial_diff_run_lengths = interstitial_run_lengths(&interstitial_diff_indices);
    let source_interstitial_diff_ff_count = interstitial_diff_indices
        .iter()
        .filter(|index| source_quill[**index] == 0xff)
        .count();
    let variant_interstitial_diff_ff_count = interstitial_diff_indices
        .iter()
        .filter(|index| variant_quill[**index] == 0xff)
        .count();
    let source_interstitial_diff_all_ff =
        source_interstitial_diff_ff_count == interstitial_diff_indices.len();
    let variant_interstitial_diff_all_ff =
        variant_interstitial_diff_ff_count == interstitial_diff_indices.len();
    let interstitial_diff_gap_bytes =
        interstitial_gap_bytes(&interstitial_diff_indices, &source_directory)?;

    let mut descriptor_presence_diff_count = 0usize;
    let mut descriptor_name_diff_count = 0usize;
    let mut descriptor_opt_a_diff_count = 0usize;
    let mut descriptor_opt_b_diff_count = 0usize;
    let mut descriptor_opt_c_diff_count = 0usize;
    let mut descriptor_bit_type_diff_count = 0usize;
    let mut descriptor_data_offset_diff_count = 0usize;
    let mut descriptor_data_length_diff_count = 0usize;
    let mut changed_descriptor_metadata_names = BTreeSet::new();

    for (source_descriptor, variant_descriptor) in source_directory
        .descriptors
        .iter()
        .zip(&variant_directory.descriptors)
    {
        let mut changed = false;
        for (relative, len, count) in [
            (0usize, 2usize, &mut descriptor_presence_diff_count),
            (2, 4, &mut descriptor_name_diff_count),
            (6, 2, &mut descriptor_opt_a_diff_count),
            (8, 2, &mut descriptor_opt_b_diff_count),
            (10, 2, &mut descriptor_opt_c_diff_count),
            (12, 4, &mut descriptor_bit_type_diff_count),
            (16, 4, &mut descriptor_data_offset_diff_count),
            (20, 4, &mut descriptor_data_length_diff_count),
        ] {
            if field_diff(
                &source_quill,
                &variant_quill,
                source_descriptor.entry_offset,
                variant_descriptor.entry_offset,
                relative,
                len,
            )? {
                *count += 1;
                changed = true;
            }
        }
        if changed {
            changed_descriptor_metadata_names.insert(descriptor_name(&source_descriptor.name));
        }
    }

    let mut node_prefix_u16_diff_count = 0usize;
    let mut node_count_diff_count = 0usize;
    let mut node_next_diff_count = 0usize;
    for (source_start, variant_start) in source_directory
        .node_starts
        .iter()
        .zip(&variant_directory.node_starts)
    {
        node_prefix_u16_diff_count += usize::from(field_diff(
            &source_quill,
            &variant_quill,
            *source_start,
            *variant_start,
            0,
            2,
        )?);
        node_count_diff_count += usize::from(field_diff(
            &source_quill,
            &variant_quill,
            *source_start,
            *variant_start,
            2,
            2,
        )?);
        node_next_diff_count += usize::from(field_diff(
            &source_quill,
            &variant_quill,
            *source_start,
            *variant_start,
            4,
            4,
        )?);
    }

    let report = Report {
        schema: "chaptera.quill-story-same-document-skeleton-diff.v2".to_owned(),
        source_sha256,
        variant_sha256,
        source_quill_len: source_quill.len(),
        variant_quill_len: variant_quill.len(),
        descriptor_count: source_directory.descriptors.len(),
        descriptor_layout_equal,
        region_layout_equal,
        masked_quill_equal: source_masked == variant_masked,
        source_masked_quill_sha256: sha256_hex(&source_masked),
        variant_masked_quill_sha256: sha256_hex(&variant_masked),
        differing_non_payload_byte_count,
        differing_node_header_byte_count,
        differing_descriptor_metadata_byte_count,
        differing_interstitial_byte_count,
        interstitial_diff_run_count: interstitial_diff_run_lengths.len(),
        interstitial_diff_run_lengths,
        source_interstitial_diff_ff_count,
        variant_interstitial_diff_ff_count,
        source_interstitial_diff_all_ff,
        variant_interstitial_diff_all_ff,
        interstitial_diff_gap_bytes,
        descriptor_presence_diff_count,
        descriptor_name_diff_count,
        descriptor_opt_a_diff_count,
        descriptor_opt_b_diff_count,
        descriptor_opt_c_diff_count,
        descriptor_bit_type_diff_count,
        descriptor_data_offset_diff_count,
        descriptor_data_length_diff_count,
        node_prefix_u16_diff_count,
        node_count_diff_count,
        node_next_diff_count,
        changed_descriptor_metadata_names: changed_descriptor_metadata_names
            .into_iter()
            .collect(),
        evidence_boundary: "exact same-document public pair only; every declared Quill descriptor payload is classified separately from descriptor metadata, list-node headers and interstitial bytes; receipt retains only source identities, lengths, equality flags, aggregate difference counts, changed metadata field classes and descriptor names; no document text, raw payloads, descriptor offsets, physical offsets or PUB artifacts".to_owned(),
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
