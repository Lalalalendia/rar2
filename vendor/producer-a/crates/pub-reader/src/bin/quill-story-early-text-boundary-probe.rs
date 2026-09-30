use anyhow::{bail, Context, Result};
use pub_contents::{
    decode_packed_field_tag, parse_0x2c_header, parse_confirmed_0x2c_chunk,
    parse_confirmed_0x2c_trailer_root, parse_confirmed_block, parse_confirmed_chunk_reference,
    parse_confirmed_mature_story_catalog, BlockReadError, Contents0x2cChunk, ContentsCursor,
    MatureStoryCatalog, RawContentsBlock, RawContentsBlockBody, CONTENTS_RAW_TYPE_STORY_CATALOG,
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
const RAW_TYPE_SHAPE: u16 = 0x01;
const CONTENTS_RAW_TYPE_STORY_FRAME_INDEX: u16 = 0x61;
const STORY_FRAME_INDEX_DECLARED_COUNT_ID: u16 = 0x01;
const STORY_FRAME_INDEX_ENTRY_ARRAY_ID: u16 = 0x02;
const STORY_FRAME_ENTRY_TEXT_ID: u16 = 0x01;
const STORY_FRAME_ENTRY_ORDINAL_ID: u16 = 0x02;
const STORY_FRAME_ENTRY_SHAPE_REF_ID: u16 = 0x03;
const STORY_FRAME_WIRE_U16_SERVICE: u8 = 0x10;
const STORY_FRAME_WIRE_U32_SERVICE: u8 = 0x58;

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
struct DescriptorPayloadProfile {
    name: String,
    data_length: u32,
    opt_a: u16,
    payload_all_ff: bool,
    payload_all_zero: bool,
    non_ff_byte_count: usize,
    non_zero_byte_count: usize,
    first_u32_is_ff: bool,
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
struct McldScalarCandidate {
    width_bits: u8,
    relative_offset: usize,
    sum_equals_text_utf16_units: bool,
    sum_equals_text_bytes: bool,
    monotonic_non_decreasing: bool,
    last_equals_text_utf16_units: bool,
    last_equals_text_bytes: bool,
}

#[derive(Debug, Serialize)]
struct McldProfile {
    descriptor_length: u32,
    chunk_all_ff: bool,
    prefix_8_all_ff: bool,
    first_u32_is_ff: bool,
    second_u32_is_ff: bool,
    modern_framing_admitted: bool,
    record_count_matches_grounded_story_count: Option<bool>,
    record_id_count_matches_grounded_story_count: Option<bool>,
    layout_key_count: usize,
    all_story_entries_have_layout_key: bool,
    layout_key_set_matches_record_ids: Option<bool>,
    record_body_lengths: Vec<usize>,
    story_order_record_lengths: Vec<usize>,
    scalar_candidates: Vec<McldScalarCandidate>,
    fixed_tail_record_width: Option<usize>,
    fixed_tail_record_count_matches_grounded_story_count: bool,
    fixed_tail_all_ff_record_count: usize,
    fixed_tail_unique_record_hash_count: usize,
}

#[derive(Debug, Serialize)]
struct FdppBoundaryStructure {
    ordinal: usize,
    style_len: usize,
    style_structure_sha256: String,
    selector_0x19_count: usize,
    unknown_wire_type_count: usize,
}

#[derive(Debug, Serialize)]
struct FdppProfile {
    descriptor_length: u32,
    stored_count: Option<u16>,
    stored_count_matches_grounded_story_count: bool,
    tables_fit: bool,
    boundary_count: usize,
    distinct_boundary_count: usize,
    boundaries_monotonic: bool,
    boundaries_inside_text_count: usize,
    terminal_boundary_closes_text: bool,
    first_boundary_after_text_start: bool,
    all_boundaries_utf16_aligned: bool,
    distinct_style_offset_count: usize,
    distinct_style_length_count: usize,
    distinct_structure_hash_count: usize,
    selector_0x19_boundary_count: usize,
    unknown_wire_boundary_count: usize,
    boundary_structures: Vec<FdppBoundaryStructure>,
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

#[derive(Debug, Clone)]
struct StoryFrameEntryProbe {
    fields: Vec<RawContentsBlock>,
    text_id: Option<u32>,
    shape_ref: Option<u32>,
    unsupported_tail: bool,
}

#[derive(Debug, Clone)]
struct StoryShapeEntryProbe {
    fields: Vec<RawContentsBlock>,
}

#[derive(Debug, Serialize)]
struct StoryShapeScalarFieldProfile {
    field_id: u16,
    present_story_count: usize,
    scalar_story_count: usize,
    duplicate_story_count: usize,
    wire_types: Vec<u8>,
    all_stories_present_once_scalar: bool,
    distinct_value_count: usize,
    monotonic_non_decreasing_in_story_order: bool,
    all_values_match_fdpp_absolute_quill_offsets: bool,
    all_values_match_fdpp_relative_bytes: bool,
    all_values_match_fdpp_utf16_units: bool,
    last_equals_text_end_absolute_quill_offset: bool,
    last_equals_text_bytes: bool,
    last_equals_text_utf16_units: bool,
}

#[derive(Debug, Serialize)]
struct StoryShapeScalarPairProfile {
    start_field_id: u16,
    end_field_id: u16,
    all_end_ge_start: bool,
    contiguous: bool,
    sum_deltas_equals_text_utf16_units: bool,
    sum_deltas_equals_text_bytes: bool,
    outer_span_equals_text_utf16_units: bool,
    outer_span_equals_text_bytes: bool,
    first_start_is_zero: bool,
    all_starts_match_fdpp_absolute_quill_offsets: bool,
    all_ends_match_fdpp_absolute_quill_offsets: bool,
    all_starts_match_fdpp_relative_bytes: bool,
    all_ends_match_fdpp_relative_bytes: bool,
    all_starts_match_fdpp_utf16_units: bool,
    all_ends_match_fdpp_utf16_units: bool,
}

#[derive(Debug, Serialize)]
struct StoryShapeFixed8FieldProfile {
    field_id: u16,
    present_story_count: usize,
    fixed8_story_count: usize,
    duplicate_story_count: usize,
    wire_types: Vec<u8>,
    all_stories_present_once_fixed8: bool,
    first_words_monotonic: bool,
    second_words_monotonic: bool,
    all_second_ge_first: bool,
    contiguous: bool,
    sum_deltas_equals_text_utf16_units: bool,
    sum_deltas_equals_text_bytes: bool,
    outer_span_equals_text_utf16_units: bool,
    outer_span_equals_text_bytes: bool,
    first_start_is_zero: bool,
    all_first_words_match_fdpp_absolute_quill_offsets: bool,
    all_second_words_match_fdpp_absolute_quill_offsets: bool,
    all_first_words_match_fdpp_relative_bytes: bool,
    all_second_words_match_fdpp_relative_bytes: bool,
    all_first_words_match_fdpp_utf16_units: bool,
    all_second_words_match_fdpp_utf16_units: bool,
}

#[derive(Debug, Serialize)]
struct StoryShapeProfile {
    story_count: usize,
    frame_entry_count: usize,
    entries_with_shape_ref: usize,
    distinct_shape_ref_count: usize,
    resolved_shape_ref_count: usize,
    shape_raw_type_match_count: usize,
    field_27_identity_match_count: usize,
    shape_chunks_with_unsupported_tail: usize,
    stories_profiled: usize,
    distinct_shape_field_ids: Vec<u16>,
    scalar_field_profiles: Vec<StoryShapeScalarFieldProfile>,
    scalar_pair_profiles: Vec<StoryShapeScalarPairProfile>,
    fixed8_field_profiles: Vec<StoryShapeFixed8FieldProfile>,
}

#[derive(Debug, Serialize)]
struct StoryFrameScalarFieldProfile {
    field_id: u16,
    present_entry_count: usize,
    scalar_entry_count: usize,
    duplicate_entry_count: usize,
    wire_types: Vec<u8>,
    grounded_story_constant_value_count: usize,
    all_grounded_stories_have_constant_scalar: bool,
    monotonic_non_decreasing_in_story_order: bool,
    all_values_match_fdpp_absolute_quill_offsets: bool,
    all_values_match_fdpp_relative_bytes: bool,
    all_values_match_fdpp_utf16_units: bool,
    last_equals_text_end_absolute_quill_offset: bool,
    last_equals_text_bytes: bool,
    last_equals_text_utf16_units: bool,
}

#[derive(Debug, Serialize)]
struct StoryFrameIndexProfile {
    declared_count: Option<u32>,
    entry_count: usize,
    declared_count_matches_entry_count: bool,
    top_level_unsupported_tail: bool,
    entries_with_unsupported_tail: usize,
    entries_with_text_id: usize,
    all_entry_text_ids_grounded: bool,
    grounded_stories_with_frames: usize,
    grounded_stories_without_frames: usize,
    distinct_entry_field_ids: Vec<u16>,
    extra_entry_field_ids: Vec<u16>,
    scalar_field_profiles: Vec<StoryFrameScalarFieldProfile>,
    shape_profile: StoryShapeProfile,
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
    descriptor_payload_profiles: Vec<DescriptorPayloadProfile>,
    syid_descriptor_metadata: DescriptorMetadataProfile,
    strs_descriptor_metadata: DescriptorMetadataProfile,
    text_descriptor_metadata: DescriptorMetadataProfile,
    mcld_profile: McldProfile,
    fdpp_profile: FdppProfile,
    story_frame_index_profile: StoryFrameIndexProfile,
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

#[derive(Debug, Serialize)]
struct SiblingScanDetails {
    contents_serialization_revision: u16,
    grounded_story_count: u32,
    descriptor_count: usize,
    syid_descriptor_length: u32,
    syid_declared_count: Option<u32>,
    syid_chunk_all_ff: bool,
    syid_length_matches_grounded_count: bool,
    strs_descriptor_length: u32,
    strs_declared_count: Option<u32>,
    strs_chunk_all_ff: bool,
    strs_length_matches_grounded_count: bool,
    text_descriptor_length: u32,
    text_utf16_units: u64,
    fdpp_descriptor_lengths: Vec<u32>,
    fdpp_first_stored_count: Option<u16>,
    descriptor_topology: BTreeMap<String, Vec<u32>>,
    descriptor_payload_profiles: Vec<DescriptorPayloadProfile>,
    ordinary_story_catalog_admitted: bool,
    ordinary_story_end_count: usize,
    ordinary_story_ends_all_in_fdpp: bool,
    ordinary_story_end_set_equals_fdpp: bool,
}

#[derive(Debug, Serialize)]
struct SiblingScanRow {
    source_sha256: String,
    byte_len: usize,
    admitted: bool,
    details: Option<SiblingScanDetails>,
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
    name.iter()
        .map(|byte| {
            if byte.is_ascii_graphic() || *byte == b' ' {
                char::from(*byte)
            } else {
                '.'
            }
        })
        .collect()
}

fn descriptor_payload_profiles(
    quill: &[u8],
    descriptors: &[Descriptor],
) -> Result<Vec<DescriptorPayloadProfile>> {
    let mut out = Vec::with_capacity(descriptors.len());
    for descriptor in descriptors {
        let payload = descriptor_range(quill, descriptor)?;
        out.push(DescriptorPayloadProfile {
            name: descriptor_name(&descriptor.name),
            data_length: descriptor.data_length,
            opt_a: descriptor.opt_a,
            payload_all_ff: payload.iter().all(|byte| *byte == 0xff),
            payload_all_zero: payload.iter().all(|byte| *byte == 0),
            non_ff_byte_count: payload.iter().filter(|byte| **byte != 0xff).count(),
            non_zero_byte_count: payload.iter().filter(|byte| **byte != 0).count(),
            first_u32_is_ff: u32_at(payload, 0) == Some(u32::MAX),
        });
    }
    Ok(out)
}

fn descriptor_metadata_profile(
    descriptor: &Descriptor,
    grounded_story_count: u32,
) -> DescriptorMetadataProfile {
    DescriptorMetadataProfile {
        opt_a_is_zero: descriptor.opt_a == 0,
        opt_a_equals_grounded_story_count: u32::from(descriptor.opt_a) == grounded_story_count,
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
    let end = start
        .checked_add(len)
        .context("descriptor range overflow")?;
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
        let count =
            usize::from(u16_at(bytes, start + 2).context("descriptor node header is truncated")?);
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

fn unique_descriptor(descriptors: &[Descriptor], name: [u8; 4]) -> Result<&Descriptor> {
    let mut found = descriptors.iter().filter(|item| item.name == name);
    let first = found
        .next()
        .with_context(|| format!("missing Quill descriptor {:?}", name))?;
    if found.next().is_some() {
        bail!("duplicate required Quill descriptor {:?}", name);
    }
    Ok(first)
}

fn descriptors_named(descriptors: &[Descriptor], name: [u8; 4]) -> Vec<&Descriptor> {
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

fn scalar_u64_block(field: &RawContentsBlock) -> Option<u64> {
    match &field.body {
        RawContentsBlockBody::U16 { value, .. } => Some(u64::from(*value)),
        RawContentsBlockBody::U32 { value, .. } => Some(u64::from(*value)),
        _ => None,
    }
}

fn parse_story_frame_block(cursor: &mut ContentsCursor<'_>) -> Result<RawContentsBlock> {
    let original = cursor.clone();
    match parse_confirmed_block(cursor) {
        Ok(block) => return Ok(block),
        Err(BlockReadError::UnsupportedType { .. }) => {
            *cursor = original;
        }
        Err(error) => return Err(error.into()),
    }

    let mut probe = cursor.clone();
    let start = probe.position();
    let (tag0, tag0_source) = probe.read_u8()?;
    let (tag1, _) = probe.read_u8()?;
    let raw_tag = [tag0, tag1];
    let (id, block_type) = decode_packed_field_tag(raw_tag);
    let tag_source = pub_core::RawSpan {
        stream: tag0_source.stream.clone(),
        offset: tag0_source.offset,
        len: 2,
    };

    let body = match block_type {
        STORY_FRAME_WIRE_U16_SERVICE => {
            let (value, value_source) = probe.read_u16_le()?;
            RawContentsBlockBody::U16 {
                value,
                value_source,
            }
        }
        STORY_FRAME_WIRE_U32_SERVICE => {
            let (value, value_source) = probe.read_u32_le()?;
            RawContentsBlockBody::U32 {
                value,
                value_source,
            }
        }
        _ => bail!(
            "unsupported StoryFrame entry wire 0x{block_type:02x} at relative parse offset {start}"
        ),
    };

    let end = probe.position();
    let block = RawContentsBlock {
        id,
        block_type,
        raw_tag,
        tag_source,
        source: pub_core::RawSpan {
            stream: tag0_source.stream,
            offset: tag0_source.offset,
            len: u64::try_from(end.saturating_sub(start))
                .context("StoryFrame block length does not fit u64")?,
        },
        body,
    };
    *cursor = probe;
    Ok(block)
}

fn blocks_in_span_story_frame(
    contents: &[u8],
    source: &pub_core::RawSpan,
    stop_on_unsupported: bool,
) -> Result<(Vec<RawContentsBlock>, bool)> {
    let start = usize::try_from(source.offset).context("StoryFrame span offset too large")?;
    let len = usize::try_from(source.len).context("StoryFrame span length too large")?;
    let mut cursor = ContentsCursor::bounded(source.stream.clone(), contents, start, len)?;
    let mut fields = Vec::new();

    while cursor.remaining() > 0 {
        let checkpoint = cursor.clone();
        match parse_story_frame_block(&mut cursor) {
            Ok(field) => fields.push(field),
            Err(error) if stop_on_unsupported => {
                cursor = checkpoint;
                let _ = error;
                return Ok((fields, cursor.remaining() > 0));
            }
            Err(error) => return Err(error),
        }
    }
    Ok((fields, false))
}

fn strict_contents_chunk_by_raw_type(
    contents: &[u8],
    raw_type: u16,
    label: &str,
) -> Result<Contents0x2cChunk> {
    let stream = StreamPath(CONTENTS_STREAM.into());
    let header =
        parse_0x2c_header(stream.clone(), contents).context("parse mature Contents header")?;
    let trailer = parse_confirmed_0x2c_trailer_root(contents, &header)
        .context("parse mature Contents trailer")?;

    let mut found = Vec::new();
    for seq_num in 0..trailer.directory.slots.len() {
        let Some(reference) =
            parse_confirmed_chunk_reference(contents, &trailer.directory, seq_num)
                .with_context(|| format!("parse chunk reference {seq_num}"))?
        else {
            continue;
        };
        if reference.raw_types.len() == 1
            && reference.raw_types[0].value == raw_type
            && reference.chunk_offsets.len() == 1
        {
            found.push(reference);
        }
    }
    if found.len() != 1 {
        bail!(
            "expected exactly one strict {label} reference, found {}",
            found.len()
        );
    }

    parse_confirmed_0x2c_chunk(stream, contents, found[0].chunk_offsets[0].value)
        .with_context(|| format!("parse {label} chunk"))
}

fn fdpp_boundary_sets(
    quill: &[u8],
    descriptor: &Descriptor,
    text_descriptor: &Descriptor,
) -> Result<(BTreeSet<u64>, BTreeSet<u64>, BTreeSet<u64>)> {
    let payload = descriptor_range(quill, descriptor)?;
    let count = usize::from(u16_at(payload, 0).context("FDPP stored count missing")?);
    let offsets_start = 8usize;
    let table_end = offsets_start
        .checked_add(count.saturating_mul(4))
        .context("FDPP boundary table overflow")?;
    if table_end > payload.len() {
        bail!("FDPP boundary table outside payload");
    }

    let text_start = u64::from(text_descriptor.data_offset);
    let mut absolute = BTreeSet::new();
    let mut relative_bytes = BTreeSet::new();
    let mut utf16_units = BTreeSet::new();

    for index in 0..count {
        let boundary = u64::from(
            u32_at(payload, offsets_start + index * 4).context("FDPP boundary word truncated")?,
        );
        absolute.insert(boundary);
        if let Some(delta) = boundary.checked_sub(text_start) {
            relative_bytes.insert(delta);
            if delta % 2 == 0 {
                utf16_units.insert(delta / 2);
            }
        }
    }

    Ok((absolute, relative_bytes, utf16_units))
}

fn profile_story_shapes(
    contents: &[u8],
    story_catalog: &MatureStoryCatalog,
    frame_entries: &[StoryFrameEntryProbe],
    quill: &[u8],
    fdpp: &Descriptor,
    text: &Descriptor,
) -> Result<StoryShapeProfile> {
    let stream = StreamPath(CONTENTS_STREAM.into());
    let header = parse_0x2c_header(stream.clone(), contents)
        .context("parse mature Contents for Story SHAPE probe")?;
    let trailer = parse_confirmed_0x2c_trailer_root(contents, &header)
        .context("parse mature Contents trailer for Story SHAPE probe")?;

    let mut entries_with_shape_ref = 0usize;
    let mut shape_refs = BTreeSet::new();
    let mut resolved_shape_ref_count = 0usize;
    let mut shape_raw_type_match_count = 0usize;
    let mut field_27_identity_match_count = 0usize;
    let mut shape_chunks_with_unsupported_tail = 0usize;
    let mut shape_entries = Vec::<StoryShapeEntryProbe>::new();

    for story in &story_catalog.entries {
        let frames = frame_entries
            .iter()
            .filter(|entry| entry.text_id == Some(story.text_id))
            .collect::<Vec<_>>();
        if frames.len() != 1 {
            continue;
        }
        let Some(shape_ref) = frames[0].shape_ref else {
            continue;
        };
        entries_with_shape_ref += 1;
        shape_refs.insert(shape_ref);

        let Ok(seq_num) = usize::try_from(shape_ref) else {
            continue;
        };
        let Some(reference) =
            parse_confirmed_chunk_reference(contents, &trailer.directory, seq_num)
                .with_context(|| format!("parse Story SHAPE reference seq {seq_num}"))?
        else {
            continue;
        };
        resolved_shape_ref_count += 1;
        if reference.raw_types.len() != 1
            || reference.raw_types[0].value != RAW_TYPE_SHAPE
            || reference.chunk_offsets.len() != 1
        {
            continue;
        }
        shape_raw_type_match_count += 1;

        let chunk =
            parse_confirmed_0x2c_chunk(stream.clone(), contents, reference.chunk_offsets[0].value)
                .with_context(|| format!("parse Story SHAPE chunk seq {seq_num}"))?;

        let story_matches = chunk
            .fields
            .iter()
            .filter(|field| field.id == 0x27)
            .filter_map(scalar_u64_block)
            .collect::<Vec<_>>();
        if story_matches.len() == 1 && story_matches[0] == u64::from(story.text_id) {
            field_27_identity_match_count += 1;
        }
        if chunk.unsupported_tail.is_some() {
            shape_chunks_with_unsupported_tail += 1;
        }
        shape_entries.push(StoryShapeEntryProbe {
            fields: chunk.fields,
        });
    }

    let (fdpp_absolute, fdpp_relative_bytes, fdpp_utf16_units) =
        fdpp_boundary_sets(quill, fdpp, text)?;
    let text_start = u64::from(text.data_offset);
    let text_bytes = u64::from(text.data_length);
    let text_utf16_units = text_bytes / 2;
    let text_end = text_start
        .checked_add(text_bytes)
        .context("TEXT end overflow while profiling Story SHAPEs")?;

    let distinct_shape_field_ids = shape_entries
        .iter()
        .flat_map(|entry| entry.fields.iter().map(|field| field.id))
        .collect::<BTreeSet<_>>();

    let mut complete_scalar_vectors = BTreeMap::<u16, Vec<u64>>::new();

    let mut scalar_field_profiles = Vec::new();
    for field_id in distinct_shape_field_ids.iter().copied() {
        let mut present_story_count = 0usize;
        let mut scalar_story_count = 0usize;
        let mut duplicate_story_count = 0usize;
        let mut wire_types = BTreeSet::new();
        let mut values = Vec::new();

        for entry in &shape_entries {
            let matches = entry
                .fields
                .iter()
                .filter(|field| field.id == field_id)
                .collect::<Vec<_>>();
            if !matches.is_empty() {
                present_story_count += 1;
            }
            if matches.len() > 1 {
                duplicate_story_count += 1;
            }
            for field in &matches {
                wire_types.insert(field.block_type);
            }
            if matches.len() == 1 {
                if let Some(value) = scalar_u64_block(matches[0]) {
                    scalar_story_count += 1;
                    values.push(value);
                }
            }
        }

        let all_stories_present_once_scalar = shape_entries.len() == story_catalog.entries.len()
            && present_story_count == shape_entries.len()
            && scalar_story_count == shape_entries.len()
            && duplicate_story_count == 0;
        if all_stories_present_once_scalar {
            complete_scalar_vectors.insert(field_id, values.clone());
        }
        let last = all_stories_present_once_scalar
            .then(|| values.last().copied())
            .flatten();

        scalar_field_profiles.push(StoryShapeScalarFieldProfile {
            field_id,
            present_story_count,
            scalar_story_count,
            duplicate_story_count,
            wire_types: wire_types.into_iter().collect(),
            all_stories_present_once_scalar,
            distinct_value_count: values.iter().copied().collect::<BTreeSet<_>>().len(),
            monotonic_non_decreasing_in_story_order: all_stories_present_once_scalar
                && values.windows(2).all(|pair| pair[0] <= pair[1]),
            all_values_match_fdpp_absolute_quill_offsets: all_stories_present_once_scalar
                && values.iter().all(|value| fdpp_absolute.contains(value)),
            all_values_match_fdpp_relative_bytes: all_stories_present_once_scalar
                && values
                    .iter()
                    .all(|value| fdpp_relative_bytes.contains(value)),
            all_values_match_fdpp_utf16_units: all_stories_present_once_scalar
                && values.iter().all(|value| fdpp_utf16_units.contains(value)),
            last_equals_text_end_absolute_quill_offset: all_stories_present_once_scalar
                && last == Some(text_end),
            last_equals_text_bytes: all_stories_present_once_scalar && last == Some(text_bytes),
            last_equals_text_utf16_units: all_stories_present_once_scalar
                && last == Some(text_utf16_units),
        });
    }

    let mut scalar_pair_profiles = Vec::new();
    for (start_field_id, starts) in &complete_scalar_vectors {
        for (end_field_id, ends) in &complete_scalar_vectors {
            if start_field_id == end_field_id {
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

            scalar_pair_profiles.push(StoryShapeScalarPairProfile {
                start_field_id: *start_field_id,
                end_field_id: *end_field_id,
                all_end_ge_start,
                contiguous,
                sum_deltas_equals_text_utf16_units,
                sum_deltas_equals_text_bytes,
                outer_span_equals_text_utf16_units,
                outer_span_equals_text_bytes,
                first_start_is_zero,
                all_starts_match_fdpp_absolute_quill_offsets: starts
                    .iter()
                    .all(|value| fdpp_absolute.contains(value)),
                all_ends_match_fdpp_absolute_quill_offsets: ends
                    .iter()
                    .all(|value| fdpp_absolute.contains(value)),
                all_starts_match_fdpp_relative_bytes: starts
                    .iter()
                    .all(|value| fdpp_relative_bytes.contains(value)),
                all_ends_match_fdpp_relative_bytes: ends
                    .iter()
                    .all(|value| fdpp_relative_bytes.contains(value)),
                all_starts_match_fdpp_utf16_units: starts
                    .iter()
                    .all(|value| fdpp_utf16_units.contains(value)),
                all_ends_match_fdpp_utf16_units: ends
                    .iter()
                    .all(|value| fdpp_utf16_units.contains(value)),
            });
        }
    }

    let mut fixed8_field_profiles = Vec::new();
    for field_id in distinct_shape_field_ids.iter().copied() {
        let mut present_story_count = 0usize;
        let mut fixed8_story_count = 0usize;
        let mut duplicate_story_count = 0usize;
        let mut wire_types = BTreeSet::new();
        let mut first_words = Vec::new();
        let mut second_words = Vec::new();

        for entry in &shape_entries {
            let matches = entry
                .fields
                .iter()
                .filter(|field| field.id == field_id)
                .collect::<Vec<_>>();
            if !matches.is_empty() {
                present_story_count += 1;
            }
            if matches.len() > 1 {
                duplicate_story_count += 1;
            }
            for field in &matches {
                wire_types.insert(field.block_type);
            }
            if matches.len() == 1 {
                if let RawContentsBlockBody::Fixed8 { bytes, .. } = &matches[0].body {
                    fixed8_story_count += 1;
                    first_words.push(u64::from(u32::from_le_bytes([
                        bytes[0], bytes[1], bytes[2], bytes[3],
                    ])));
                    second_words.push(u64::from(u32::from_le_bytes([
                        bytes[4], bytes[5], bytes[6], bytes[7],
                    ])));
                }
            }
        }
        if fixed8_story_count == 0 {
            continue;
        }

        let all_stories_present_once_fixed8 = shape_entries.len() == story_catalog.entries.len()
            && present_story_count == shape_entries.len()
            && fixed8_story_count == shape_entries.len()
            && duplicate_story_count == 0;
        let (
            all_second_ge_first,
            contiguous,
            sum_deltas_equals_text_utf16_units,
            sum_deltas_equals_text_bytes,
            outer_span_equals_text_utf16_units,
            outer_span_equals_text_bytes,
            first_start_is_zero,
        ) = if all_stories_present_once_fixed8 {
            profile_offset_pair(&first_words, &second_words, text_utf16_units, text_bytes)
        } else {
            (false, false, false, false, false, false, false)
        };

        fixed8_field_profiles.push(StoryShapeFixed8FieldProfile {
            field_id,
            present_story_count,
            fixed8_story_count,
            duplicate_story_count,
            wire_types: wire_types.into_iter().collect(),
            all_stories_present_once_fixed8,
            first_words_monotonic: all_stories_present_once_fixed8
                && first_words.windows(2).all(|pair| pair[0] <= pair[1]),
            second_words_monotonic: all_stories_present_once_fixed8
                && second_words.windows(2).all(|pair| pair[0] <= pair[1]),
            all_second_ge_first,
            contiguous,
            sum_deltas_equals_text_utf16_units,
            sum_deltas_equals_text_bytes,
            outer_span_equals_text_utf16_units,
            outer_span_equals_text_bytes,
            first_start_is_zero,
            all_first_words_match_fdpp_absolute_quill_offsets: all_stories_present_once_fixed8
                && first_words
                    .iter()
                    .all(|value| fdpp_absolute.contains(value)),
            all_second_words_match_fdpp_absolute_quill_offsets: all_stories_present_once_fixed8
                && second_words
                    .iter()
                    .all(|value| fdpp_absolute.contains(value)),
            all_first_words_match_fdpp_relative_bytes: all_stories_present_once_fixed8
                && first_words
                    .iter()
                    .all(|value| fdpp_relative_bytes.contains(value)),
            all_second_words_match_fdpp_relative_bytes: all_stories_present_once_fixed8
                && second_words
                    .iter()
                    .all(|value| fdpp_relative_bytes.contains(value)),
            all_first_words_match_fdpp_utf16_units: all_stories_present_once_fixed8
                && first_words
                    .iter()
                    .all(|value| fdpp_utf16_units.contains(value)),
            all_second_words_match_fdpp_utf16_units: all_stories_present_once_fixed8
                && second_words
                    .iter()
                    .all(|value| fdpp_utf16_units.contains(value)),
        });
    }

    Ok(StoryShapeProfile {
        story_count: story_catalog.entries.len(),
        frame_entry_count: frame_entries.len(),
        entries_with_shape_ref,
        distinct_shape_ref_count: shape_refs.len(),
        resolved_shape_ref_count,
        shape_raw_type_match_count,
        field_27_identity_match_count,
        shape_chunks_with_unsupported_tail,
        stories_profiled: shape_entries.len(),
        distinct_shape_field_ids: distinct_shape_field_ids.into_iter().collect(),
        scalar_field_profiles,
        scalar_pair_profiles,
        fixed8_field_profiles,
    })
}

fn profile_story_frame_index(
    contents: &[u8],
    chunk: &Contents0x2cChunk,
    story_catalog: &MatureStoryCatalog,
    quill: &[u8],
    fdpp: &Descriptor,
    text: &Descriptor,
) -> Result<StoryFrameIndexProfile> {
    let declared_count = chunk
        .fields
        .iter()
        .filter(|field| field.id == STORY_FRAME_INDEX_DECLARED_COUNT_ID)
        .filter_map(scalar_u64_block)
        .next()
        .and_then(|value| u32::try_from(value).ok());

    let array_fields = chunk
        .fields
        .iter()
        .filter(|field| field.id == STORY_FRAME_INDEX_ENTRY_ARRAY_ID)
        .collect::<Vec<_>>();
    if array_fields.len() != 1 {
        bail!(
            "StoryFrame index requires exactly one entry array, found {}",
            array_fields.len()
        );
    }
    let RawContentsBlockBody::Container {
        content_source: array_source,
        ..
    } = &array_fields[0].body
    else {
        bail!("StoryFrame index entry array is not a container");
    };

    let (entry_blocks, array_unsupported) =
        blocks_in_span_story_frame(contents, array_source, false)?;
    if array_unsupported {
        bail!("StoryFrame index array has unsupported top-level tail");
    }

    let mut entries = Vec::with_capacity(entry_blocks.len());
    for item in entry_blocks {
        if item.id != 0 {
            bail!("StoryFrame index entry id is not zero");
        }
        let RawContentsBlockBody::Container {
            content_source: entry_source,
            ..
        } = item.body
        else {
            bail!("StoryFrame index entry is not a container");
        };

        let (fields, unsupported_tail) = blocks_in_span_story_frame(contents, &entry_source, true)?;
        let text_matches = fields
            .iter()
            .filter(|field| field.id == STORY_FRAME_ENTRY_TEXT_ID)
            .filter_map(scalar_u64_block)
            .collect::<Vec<_>>();
        let text_id = (text_matches.len() == 1)
            .then(|| u32::try_from(text_matches[0]).ok())
            .flatten();
        let shape_matches = fields
            .iter()
            .filter(|field| field.id == STORY_FRAME_ENTRY_SHAPE_REF_ID)
            .filter_map(scalar_u64_block)
            .collect::<Vec<_>>();
        let shape_ref = (shape_matches.len() == 1)
            .then(|| u32::try_from(shape_matches[0]).ok())
            .flatten();

        entries.push(StoryFrameEntryProbe {
            fields,
            text_id,
            shape_ref,
            unsupported_tail,
        });
    }

    let grounded_story_ids = story_catalog
        .entries
        .iter()
        .map(|entry| entry.text_id)
        .collect::<BTreeSet<_>>();
    let entries_with_text_id = entries
        .iter()
        .filter(|entry| entry.text_id.is_some())
        .count();
    let all_entry_text_ids_grounded = entries.iter().all(|entry| {
        entry
            .text_id
            .is_some_and(|text_id| grounded_story_ids.contains(&text_id))
    });

    let grounded_stories_with_frames = story_catalog
        .entries
        .iter()
        .filter(|story| {
            entries
                .iter()
                .any(|entry| entry.text_id == Some(story.text_id))
        })
        .count();
    let grounded_stories_without_frames = story_catalog
        .entries
        .len()
        .saturating_sub(grounded_stories_with_frames);

    let shape_profile = profile_story_shapes(contents, story_catalog, &entries, quill, fdpp, text)?;

    let mut distinct_entry_field_ids = BTreeSet::new();
    for entry in &entries {
        for field in &entry.fields {
            distinct_entry_field_ids.insert(field.id);
        }
    }
    let extra_entry_field_ids = distinct_entry_field_ids
        .iter()
        .copied()
        .filter(|id| {
            !matches!(
                *id,
                STORY_FRAME_ENTRY_TEXT_ID
                    | STORY_FRAME_ENTRY_ORDINAL_ID
                    | STORY_FRAME_ENTRY_SHAPE_REF_ID
            )
        })
        .collect::<Vec<_>>();

    let (fdpp_absolute, fdpp_relative_bytes, fdpp_utf16_units) =
        fdpp_boundary_sets(quill, fdpp, text)?;
    let text_start = u64::from(text.data_offset);
    let text_bytes = u64::from(text.data_length);
    let text_utf16_units = text_bytes / 2;
    let text_end = text_start
        .checked_add(text_bytes)
        .context("TEXT end overflow while profiling StoryFrame index")?;

    let mut scalar_field_profiles = Vec::new();
    for field_id in distinct_entry_field_ids.iter().copied() {
        let mut present_entry_count = 0usize;
        let mut scalar_entry_count = 0usize;
        let mut duplicate_entry_count = 0usize;
        let mut wire_types = BTreeSet::new();

        for entry in &entries {
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
            if matches.len() == 1 && scalar_u64_block(matches[0]).is_some() {
                scalar_entry_count += 1;
            }
        }

        let mut story_values = Vec::new();
        for story in &story_catalog.entries {
            let frames = entries
                .iter()
                .filter(|entry| entry.text_id == Some(story.text_id))
                .collect::<Vec<_>>();
            if frames.is_empty() {
                continue;
            }
            let mut values = Vec::with_capacity(frames.len());
            let mut valid = true;
            for frame in frames {
                let matches = frame
                    .fields
                    .iter()
                    .filter(|field| field.id == field_id)
                    .collect::<Vec<_>>();
                if matches.len() != 1 {
                    valid = false;
                    break;
                }
                let Some(value) = scalar_u64_block(matches[0]) else {
                    valid = false;
                    break;
                };
                values.push(value);
            }
            if valid
                && values
                    .first()
                    .is_some_and(|first| values.iter().all(|value| value == first))
            {
                story_values.push(values[0]);
            }
        }

        let all_grounded_stories_have_constant_scalar =
            story_values.len() == story_catalog.entries.len();
        let monotonic_non_decreasing_in_story_order = all_grounded_stories_have_constant_scalar
            && story_values.windows(2).all(|pair| pair[0] <= pair[1]);
        let all_values_match_fdpp_absolute_quill_offsets = all_grounded_stories_have_constant_scalar
            && story_values
                .iter()
                .all(|value| fdpp_absolute.contains(value));
        let all_values_match_fdpp_relative_bytes = all_grounded_stories_have_constant_scalar
            && story_values
                .iter()
                .all(|value| fdpp_relative_bytes.contains(value));
        let all_values_match_fdpp_utf16_units = all_grounded_stories_have_constant_scalar
            && story_values
                .iter()
                .all(|value| fdpp_utf16_units.contains(value));
        let last = story_values.last().copied();

        scalar_field_profiles.push(StoryFrameScalarFieldProfile {
            field_id,
            present_entry_count,
            scalar_entry_count,
            duplicate_entry_count,
            wire_types: wire_types.into_iter().collect(),
            grounded_story_constant_value_count: story_values.len(),
            all_grounded_stories_have_constant_scalar,
            monotonic_non_decreasing_in_story_order,
            all_values_match_fdpp_absolute_quill_offsets,
            all_values_match_fdpp_relative_bytes,
            all_values_match_fdpp_utf16_units,
            last_equals_text_end_absolute_quill_offset: all_grounded_stories_have_constant_scalar
                && last == Some(text_end),
            last_equals_text_bytes: all_grounded_stories_have_constant_scalar
                && last == Some(text_bytes),
            last_equals_text_utf16_units: all_grounded_stories_have_constant_scalar
                && last == Some(text_utf16_units),
        });
    }

    Ok(StoryFrameIndexProfile {
        declared_count,
        entry_count: entries.len(),
        declared_count_matches_entry_count: declared_count
            .and_then(|value| usize::try_from(value).ok())
            == Some(entries.len()),
        top_level_unsupported_tail: chunk.unsupported_tail.is_some(),
        entries_with_unsupported_tail: entries
            .iter()
            .filter(|entry| entry.unsupported_tail)
            .count(),
        entries_with_text_id,
        all_entry_text_ids_grounded,
        grounded_stories_with_frames,
        grounded_stories_without_frames,
        distinct_entry_field_ids: distinct_entry_field_ids.into_iter().collect(),
        extra_entry_field_ids,
        scalar_field_profiles,
        shape_profile,
    })
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
    let chunk = parse_confirmed_0x2c_chunk(stream, contents, reference.chunk_offsets[0].value)
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

            let all_entries_present_once_scalar = present_entry_count == catalog.entries.len()
                && scalar_entry_count == catalog.entries.len()
                && duplicate_entry_count == 0;
            let sum = if all_entries_present_once_scalar {
                values
                    .iter()
                    .try_fold(0u64, |acc, value| acc.checked_add(*value))
            } else {
                None
            };
            let monotonic_non_decreasing =
                all_entries_present_once_scalar && values.windows(2).all(|pair| pair[0] <= pair[1]);
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

        let all_entries_present_once_fixed8 = present_entry_count == catalog.entries.len()
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

fn checked_range<'a>(bytes: &'a [u8], start: usize, len: usize, label: &str) -> Result<&'a [u8]> {
    let end = start
        .checked_add(len)
        .with_context(|| format!("{label} range overflow"))?;
    bytes
        .get(start..end)
        .with_context(|| format!("{label} range outside payload"))
}

type McldRecordSpans = (u32, u32, Vec<u32>, Vec<(usize, usize)>);

fn parse_mcld_record_spans(payload: &[u8]) -> Result<McldRecordSpans> {
    let record_count = u32_at(payload, 0).context("MCLD record_count truncated")?;
    let record_id_count = u32_at(payload, 4).context("MCLD record_id_count truncated")?;
    if record_count != record_id_count {
        bail!("MCLD record_count/record_id_count mismatch");
    }

    let record_id_count_usize =
        usize::try_from(record_id_count).context("MCLD record_id_count too large")?;
    let ids_bytes = record_id_count_usize
        .checked_mul(4)
        .context("MCLD record id table overflow")?;
    checked_range(payload, 8, ids_bytes, "MCLD record id table")?;

    let mut record_ids = Vec::with_capacity(record_id_count_usize);
    for index in 0..record_id_count_usize {
        record_ids.push(u32_at(payload, 8 + index * 4).context("MCLD record id truncated")?);
    }
    if record_ids.iter().copied().collect::<BTreeSet<_>>().len() != record_ids.len() {
        bail!("MCLD duplicate record id");
    }

    let mut cursor = 8usize
        .checked_add(ids_bytes)
        .context("MCLD record start overflow")?;
    let mut spans = Vec::with_capacity(record_id_count_usize);
    for _ in 0..record_id_count_usize {
        let record_start = cursor;
        let header_size =
            usize::try_from(u32_at(payload, cursor).context("MCLD record header size truncated")?)
                .context("MCLD record header size too large")?;
        if header_size < 4 {
            bail!("MCLD record header size below minimum");
        }
        checked_range(payload, cursor, header_size, "MCLD record header")?;
        cursor = cursor
            .checked_add(header_size)
            .context("MCLD record header end overflow")?;

        let child_count =
            usize::try_from(u32_at(payload, cursor).context("MCLD child_count truncated")?)
                .context("MCLD child_count too large")?;
        cursor = cursor
            .checked_add(4)
            .context("MCLD child_count end overflow")?;
        for _ in 0..child_count {
            let child_size =
                usize::try_from(u32_at(payload, cursor).context("MCLD child size truncated")?)
                    .context("MCLD child size too large")?;
            if child_size < 4 {
                bail!("MCLD child size below minimum");
            }
            checked_range(payload, cursor, child_size, "MCLD child")?;
            cursor = cursor
                .checked_add(child_size)
                .context("MCLD child end overflow")?;
        }
        spans.push((record_start, cursor));
    }

    if cursor != payload.len() {
        bail!("MCLD trailing bytes after bounded record walk");
    }

    Ok((record_count, record_id_count, record_ids, spans))
}

fn mcld_scalar_candidates(
    records: &[&[u8]],
    text_utf16_units: u64,
    text_bytes: u64,
) -> Vec<McldScalarCandidate> {
    let Some(min_len) = records.iter().map(|record| record.len()).min() else {
        return Vec::new();
    };
    let mut out = Vec::new();

    for (width_bits, width) in [(16u8, 2usize), (32u8, 4usize)] {
        if min_len < width {
            continue;
        }
        for relative_offset in 0..=min_len - width {
            let values = records
                .iter()
                .map(|record| {
                    if width == 2 {
                        u16_at(record, relative_offset).map(u64::from)
                    } else {
                        u32_at(record, relative_offset).map(u64::from)
                    }
                })
                .collect::<Option<Vec<_>>>();
            let Some(values) = values else {
                continue;
            };

            let sum = values
                .iter()
                .try_fold(0u64, |acc, value| acc.checked_add(*value));
            let monotonic_non_decreasing = values.windows(2).all(|pair| pair[0] <= pair[1]);
            let last = values.last().copied();
            let sum_equals_text_utf16_units = sum == Some(text_utf16_units);
            let sum_equals_text_bytes = sum == Some(text_bytes);
            let last_equals_text_utf16_units =
                monotonic_non_decreasing && last == Some(text_utf16_units);
            let last_equals_text_bytes = monotonic_non_decreasing && last == Some(text_bytes);

            if sum_equals_text_utf16_units
                || sum_equals_text_bytes
                || last_equals_text_utf16_units
                || last_equals_text_bytes
            {
                out.push(McldScalarCandidate {
                    width_bits,
                    relative_offset,
                    sum_equals_text_utf16_units,
                    sum_equals_text_bytes,
                    monotonic_non_decreasing,
                    last_equals_text_utf16_units,
                    last_equals_text_bytes,
                });
            }
        }
    }

    out
}

fn profile_mcld(
    quill: &[u8],
    descriptor: &Descriptor,
    story_catalog: &MatureStoryCatalog,
    text_utf16_units: u64,
    text_bytes: u64,
) -> Result<McldProfile> {
    let payload = descriptor_range(quill, descriptor)?;
    let layout_keys = story_catalog
        .entries
        .iter()
        .filter_map(|entry| entry.layout_key)
        .collect::<Vec<_>>();
    let all_story_entries_have_layout_key = layout_keys.len() == story_catalog.entries.len();

    let fixed_tail_record_width = usize::try_from(story_catalog.declared_count)
        .ok()
        .filter(|count| *count > 0)
        .and_then(|count| {
            payload
                .len()
                .checked_sub(8)
                .filter(|tail_len| tail_len % count == 0)
                .map(|tail_len| tail_len / count)
        });

    let mut fixed_tail_all_ff_record_count = 0usize;
    let mut fixed_tail_hashes = BTreeSet::new();
    if let (Some(width), Ok(count)) = (
        fixed_tail_record_width,
        usize::try_from(story_catalog.declared_count),
    ) {
        if width > 0 {
            for index in 0..count {
                let start = 8usize
                    .checked_add(index.saturating_mul(width))
                    .context("MCLD fixed-tail record start overflow")?;
                if let Some(record) = payload.get(start..start.saturating_add(width)) {
                    if record.iter().all(|byte| *byte == 0xff) {
                        fixed_tail_all_ff_record_count += 1;
                    }
                    fixed_tail_hashes.insert(sha256_hex(record));
                }
            }
        }
    }

    let mut modern_framing_admitted = false;
    let mut record_count_matches_grounded_story_count = None;
    let mut record_id_count_matches_grounded_story_count = None;
    let mut layout_key_set_matches_record_ids = None;
    let mut record_body_lengths = Vec::new();
    let mut story_order_record_lengths = Vec::new();
    let mut scalar_candidates = Vec::new();

    if let Ok((record_count, record_id_count, record_ids, spans)) = parse_mcld_record_spans(payload)
    {
        modern_framing_admitted = true;
        record_count_matches_grounded_story_count =
            Some(record_count == story_catalog.declared_count);
        record_id_count_matches_grounded_story_count =
            Some(record_id_count == story_catalog.declared_count);

        let layout_key_set = layout_keys.iter().copied().collect::<BTreeSet<_>>();
        let record_id_set = record_ids.iter().copied().collect::<BTreeSet<_>>();
        let set_matches = all_story_entries_have_layout_key && layout_key_set == record_id_set;
        layout_key_set_matches_record_ids = Some(set_matches);

        record_body_lengths = spans
            .iter()
            .map(|(start, end)| end.saturating_sub(*start))
            .collect::<Vec<_>>();

        if set_matches {
            let mut story_order_records = Vec::new();
            for layout_key in &layout_keys {
                let index = record_ids
                    .iter()
                    .position(|record_id| record_id == layout_key)
                    .context("MCLD layout key set matched but record id lookup failed")?;
                let (start, end) = spans[index];
                let record = payload
                    .get(start..end)
                    .context("MCLD story-order record range outside payload")?;
                story_order_record_lengths.push(record.len());
                story_order_records.push(record);
            }
            scalar_candidates =
                mcld_scalar_candidates(&story_order_records, text_utf16_units, text_bytes);
        }
    }

    Ok(McldProfile {
        descriptor_length: descriptor.data_length,
        chunk_all_ff: payload.iter().all(|byte| *byte == 0xff),
        prefix_8_all_ff: payload
            .get(0..8)
            .is_some_and(|prefix| prefix.iter().all(|byte| *byte == 0xff)),
        first_u32_is_ff: u32_at(payload, 0) == Some(u32::MAX),
        second_u32_is_ff: u32_at(payload, 4) == Some(u32::MAX),
        modern_framing_admitted,
        record_count_matches_grounded_story_count,
        record_id_count_matches_grounded_story_count,
        layout_key_count: layout_keys.len(),
        all_story_entries_have_layout_key,
        layout_key_set_matches_record_ids,
        record_body_lengths,
        story_order_record_lengths,
        scalar_candidates,
        fixed_tail_record_width,
        fixed_tail_record_count_matches_grounded_story_count: fixed_tail_record_width.is_some(),
        fixed_tail_all_ff_record_count,
        fixed_tail_unique_record_hash_count: fixed_tail_hashes.len(),
    })
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

fn decode_quill_style_tag_probe(raw_tag: [u8; 2]) -> (u16, u8) {
    let raw_type = raw_tag[1];
    if raw_type & 0x07 == 0x02 {
        (
            u16::from(raw_tag[0]) | (u16::from(raw_type & 0x07) << 8),
            raw_type & 0xf8,
        )
    } else {
        (u16::from(raw_tag[0]), raw_type)
    }
}

fn quill_style_fixed_width(id: u16, block_type: u8) -> Option<usize> {
    match block_type {
        0x00 if matches!(id, 0x0202 | 0x0237) => Some(0),
        0x78 | 0x05 | 0x08 => Some(0),
        0x10 | 0x18 | 0x07 => Some(2),
        0x20 | 0x58 | 0x68 | 0x70 | 0xb8 => Some(4),
        0x28 => Some(8),
        0x38 => Some(16),
        0x48 => Some(24),
        _ => None,
    }
}

fn fdpp_style_structure(
    payload: &[u8],
    style_start: usize,
) -> Result<(usize, String, usize, usize)> {
    let style_len =
        usize::try_from(u32_at(payload, style_start).context("FDPP style length truncated")?)
            .context("FDPP style length too large")?;
    if style_len < 4 {
        bail!("FDPP style length below minimum");
    }
    let style_end = style_start
        .checked_add(style_len)
        .context("FDPP style end overflow")?;
    if style_end > payload.len() {
        bail!("FDPP style outside payload");
    }

    let mut cursor = style_start + 4;
    let mut structure = Vec::new();
    let mut selector_0x19_count = 0usize;
    let mut unknown_wire_types = BTreeSet::new();
    const VARIABLE_BLOCK_TYPES: [u8; 6] = [0xc0, 0x80, 0x88, 0x90, 0x98, 0xa0];

    while cursor < style_end {
        let header = payload
            .get(cursor..cursor + 2)
            .context("FDPP style block header truncated")?;
        let raw_tag = [header[0], header[1]];
        let (id, block_type) = decode_quill_style_tag_probe(raw_tag);
        let data_offset = cursor + 2;
        let block_end = if VARIABLE_BLOCK_TYPES.contains(&block_type) {
            let declared = usize::try_from(
                u32_at(payload, data_offset).context("FDPP variable block length truncated")?,
            )
            .context("FDPP variable block length too large")?;
            if declared < 4 {
                bail!("FDPP variable block length below minimum");
            }
            data_offset
                .checked_add(declared)
                .context("FDPP variable block end overflow")?
        } else if let Some(width) = quill_style_fixed_width(id, block_type) {
            data_offset
                .checked_add(width)
                .context("FDPP fixed block end overflow")?
        } else {
            unknown_wire_types.insert(raw_tag[1]);
            data_offset
        };
        if block_end > style_end {
            bail!("FDPP style block exceeds style record");
        }

        structure.extend_from_slice(&id.to_le_bytes());
        structure.push(block_type);
        structure.extend_from_slice(
            &u32::try_from(block_end - cursor)
                .context("FDPP style block length exceeds u32")?
                .to_le_bytes(),
        );
        if id == 0x0019 {
            selector_0x19_count += 1;
        }

        cursor = block_end;
    }
    if cursor != style_end {
        bail!("FDPP style structure did not close exactly");
    }

    Ok((
        style_len,
        sha256_hex(&structure),
        selector_0x19_count,
        unknown_wire_types.len(),
    ))
}

fn profile_fdpp(
    quill: &[u8],
    descriptor: &Descriptor,
    text_descriptor: &Descriptor,
    grounded_story_count: u32,
) -> Result<FdppProfile> {
    let payload = descriptor_range(quill, descriptor)?;
    let stored_count = u16_at(payload, 0);
    let Some(count_u16) = stored_count else {
        return Ok(FdppProfile {
            descriptor_length: descriptor.data_length,
            stored_count,
            stored_count_matches_grounded_story_count: false,
            tables_fit: false,
            boundary_count: 0,
            distinct_boundary_count: 0,
            boundaries_monotonic: false,
            boundaries_inside_text_count: 0,
            terminal_boundary_closes_text: false,
            first_boundary_after_text_start: false,
            all_boundaries_utf16_aligned: false,
            distinct_style_offset_count: 0,
            distinct_style_length_count: 0,
            distinct_structure_hash_count: 0,
            selector_0x19_boundary_count: 0,
            unknown_wire_boundary_count: 0,
            boundary_structures: Vec::new(),
        });
    };
    let count = usize::from(count_u16);
    let offsets_start = 8usize;
    let Some(chunk_offsets_start) = offsets_start.checked_add(count.saturating_mul(4)) else {
        bail!("FDPP offset table overflow");
    };
    let Some(body_start) = chunk_offsets_start.checked_add(count.saturating_mul(2)) else {
        bail!("FDPP style-offset table overflow");
    };
    let tables_fit = body_start <= payload.len();

    let text_start = u64::from(text_descriptor.data_offset);
    let text_end = text_start
        .checked_add(u64::from(text_descriptor.data_length))
        .context("TEXT range overflow")?;

    let mut boundaries = Vec::new();
    let mut style_offsets = Vec::new();
    let mut boundary_structures = Vec::new();
    if tables_fit {
        for index in 0..count {
            let Some(value) = u32_at(payload, offsets_start + index * 4) else {
                boundaries.clear();
                break;
            };
            boundaries.push(u64::from(value));

            let relative_style_offset = usize::from(
                u16_at(payload, chunk_offsets_start + index * 2)
                    .context("FDPP style offset truncated")?,
            );
            style_offsets.push(relative_style_offset);
            let (style_len, style_structure_sha256, selector_0x19_count, unknown_wire_type_count) =
                fdpp_style_structure(payload, relative_style_offset)?;
            boundary_structures.push(FdppBoundaryStructure {
                ordinal: index,
                style_len,
                style_structure_sha256,
                selector_0x19_count,
                unknown_wire_type_count,
            });
        }
    }

    let boundary_count = boundaries.len();
    let distinct_boundary_count = boundaries.iter().copied().collect::<BTreeSet<_>>().len();
    let boundaries_monotonic =
        boundary_count == count && boundaries.windows(2).all(|pair| pair[0] <= pair[1]);
    let boundaries_inside_text_count = boundaries
        .iter()
        .filter(|value| **value >= text_start && **value <= text_end)
        .count();
    let terminal_boundary_closes_text = boundaries.last().copied() == Some(text_end);
    let first_boundary_after_text_start = boundaries
        .first()
        .is_some_and(|value| *value > text_start && *value <= text_end);
    let all_boundaries_utf16_aligned = boundaries.iter().all(|value| {
        value
            .checked_sub(text_start)
            .is_some_and(|delta| delta % 2 == 0)
    });

    let distinct_style_offset_count = style_offsets.iter().copied().collect::<BTreeSet<_>>().len();
    let distinct_style_length_count = boundary_structures
        .iter()
        .map(|item| item.style_len)
        .collect::<BTreeSet<_>>()
        .len();
    let distinct_structure_hash_count = boundary_structures
        .iter()
        .map(|item| item.style_structure_sha256.clone())
        .collect::<BTreeSet<_>>()
        .len();
    let selector_0x19_boundary_count = boundary_structures
        .iter()
        .filter(|item| item.selector_0x19_count > 0)
        .count();
    let unknown_wire_boundary_count = boundary_structures
        .iter()
        .filter(|item| item.unknown_wire_type_count > 0)
        .count();

    Ok(FdppProfile {
        descriptor_length: descriptor.data_length,
        stored_count,
        stored_count_matches_grounded_story_count: u32::from(count_u16) == grounded_story_count,
        tables_fit,
        boundary_count,
        distinct_boundary_count,
        boundaries_monotonic,
        boundaries_inside_text_count,
        terminal_boundary_closes_text,
        first_boundary_after_text_start,
        all_boundaries_utf16_aligned,
        distinct_style_offset_count,
        distinct_style_length_count,
        distinct_structure_hash_count,
        selector_0x19_boundary_count,
        unknown_wire_boundary_count,
        boundary_structures,
    })
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

            let Some(position_bytes) = count.checked_add(1).and_then(|value| value.checked_mul(4))
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
                positions.push(if raw == 0 { text_start } else { u64::from(raw) });
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
    descriptors
        .iter()
        .map(|descriptor| descriptor.data_length)
        .collect()
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
    let story_frame_index_chunk = strict_contents_chunk_by_raw_type(
        &contents,
        CONTENTS_RAW_TYPE_STORY_FRAME_INDEX,
        "StoryFrame index 0x61",
    )?;
    let grounded_story_count = story_catalog.declared_count;

    let descriptors = parse_descriptor_directory(&quill)?;
    let syid = unique_descriptor(&descriptors, *b"SYID")?;
    let strs = unique_descriptor(&descriptors, *b"STRS")?;
    let text = unique_descriptor(&descriptors, *b"TEXT")?;
    let mcld = unique_descriptor(&descriptors, *b"MCLD")?;
    let btep = descriptors_named(&descriptors, *b"BTEP");
    let btec = descriptors_named(&descriptors, *b"BTEC");
    let fdpp = descriptors_named(&descriptors, *b"FDPP");
    let fdpc = descriptors_named(&descriptors, *b"FDPC");

    let syid_payload = descriptor_range(&quill, syid)?;
    let strs_payload = descriptor_range(&quill, strs)?;
    descriptor_range(&quill, text)?;
    let descriptor_payload_profiles = descriptor_payload_profiles(&quill, &descriptors)?;
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
    let mcld_profile = profile_mcld(&quill, mcld, &story_catalog, text_utf16_units, text_bytes)?;
    let fdpp_profile = profile_fdpp(&quill, fdpp[0], text, grounded_story_count)?;
    let story_frame_index_profile = profile_story_frame_index(
        &contents,
        &story_frame_index_chunk,
        &story_catalog,
        &quill,
        fdpp[0],
        text,
    )?;
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
        descriptor_payload_profiles,
        syid_descriptor_metadata,
        strs_descriptor_metadata,
        text_descriptor_metadata,
        mcld_profile,
        fdpp_profile,
        story_frame_index_profile,
        syid_strs_text_opt_a_all_equal: syid.opt_a == strs.opt_a && strs.opt_a == text.opt_a,
        syid_strs_text_bit_type_all_equal: syid.bit_type == strs.bit_type
            && strs.bit_type == text.bit_type,
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
        strs_direct_generic_plc_candidate_count: scan_direct_generic_plc_count(
            strs_payload,
            grounded_story_count,
        ),

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

fn diagnose_sibling_scan(bytes: &[u8]) -> SiblingScanRow {
    let source_sha256 = sha256_hex(bytes);
    let byte_len = bytes.len();
    let details = (|| -> Result<SiblingScanDetails> {
        let contents = pub_cfb::read_stream_reader(Cursor::new(bytes), CONTENTS_STREAM)
            .context("read Contents stream")?;
        let quill = pub_cfb::read_stream_reader(Cursor::new(bytes), QUILL_STREAM)
            .context("read Quill stream")?;

        let (revision, story_catalog) = grounded_contents_story_catalog(&contents)?;
        let grounded_story_count = story_catalog.declared_count;
        let descriptors = parse_descriptor_directory(&quill)?;
        let descriptor_payload_profiles = descriptor_payload_profiles(&quill, &descriptors)?;
        let syid = unique_descriptor(&descriptors, *b"SYID")?;
        let strs = unique_descriptor(&descriptors, *b"STRS")?;
        let text = unique_descriptor(&descriptors, *b"TEXT")?;
        let fdpp = descriptors_named(&descriptors, *b"FDPP");

        let syid_payload = descriptor_range(&quill, syid)?;
        let strs_payload = descriptor_range(&quill, strs)?;
        descriptor_range(&quill, text)?;

        let fdpp_first_stored_count = if let Some(first) = fdpp.first() {
            let payload = descriptor_range(&quill, first)?;
            u16_at(payload, 0)
        } else {
            None
        };

        let expected_syid_len = 8u64 + 4u64 * u64::from(grounded_story_count);
        let expected_strs_len = 22u64 + 8u64 * u64::from(grounded_story_count);

        let (_, _, fdpp_utf16_units) = if let Some(first) = fdpp.first() {
            fdpp_boundary_sets(&quill, first, text)?
        } else {
            (BTreeSet::new(), BTreeSet::new(), BTreeSet::new())
        };
        let ordinary_story_catalog =
            pub_quill::parse_confirmed_story_catalog(StreamPath(QUILL_STREAM.into()), &quill).ok();
        let mut ordinary_story_ends = BTreeSet::new();
        if let Some(catalog) = &ordinary_story_catalog {
            let mut cumulative = 0u64;
            for story in &catalog.stories {
                cumulative = cumulative
                    .checked_add(u64::from(story.utf16_code_units))
                    .context("ordinary STRS cumulative Story length overflow")?;
                ordinary_story_ends.insert(cumulative);
            }
        }
        let ordinary_story_end_count = ordinary_story_ends.len();
        let ordinary_story_ends_all_in_fdpp = ordinary_story_catalog.is_some()
            && ordinary_story_ends
                .iter()
                .all(|value| fdpp_utf16_units.contains(value));
        let ordinary_story_end_set_equals_fdpp = ordinary_story_catalog.is_some()
            && ordinary_story_ends == fdpp_utf16_units;

        Ok(SiblingScanDetails {
            contents_serialization_revision: revision,
            grounded_story_count,
            descriptor_count: descriptors.len(),
            syid_descriptor_length: syid.data_length,
            syid_declared_count: u32_at(syid_payload, 4),
            syid_chunk_all_ff: syid_payload.iter().all(|byte| *byte == 0xff),
            syid_length_matches_grounded_count: u64::from(syid.data_length) == expected_syid_len,
            strs_descriptor_length: strs.data_length,
            strs_declared_count: u32_at(strs_payload, 0),
            strs_chunk_all_ff: strs_payload.iter().all(|byte| *byte == 0xff),
            strs_length_matches_grounded_count: u64::from(strs.data_length) == expected_strs_len,
            text_descriptor_length: text.data_length,
            text_utf16_units: u64::from(text.data_length) / 2,
            fdpp_descriptor_lengths: descriptor_lengths(&fdpp),
            fdpp_first_stored_count,
            descriptor_topology: descriptor_topology(&descriptors),
            descriptor_payload_profiles,
            ordinary_story_catalog_admitted: ordinary_story_catalog.is_some(),
            ordinary_story_end_count,
            ordinary_story_ends_all_in_fdpp,
            ordinary_story_end_set_equals_fdpp,
        })
    })()
    .ok();

    SiblingScanRow {
        source_sha256,
        byte_len,
        admitted: details.is_some(),
        details,
    }
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
    let args = env::args_os().skip(1).collect::<Vec<_>>();
    let sibling_mode = args
        .first()
        .is_some_and(|arg| arg.to_string_lossy() == "--siblings");

    let (root, output) = if sibling_mode {
        if args.len() != 3 {
            bail!(
                "usage: quill-story-early-text-boundary-probe --siblings WITNESS_DIR OUTPUT.json"
            );
        }
        (PathBuf::from(&args[1]), PathBuf::from(&args[2]))
    } else {
        if args.len() != 2 {
            bail!("usage: quill-story-early-text-boundary-probe WITNESS_DIR OUTPUT.json");
        }
        (PathBuf::from(&args[0]), PathBuf::from(&args[1]))
    };

    let paths = pub_paths(&root)?;

    if sibling_mode {
        let mut rows = Vec::with_capacity(paths.len());
        for path in paths {
            let bytes = fs::read(&path).with_context(|| format!("read {}", path.display()))?;
            rows.push(diagnose_sibling_scan(&bytes));
        }
        rows.sort_by(|left, right| left.source_sha256.cmp(&right.source_sha256));
        let admitted_count = rows.iter().filter(|row| row.admitted).count();

        let report = serde_json::json!({
            "schema": "chaptera.quill-story-sibling-census.v1",
            "witness_count": rows.len(),
            "admitted_count": admitted_count,
            "skipped_count": rows.len().saturating_sub(admitted_count),
            "rows": rows,
            "evidence_boundary": "SHA-addressed same-source sibling census; source-safe structural counts, lengths and booleans only; no filenames, paths, document text, Story IDs, raw payload bytes, absolute offsets or parser error text",
        });

        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&output, serde_json::to_vec_pretty(&report)?)?;
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

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
        "schema": "chaptera.quill-story-early-text-boundary.v12",
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
