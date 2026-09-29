use anyhow::{bail, Context, Result};
use pub_contents::{
    CONTENTS_RAW_TYPE_STORY_CATALOG, parse_0x2c_header, parse_confirmed_0x2c_chunk,
    parse_confirmed_0x2c_trailer_root, parse_confirmed_chunk_reference,
    parse_confirmed_mature_story_catalog, MatureStoryCatalog, RawContentsBlockBody,
};
use pub_core::StreamPath;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    io::Cursor,
    path::{Path, PathBuf},
};

const CONTENTS_STREAM: &str = "/Contents";
const QUILL_STREAM: &str = "/Quill/QuillSub/CONTENTS";
const DESCRIPTOR_ROOT: u32 = 0x18;
const DESCRIPTOR_END: u32 = 0xffff_ffff;
const DESCRIPTOR_SIZE: usize = 24;
const DESCRIPTOR_PRESENT: u16 = 0x0018;

#[derive(Debug, Clone)]
struct Descriptor {
    name: [u8; 4],
    opt_a: u16,
    opt_b: u16,
    opt_c: u16,
    bit_type: [u8; 4],
    data_offset: u32,
    data_length: u32,
}

#[derive(Debug, Serialize)]
struct DescriptorMetadataProfile {
    opt_a_is_zero: bool,
    opt_a_equals_grounded_story_count: bool,
    opt_b_is_one: bool,
    opt_c_is_zero: bool,
    bit_type_all_zero: bool,
    bit_type_all_ff: bool,
    bit_type_sha256: String,
}

#[derive(Debug, Serialize)]
struct BtePlcCandidate {
    carrier_index: usize,
    prefix_offset: usize,
    count: u32,
    data_size: u32,
    nonzero_flag_count: usize,
    position_count: usize,
    positions_monotonic: bool,
    positions_inside_text_count: usize,
    text_start_present: bool,
    text_end_present: bool,
    count_covers_story_count: bool,
    target_count: usize,
    targets_inside_paired_format_ranges_count: usize,
}

#[derive(Debug, Serialize)]
struct BteCarrierProfile {
    carrier_index: usize,
    descriptor_length: u32,
    plausible_count_prefix_count: usize,
    data_size_4_prefix_count: usize,
    count_and_data_size_4_prefix_count: usize,
    exact_consumption_prefix_count: usize,
    canonical_shape_prefix_count: usize,
    prefix0_count: Option<u32>,
    prefix0_data_size: Option<u32>,
    prefix0_implied_count: Option<u32>,
    prefix0_implied_count_matches_header: bool,
}

#[derive(Debug, Serialize)]
struct StoryCatalogScalarFieldProfile {
    field_id: u16,
    present_entry_count: usize,
    scalar_entry_count: usize,
    duplicate_entry_count: usize,
    wire_types: Vec<u8>,
    all_entries_present_once_scalar: bool,
    sum_equals_text_utf16_units: bool,
    sum_equals_text_bytes: bool,
    monotonic_non_decreasing: bool,
    last_equals_text_utf16_units: bool,
    last_equals_text_bytes: bool,
}

#[derive(Debug, Serialize)]
struct StoryCatalogScalarPairProfile {
    start_field_id: u16,
    end_field_id: u16,
    all_end_ge_start: bool,
    contiguous: bool,
    sum_deltas_equals_text_utf16_units: bool,
    sum_deltas_equals_text_bytes: bool,
    outer_span_equals_text_utf16_units: bool,
    outer_span_equals_text_bytes: bool,
    first_start_is_zero: bool,
}

#[derive(Debug, Serialize)]
struct StoryCatalogFixed8PairProfile {
    field_id: u16,
    present_entry_count: usize,
    fixed8_entry_count: usize,
    duplicate_entry_count: usize,
    wire_types: Vec<u8>,
    all_entries_present_once_fixed8: bool,
    first_words_monotonic: bool,
    second_words_monotonic: bool,
    all_second_ge_first: bool,
    contiguous: bool,
    sum_deltas_equals_text_utf16_units: bool,
    sum_deltas_equals_text_bytes: bool,
    outer_span_equals_text_utf16_units: bool,
    outer_span_equals_text_bytes: bool,
    first_start_is_zero: bool,
}

#[derive(Debug, Serialize)]
struct WitnessRow {
    source_sha256: String,
    byte_len: usize,
    contents_serialization_revision: u16,
    grounded_story_count: u32,
    descriptor_count: usize,
    descriptor_opt_b_one_count: usize,
    descriptor_opt_c_zero_count: usize,
    descriptor_opt_b_c_ordinary_count: usize,
    syid_descriptor_metadata: DescriptorMetadataProfile,
    strs_descriptor_metadata: DescriptorMetadataProfile,
    text_descriptor_metadata: DescriptorMetadataProfile,
    syid_strs_text_opt_a_all_equal: bool,
    syid_strs_text_bit_type_all_equal: bool,
    story_catalog_entries_with_unsupported_tail: usize,
    story_catalog_scalar_profiles: Vec<StoryCatalogScalarFieldProfile>,
    story_catalog_scalar_pair_profiles: Vec<StoryCatalogScalarPairProfile>,
    story_catalog_fixed8_pair_profiles: Vec<StoryCatalogFixed8PairProfile>,

    syid_descriptor_length: u32,
    syid_length_matches_grounded_count: bool,
    syid_chunk_all_ff: bool,

    strs_descriptor_length: u32,
    strs_length_matches_observed_22_plus_8n: bool,
    strs_chunk_all_ff: bool,
    strs_direct_generic_plc_candidate_count: usize,

    text_descriptor_length: u32,
    text_utf16_units: u64,

    btep_descriptor_lengths: Vec<u32>,
    btec_descriptor_lengths: Vec<u32>,
    fdpp_descriptor_lengths: Vec<u32>,
    fdpc_descriptor_lengths: Vec<u32>,
    descriptor_topology: BTreeMap<String, Vec<u32>>,
    btep_profiles: Vec<BteCarrierProfile>,
    btec_profiles: Vec<BteCarrierProfile>,
    btep_candidates: Vec<BtePlcCandidate>,
    btec_candidates: Vec<BtePlcCandidate>,
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

fn descriptor_metadata_profile(
    descriptor: &Descriptor,
    grounded_story_count: u32,
) -> DescriptorMetadataProfile {
    DescriptorMetadataProfile {
        opt_a_is_zero: descriptor.opt_a == 0,
        opt_a_equals_grounded_story_count:
            u32::from(descriptor.opt_a) == grounded_story_count,
        opt_b_is_one: descriptor.opt_b == 1,
        opt_c_is_zero: descriptor.opt_c == 0,
        bit_type_all_zero: descriptor.bit_type.iter().all(|byte| *byte == 0),
        bit_type_all_ff: descriptor.bit_type.iter().all(|byte| *byte == 0xff),
        bit_type_sha256: sha256_hex(&descriptor.bit_type),
    }
}

fn descriptor_range<'a>(bytes: &'a [u8], descriptor: &Descriptor) -> Result<&'a [u8]> {
    let start = usize::try_from(descriptor.data_offset).context("descriptor offset too large")?;
    let len = usize::try_from(descriptor.data_length).context("descriptor length too large")?;
    let end = start.checked_add(len).context("descriptor range overflow")?;
    bytes
        .get(start..end)
        .with_context(|| format!("descriptor {:?} range outside Quill", descriptor.name))
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
        let count = usize::from(
            u16_at(bytes, start + 2).context("descriptor node header is truncated")?,
        );
        let next = u32_at(bytes, start + 4).context("descriptor node next is truncated")?;
        let array_start = start.checked_add(8).context("descriptor array overflow")?;
        let array_len = count
            .checked_mul(DESCRIPTOR_SIZE)
            .context("descriptor array size overflow")?;
        let array_end = array_start
            .checked_add(array_len)
            .context("descriptor array end overflow")?;
        if array_end > bytes.len() {
            bail!("descriptor array outside Quill");
        }

        for index in 0..count {
            let offset = array_start + index * DESCRIPTOR_SIZE;
            let present = u16_at(bytes, offset).context("descriptor presence is truncated")?;
            if present != DESCRIPTOR_PRESENT {
                bail!("unexpected descriptor presence marker");
            }
            let name_raw = bytes
                .get(offset + 2..offset + 6)
                .context("descriptor name is truncated")?;
            let name = [name_raw[0], name_raw[1], name_raw[2], name_raw[3]];
            let opt_a = u16_at(bytes, offset + 6).context("descriptor optA is truncated")?;
            let opt_b = u16_at(bytes, offset + 8).context("descriptor optB is truncated")?;
            let opt_c = u16_at(bytes, offset + 10).context("descriptor optC is truncated")?;
            let bit_type_raw = bytes
                .get(offset + 12..offset + 16)
                .context("descriptor bitType is truncated")?;
            let bit_type = [
                bit_type_raw[0],
                bit_type_raw[1],
                bit_type_raw[2],
                bit_type_raw[3],
            ];
            let data_offset =
                u32_at(bytes, offset + 16).context("descriptor data offset is truncated")?;
            let data_length =
                u32_at(bytes, offset + 20).context("descriptor data length is truncated")?;
            out.push(Descriptor {
                name,
                opt_a,
                opt_b,
                opt_c,
                bit_type,
                data_offset,
                data_length,
            });
        }

        current = next;
    }

    Ok(out)
}

fn unique_descriptor<'a>(descriptors: &'a [Descriptor], name: [u8; 4]) -> Result<&'a Descriptor> {
    let mut found = descriptors.iter().filter(|item| item.name == name);
    let first = found
        .next()
        .with_context(|| format!("missing Quill descriptor {:?}", name))?;
    if found.next().is_some() {
        bail!("duplicate required Quill descriptor {:?}", name);
    }
    Ok(first)
}

fn descriptors_named<'a>(
    descriptors: &'a [Descriptor],
    name: [u8; 4],
) -> Vec<&'a Descriptor> {
    descriptors
        .iter()
        .filter(|item| item.name == name)
        .collect()
}

fn scan_direct_generic_plc_count(payload: &[u8], grounded_count: u32) -> usize {
    let max_prefix = payload.len().saturating_sub(12).min(32);
    (0..=max_prefix)
        .filter(|prefix| u32_at(payload, *prefix) == Some(grounded_count))
        .count()
}

fn grounded_contents_story_catalog(contents: &[u8]) -> Result<(u16, MatureStoryCatalog)> {
    let stream = StreamPath(CONTENTS_STREAM.into());
    let header =
        parse_0x2c_header(stream.clone(), contents).context("parse mature Contents header")?;
    let trailer = parse_confirmed_0x2c_trailer_root(contents, &header)
        .context("parse mature Contents trailer")?;

    let mut story_refs = Vec::new();
    for seq_num in 0..trailer.directory.slots.len() {
        let Some(reference) =
            parse_confirmed_chunk_reference(contents, &trailer.directory, seq_num)
                .with_context(|| format!("parse chunk reference {seq_num}"))?
        else {
            continue;
        };
        if reference.raw_types.len() == 1
            && reference.raw_types[0].value == CONTENTS_RAW_TYPE_STORY_CATALOG
            && reference.chunk_offsets.len() == 1
        {
            story_refs.push(reference);
        }
    }
    if story_refs.len() != 1 {
        bail!(
            "expected exactly one strict 0x65 reference, found {}",
            story_refs.len()
        );
    }

    let reference = &story_refs[0];
    let chunk = parse_confirmed_0x2c_chunk(
        stream,
        contents,
        reference.chunk_offsets[0].value,
    )
    .context("parse Story catalog chunk")?;
    let catalog = parse_confirmed_mature_story_catalog(contents, &chunk)
        .context("parse grounded Story catalog")?;
    Ok((header.preamble.serialization_revision, catalog))
}

fn story_catalog_scalar_profiles(
    catalog: &MatureStoryCatalog,
    text_utf16_units: u64,
    text_bytes: u64,
) -> Vec<StoryCatalogScalarFieldProfile> {
    let mut field_ids = BTreeSet::new();
    for entry in &catalog.entries {
        for field in &entry.fields {
            field_ids.insert(field.id);
        }
    }

    field_ids
        .into_iter()
        .map(|field_id| {
            let mut present_entry_count = 0usize;
            let mut scalar_entry_count = 0usize;
            let mut duplicate_entry_count = 0usize;
            let mut wire_types = BTreeSet::new();
            let mut values = Vec::new();

            for entry in &catalog.entries {
                let matches = entry
                    .fields
                    .iter()
                    .filter(|field| field.id == field_id)
                    .collect::<Vec<_>>();
                if !matches.is_empty() {
                    present_entry_count += 1;
                }
                if matches.len() > 1 {
                    duplicate_entry_count += 1;
                }
                for field in &matches {
                    wire_types.insert(field.block_type);
                }
                if matches.len() == 1 {
                    let value = match &matches[0].body {
                        RawContentsBlockBody::U16 { value, .. } => Some(u64::from(*value)),
                        RawContentsBlockBody::U32 { value, .. } => Some(u64::from(*value)),
                        _ => None,
                    };
                    if let Some(value) = value {
                        scalar_entry_count += 1;
                        values.push(value);
                    }
                }
            }

            let all_entries_present_once_scalar =
                present_entry_count == catalog.entries.len()
                    && scalar_entry_count == catalog.entries.len()
                    && duplicate_entry_count == 0;
            let sum = if all_entries_present_once_scalar {
                values
                    .iter()
                    .try_fold(0u64, |acc, value| acc.checked_add(*value))
            } else {
                None
            };
            let monotonic_non_decreasing = all_entries_present_once_scalar
                && values.windows(2).all(|pair| pair[0] <= pair[1]);
            let last = if all_entries_present_once_scalar {
                values.last().copied()
            } else {
                None
            };

            StoryCatalogScalarFieldProfile {
                field_id,
                present_entry_count,
                scalar_entry_count,
                duplicate_entry_count,
                wire_types: wire_types.into_iter().collect(),
                all_entries_present_once_scalar,
                sum_equals_text_utf16_units: sum == Some(text_utf16_units),
                sum_equals_text_bytes: sum == Some(text_bytes),
                monotonic_non_decreasing,
                last_equals_text_utf16_units: last == Some(text_utf16_units),
                last_equals_text_bytes: last == Some(text_bytes),
            }
        })
        .collect()
}

fn complete_story_scalar_vector(catalog: &MatureStoryCatalog, field_id: u16) -> Option<Vec<u64>> {
    let mut values = Vec::with_capacity(catalog.entries.len());
    for entry in &catalog.entries {
        let matches = entry
            .fields
            .iter()
            .filter(|field| field.id == field_id)
            .collect::<Vec<_>>();
        if matches.len() != 1 {
            return None;
        }
        let value = match &matches[0].body {
            RawContentsBlockBody::U16 { value, .. } => u64::from(*value),
            RawContentsBlockBody::U32 { value, .. } => u64::from(*value),
            _ => return None,
        };
        values.push(value);
    }
    Some(values)
}

fn profile_offset_pair(
    starts: &[u64],
    ends: &[u64],
    text_utf16_units: u64,
    text_bytes: u64,
) -> (bool, bool, bool, bool, bool, bool, bool) {
    if starts.len() != ends.len() || starts.is_empty() {
        return (false, false, false, false, false, false, false);
    }

    let all_end_ge_start = starts
        .iter()
        .zip(ends.iter())
        .all(|(start, end)| end >= start);
    let contiguous = ends
        .iter()
        .take(ends.len().saturating_sub(1))
        .zip(starts.iter().skip(1))
        .all(|(end, next_start)| end == next_start);
    let sum_deltas = if all_end_ge_start {
        starts
            .iter()
            .zip(ends.iter())
            .try_fold(0u64, |acc, (start, end)| acc.checked_add(end - start))
    } else {
        None
    };
    let outer_span = ends
        .last()
        .zip(starts.first())
        .and_then(|(end, start)| end.checked_sub(*start));

    (
        all_end_ge_start,
        contiguous,
        sum_deltas == Some(text_utf16_units),
        sum_deltas == Some(text_bytes),
        outer_span == Some(text_utf16_units),
        outer_span == Some(text_bytes),
        starts.first().copied() == Some(0),
    )
}

fn story_catalog_scalar_pair_profiles(
    catalog: &MatureStoryCatalog,
    text_utf16_units: u64,
    text_bytes: u64,
) -> Vec<StoryCatalogScalarPairProfile> {
    let ids = catalog
        .entries
        .iter()
        .flat_map(|entry| entry.fields.iter().map(|field| field.id))
        .collect::<BTreeSet<_>>();

    let vectors = ids
        .iter()
        .filter_map(|id| complete_story_scalar_vector(catalog, *id).map(|values| (*id, values)))
        .collect::<BTreeMap<_, _>>();

    let mut out = Vec::new();
    for (start_id, starts) in &vectors {
        for (end_id, ends) in &vectors {
            if start_id == end_id {
                continue;
            }
            let (
                all_end_ge_start,
                contiguous,
                sum_deltas_equals_text_utf16_units,
                sum_deltas_equals_text_bytes,
                outer_span_equals_text_utf16_units,
                outer_span_equals_text_bytes,
                first_start_is_zero,
            ) = profile_offset_pair(starts, ends, text_utf16_units, text_bytes);

            out.push(StoryCatalogScalarPairProfile {
                start_field_id: *start_id,
                end_field_id: *end_id,
                all_end_ge_start,
                contiguous,
                sum_deltas_equals_text_utf16_units,
                sum_deltas_equals_text_bytes,
                outer_span_equals_text_utf16_units,
                outer_span_equals_text_bytes,
                first_start_is_zero,
            });
        }
    }
    out
}

fn story_catalog_fixed8_pair_profiles(
    catalog: &MatureStoryCatalog,
    text_utf16_units: u64,
    text_bytes: u64,
) -> Vec<StoryCatalogFixed8PairProfile> {
    let ids = catalog
        .entries
        .iter()
        .flat_map(|entry| entry.fields.iter().map(|field| field.id))
        .collect::<BTreeSet<_>>();
    let mut out = Vec::new();

    for field_id in ids {
        let mut present_entry_count = 0usize;
        let mut fixed8_entry_count = 0usize;
        let mut duplicate_entry_count = 0usize;
        let mut wire_types = BTreeSet::new();
        let mut first_words = Vec::new();
        let mut second_words = Vec::new();

        for entry in &catalog.entries {
            let matches = entry
                .fields
                .iter()
                .filter(|field| field.id == field_id)
                .collect::<Vec<_>>();
            if !matches.is_empty() {
                present_entry_count += 1;
            }
            if matches.len() > 1 {
                duplicate_entry_count += 1;
            }
            for field in &matches {
                wire_types.insert(field.block_type);
            }
            if matches.len() == 1 {
                if let RawContentsBlockBody::Fixed8 { bytes, .. } = &matches[0].body {
                    fixed8_entry_count += 1;
                    first_words.push(u64::from(u32::from_le_bytes([
                        bytes[0], bytes[1], bytes[2], bytes[3],
                    ])));
                    second_words.push(u64::from(u32::from_le_bytes([
                        bytes[4], bytes[5], bytes[6], bytes[7],
                    ])));
                }
            }
        }

        if fixed8_entry_count == 0 {
            continue;
        }

        let all_entries_present_once_fixed8 =
            present_entry_count == catalog.entries.len()
                && fixed8_entry_count == catalog.entries.len()
                && duplicate_entry_count == 0;
        let first_words_monotonic = all_entries_present_once_fixed8
            && first_words.windows(2).all(|pair| pair[0] <= pair[1]);
        let second_words_monotonic = all_entries_present_once_fixed8
            && second_words.windows(2).all(|pair| pair[0] <= pair[1]);
        let (
            all_second_ge_first,
            contiguous,
            sum_deltas_equals_text_utf16_units,
            sum_deltas_equals_text_bytes,
            outer_span_equals_text_utf16_units,
            outer_span_equals_text_bytes,
            first_start_is_zero,
        ) = if all_entries_present_once_fixed8 {
            profile_offset_pair(&first_words, &second_words, text_utf16_units, text_bytes)
        } else {
            (false, false, false, false, false, false, false)
        };

        out.push(StoryCatalogFixed8PairProfile {
            field_id,
            present_entry_count,
            fixed8_entry_count,
            duplicate_entry_count,
            wire_types: wire_types.into_iter().collect(),
            all_entries_present_once_fixed8,
            first_words_monotonic,
            second_words_monotonic,
            all_second_ge_first,
            contiguous,
            sum_deltas_equals_text_utf16_units,
            sum_deltas_equals_text_bytes,
            outer_span_equals_text_utf16_units,
            outer_span_equals_text_bytes,
            first_start_is_zero,
        });
    }

    out
}

fn format_ranges(descriptors: &[&Descriptor]) -> Vec<(u64, u64)> {
    descriptors
        .iter()
        .filter_map(|descriptor| {
            let start = u64::from(descriptor.data_offset);
            let end = start.checked_add(u64::from(descriptor.data_length))?;
            Some((start, end))
        })
        .collect()
}

fn profile_bte_carriers(quill: &[u8], carriers: &[&Descriptor]) -> Result<Vec<BteCarrierProfile>> {
    let mut out = Vec::with_capacity(carriers.len());

    for (carrier_index, descriptor) in carriers.iter().enumerate() {
        let payload = descriptor_range(quill, descriptor)?;
        let max_prefix = payload.len().saturating_sub(20).min(64);

        let mut plausible_count_prefix_count = 0usize;
        let mut data_size_4_prefix_count = 0usize;
        let mut count_and_data_size_4_prefix_count = 0usize;
        let mut exact_consumption_prefix_count = 0usize;
        let mut canonical_shape_prefix_count = 0usize;

        for prefix in 0..=max_prefix {
            let raw_count = u32_at(payload, prefix);
            let raw_data_size = u32_at(payload, prefix + 4);
            let count = raw_count
                .and_then(|value| usize::try_from(value).ok())
                .filter(|value| *value > 0 && *value <= 4096);

            if count.is_some() {
                plausible_count_prefix_count += 1;
            }
            if raw_data_size == Some(4) {
                data_size_4_prefix_count += 1;
            }
            if count.is_some() && raw_data_size == Some(4) {
                count_and_data_size_4_prefix_count += 1;
            }

            let exact_consumption = count.is_some_and(|count| {
                prefix
                    .checked_add(12)
                    .and_then(|value| value.checked_add((count + 1).saturating_mul(4)))
                    .and_then(|value| value.checked_add(count.saturating_mul(4)))
                    == Some(payload.len())
            });
            if exact_consumption {
                exact_consumption_prefix_count += 1;
            }
            if exact_consumption && raw_data_size == Some(4) {
                canonical_shape_prefix_count += 1;
            }
        }

        let prefix0_count = u32_at(payload, 0);
        let prefix0_data_size = u32_at(payload, 4);
        let prefix0_implied_count = payload
            .len()
            .checked_sub(16)
            .filter(|remaining| remaining % 8 == 0)
            .and_then(|remaining| u32::try_from(remaining / 8).ok());

        out.push(BteCarrierProfile {
            carrier_index,
            descriptor_length: descriptor.data_length,
            plausible_count_prefix_count,
            data_size_4_prefix_count,
            count_and_data_size_4_prefix_count,
            exact_consumption_prefix_count,
            canonical_shape_prefix_count,
            prefix0_count,
            prefix0_data_size,
            prefix0_implied_count,
            prefix0_implied_count_matches_header: prefix0_implied_count == prefix0_count,
        });
    }

    Ok(out)
}

fn scan_bte_plc_candidates(
    quill: &[u8],
    carriers: &[&Descriptor],
    paired_formats: &[&Descriptor],
    text_descriptor: &Descriptor,
    grounded_story_count: u32,
) -> Result<Vec<BtePlcCandidate>> {
    let text_start = u64::from(text_descriptor.data_offset);
    let text_end = text_start
        .checked_add(u64::from(text_descriptor.data_length))
        .context("TEXT range overflow")?;
    let paired_ranges = format_ranges(paired_formats);

    let mut out = Vec::new();
    for (carrier_index, descriptor) in carriers.iter().enumerate() {
        let payload = descriptor_range(quill, descriptor)?;
        if payload.len() < 20 {
            continue;
        }

        let max_prefix = payload.len().saturating_sub(20).min(64);
        for prefix in 0..=max_prefix {
            let Some(raw_count) = u32_at(payload, prefix) else {
                continue;
            };
            let Ok(count) = usize::try_from(raw_count) else {
                continue;
            };
            if count == 0 || count > 4096 {
                continue;
            }

            let Some(raw_data_size) = u32_at(payload, prefix + 4) else {
                continue;
            };
            if raw_data_size != 4 {
                continue;
            }
            let Some(flags) = payload.get(prefix + 8..prefix + 12) else {
                continue;
            };

            let Some(position_bytes) = count
                .checked_add(1)
                .and_then(|value| value.checked_mul(4))
            else {
                continue;
            };
            let Some(target_bytes) = count.checked_mul(4) else {
                continue;
            };
            let Some(expected_end) = prefix
                .checked_add(12)
                .and_then(|value| value.checked_add(position_bytes))
                .and_then(|value| value.checked_add(target_bytes))
            else {
                continue;
            };
            if expected_end != payload.len() {
                continue;
            }

            let positions_start = prefix + 12;
            let targets_start = positions_start + position_bytes;

            let mut positions = Vec::with_capacity(count + 1);
            for index in 0..=count {
                let Some(raw) = u32_at(payload, positions_start + index * 4) else {
                    positions.clear();
                    break;
                };
                positions.push(if raw == 0 {
                    text_start
                } else {
                    u64::from(raw)
                });
            }
            if positions.len() != count + 1 {
                continue;
            }

            let positions_monotonic = positions.windows(2).all(|pair| pair[0] <= pair[1]);
            let positions_inside_text_count = positions
                .iter()
                .filter(|value| **value >= text_start && **value <= text_end)
                .count();

            let mut targets = Vec::with_capacity(count);
            for index in 0..count {
                let Some(raw) = u32_at(payload, targets_start + index * 4) else {
                    targets.clear();
                    break;
                };
                targets.push(u64::from(raw));
            }
            if targets.len() != count {
                continue;
            }

            let targets_inside_paired_format_ranges_count = targets
                .iter()
                .filter(|target| {
                    paired_ranges
                        .iter()
                        .any(|(start, end)| **target >= *start && **target < *end)
                })
                .count();

            out.push(BtePlcCandidate {
                carrier_index,
                prefix_offset: prefix,
                count: raw_count,
                data_size: raw_data_size,
                nonzero_flag_count: flags.iter().filter(|byte| **byte != 0).count(),
                position_count: positions.len(),
                positions_monotonic,
                positions_inside_text_count,
                text_start_present: positions.contains(&text_start),
                text_end_present: positions.contains(&text_end),
                count_covers_story_count: raw_count >= grounded_story_count,
                target_count: targets.len(),
                targets_inside_paired_format_ranges_count,
            });
        }
    }

    Ok(out)
}

fn descriptor_lengths(descriptors: &[&Descriptor]) -> Vec<u32> {
    descriptors.iter().map(|descriptor| descriptor.data_length).collect()
}

fn descriptor_topology(descriptors: &[Descriptor]) -> BTreeMap<String, Vec<u32>> {
    let mut out = BTreeMap::<String, Vec<u32>>::new();
    for descriptor in descriptors {
        let name = descriptor
            .name
            .iter()
            .map(|byte| {
                if byte.is_ascii_graphic() || *byte == b' ' {
                    char::from(*byte)
                } else {
                    '.'
                }
            })
            .collect::<String>();
        out.entry(name).or_default().push(descriptor.data_length);
    }
    for lengths in out.values_mut() {
        lengths.sort_unstable();
    }
    out
}

fn diagnose(bytes: &[u8]) -> Result<WitnessRow> {
    let contents = pub_cfb::read_stream_reader(Cursor::new(bytes), CONTENTS_STREAM)
        .context("read Contents stream")?;
    let quill = pub_cfb::read_stream_reader(Cursor::new(bytes), QUILL_STREAM)
        .context("read Quill stream")?;

    let (revision, story_catalog) = grounded_contents_story_catalog(&contents)?;
    let grounded_story_count = story_catalog.declared_count;

    let descriptors = parse_descriptor_directory(&quill)?;
    let syid = unique_descriptor(&descriptors, *b"SYID")?;
    let strs = unique_descriptor(&descriptors, *b"STRS")?;
    let text = unique_descriptor(&descriptors, *b"TEXT")?;
    let btep = descriptors_named(&descriptors, *b"BTEP");
    let btec = descriptors_named(&descriptors, *b"BTEC");
    let fdpp = descriptors_named(&descriptors, *b"FDPP");
    let fdpc = descriptors_named(&descriptors, *b"FDPC");

    let syid_payload = descriptor_range(&quill, syid)?;
    let strs_payload = descriptor_range(&quill, strs)?;
    descriptor_range(&quill, text)?;
    let syid_descriptor_metadata = descriptor_metadata_profile(syid, grounded_story_count);
    let strs_descriptor_metadata = descriptor_metadata_profile(strs, grounded_story_count);
    let text_descriptor_metadata = descriptor_metadata_profile(text, grounded_story_count);
    let descriptor_opt_b_one_count = descriptors
        .iter()
        .filter(|descriptor| descriptor.opt_b == 1)
        .count();
    let descriptor_opt_c_zero_count = descriptors
        .iter()
        .filter(|descriptor| descriptor.opt_c == 0)
        .count();
    let descriptor_opt_b_c_ordinary_count = descriptors
        .iter()
        .filter(|descriptor| descriptor.opt_b == 1 && descriptor.opt_c == 0)
        .count();

    let expected_syid_len = 8u64 + 4u64 * u64::from(grounded_story_count);
    let expected_strs_len = 22u64 + 8u64 * u64::from(grounded_story_count);
    let text_bytes = u64::from(text.data_length);
    let text_utf16_units = text_bytes / 2;
    let story_catalog_scalar_profiles =
        story_catalog_scalar_profiles(&story_catalog, text_utf16_units, text_bytes);
    let story_catalog_scalar_pair_profiles =
        story_catalog_scalar_pair_profiles(&story_catalog, text_utf16_units, text_bytes);
    let story_catalog_fixed8_pair_profiles =
        story_catalog_fixed8_pair_profiles(&story_catalog, text_utf16_units, text_bytes);
    let story_catalog_entries_with_unsupported_tail = story_catalog
        .entries
        .iter()
        .filter(|entry| entry.unsupported_tail.is_some())
        .count();

    let btep_profiles = profile_bte_carriers(&quill, &btep)?;
    let btec_profiles = profile_bte_carriers(&quill, &btec)?;
    let btep_candidates =
        scan_bte_plc_candidates(&quill, &btep, &fdpp, text, grounded_story_count)?;
    let btec_candidates =
        scan_bte_plc_candidates(&quill, &btec, &fdpc, text, grounded_story_count)?;

    Ok(WitnessRow {
        source_sha256: sha256_hex(bytes),
        byte_len: bytes.len(),
        contents_serialization_revision: revision,
        grounded_story_count,
        descriptor_count: descriptors.len(),
        descriptor_opt_b_one_count,
        descriptor_opt_c_zero_count,
        descriptor_opt_b_c_ordinary_count,
        syid_descriptor_metadata,
        strs_descriptor_metadata,
        text_descriptor_metadata,
        syid_strs_text_opt_a_all_equal:
            syid.opt_a == strs.opt_a && strs.opt_a == text.opt_a,
        syid_strs_text_bit_type_all_equal:
            syid.bit_type == strs.bit_type && strs.bit_type == text.bit_type,
        story_catalog_entries_with_unsupported_tail,
        story_catalog_scalar_profiles,
        story_catalog_scalar_pair_profiles,
        story_catalog_fixed8_pair_profiles,

        syid_descriptor_length: syid.data_length,
        syid_length_matches_grounded_count: u64::from(syid.data_length) == expected_syid_len,
        syid_chunk_all_ff: syid_payload.iter().all(|byte| *byte == 0xff),

        strs_descriptor_length: strs.data_length,
        strs_length_matches_observed_22_plus_8n: u64::from(strs.data_length) == expected_strs_len,
        strs_chunk_all_ff: strs_payload.iter().all(|byte| *byte == 0xff),
        strs_direct_generic_plc_candidate_count:
            scan_direct_generic_plc_count(strs_payload, grounded_story_count),

        text_descriptor_length: text.data_length,
        text_utf16_units,

        btep_descriptor_lengths: descriptor_lengths(&btep),
        btec_descriptor_lengths: descriptor_lengths(&btec),
        fdpp_descriptor_lengths: descriptor_lengths(&fdpp),
        fdpc_descriptor_lengths: descriptor_lengths(&fdpc),
        descriptor_topology: descriptor_topology(&descriptors),
        btep_profiles,
        btec_profiles,
        btep_candidates,
        btec_candidates,
    })
}

fn pub_paths(root: &Path) -> Result<Vec<PathBuf>> {
    let mut out = fs::read_dir(root)
        .with_context(|| format!("read witness dir {}", root.display()))?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            path.extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("pub"))
        })
        .collect::<Vec<_>>();
    out.sort();
    Ok(out)
}

fn main() -> Result<()> {
    let mut args = env::args_os().skip(1);
    let root = PathBuf::from(
        args.next()
            .context("usage: quill-story-early-text-boundary-probe WITNESS_DIR OUTPUT.json")?,
    );
    let output = PathBuf::from(
        args.next()
            .context("usage: quill-story-early-text-boundary-probe WITNESS_DIR OUTPUT.json")?,
    );
    if args.next().is_some() {
        bail!("expected exactly WITNESS_DIR OUTPUT.json");
    }

    let paths = pub_paths(&root)?;
    if paths.len() != 7 {
        bail!("expected exactly 7 witness PUBs, found {}", paths.len());
    }

    let mut rows = Vec::with_capacity(paths.len());
    for path in paths {
        let bytes = fs::read(&path).with_context(|| format!("read {}", path.display()))?;
        rows.push(diagnose(&bytes)?);
    }
    rows.sort_by(|left, right| left.source_sha256.cmp(&right.source_sha256));

    let report = serde_json::json!({
        "schema": "chaptera.quill-story-early-text-boundary.v5",
        "witness_count": rows.len(),
        "rows": rows,
        "evidence_boundary": "exact witness SHA plus source-safe structural counts, lengths and booleans only; no filenames, paths, document text, Story IDs, raw payload bytes, absolute offsets or parser error text",
    });

    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&output, serde_json::to_vec_pretty(&report)?)?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}
