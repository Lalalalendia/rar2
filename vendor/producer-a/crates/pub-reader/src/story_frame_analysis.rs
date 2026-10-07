//! Research-only Story/frame identity and grouped-geometry correlation.
//!
//! This module gathers source evidence and corpus correlations. It does not
//! own production source-graph construction, resolved StoryFrame semantics,
//! page projection, paint, or typography.
//! Routing control: this file is a research-only correlation owner.

use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PubStoryFrameCorrelation {
    pub quill_syid_count: usize,
    pub shape_chunk_count: usize,
    pub shape_chunks_with_opaque_tail: usize,
    pub decoded_field_27_syid_matches: usize,
    pub raw_scalar_syid_matches: usize,
    pub raw_field_27_syid_matches: usize,
    pub raw_field_27_syid_matches_in_opaque_tail: usize,
    pub shape_chunks_with_one_field_27_syid_match: usize,
    pub shape_chunks_with_multiple_field_27_syid_matches: usize,
    pub distinct_field_27_matched_syids: usize,
    pub shape_chunks_with_field_27_match_on_document_pages: usize,
    pub shape_chunks_with_field_27_match_outside_document_pages: usize,
    pub distinct_field_27_matched_syids_on_document_pages: usize,
    pub distinct_field_27_matched_syids_outside_document_pages: usize,
    pub table_chunk_count: usize,
    pub table_chunks_with_field_27_syid_match: usize,
    pub distinct_table_field_27_matched_syids: usize,
    pub table_chunks_with_field_27_match_on_document_pages: usize,
    pub table_chunks_with_field_27_match_outside_document_pages: usize,
    pub distinct_table_field_27_matched_syids_on_document_pages: usize,
    pub distinct_table_field_27_matched_syids_outside_document_pages: usize,
    pub distinct_story_syids_with_shape_or_table_identity: usize,
    pub distinct_story_syids_with_both_shape_and_table_identity: usize,
    pub distinct_story_syids_with_shape_only_identity: usize,
    pub distinct_story_syids_with_table_only_identity: usize,
    pub distinct_story_syids_without_shape_or_table_identity: usize,
    pub field_wire_match_counts: BTreeMap<String, usize>,
    pub field_27_parent_class_counts: BTreeMap<String, usize>,
    pub table_field_27_parent_class_counts: BTreeMap<String, usize>,
    pub multiframe_story_syid_count: usize,
    pub multiframe_shape_count: usize,
    pub multiframe_peer_seq_reference_counts: BTreeMap<String, usize>,
    pub multiframe_peer_seq_reference_story_counts: BTreeMap<String, usize>,
    pub multiframe_zero_based_ordinal_candidate_story_counts: BTreeMap<String, usize>,
    pub multiframe_one_based_ordinal_candidate_story_counts: BTreeMap<String, usize>,
    pub multiframe_unique_scalar_candidate_story_counts: BTreeMap<String, usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PubGroupedStoryGeometryCorrelation {
    pub grouped_story_shape_count: usize,
    pub grouped_story_distinct_syid_count: usize,
    pub unique_child_escher_shape_count: usize,
    pub child_anchor_count: usize,
    pub parent_group_shape_link_count: usize,
    pub parent_group_contents_identity_match_count: usize,
    pub parent_group_fspgr_count: usize,
    pub parent_group_complete_client_anchor_count: usize,
    pub one_level_page_group_count: usize,
    pub nested_group_count: usize,
    pub complete_one_level_geometry_count: usize,
    pub complete_recursive_geometry_count: usize,
    pub max_group_depth: usize,
    pub group_depth_counts: BTreeMap<String, usize>,
    pub grouped_story_child_rotation_count: usize,
    pub grouped_story_child_flip_h_count: usize,
    pub grouped_story_child_flip_v_count: usize,
    pub grouped_story_group_ancestor_rotation_count: usize,
    pub grouped_story_group_ancestor_flip_h_count: usize,
    pub grouped_story_group_ancestor_flip_v_count: usize,
    pub grouped_story_group_ancestor_nontrivial_transform_count: usize,
    pub direct_page_story_shape_count: usize,
    pub direct_page_story_incomplete_client_anchor_count: usize,
    pub direct_page_story_incomplete_client_anchor_with_child_anchor_count: usize,
    pub direct_page_story_incomplete_client_anchor_with_fspgr_count: usize,
    pub direct_page_story_incomplete_client_anchor_missing_field_counts: BTreeMap<String, usize>,
    pub top_group_incomplete_client_anchor_missing_field_counts: BTreeMap<String, usize>,
    pub grouped_table_story_count: usize,
    pub grouped_table_complete_recursive_geometry_count: usize,
    pub grouped_table_nontrivial_transform_count: usize,
    pub grouped_table_max_group_depth: usize,
    pub grouped_table_incomplete_reason_counts: BTreeMap<String, usize>,
    pub incomplete_reason_counts: BTreeMap<String, usize>,
    pub recursive_incomplete_reason_counts: BTreeMap<String, usize>,
}

/// Research-only correlation scan for the Story ↔ shape ownership bridge.
///
/// This function does not promote any raw candidate to semantics. It searches
/// mature-0x2C SHAPE chunks for known scalar wire forms whose u32 payload equals
/// a Quill SYID, separately records whether the currently expected field 0x27
/// occurs after the bounded chunk parser entered its opaque suffix, and
/// classifies matched SHAPE ownership by DOCUMENT PageList membership.
///
/// The result is suitable for corpus evidence gathering. Semantic readers must
/// continue to rely on explicitly decoded fields until a wire form is proven.
pub fn analyze_mature_0x2c_story_frame_candidates<R: Read + Seek>(
    mut reader: R,
) -> Result<PubStoryFrameCorrelation> {
    reader.seek(SeekFrom::Start(0))?;
    let mut pub_bytes = Vec::new();
    reader.read_to_end(&mut pub_bytes)?;

    let contents =
        pub_cfb::read_stream_reader(Cursor::new(pub_bytes.as_slice()), CONTENTS_STREAM_PATH)
            .with_context(|| format!("read {CONTENTS_STREAM_PATH}"))?;
    let quill = pub_cfb::read_stream_reader(Cursor::new(pub_bytes.as_slice()), QUILL_STREAM_PATH)
        .with_context(|| format!("read {QUILL_STREAM_PATH}"))?;

    analyze_mature_0x2c_story_frame_candidates_from_streams(&contents, &quill)
}

pub fn analyze_mature_0x2c_story_frame_candidates_from_streams(
    contents: &[u8],
    quill: &[u8],
) -> Result<PubStoryFrameCorrelation> {
    let contents_stream = StreamPath(CONTENTS_STREAM_PATH.into());
    let header = parse_0x2c_header(contents_stream.clone(), contents)
        .context("parse mature-0x2C Contents header for Story-frame correlation")?;
    let trailer = parse_confirmed_0x2c_trailer_root(contents, &header)
        .context("parse mature-0x2C Contents trailer for Story-frame correlation")?;
    let references = build_reference_index(contents, &trailer.directory)?;

    let document_reference =
        unique_reference_by_raw_type(&references, RAW_TYPE_DOCUMENT, "DOCUMENT")?;
    let document_chunk =
        chunk_for_reference(contents_stream.clone(), contents, document_reference)?;
    let page_list_block = unique_block(&document_chunk, DOCUMENT_PAGE_LIST_ID)?.clone();
    let page_list = parse_confirmed_document_page_list(contents, page_list_block)
        .context("parse DOCUMENT PageList for Story-frame correlation")?;
    let document_page_handles = page_list
        .entries
        .iter()
        .filter_map(|entry| {
            references
                .get(&entry.handle)
                .is_some_and(|reference| single_raw_type(reference) == Some(RAW_TYPE_PAGE))
                .then_some(entry.handle)
        })
        .collect::<BTreeSet<_>>();

    let quill_catalog = parse_confirmed_story_catalog(StreamPath(QUILL_STREAM_PATH.into()), quill)
        .context("parse Quill story catalog for Story-frame correlation")?;
    let syids = quill_catalog
        .stories
        .iter()
        .map(|story| story.syid.0)
        .collect::<BTreeSet<_>>();

    let mut result = PubStoryFrameCorrelation {
        quill_syid_count: syids.len(),
        ..Default::default()
    };
    let mut field_27_matched_syids = BTreeSet::new();
    let mut field_27_matched_syids_on_document_pages = BTreeSet::new();
    let mut field_27_matched_syids_outside_document_pages = BTreeSet::new();
    let mut shape_scalars_by_story =
        BTreeMap::<u32, Vec<(u32, BTreeMap<(u16, u8), Vec<u32>>)>>::new();

    for reference in references.values() {
        if single_raw_type(reference) != Some(RAW_TYPE_SHAPE) {
            continue;
        }

        result.shape_chunk_count += 1;

        let parent_seq = single_parent_seq(reference);
        let parent_is_document_page =
            parent_seq.is_some_and(|seq_num| document_page_handles.contains(&seq_num));
        let parent_class = match parent_seq {
            Some(_) if parent_is_document_page => "document_page/0x43".to_owned(),
            Some(seq_num) => references
                .get(&seq_num)
                .and_then(single_raw_type)
                .map(|raw_type| format!("outside_document_pages/0x{raw_type:02x}"))
                .unwrap_or_else(|| "outside_document_pages/unknown".to_owned()),
            None => "missing_or_ambiguous_parent".to_owned(),
        };

        let chunk = chunk_for_reference(contents_stream.clone(), contents, reference)?;
        if chunk.unsupported_tail.is_some() {
            result.shape_chunks_with_opaque_tail += 1;
        }

        let decoded_story_id = chunk.fields.iter().find_map(|field| {
            (field.id == FIELD_STORY_ID)
                .then(|| match &field.body {
                    RawContentsBlockBody::U16 { value, .. } => Some(u32::from(*value)),
                    RawContentsBlockBody::U32 { value, .. } => Some(*value),
                    _ => None,
                })
                .flatten()
        });
        if let Some(story_id) = decoded_story_id.filter(|value| syids.contains(value)) {
            let seq_num = seq_u32(reference.seq_num)?;
            let mut scalars = BTreeMap::<(u16, u8), Vec<u32>>::new();
            for field in &chunk.fields {
                let value = match &field.body {
                    RawContentsBlockBody::U16 { value, .. } => Some(u32::from(*value)),
                    RawContentsBlockBody::U32 { value, .. } => Some(*value),
                    _ => None,
                };
                if let Some(value) = value {
                    scalars
                        .entry((field.id, field.block_type))
                        .or_default()
                        .push(value);
                }
            }
            shape_scalars_by_story
                .entry(story_id)
                .or_default()
                .push((seq_num, scalars));
        }

        for field in &chunk.fields {
            if field.id != FIELD_STORY_ID {
                continue;
            }
            if let RawContentsBlockBody::U32 { value, .. } = &field.body {
                if syids.contains(value) {
                    result.decoded_field_27_syid_matches += 1;
                }
            }
        }

        let start = usize::try_from(chunk.source.offset)
            .context("Story-frame correlation chunk offset does not fit usize")?;
        let len = usize::try_from(chunk.source.len)
            .context("Story-frame correlation chunk length does not fit usize")?;
        let end = start
            .checked_add(len)
            .filter(|end| *end <= contents.len())
            .context("Story-frame correlation chunk range is out of bounds")?;
        let raw = &contents[start..end];

        let tail_range = chunk.unsupported_tail.as_ref().and_then(|tail| {
            let tail_start = usize::try_from(tail.offset).ok()?;
            let tail_len = usize::try_from(tail.len).ok()?;
            let tail_end = tail_start.checked_add(tail_len)?;
            Some((tail_start, tail_end))
        });

        let mut field_27_matches_in_shape = 0_usize;
        if raw.len() >= 6 {
            for relative in 4..=raw.len() - 6 {
                let raw_tag = [raw[relative], raw[relative + 1]];
                let (id, wire) = pub_contents::decode_packed_field_tag(raw_tag);
                if !matches!(
                    wire,
                    pub_contents::BLOCK_TYPE_U32
                        | pub_contents::BLOCK_TYPE_REFERENCE_U32
                        | pub_contents::BLOCK_TYPE_HANDLE_U32
                ) {
                    continue;
                }

                let value = u32::from_le_bytes([
                    raw[relative + 2],
                    raw[relative + 3],
                    raw[relative + 4],
                    raw[relative + 5],
                ]);
                if !syids.contains(&value) {
                    continue;
                }

                result.raw_scalar_syid_matches += 1;
                *result
                    .field_wire_match_counts
                    .entry(format!("0x{id:02x}/0x{wire:02x}"))
                    .or_insert(0) += 1;

                if id != FIELD_STORY_ID {
                    continue;
                }

                result.raw_field_27_syid_matches += 1;
                field_27_matches_in_shape += 1;
                field_27_matched_syids.insert(value);
                if parent_is_document_page {
                    field_27_matched_syids_on_document_pages.insert(value);
                } else {
                    field_27_matched_syids_outside_document_pages.insert(value);
                }

                let absolute = start + relative;
                if tail_range.is_some_and(|(tail_start, tail_end)| {
                    absolute >= tail_start && absolute < tail_end
                }) {
                    result.raw_field_27_syid_matches_in_opaque_tail += 1;
                }
            }
        }

        match field_27_matches_in_shape {
            1 => result.shape_chunks_with_one_field_27_syid_match += 1,
            2.. => result.shape_chunks_with_multiple_field_27_syid_matches += 1,
            _ => {}
        }

        if field_27_matches_in_shape > 0 {
            if parent_is_document_page {
                result.shape_chunks_with_field_27_match_on_document_pages += 1;
            } else {
                result.shape_chunks_with_field_27_match_outside_document_pages += 1;
            }
            *result
                .field_27_parent_class_counts
                .entry(parent_class)
                .or_insert(0) += 1;
        }
    }

    result.distinct_field_27_matched_syids = field_27_matched_syids.len();
    result.distinct_field_27_matched_syids_on_document_pages =
        field_27_matched_syids_on_document_pages.len();
    result.distinct_field_27_matched_syids_outside_document_pages =
        field_27_matched_syids_outside_document_pages.len();

    for frames in shape_scalars_by_story
        .values()
        .filter(|frames| frames.len() > 1)
    {
        result.multiframe_story_syid_count += 1;
        result.multiframe_shape_count += frames.len();

        let frame_seq_nums = frames
            .iter()
            .map(|(seq_num, _)| *seq_num)
            .collect::<BTreeSet<_>>();
        let mut peer_fields_seen = BTreeSet::new();
        let mut all_keys = BTreeSet::new();
        for (_, scalars) in frames {
            all_keys.extend(scalars.keys().copied());
            for (&(id, wire), values) in scalars {
                if id == FIELD_STORY_ID {
                    continue;
                }
                for value in values {
                    if frame_seq_nums.contains(value) {
                        let key = format!("0x{id:02x}/0x{wire:02x}");
                        *result
                            .multiframe_peer_seq_reference_counts
                            .entry(key.clone())
                            .or_insert(0) += 1;
                        peer_fields_seen.insert(key);
                    }
                }
            }
        }
        for key in peer_fields_seen {
            *result
                .multiframe_peer_seq_reference_story_counts
                .entry(key)
                .or_insert(0) += 1;
        }

        let expected_zero =
            (0..u32::try_from(frames.len()).unwrap_or(u32::MAX)).collect::<BTreeSet<_>>();
        let expected_one =
            (1..=u32::try_from(frames.len()).unwrap_or(u32::MAX)).collect::<BTreeSet<_>>();

        for (id, wire) in all_keys {
            if id == FIELD_STORY_ID {
                continue;
            }
            let values = frames
                .iter()
                .filter_map(|(_, scalars)| {
                    let values = scalars.get(&(id, wire))?;
                    (values.len() == 1).then_some(values[0])
                })
                .collect::<Vec<_>>();
            if values.len() != frames.len() {
                continue;
            }

            let distinct = values.iter().copied().collect::<BTreeSet<_>>();
            if distinct.len() == frames.len() {
                let key = format!("0x{id:02x}/0x{wire:02x}");
                *result
                    .multiframe_unique_scalar_candidate_story_counts
                    .entry(key.clone())
                    .or_insert(0) += 1;
                if distinct == expected_zero {
                    *result
                        .multiframe_zero_based_ordinal_candidate_story_counts
                        .entry(key.clone())
                        .or_insert(0) += 1;
                }
                if distinct == expected_one {
                    *result
                        .multiframe_one_based_ordinal_candidate_story_counts
                        .entry(key)
                        .or_insert(0) += 1;
                }
            }
        }
    }

    let mut table_field_27_matched_syids = BTreeSet::new();
    let mut table_field_27_matched_syids_on_document_pages = BTreeSet::new();
    let mut table_field_27_matched_syids_outside_document_pages = BTreeSet::new();
    for reference in references.values() {
        if single_raw_type(reference) != Some(RAW_TYPE_TABLE) {
            continue;
        }

        result.table_chunk_count += 1;

        let parent_seq = single_parent_seq(reference);
        let parent_is_document_page =
            parent_seq.is_some_and(|seq_num| document_page_handles.contains(&seq_num));
        let parent_class = match parent_seq {
            Some(_) if parent_is_document_page => "document_page/0x43".to_owned(),
            Some(seq_num) => references
                .get(&seq_num)
                .and_then(single_raw_type)
                .map(|raw_type| format!("outside_document_pages/0x{raw_type:02x}"))
                .unwrap_or_else(|| "outside_document_pages/unknown".to_owned()),
            None => "missing_or_ambiguous_parent".to_owned(),
        };

        let chunk = chunk_for_reference(contents_stream.clone(), contents, reference)?;
        let mut matched = false;
        for field in &chunk.fields {
            if field.id != FIELD_STORY_ID {
                continue;
            }
            let value = match &field.body {
                RawContentsBlockBody::U16 { value, .. } => u32::from(*value),
                RawContentsBlockBody::U32 { value, .. } => *value,
                _ => continue,
            };
            if syids.contains(&value) {
                matched = true;
                table_field_27_matched_syids.insert(value);
                if parent_is_document_page {
                    table_field_27_matched_syids_on_document_pages.insert(value);
                } else {
                    table_field_27_matched_syids_outside_document_pages.insert(value);
                }
            }
        }
        if matched {
            result.table_chunks_with_field_27_syid_match += 1;
            if parent_is_document_page {
                result.table_chunks_with_field_27_match_on_document_pages += 1;
            } else {
                result.table_chunks_with_field_27_match_outside_document_pages += 1;
            }
            *result
                .table_field_27_parent_class_counts
                .entry(parent_class)
                .or_insert(0) += 1;
        }
    }

    result.distinct_table_field_27_matched_syids = table_field_27_matched_syids.len();
    result.distinct_table_field_27_matched_syids_on_document_pages =
        table_field_27_matched_syids_on_document_pages.len();
    result.distinct_table_field_27_matched_syids_outside_document_pages =
        table_field_27_matched_syids_outside_document_pages.len();

    let shape_or_table = field_27_matched_syids
        .union(&table_field_27_matched_syids)
        .copied()
        .collect::<BTreeSet<_>>();
    result.distinct_story_syids_with_shape_or_table_identity = shape_or_table.len();
    result.distinct_story_syids_with_both_shape_and_table_identity = field_27_matched_syids
        .intersection(&table_field_27_matched_syids)
        .count();
    result.distinct_story_syids_with_shape_only_identity = field_27_matched_syids
        .difference(&table_field_27_matched_syids)
        .count();
    result.distinct_story_syids_with_table_only_identity = table_field_27_matched_syids
        .difference(&field_27_matched_syids)
        .count();
    result.distinct_story_syids_without_shape_or_table_identity =
        syids.difference(&shape_or_table).count();

    Ok(result)
}

/// Research-only census for grouped Story geometry prerequisites.
///
/// This does not materialize Group nodes or derive absolute child bounds. It
/// measures whether exact Contents Story ownership can be joined to the
/// OfficeArt group hierarchy and to the raw coordinate records required by
/// the proven FSPGR + ChildAnchor transform model.
pub fn analyze_mature_0x2c_grouped_story_geometry<R: Read + Seek>(
    mut reader: R,
) -> Result<PubGroupedStoryGeometryCorrelation> {
    reader.seek(SeekFrom::Start(0))?;
    let mut pub_bytes = Vec::new();
    reader.read_to_end(&mut pub_bytes)?;

    let contents =
        pub_cfb::read_stream_reader(Cursor::new(pub_bytes.as_slice()), CONTENTS_STREAM_PATH)
            .with_context(|| format!("read {CONTENTS_STREAM_PATH}"))?;
    let quill = pub_cfb::read_stream_reader(Cursor::new(pub_bytes.as_slice()), QUILL_STREAM_PATH)
        .with_context(|| format!("read {QUILL_STREAM_PATH}"))?;
    let escher = pub_cfb::read_stream_reader(Cursor::new(pub_bytes.as_slice()), ESCHER_STREAM_PATH)
        .with_context(|| format!("read {ESCHER_STREAM_PATH}"))?;

    analyze_mature_0x2c_grouped_story_geometry_from_streams(&contents, &quill, &escher)
}

pub fn analyze_mature_0x2c_grouped_story_geometry_from_streams(
    contents: &[u8],
    quill: &[u8],
    escher: &[u8],
) -> Result<PubGroupedStoryGeometryCorrelation> {
    let contents_stream = StreamPath(CONTENTS_STREAM_PATH.into());
    let header = parse_0x2c_header(contents_stream.clone(), contents)
        .context("parse mature-0x2C Contents header for grouped Story census")?;
    let trailer = parse_confirmed_0x2c_trailer_root(contents, &header)
        .context("parse mature-0x2C Contents trailer for grouped Story census")?;
    let references = build_reference_index(contents, &trailer.directory)?;

    let document_reference =
        unique_reference_by_raw_type(&references, RAW_TYPE_DOCUMENT, "DOCUMENT")?;
    let document_chunk =
        chunk_for_reference(contents_stream.clone(), contents, document_reference)?;
    let page_list_block = unique_block(&document_chunk, DOCUMENT_PAGE_LIST_ID)?.clone();
    let page_list = parse_confirmed_document_page_list(contents, page_list_block)
        .context("parse DOCUMENT PageList for grouped Story census")?;
    let document_page_handles = page_list
        .entries
        .iter()
        .filter_map(|entry| {
            references
                .get(&entry.handle)
                .is_some_and(|reference| single_raw_type(reference) == Some(RAW_TYPE_PAGE))
                .then_some(entry.handle)
        })
        .collect::<BTreeSet<_>>();

    let quill_catalog = parse_confirmed_story_catalog(StreamPath(QUILL_STREAM_PATH.into()), quill)
        .context("parse Quill story catalog for grouped Story census")?;
    let syids = quill_catalog
        .stories
        .iter()
        .map(|story| story.syid.0)
        .collect::<BTreeSet<_>>();

    let escher_inventory = inspect_sp_containers(StreamPath(ESCHER_STREAM_PATH.into()), escher)
        .context("parse OfficeArt SpContainers for grouped Story census")?;
    let escher_by_contents_seq = index_escher_by_contents_seq(&escher_inventory);

    let mut result = PubGroupedStoryGeometryCorrelation::default();
    let mut grouped_syids = BTreeSet::new();

    // Measure the direct-page Story shapes that the runtime currently rejects
    // only because their Publisher ClientAnchor is incomplete. This is
    // evidence-only: alternate OfficeArt records are counted, not promoted.
    for reference in references.values() {
        if single_raw_type(reference) != Some(RAW_TYPE_SHAPE) {
            continue;
        }
        let Some(parent_seq) = single_parent_seq(reference) else {
            continue;
        };
        if !document_page_handles.contains(&parent_seq) {
            continue;
        }

        let seq_num = seq_u32(reference.seq_num)?;
        let chunk = chunk_for_reference(contents_stream.clone(), contents, reference)?;
        let Some((text_id, _)) = unique_u32_field(&chunk, FIELD_STORY_ID)? else {
            continue;
        };
        if !syids.contains(&text_id) {
            continue;
        }

        result.direct_page_story_shape_count += 1;
        let matches = escher_by_contents_seq
            .get(&seq_num)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let shape = match matches {
            [index] => &escher_inventory.shapes[*index],
            _ => continue,
        };

        let complete = shape
            .client_anchor
            .as_ref()
            .is_some_and(anchor_has_unique_geometry_fields);
        if complete {
            continue;
        }

        result.direct_page_story_incomplete_client_anchor_count += 1;
        if shape.child_anchor.is_some() {
            result.direct_page_story_incomplete_client_anchor_with_child_anchor_count += 1;
        }
        if shape.fspgr.is_some() {
            result.direct_page_story_incomplete_client_anchor_with_fspgr_count += 1;
        }
        record_missing_anchor_fields(
            shape.client_anchor.as_ref(),
            &mut result.direct_page_story_incomplete_client_anchor_missing_field_counts,
        );
    }

    for reference in references.values() {
        if single_raw_type(reference) != Some(RAW_TYPE_SHAPE) {
            continue;
        }

        let Some(parent_seq) = single_parent_seq(reference) else {
            continue;
        };
        let Some(parent_reference) = references.get(&parent_seq) else {
            continue;
        };
        if single_raw_type(parent_reference) != Some(RAW_TYPE_GROUP) {
            continue;
        }

        let seq_num = seq_u32(reference.seq_num)?;
        let chunk = chunk_for_reference(contents_stream.clone(), contents, reference)?;
        let Some((text_id, _)) = unique_u32_field(&chunk, FIELD_STORY_ID)? else {
            continue;
        };
        if !syids.contains(&text_id) {
            continue;
        }

        result.grouped_story_shape_count += 1;
        grouped_syids.insert(text_id);

        let mut complete = true;
        let matches = escher_by_contents_seq
            .get(&seq_num)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let child = match matches {
            [index] => {
                result.unique_child_escher_shape_count += 1;
                &escher_inventory.shapes[*index]
            }
            [] => {
                increment_reason(&mut result, "child_escher_missing");
                continue;
            }
            _ => {
                increment_reason(&mut result, "child_escher_ambiguous");
                continue;
            }
        };

        if child.child_anchor.is_some() {
            result.child_anchor_count += 1;
        } else {
            complete = false;
            increment_reason(&mut result, "child_anchor_missing");
        }

        let Some(parent_source) = child.parent_group_shape_source.as_ref() else {
            increment_reason(&mut result, "parent_group_shape_link_missing");
            continue;
        };
        result.parent_group_shape_link_count += 1;

        let mut parent_matches = escher_inventory
            .shapes
            .iter()
            .filter(|shape| &shape.source == parent_source);
        let Some(group_shape) = parent_matches.next() else {
            increment_reason(&mut result, "parent_group_shape_missing");
            continue;
        };
        if parent_matches.next().is_some() {
            increment_reason(&mut result, "parent_group_shape_ambiguous");
            continue;
        }

        let group_contents_id_matches = group_shape
            .client_data
            .as_ref()
            .and_then(|record| unique_escher_field(record, PUBLISHER_FIELD_SHAPE_ID))
            .is_some_and(|field| field.value == parent_seq);
        if group_contents_id_matches {
            result.parent_group_contents_identity_match_count += 1;
        } else {
            complete = false;
            increment_reason(&mut result, "parent_group_contents_identity_mismatch");
        }

        if group_shape.fspgr.is_some() {
            result.parent_group_fspgr_count += 1;
        } else {
            complete = false;
            increment_reason(&mut result, "parent_group_fspgr_missing");
        }

        let complete_group_anchor = group_shape
            .client_anchor
            .as_ref()
            .is_some_and(anchor_has_unique_geometry_fields);
        if complete_group_anchor {
            result.parent_group_complete_client_anchor_count += 1;
        } else {
            complete = false;
            increment_reason(&mut result, "parent_group_client_anchor_incomplete");
        }

        let group_parent_seq = single_parent_seq(parent_reference);
        let one_level = group_parent_seq
            .is_some_and(|group_parent| document_page_handles.contains(&group_parent));
        if one_level {
            result.one_level_page_group_count += 1;
        } else {
            complete = false;
            let nested = group_parent_seq
                .and_then(|group_parent| references.get(&group_parent))
                .and_then(single_raw_type)
                == Some(RAW_TYPE_GROUP);
            if nested {
                result.nested_group_count += 1;
                increment_reason(&mut result, "nested_group");
            } else {
                increment_reason(&mut result, "group_parent_not_document_page");
            }
        }

        if complete {
            result.complete_one_level_geometry_count += 1;
        }

        // A nested group does not have a page-level ClientAnchor. Its own
        // ChildAnchor positions it inside the next parent group's FSPGR
        // coordinate space. Walk that exact hierarchy until a DOCUMENT page
        // is reached, preserving the one-level counters above as a separate
        // measurement.
        let child_rotation = shape_has_nonzero_rotation(child);
        let child_flip_h = shape_has_fsp_flag(child, OFFICEART_FSP_FLIP_H);
        let child_flip_v = shape_has_fsp_flag(child, OFFICEART_FSP_FLIP_V);
        result.grouped_story_child_rotation_count += usize::from(child_rotation);
        result.grouped_story_child_flip_h_count += usize::from(child_flip_h);
        result.grouped_story_child_flip_v_count += usize::from(child_flip_v);

        let mut recursive_complete = child.child_anchor.is_some();
        if !recursive_complete {
            increment_recursive_reason(&mut result, "child_anchor_missing");
        }

        let mut has_group_rotation = false;
        let mut has_group_flip_h = false;
        let mut has_group_flip_v = false;
        let mut current_shape = child;
        let mut current_group_seq = parent_seq;
        let mut seen_group_seq = BTreeSet::new();
        let mut group_depth = 0_usize;

        loop {
            if !seen_group_seq.insert(current_group_seq) {
                recursive_complete = false;
                increment_recursive_reason(&mut result, "group_cycle");
                break;
            }
            group_depth += 1;

            let Some(group_reference) = references.get(&current_group_seq) else {
                recursive_complete = false;
                increment_recursive_reason(&mut result, "group_contents_reference_missing");
                break;
            };
            if single_raw_type(group_reference) != Some(RAW_TYPE_GROUP) {
                recursive_complete = false;
                increment_recursive_reason(&mut result, "group_contents_wrong_raw_type");
                break;
            }

            let group_matches = escher_by_contents_seq
                .get(&current_group_seq)
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            let group_shape = match group_matches {
                [index] => &escher_inventory.shapes[*index],
                [] => {
                    recursive_complete = false;
                    increment_recursive_reason(&mut result, "group_escher_missing");
                    break;
                }
                _ => {
                    recursive_complete = false;
                    increment_recursive_reason(&mut result, "group_escher_ambiguous");
                    break;
                }
            };

            if current_shape.parent_group_shape_source.as_ref() != Some(&group_shape.source) {
                recursive_complete = false;
                increment_recursive_reason(&mut result, "group_parent_link_mismatch");
            }
            if group_shape.fspgr.is_none() {
                recursive_complete = false;
                increment_recursive_reason(&mut result, "group_fspgr_missing");
            }

            has_group_rotation |= shape_has_nonzero_rotation(group_shape);
            has_group_flip_h |= shape_has_fsp_flag(group_shape, OFFICEART_FSP_FLIP_H);
            has_group_flip_v |= shape_has_fsp_flag(group_shape, OFFICEART_FSP_FLIP_V);

            let Some(next_parent_seq) = single_parent_seq(group_reference) else {
                recursive_complete = false;
                increment_recursive_reason(&mut result, "group_parent_missing_or_ambiguous");
                break;
            };

            if document_page_handles.contains(&next_parent_seq) {
                if !group_shape
                    .client_anchor
                    .as_ref()
                    .is_some_and(anchor_has_unique_geometry_fields)
                {
                    recursive_complete = false;
                    increment_recursive_reason(&mut result, "top_group_client_anchor_incomplete");
                    record_missing_anchor_fields(
                        group_shape.client_anchor.as_ref(),
                        &mut result.top_group_incomplete_client_anchor_missing_field_counts,
                    );
                }
                break;
            }

            let nested =
                references.get(&next_parent_seq).and_then(single_raw_type) == Some(RAW_TYPE_GROUP);
            if !nested {
                recursive_complete = false;
                increment_recursive_reason(&mut result, "group_parent_not_document_page_or_group");
                break;
            }

            if group_shape.child_anchor.is_none() {
                recursive_complete = false;
                increment_recursive_reason(&mut result, "nested_group_child_anchor_missing");
            }

            current_shape = group_shape;
            current_group_seq = next_parent_seq;
        }

        result.max_group_depth = result.max_group_depth.max(group_depth);
        *result
            .group_depth_counts
            .entry(group_depth.to_string())
            .or_insert(0) += 1;
        result.grouped_story_group_ancestor_rotation_count += usize::from(has_group_rotation);
        result.grouped_story_group_ancestor_flip_h_count += usize::from(has_group_flip_h);
        result.grouped_story_group_ancestor_flip_v_count += usize::from(has_group_flip_v);
        result.grouped_story_group_ancestor_nontrivial_transform_count +=
            usize::from(has_group_rotation || has_group_flip_h || has_group_flip_v);
        if recursive_complete {
            result.complete_recursive_geometry_count += 1;
        }
    }

    result.grouped_story_distinct_syid_count = grouped_syids.len();

    // TABLE ownership is distinct from ordinary StoryFrame ownership, but a
    // grouped TABLE still needs the same OfficeArt ancestry to obtain runtime
    // geometry. Measure that fence independently without changing semantics.
    for reference in references.values() {
        if single_raw_type(reference) != Some(RAW_TYPE_TABLE) {
            continue;
        }
        let Some(parent_seq) = single_parent_seq(reference) else {
            continue;
        };
        if references.get(&parent_seq).and_then(single_raw_type) != Some(RAW_TYPE_GROUP) {
            continue;
        }

        let table_seq = seq_u32(reference.seq_num)?;
        let chunk = chunk_for_reference(contents_stream.clone(), contents, reference)?;
        let Some((text_id, _)) = unique_u32_field(&chunk, FIELD_STORY_ID)? else {
            continue;
        };
        if !syids.contains(&text_id) {
            continue;
        }

        result.grouped_table_story_count += 1;
        let child_matches = escher_by_contents_seq
            .get(&table_seq)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let child = match child_matches {
            [index] => &escher_inventory.shapes[*index],
            [] => {
                *result
                    .grouped_table_incomplete_reason_counts
                    .entry("child_escher_missing".into())
                    .or_insert(0) += 1;
                continue;
            }
            _ => {
                *result
                    .grouped_table_incomplete_reason_counts
                    .entry("child_escher_ambiguous".into())
                    .or_insert(0) += 1;
                continue;
            }
        };

        match inspect_grouped_geometry_chain(
            child,
            parent_seq,
            &references,
            &document_page_handles,
            &escher_inventory,
            &escher_by_contents_seq,
        ) {
            Ok((depth, has_nontrivial_transform)) => {
                result.grouped_table_max_group_depth =
                    result.grouped_table_max_group_depth.max(depth);
                if has_nontrivial_transform {
                    result.grouped_table_nontrivial_transform_count += 1;
                } else {
                    result.grouped_table_complete_recursive_geometry_count += 1;
                }
            }
            Err(reason) => {
                *result
                    .grouped_table_incomplete_reason_counts
                    .entry(reason)
                    .or_insert(0) += 1;
            }
        }
    }

    Ok(result)
}

fn inspect_grouped_geometry_chain(
    child: &pub_escher::SpContainerObservation,
    first_group_seq: u32,
    references: &BTreeMap<u32, Contents0x2cChunkReference>,
    document_page_handles: &BTreeSet<u32>,
    escher_inventory: &SpContainerInventory,
    escher_by_contents_seq: &BTreeMap<u32, Vec<usize>>,
) -> std::result::Result<(usize, bool), String> {
    if child.child_anchor.is_none() {
        return Err("child_anchor_missing".into());
    }

    let mut current_shape = child;
    let mut current_group_seq = first_group_seq;
    let mut seen = BTreeSet::new();
    let mut depth = 0_usize;
    let mut nontrivial_transform = shape_has_nonzero_rotation(child)
        || shape_has_fsp_flag(child, OFFICEART_FSP_FLIP_H)
        || shape_has_fsp_flag(child, OFFICEART_FSP_FLIP_V);

    loop {
        if !seen.insert(current_group_seq) {
            return Err("group_cycle".into());
        }
        depth += 1;

        let group_reference = references
            .get(&current_group_seq)
            .ok_or_else(|| "group_contents_reference_missing".to_owned())?;
        if single_raw_type(group_reference) != Some(RAW_TYPE_GROUP) {
            return Err("group_contents_wrong_raw_type".into());
        }

        let group_matches = escher_by_contents_seq
            .get(&current_group_seq)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let group_shape = match group_matches {
            [index] => &escher_inventory.shapes[*index],
            [] => return Err("group_escher_missing".into()),
            _ => return Err("group_escher_ambiguous".into()),
        };

        if current_shape.parent_group_shape_source.as_ref() != Some(&group_shape.source) {
            return Err("group_parent_link_mismatch".into());
        }
        if group_shape.fspgr.is_none() {
            return Err("group_fspgr_missing".into());
        }
        nontrivial_transform |= shape_has_nonzero_rotation(group_shape)
            || shape_has_fsp_flag(group_shape, OFFICEART_FSP_FLIP_H)
            || shape_has_fsp_flag(group_shape, OFFICEART_FSP_FLIP_V);

        let parent_seq = single_parent_seq(group_reference)
            .ok_or_else(|| "group_parent_missing_or_ambiguous".to_owned())?;
        if document_page_handles.contains(&parent_seq) {
            if !group_shape
                .client_anchor
                .as_ref()
                .is_some_and(anchor_has_unique_geometry_fields)
            {
                return Err("top_group_client_anchor_incomplete".into());
            }
            return Ok((depth, nontrivial_transform));
        }

        if references.get(&parent_seq).and_then(single_raw_type) != Some(RAW_TYPE_GROUP) {
            return Err("group_parent_not_document_page_or_group".into());
        }
        if group_shape.child_anchor.is_none() {
            return Err("nested_group_child_anchor_missing".into());
        }

        current_shape = group_shape;
        current_group_seq = parent_seq;
    }
}

fn increment_reason(result: &mut PubGroupedStoryGeometryCorrelation, reason: &str) {
    *result
        .incomplete_reason_counts
        .entry(reason.to_owned())
        .or_insert(0) += 1;
}

fn increment_recursive_reason(result: &mut PubGroupedStoryGeometryCorrelation, reason: &str) {
    *result
        .recursive_incomplete_reason_counts
        .entry(reason.to_owned())
        .or_insert(0) += 1;
}
