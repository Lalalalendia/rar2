use super::*;
use anyhow::{Context, Result};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Read, Seek, SeekFrom};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PubTableMcldStyleSignatureClass {
    LayoutKeyMissing,
    McldUnavailable,
    RecordMissing,
    StyleRangeAbsent,
    StyleRangeUniform,
    StyleRangeVariesByChild,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PubTableMcldOpaque1dClass {
    AbsentOrNonSingle,
    PartialOrAmbiguous,
    Uniform,
    VariesByChild,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PubTableMcldStyleObservation {
    pub contents_seq_num: u32,
    pub table_cell_count: usize,
    pub layout_key_present: bool,
    pub mcld_record_present: bool,
    pub mcld_child_count: usize,
    pub child_count_matches_cell_count: bool,
    pub signature_class: PubTableMcldStyleSignatureClass,
    pub style_field_child_presence: BTreeMap<String, usize>,
    pub control_field_child_presence: BTreeMap<String, usize>,
    pub opaque_1d_class: Option<PubTableMcldOpaque1dClass>,
    pub opaque_1d_single_child_count: usize,
    pub opaque_1d_distinct_payload_count: usize,
    pub opaque_1d_payload_length_histogram: BTreeMap<usize, usize>,
    /// Arbitrary source-local equality partition for the single uniform
    /// 0x1D/wire0x8A payload. The numeric class has no semantic meaning.
    pub opaque_1d_cross_table_class: Option<u32>,
}

fn field_key(id: u8, wire_type: u8) -> String {
    format!("0x{id:02x}/wire_0x{wire_type:02x}")
}

fn empty_observation(
    contents_seq_num: u32,
    table_cell_count: usize,
    layout_key_present: bool,
    signature_class: PubTableMcldStyleSignatureClass,
) -> PubTableMcldStyleObservation {
    PubTableMcldStyleObservation {
        contents_seq_num,
        table_cell_count,
        layout_key_present,
        mcld_record_present: false,
        mcld_child_count: 0,
        child_count_matches_cell_count: false,
        signature_class,
        style_field_child_presence: BTreeMap::new(),
        control_field_child_presence: BTreeMap::new(),
        opaque_1d_class: None,
        opaque_1d_single_child_count: 0,
        opaque_1d_distinct_payload_count: 0,
        opaque_1d_payload_length_histogram: BTreeMap::new(),
        opaque_1d_cross_table_class: None,
    }
}

/// Research-only TABLE -> MCLD field-presence observation.
///
/// This deliberately exposes no MCLD field values and assigns no table-style,
/// fill, border-side, color, or width semantics to the open 0x1D..0x2C range.
/// The 0x1D/wire0x8A opaque payload is reduced internally to equality class,
/// distinct-count, and byte-length topology only.
pub fn analyze_mature_0x2c_table_mcld_style_fields<R: Read + Seek>(
    mut reader: R,
    source_hash: Sha256Digest,
) -> Result<Vec<PubTableMcldStyleObservation>> {
    reader.seek(SeekFrom::Start(0))?;
    let mut pub_bytes = Vec::new();
    reader.read_to_end(&mut pub_bytes)?;

    let build = build_mature_0x2c_source_graph(Cursor::new(pub_bytes.as_slice()), source_hash)
        .context("build mature source graph for TABLE MCLD style observation")?;

    let quill = pub_cfb::read_stream_reader(Cursor::new(pub_bytes.as_slice()), QUILL_STREAM_PATH)
        .with_context(|| {
        format!("read {QUILL_STREAM_PATH} for TABLE MCLD style observation")
    })?;
    let quill_stream = StreamPath(QUILL_STREAM_PATH.into());
    let catalog = parse_confirmed_story_catalog(quill_stream.clone(), &quill)
        .context("parse Quill story catalog for TABLE MCLD style observation")?;
    let mcld = match parse_bounded_mcld(quill_stream, &quill, &catalog.descriptor_nodes) {
        Ok(mcld) => Some(mcld),
        Err(QuillMcldReadError::MissingMcldDescriptor) => None,
        Err(error) => return Err(error).context("parse bounded MCLD for TABLE style observation"),
    };

    let mut diagnostic_layout_key_by_seq = BTreeMap::<u32, Option<u32>>::new();
    for diagnostic in &build.diagnostics {
        if let PubBridgeDiagnostic::TableLayoutMetricsUnavailable {
            seq_num,
            layout_key,
            ..
        } = diagnostic
        {
            diagnostic_layout_key_by_seq.insert(*seq_num, *layout_key);
        }
    }

    let mut observations = Vec::new();
    let mut opaque_1d_cross_table_classes = BTreeMap::<Vec<u8>, u32>::new();
    for node in build.graph.nodes.values() {
        let Some(table) = node.payload.table.as_ref() else {
            continue;
        };
        let seq_num = node.payload.contents_seq_num;
        let layout_key = table
            .layout_metrics
            .as_ref()
            .map(|metrics| metrics.story_layout_key)
            .or_else(|| {
                diagnostic_layout_key_by_seq
                    .get(&seq_num)
                    .copied()
                    .flatten()
            });

        let Some(layout_key) = layout_key else {
            observations.push(empty_observation(
                seq_num,
                table.cells.len(),
                false,
                PubTableMcldStyleSignatureClass::LayoutKeyMissing,
            ));
            continue;
        };

        let Some(mcld) = mcld.as_ref() else {
            observations.push(empty_observation(
                seq_num,
                table.cells.len(),
                true,
                PubTableMcldStyleSignatureClass::McldUnavailable,
            ));
            continue;
        };

        let Some(record) = mcld
            .records
            .iter()
            .find(|record| record.record_id == layout_key)
        else {
            observations.push(empty_observation(
                seq_num,
                table.cells.len(),
                true,
                PubTableMcldStyleSignatureClass::RecordMissing,
            ));
            continue;
        };

        let mut style_field_child_presence = BTreeMap::<String, usize>::new();
        let mut control_field_child_presence = BTreeMap::<String, usize>::new();
        let mut child_signatures = BTreeSet::<Vec<String>>::new();
        let mut any_style_field = false;
        let mut opaque_1d_single_child_count = 0_usize;
        let mut opaque_1d_distinct_payloads = BTreeSet::<Vec<u8>>::new();
        let mut opaque_1d_payload_length_histogram = BTreeMap::<usize, usize>::new();

        for child in &record.children {
            let style_signature = child
                .fields
                .iter()
                .filter(|field| (0x1d..=0x2c).contains(&field.id))
                .map(|field| field_key(field.id, field.wire_type))
                .collect::<BTreeSet<_>>();
            any_style_field |= !style_signature.is_empty();
            for key in &style_signature {
                *style_field_child_presence.entry(key.clone()).or_default() += 1;
            }
            child_signatures.insert(style_signature.into_iter().collect());

            let control_signature = child
                .fields
                .iter()
                .filter(|field| (0x04..=0x09).contains(&field.id))
                .map(|field| field_key(field.id, field.wire_type))
                .collect::<BTreeSet<_>>();
            for key in control_signature {
                *control_field_child_presence.entry(key).or_default() += 1;
            }

            let opaque_1d_payloads = child
                .fields
                .iter()
                .filter(|field| field.id == 0x1d && field.wire_type == 0x8a)
                .filter_map(|field| match &field.value {
                    pub_quill::QuillMcldFieldValue::OpaqueNested(payload) => Some(payload),
                    _ => None,
                })
                .collect::<Vec<_>>();
            if let [payload] = opaque_1d_payloads.as_slice() {
                opaque_1d_single_child_count += 1;
                opaque_1d_distinct_payloads.insert((*payload).clone());
                *opaque_1d_payload_length_histogram
                    .entry(payload.len())
                    .or_default() += 1;
            }
        }

        let signature_class = if !any_style_field {
            PubTableMcldStyleSignatureClass::StyleRangeAbsent
        } else if child_signatures.len() == 1 {
            PubTableMcldStyleSignatureClass::StyleRangeUniform
        } else {
            PubTableMcldStyleSignatureClass::StyleRangeVariesByChild
        };
        let opaque_1d_class = if opaque_1d_single_child_count == 0 {
            PubTableMcldOpaque1dClass::AbsentOrNonSingle
        } else if opaque_1d_single_child_count != record.children.len() {
            PubTableMcldOpaque1dClass::PartialOrAmbiguous
        } else if opaque_1d_distinct_payloads.len() == 1 {
            PubTableMcldOpaque1dClass::Uniform
        } else {
            PubTableMcldOpaque1dClass::VariesByChild
        };

        let opaque_1d_cross_table_class = if opaque_1d_class == PubTableMcldOpaque1dClass::Uniform {
            let payload = opaque_1d_distinct_payloads
                .iter()
                .next()
                .expect("uniform 0x1D class must retain one payload");
            if let Some(class) = opaque_1d_cross_table_classes.get(payload) {
                Some(*class)
            } else {
                let class = u32::try_from(opaque_1d_cross_table_classes.len())
                    .context("TABLE 0x1D equality class count does not fit u32")?;
                opaque_1d_cross_table_classes.insert(payload.clone(), class);
                Some(class)
            }
        } else {
            None
        };

        observations.push(PubTableMcldStyleObservation {
            contents_seq_num: seq_num,
            table_cell_count: table.cells.len(),
            layout_key_present: true,
            mcld_record_present: true,
            mcld_child_count: record.children.len(),
            child_count_matches_cell_count: record.children.len() == table.cells.len(),
            signature_class,
            style_field_child_presence,
            control_field_child_presence,
            opaque_1d_class: Some(opaque_1d_class),
            opaque_1d_single_child_count,
            opaque_1d_distinct_payload_count: opaque_1d_distinct_payloads.len(),
            opaque_1d_payload_length_histogram,
            opaque_1d_cross_table_class,
        });
    }

    observations.sort_by_key(|observation| observation.contents_seq_num);
    Ok(observations)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PubTableOfficeArtOwnerJoinClass {
    Missing,
    Unique,
    Ambiguous,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PubTableDefaultStyleObservation {
    pub contents_seq_num: u32,
    pub table_field_presence: BTreeSet<String>,
    pub table_tail_field_presence: BTreeSet<String>,
    pub officeart_owner_join_class: PubTableOfficeArtOwnerJoinClass,
    pub owner_fopt_property_presence: BTreeSet<String>,
    pub owner_paint_family_property_presence: BTreeSet<String>,
}

fn contents_presence_key(id: u16, wire_type: u8) -> String {
    format!("0x{id:03x}/wire_0x{wire_type:02x}")
}

fn table_default_style_known_field(id: u16) -> bool {
    matches!(
        id,
        FIELD_STORY_ID
            | table_bridge::TABLE_NUM_ROWS_ID
            | table_bridge::TABLE_NUM_COLUMNS_ID
            | table_bridge::TABLE_WIDTH_ID
            | table_bridge::TABLE_HEIGHT_ID
            | table_bridge::TABLE_CELLS_SEQ_NUM_ID
            | table_bridge::TABLE_ROWCOL_ARRAY_ID
    )
}

fn scan_table_tail_field_presence(
    contents: &[u8],
    chunk: &Contents0x2cChunk,
) -> Result<BTreeSet<String>> {
    let Some(tail) = chunk.unsupported_tail.as_ref() else {
        return Ok(BTreeSet::new());
    };

    let start = usize::try_from(tail.offset).context("TABLE tail offset does not fit usize")?;
    let len = usize::try_from(tail.len).context("TABLE tail length does not fit usize")?;
    let end = start
        .checked_add(len)
        .filter(|end| *end <= contents.len())
        .context("TABLE tail is outside Contents stream")?;

    let mut position = start;
    let mut presence = BTreeSet::new();
    while position < end {
        if end - position < 2 {
            anyhow::bail!("truncated TABLE tail field tag at {position}");
        }
        let raw_tag = [contents[position], contents[position + 1]];
        let (id, wire_type) = pub_contents::decode_packed_field_tag(raw_tag);
        position += 2;

        if !table_default_style_known_field(id) {
            presence.insert(contents_presence_key(id, wire_type));
        }

        match wire_type {
            0x00 | 0x08 | 0x78 => {}
            0x10 | 0x18 => {
                position = position
                    .checked_add(2)
                    .filter(|next| *next <= end)
                    .context("TABLE tail u16 exceeds bounded tail")?;
            }
            0x20 | 0x58 | 0x68 | 0x70 | 0xB8 => {
                position = position
                    .checked_add(4)
                    .filter(|next| *next <= end)
                    .context("TABLE tail u32 exceeds bounded tail")?;
            }
            0x28 => {
                position = position
                    .checked_add(8)
                    .filter(|next| *next <= end)
                    .context("TABLE tail fixed8 exceeds bounded tail")?;
            }
            0x38 => {
                position = position
                    .checked_add(16)
                    .filter(|next| *next <= end)
                    .context("TABLE tail fixed16 exceeds bounded tail")?;
            }
            0x48 => {
                position = position
                    .checked_add(24)
                    .filter(|next| *next <= end)
                    .context("TABLE tail fixed24 exceeds bounded tail")?;
            }
            0x80 | 0x88 | 0x90 | 0x98 | 0xA0 | 0xC0 => {
                if end - position < 4 {
                    anyhow::bail!("truncated TABLE tail variable length at {position}");
                }
                let declared_length = u32::from_le_bytes([
                    contents[position],
                    contents[position + 1],
                    contents[position + 2],
                    contents[position + 3],
                ]);
                if declared_length < 4 {
                    anyhow::bail!("invalid TABLE tail variable length {declared_length}");
                }
                let declared_length = usize::try_from(declared_length)
                    .context("TABLE tail length does not fit usize")?;
                position = position
                    .checked_add(declared_length)
                    .filter(|next| *next <= end)
                    .context("TABLE tail variable field exceeds bounded tail")?;
            }
            other => {
                anyhow::bail!("unsupported TABLE tail wire type 0x{other:02x} for field 0x{id:03x}")
            }
        }
    }

    Ok(presence)
}

/// Research-only TABLE-level/default-style presence census.
///
/// This reports only field/property identifiers and wire types. It deliberately
/// emits no scalar/reference values and does not assign table-style, grid,
/// border, fill, color, or width semantics.
pub fn analyze_mature_0x2c_table_default_style_fields<R: Read + Seek>(
    mut reader: R,
    source_hash: Sha256Digest,
) -> Result<Vec<PubTableDefaultStyleObservation>> {
    reader.seek(SeekFrom::Start(0))?;
    let mut pub_bytes = Vec::new();
    reader.read_to_end(&mut pub_bytes)?;

    let build = build_mature_0x2c_source_graph(Cursor::new(pub_bytes.as_slice()), source_hash)
        .context("build mature source graph for TABLE default-style observation")?;

    let contents =
        pub_cfb::read_stream_reader(Cursor::new(pub_bytes.as_slice()), CONTENTS_STREAM_PATH)
            .with_context(|| {
                format!("read {CONTENTS_STREAM_PATH} for TABLE default-style observation")
            })?;
    let contents_stream = StreamPath(CONTENTS_STREAM_PATH.into());
    let header = parse_0x2c_header(contents_stream.clone(), &contents)
        .context("parse Contents header for TABLE default-style observation")?;
    let trailer = parse_confirmed_0x2c_trailer_root(&contents, &header)
        .context("parse Contents trailer for TABLE default-style observation")?;
    let references = build_reference_index(&contents, &trailer.directory)?;

    let escher = pub_cfb::read_stream_reader(Cursor::new(pub_bytes.as_slice()), ESCHER_STREAM_PATH)
        .with_context(|| {
            format!("read {ESCHER_STREAM_PATH} for TABLE default-style observation")
        })?;
    let inventory = inspect_sp_containers(StreamPath(ESCHER_STREAM_PATH.into()), &escher)
        .context("inspect OfficeArt owners for TABLE default-style observation")?;
    let escher_by_seq = index_escher_by_contents_seq(&inventory);

    let mut observations = Vec::new();
    for node in build.graph.nodes.values() {
        if node.payload.table.is_none() {
            continue;
        }

        let seq_num = node.payload.contents_seq_num;
        let reference = references
            .get(&seq_num)
            .with_context(|| format!("TABLE Contents reference missing for seq {seq_num}"))?;
        if single_raw_type(reference) != Some(table_bridge::RAW_TYPE_TABLE) {
            continue;
        }
        let chunk = chunk_for_reference(contents_stream.clone(), &contents, reference)?;

        let table_field_presence = chunk
            .fields
            .iter()
            .filter(|field| !table_default_style_known_field(field.id))
            .map(|field| contents_presence_key(field.id, field.block_type))
            .collect::<BTreeSet<_>>();
        let table_tail_field_presence = scan_table_tail_field_presence(&contents, &chunk)?;

        let owner_matches = escher_by_seq
            .get(&seq_num)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let officeart_owner_join_class = match owner_matches {
            [] => PubTableOfficeArtOwnerJoinClass::Missing,
            [_] => PubTableOfficeArtOwnerJoinClass::Unique,
            _ => PubTableOfficeArtOwnerJoinClass::Ambiguous,
        };
        let mut owner_fopt_property_presence = BTreeSet::new();
        let mut owner_paint_family_property_presence = BTreeSet::new();
        if let [owner_index] = owner_matches {
            let owner = &inventory.shapes[*owner_index];
            for property in owner
                .fopts
                .iter()
                .flat_map(|record| record.properties.iter())
            {
                let property_id = property.property_id();
                owner_fopt_property_presence.insert(format!("0x{property_id:04x}"));
                if (0x0180..=0x01ff).contains(&property_id) {
                    owner_paint_family_property_presence.insert(format!("0x{property_id:04x}"));
                }
            }
        }

        observations.push(PubTableDefaultStyleObservation {
            contents_seq_num: seq_num,
            table_field_presence,
            table_tail_field_presence,
            officeart_owner_join_class,
            owner_fopt_property_presence,
            owner_paint_family_property_presence,
        });
    }

    observations.sort_by_key(|observation| observation.contents_seq_num);
    Ok(observations)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PubTablePublicationDefaultObservation {
    pub contents_seq_num: u32,
    pub publication_target_presence: BTreeSet<String>,
    pub publication_target_raw_type_presence: BTreeSet<String>,
    pub relation_class: String,
}

fn publication_default_field_name(id: u16) -> Option<&'static str> {
    match id {
        0x14 => Some("oh_morphing_context"),
        0x18 => Some("oh_gallery"),
        0x19 => Some("oh_fancy_borders"),
        0x22 => Some("oh_color_scheme"),
        _ => None,
    }
}

fn raw_u32_value(field: &RawContentsBlock) -> Option<u32> {
    match &field.body {
        RawContentsBlockBody::U32 { value, .. } => Some(*value),
        _ => None,
    }
}

/// Research-only publication/default topology screen for TABLEs.
///
/// The four DOCUMENT fields are named only because their Microsoft OplPub
/// descriptors are already independently grounded. This probe emits target
/// raw-type classes and TABLE relation classes only; it never emits reference
/// values/handles or assigns table-style semantics to any target object.
pub fn analyze_mature_0x2c_table_publication_default_topology<R: Read + Seek>(
    mut reader: R,
) -> Result<Vec<PubTablePublicationDefaultObservation>> {
    reader.seek(SeekFrom::Start(0))?;
    let mut pub_bytes = Vec::new();
    reader.read_to_end(&mut pub_bytes)?;

    let contents =
        pub_cfb::read_stream_reader(Cursor::new(pub_bytes.as_slice()), CONTENTS_STREAM_PATH)
            .with_context(|| {
                format!("read {CONTENTS_STREAM_PATH} for TABLE publication-default topology")
            })?;
    let contents_stream = StreamPath(CONTENTS_STREAM_PATH.into());
    let header = parse_0x2c_header(contents_stream.clone(), &contents)
        .context("parse Contents header for TABLE publication-default topology")?;
    let trailer = parse_confirmed_0x2c_trailer_root(&contents, &header)
        .context("parse Contents trailer for TABLE publication-default topology")?;
    let references = build_reference_index(&contents, &trailer.directory)?;

    let document_reference =
        unique_reference_by_raw_type(&references, RAW_TYPE_DOCUMENT, "DOCUMENT")?;
    let document_seq = seq_u32(document_reference.seq_num)?;
    let document_chunk =
        chunk_for_reference(contents_stream.clone(), &contents, document_reference)?;

    let mut target_seq_to_name = BTreeMap::<u32, &'static str>::new();
    let mut publication_target_presence = BTreeSet::<String>::new();
    let mut publication_target_raw_type_presence = BTreeSet::<String>::new();

    for field in &document_chunk.fields {
        let Some(name) = publication_default_field_name(field.id) else {
            continue;
        };
        publication_target_presence.insert(name.to_owned());
        let Some(target_seq) = raw_u32_value(field) else {
            publication_target_raw_type_presence.insert(format!("{name}:non_u32"));
            continue;
        };
        let target_class = references
            .get(&target_seq)
            .and_then(single_raw_type)
            .map(|raw_type| format!("{name}:raw_0x{raw_type:02x}"))
            .unwrap_or_else(|| format!("{name}:unresolved"));
        publication_target_raw_type_presence.insert(target_class);
        target_seq_to_name.insert(target_seq, name);
    }

    let target_seqs = target_seq_to_name.keys().copied().collect::<BTreeSet<_>>();
    let mut observations = Vec::new();
    for reference in references.values() {
        if single_raw_type(reference) != Some(table_bridge::RAW_TYPE_TABLE) {
            continue;
        }
        let seq_num = seq_u32(reference.seq_num)?;
        let parent_seq = single_parent_seq(reference);
        let chunk = chunk_for_reference(contents_stream.clone(), &contents, reference)?;

        let direct_parent_target = parent_seq
            .filter(|parent| target_seqs.contains(parent))
            .and_then(|parent| target_seq_to_name.get(&parent).copied());

        let decoded_target_refs = chunk
            .fields
            .iter()
            .filter_map(raw_u32_value)
            .filter_map(|value| target_seq_to_name.get(&value).copied())
            .collect::<BTreeSet<_>>();

        let relation_class = if let Some(name) = direct_parent_target {
            format!("direct_parent:{name}")
        } else if decoded_target_refs.len() == 1 {
            format!(
                "decoded_reference:{}",
                decoded_target_refs
                    .iter()
                    .next()
                    .copied()
                    .unwrap_or("unknown")
            )
        } else if decoded_target_refs.len() > 1 {
            "decoded_reference:multiple".to_owned()
        } else if parent_seq == Some(document_seq) {
            "document_child_no_target_reference".to_owned()
        } else {
            "no_target_reference".to_owned()
        };

        observations.push(PubTablePublicationDefaultObservation {
            contents_seq_num: seq_num,
            publication_target_presence: publication_target_presence.clone(),
            publication_target_raw_type_presence: publication_target_raw_type_presence.clone(),
            relation_class,
        });
    }

    observations.sort_by_key(|observation| observation.contents_seq_num);
    Ok(observations)
}
