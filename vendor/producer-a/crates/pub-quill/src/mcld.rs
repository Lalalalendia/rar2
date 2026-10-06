use crate::QuillDescriptorListNode;
use pub_core::{Decoded, RawSpan, StreamPath};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fmt;

const MCLD: [u8; 4] = *b"MCLD";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuillMcldChunk {
    pub source: RawSpan,
    pub record_count: Decoded<u32>,
    pub record_id_count: Decoded<u32>,
    pub record_ids: Vec<Decoded<u32>>,
    pub records: Vec<QuillMcldRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuillMcldRecord {
    pub record_id: u32,
    pub source: RawSpan,
    pub header_source: RawSpan,
    pub child_count: Decoded<u32>,
    pub children: Vec<QuillMcldChild>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuillMcldChild {
    pub source: RawSpan,
    pub fields: Vec<QuillMcldField>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuillMcldField {
    pub id: u8,
    pub wire_type: u8,
    pub source: RawSpan,
    pub value: QuillMcldFieldValue,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum QuillMcldFieldValue {
    U32(u32),
    U16(u16),
    True,
    False,
    OpaqueNested(Vec<u8>),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuillMcldConsensusU32 {
    pub value: u32,
    pub sources: Vec<RawSpan>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuillMcldTableMetrics {
    pub record_id: u32,
    pub child_count: u32,
    pub cell_width_emu: QuillMcldConsensusU32,
    pub row_pitch_emu: QuillMcldConsensusU32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuillMcldUniformTextInset {
    pub record_id: u32,
    pub inset_emu: u32,
    pub sources: Vec<RawSpan>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuillMcldTextInsets {
    pub record_id: u32,
    pub top_emu: u32,
    pub left_emu: u32,
    pub bottom_emu: u32,
    pub right_emu: u32,
    pub sources: Vec<RawSpan>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuillMcldVerticalAlignment {
    Top,
    Center,
    Bottom,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuillMcldTextFrameVerticalAlignment {
    pub record_id: u32,
    pub alignment: QuillMcldVerticalAlignment,
    pub source: RawSpan,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuillMcldTableVerticalAlignment {
    pub record_id: u32,
    pub alignment: QuillMcldVerticalAlignment,
    pub sources: Vec<RawSpan>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuillMcldReadError {
    MissingMcldDescriptor,
    DuplicateMcldDescriptor,
    ChunkOutOfBounds {
        offset: u32,
        length: u32,
        stream_len: usize,
    },
    TooShort {
        offset: usize,
        requested: usize,
        available: usize,
    },
    RecordCountMismatch {
        record_count: u32,
        record_id_count: u32,
    },
    DuplicateRecordId {
        record_id: u32,
    },
    InvalidSizedBlock {
        offset: u64,
        declared_size: u32,
        minimum_size: u32,
    },
    SizedBlockOutOfBounds {
        offset: u64,
        declared_size: u32,
        remaining: usize,
    },
    UnsupportedFieldType {
        record_id: u32,
        child_index: u32,
        field_id: u8,
        wire_type: u8,
    },
    InvalidNestedSize {
        record_id: u32,
        child_index: u32,
        field_id: u8,
        declared_size: u32,
    },
    TrailingChunkBytes {
        remaining: usize,
    },
    RecordIdNotFound {
        record_id: u32,
    },
    UnexpectedChildCount {
        record_id: u32,
        expected: u32,
        found: u32,
    },
    MissingRequiredField {
        record_id: u32,
        child_index: u32,
        field_id: u8,
    },
    DuplicateRequiredField {
        record_id: u32,
        child_index: u32,
        field_id: u8,
    },
    RequiredFieldWrongType {
        record_id: u32,
        child_index: u32,
        field_id: u8,
        wire_type: u8,
    },
    NonUniformRequiredField {
        record_id: u32,
        field_id: u8,
        expected: u32,
        found: u32,
        child_index: u32,
    },
    UnsupportedVerticalAlignmentValue {
        record_id: u32,
        value: u32,
    },
}

impl fmt::Display for QuillMcldReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for QuillMcldReadError {}

/// Parses the bounded modern MCLD record/child grammar observed in grounded
/// Publisher fixtures.
///
/// The outer layout is:
/// - u32 record_count
/// - u32 record_id_count
/// - u32 record_ids[record_id_count]
/// - record bodies in ordinal order, keyed by the parallel record_id table
///
/// Each record begins with a sized header block, followed by u32 child_count.
/// Each child is another sized block. Sizes include their own u32 size field.
/// Child property types supported here are only the grounded wire forms needed
/// to walk the complete modern Sample payload without byte-pattern scanning.
pub fn parse_bounded_mcld(
    stream: StreamPath,
    quill_bytes: &[u8],
    descriptor_nodes: &[QuillDescriptorListNode],
) -> Result<QuillMcldChunk, QuillMcldReadError> {
    let descriptor = unique_mcld_descriptor(descriptor_nodes)?;
    let start = usize::try_from(descriptor.data_offset.value).map_err(|_| {
        QuillMcldReadError::ChunkOutOfBounds {
            offset: descriptor.data_offset.value,
            length: descriptor.data_length.value,
            stream_len: quill_bytes.len(),
        }
    })?;
    let len = usize::try_from(descriptor.data_length.value).map_err(|_| {
        QuillMcldReadError::ChunkOutOfBounds {
            offset: descriptor.data_offset.value,
            length: descriptor.data_length.value,
            stream_len: quill_bytes.len(),
        }
    })?;
    let end = start
        .checked_add(len)
        .ok_or(QuillMcldReadError::ChunkOutOfBounds {
            offset: descriptor.data_offset.value,
            length: descriptor.data_length.value,
            stream_len: quill_bytes.len(),
        })?;
    let bytes = quill_bytes
        .get(start..end)
        .ok_or(QuillMcldReadError::ChunkOutOfBounds {
            offset: descriptor.data_offset.value,
            length: descriptor.data_length.value,
            stream_len: quill_bytes.len(),
        })?;

    let mut cursor = Cursor::new(stream.clone(), bytes, start);
    let record_count = cursor.read_u32()?;
    let record_id_count = cursor.read_u32()?;
    if record_count.value != record_id_count.value {
        return Err(QuillMcldReadError::RecordCountMismatch {
            record_count: record_count.value,
            record_id_count: record_id_count.value,
        });
    }

    let mut record_ids = Vec::with_capacity(record_id_count.value as usize);
    let mut seen = BTreeSet::new();
    for _ in 0..record_id_count.value {
        let record_id = cursor.read_u32()?;
        if !seen.insert(record_id.value) {
            return Err(QuillMcldReadError::DuplicateRecordId {
                record_id: record_id.value,
            });
        }
        record_ids.push(record_id);
    }

    let mut records = Vec::with_capacity(record_count.value as usize);
    for record_id in &record_ids {
        records.push(parse_record(&mut cursor, record_id.value)?);
    }

    if cursor.remaining() != 0 {
        return Err(QuillMcldReadError::TrailingChunkBytes {
            remaining: cursor.remaining(),
        });
    }

    Ok(QuillMcldChunk {
        source: span(stream, start, len),
        record_count,
        record_id_count,
        record_ids,
        records,
    })
}

/// Promotes only the two grounded uniform table metrics from one keyed MCLD
/// record. Every child must contain exactly one u32 field 0x04 and 0x05 and all
/// children must agree; disagreement is an error, never a coercion.
/// Promotes only a one-child TextFrame record whose four grounded MCLD
/// text-inset fields (0x06..0x09) are present as u32 and agree exactly.
///
/// This is intentionally narrower than the known asymmetric Publisher grammar:
/// the exact within-axis side permutation remains separate research authority.
/// A non-uniform record therefore stays unprojected rather than being guessed.
/// Promotes one ordinary TextFrame's four grounded MCLD text insets.
///
/// Natural asymmetric Publisher evidence closes the stored side order as:
/// 0x06=Top, 0x07=Left, 0x08=Bottom, 0x09=Right. Each field must be one
/// exact u32 on the record's single child; malformed or ambiguous state fails
/// closed. The older uniform helper remains as the stricter table/legacy gate.
pub fn bounded_mcld_text_insets(
    mcld: &QuillMcldChunk,
    record_id: u32,
) -> Result<QuillMcldTextInsets, QuillMcldReadError> {
    let record = mcld
        .records
        .iter()
        .find(|record| record.record_id == record_id)
        .ok_or(QuillMcldReadError::RecordIdNotFound { record_id })?;
    if record.children.len() != 1 {
        return Err(QuillMcldReadError::UnexpectedChildCount {
            record_id,
            expected: 1,
            found: u32::try_from(record.children.len()).unwrap_or(u32::MAX),
        });
    }

    let child = &record.children[0];
    let (top_emu, top_source) = required_u32_field(record_id, 0, child, 0x06)?;
    let (left_emu, left_source) = required_u32_field(record_id, 0, child, 0x07)?;
    let (bottom_emu, bottom_source) = required_u32_field(record_id, 0, child, 0x08)?;
    let (right_emu, right_source) = required_u32_field(record_id, 0, child, 0x09)?;

    Ok(QuillMcldTextInsets {
        record_id,
        top_emu,
        left_emu,
        bottom_emu,
        right_emu,
        sources: vec![top_source, left_source, bottom_source, right_source],
    })
}

pub fn bounded_mcld_uniform_text_inset(
    mcld: &QuillMcldChunk,
    record_id: u32,
) -> Result<QuillMcldUniformTextInset, QuillMcldReadError> {
    let record = mcld
        .records
        .iter()
        .find(|record| record.record_id == record_id)
        .ok_or(QuillMcldReadError::RecordIdNotFound { record_id })?;
    if record.children.len() != 1 {
        return Err(QuillMcldReadError::UnexpectedChildCount {
            record_id,
            expected: 1,
            found: u32::try_from(record.children.len()).unwrap_or(u32::MAX),
        });
    }

    let child = &record.children[0];
    let values = [0x06_u8, 0x07, 0x08, 0x09]
        .into_iter()
        .map(|field_id| required_u32_field(record_id, 0, child, field_id))
        .collect::<Result<Vec<_>, _>>()?;
    let expected = values[0].0;
    for (index, (found, _)) in values.iter().enumerate().skip(1) {
        if *found != expected {
            return Err(QuillMcldReadError::NonUniformRequiredField {
                record_id,
                field_id: [0x06_u8, 0x07, 0x08, 0x09][index],
                expected,
                found: *found,
                child_index: 0,
            });
        }
    }

    Ok(QuillMcldUniformTextInset {
        record_id,
        inset_emu: expected,
        sources: values.into_iter().map(|(_, source)| source).collect(),
    })
}

/// Promotes one record-level TABLE text inset only when every child is
/// individually symmetric and the entire multi-child record is unanimous.
///
/// This deliberately avoids any child-to-cell ordinal claim: when every child
/// carries the same exact inset, mapping order is irrelevant. Missing,
/// duplicate, wrong-typed, asymmetric, or cross-child-disagreeing state fails
/// closed.
pub fn bounded_mcld_table_uniform_text_inset(
    mcld: &QuillMcldChunk,
    record_id: u32,
) -> Result<QuillMcldUniformTextInset, QuillMcldReadError> {
    let record = mcld
        .records
        .iter()
        .find(|record| record.record_id == record_id)
        .ok_or(QuillMcldReadError::RecordIdNotFound { record_id })?;

    if record.children.is_empty() {
        return Err(QuillMcldReadError::MissingRequiredField {
            record_id,
            child_index: 0,
            field_id: 0x06,
        });
    }

    let mut expected_record_inset = None;
    let mut sources = Vec::with_capacity(record.children.len() * 4);

    for (child_index, child) in record.children.iter().enumerate() {
        let child_index = u32::try_from(child_index).unwrap_or(u32::MAX);
        let values = [0x06_u8, 0x07, 0x08, 0x09]
            .into_iter()
            .map(|field_id| required_u32_field(record_id, child_index, child, field_id))
            .collect::<Result<Vec<_>, _>>()?;
        let child_inset = values[0].0;

        for (index, (found, _)) in values.iter().enumerate().skip(1) {
            if *found != child_inset {
                return Err(QuillMcldReadError::NonUniformRequiredField {
                    record_id,
                    field_id: [0x06_u8, 0x07, 0x08, 0x09][index],
                    expected: child_inset,
                    found: *found,
                    child_index,
                });
            }
        }

        match expected_record_inset {
            None => expected_record_inset = Some(child_inset),
            Some(expected) if expected == child_inset => {}
            Some(expected) => {
                return Err(QuillMcldReadError::NonUniformRequiredField {
                    record_id,
                    field_id: 0x06,
                    expected,
                    found: child_inset,
                    child_index,
                });
            }
        }

        sources.extend(values.into_iter().map(|(_, source)| source));
    }

    Ok(QuillMcldUniformTextInset {
        record_id,
        inset_emu: expected_record_inset.expect("non-empty child cohort"),
        sources,
    })
}

/// Promotes a unanimous multi-child TABLE vertical-alignment field.
///
/// Every child must carry exactly one u32 field 0x18, every value must be one
/// of the already-grounded Publisher values 0=Top, 1=Center, 2=Bottom, and the
/// complete child cohort must agree. Mixed or incomplete records fail closed.
pub fn bounded_mcld_table_uniform_vertical_alignment(
    mcld: &QuillMcldChunk,
    record_id: u32,
) -> Result<QuillMcldTableVerticalAlignment, QuillMcldReadError> {
    let record = mcld
        .records
        .iter()
        .find(|record| record.record_id == record_id)
        .ok_or(QuillMcldReadError::RecordIdNotFound { record_id })?;

    if record.children.is_empty() {
        return Err(QuillMcldReadError::MissingRequiredField {
            record_id,
            child_index: 0,
            field_id: 0x18,
        });
    }

    let mut expected_raw = None;
    let mut expected_alignment = None;
    let mut sources = Vec::with_capacity(record.children.len());

    for (child_index, child) in record.children.iter().enumerate() {
        let child_index = u32::try_from(child_index).unwrap_or(u32::MAX);
        let (value, source) = required_u32_field(record_id, child_index, child, 0x18)?;
        let alignment = match value {
            0 => QuillMcldVerticalAlignment::Top,
            1 => QuillMcldVerticalAlignment::Center,
            2 => QuillMcldVerticalAlignment::Bottom,
            value => {
                return Err(QuillMcldReadError::UnsupportedVerticalAlignmentValue {
                    record_id,
                    value,
                });
            }
        };

        match expected_raw {
            None => {
                expected_raw = Some(value);
                expected_alignment = Some(alignment);
            }
            Some(expected) if expected == value => {}
            Some(expected) => {
                return Err(QuillMcldReadError::NonUniformRequiredField {
                    record_id,
                    field_id: 0x18,
                    expected,
                    found: value,
                    child_index,
                });
            }
        }
        sources.push(source);
    }

    Ok(QuillMcldTableVerticalAlignment {
        record_id,
        alignment: expected_alignment.expect("non-empty child cohort"),
        sources,
    })
}

/// Promotes the confirmed ordinary-TextFrame MCLD vertical-alignment field.
///
/// The admitted profile is deliberately narrow: one keyed child, exactly one
/// u32 field 0x18, and only the corpus-proven Publisher values
/// 0=Top, 1=Center, 2=Bottom.
pub fn bounded_mcld_text_frame_vertical_alignment(
    mcld: &QuillMcldChunk,
    record_id: u32,
) -> Result<QuillMcldTextFrameVerticalAlignment, QuillMcldReadError> {
    let record = mcld
        .records
        .iter()
        .find(|record| record.record_id == record_id)
        .ok_or(QuillMcldReadError::RecordIdNotFound { record_id })?;
    if record.children.len() != 1 {
        return Err(QuillMcldReadError::UnexpectedChildCount {
            record_id,
            expected: 1,
            found: u32::try_from(record.children.len()).unwrap_or(u32::MAX),
        });
    }

    let (value, source) = required_u32_field(record_id, 0, &record.children[0], 0x18)?;
    let alignment = match value {
        0 => QuillMcldVerticalAlignment::Top,
        1 => QuillMcldVerticalAlignment::Center,
        2 => QuillMcldVerticalAlignment::Bottom,
        value => {
            return Err(QuillMcldReadError::UnsupportedVerticalAlignmentValue { record_id, value });
        }
    };

    Ok(QuillMcldTextFrameVerticalAlignment {
        record_id,
        alignment,
        source,
    })
}

pub fn bounded_mcld_table_metrics(
    mcld: &QuillMcldChunk,
    record_id: u32,
) -> Result<QuillMcldTableMetrics, QuillMcldReadError> {
    let record = mcld
        .records
        .iter()
        .find(|record| record.record_id == record_id)
        .ok_or(QuillMcldReadError::RecordIdNotFound { record_id })?;

    let mut widths = Vec::with_capacity(record.children.len());
    let mut pitches = Vec::with_capacity(record.children.len());

    for (child_index, child) in record.children.iter().enumerate() {
        let child_index = u32::try_from(child_index).unwrap_or(u32::MAX);
        widths.push(required_u32_field(record_id, child_index, child, 0x04)?);
        pitches.push(required_u32_field(record_id, child_index, child, 0x05)?);
    }

    let width = consensus(record_id, 0x04, &widths)?;
    let pitch = consensus(record_id, 0x05, &pitches)?;

    Ok(QuillMcldTableMetrics {
        record_id,
        child_count: record.child_count.value,
        cell_width_emu: width,
        row_pitch_emu: pitch,
    })
}

fn unique_mcld_descriptor(
    nodes: &[QuillDescriptorListNode],
) -> Result<&crate::QuillChunkDescriptor, QuillMcldReadError> {
    let mut found = nodes
        .iter()
        .flat_map(|node| node.descriptors.iter())
        .filter(|descriptor| descriptor.name.value == MCLD);
    let first = found
        .next()
        .ok_or(QuillMcldReadError::MissingMcldDescriptor)?;
    if found.next().is_some() {
        return Err(QuillMcldReadError::DuplicateMcldDescriptor);
    }
    Ok(first)
}

fn parse_record(
    cursor: &mut Cursor<'_>,
    record_id: u32,
) -> Result<QuillMcldRecord, QuillMcldReadError> {
    let record_start = cursor.position();
    let header_source = cursor.skip_sized_block(4)?;
    let child_count = cursor.read_u32()?;
    let mut children = Vec::with_capacity(child_count.value as usize);

    for child_index in 0..child_count.value {
        children.push(parse_child(cursor, record_id, child_index)?);
    }

    let record_end = cursor.position();
    Ok(QuillMcldRecord {
        record_id,
        source: span(
            cursor.stream.clone(),
            record_start,
            record_end - record_start,
        ),
        header_source,
        child_count,
        children,
    })
}

fn parse_child(
    cursor: &mut Cursor<'_>,
    record_id: u32,
    child_index: u32,
) -> Result<QuillMcldChild, QuillMcldReadError> {
    let child_start = cursor.position();
    let declared_size = cursor.peek_u32()?;
    if declared_size < 4 {
        return Err(QuillMcldReadError::InvalidSizedBlock {
            offset: child_start as u64,
            declared_size,
            minimum_size: 4,
        });
    }
    let declared_size =
        usize::try_from(declared_size).map_err(|_| QuillMcldReadError::SizedBlockOutOfBounds {
            offset: child_start as u64,
            declared_size,
            remaining: cursor.remaining(),
        })?;
    if declared_size > cursor.remaining() {
        return Err(QuillMcldReadError::SizedBlockOutOfBounds {
            offset: child_start as u64,
            declared_size: u32::try_from(declared_size).unwrap_or(u32::MAX),
            remaining: cursor.remaining(),
        });
    }

    let child_end = child_start + declared_size;
    cursor.read_u32()?;
    let mut fields = Vec::new();
    while cursor.position() < child_end {
        fields.push(parse_field(cursor, record_id, child_index, child_end)?);
    }
    if cursor.position() != child_end {
        return Err(QuillMcldReadError::SizedBlockOutOfBounds {
            offset: child_start as u64,
            declared_size: u32::try_from(declared_size).unwrap_or(u32::MAX),
            remaining: cursor.remaining(),
        });
    }

    Ok(QuillMcldChild {
        source: span(cursor.stream.clone(), child_start, declared_size),
        fields,
    })
}

fn parse_field(
    cursor: &mut Cursor<'_>,
    record_id: u32,
    child_index: u32,
    child_end: usize,
) -> Result<QuillMcldField, QuillMcldReadError> {
    let field_start = cursor.position();
    let id = cursor.read_u8_bounded(child_end)?;
    let wire_type = cursor.read_u8_bounded(child_end)?;

    let value = match wire_type {
        0x22 => QuillMcldFieldValue::U32(cursor.read_u32_bounded(child_end)?),
        0x12 | 0x1a => QuillMcldFieldValue::U16(cursor.read_u16_bounded(child_end)?),
        0x0a => QuillMcldFieldValue::True,
        0x02 => QuillMcldFieldValue::False,
        0x8a => {
            let nested_start = cursor.position();
            let declared_size = cursor.read_u32_bounded(child_end)?;
            if declared_size < 4 {
                return Err(QuillMcldReadError::InvalidNestedSize {
                    record_id,
                    child_index,
                    field_id: id,
                    declared_size,
                });
            }
            let payload_len = usize::try_from(declared_size - 4).map_err(|_| {
                QuillMcldReadError::InvalidNestedSize {
                    record_id,
                    child_index,
                    field_id: id,
                    declared_size,
                }
            })?;
            let payload = cursor.read_bytes_bounded(child_end, payload_len)?.to_vec();
            let _nested_source = span(
                cursor.stream.clone(),
                nested_start,
                usize::try_from(declared_size).unwrap_or(usize::MAX),
            );
            QuillMcldFieldValue::OpaqueNested(payload)
        }
        _ => {
            return Err(QuillMcldReadError::UnsupportedFieldType {
                record_id,
                child_index,
                field_id: id,
                wire_type,
            });
        }
    };

    let field_end = cursor.position();
    Ok(QuillMcldField {
        id,
        wire_type,
        source: span(cursor.stream.clone(), field_start, field_end - field_start),
        value,
    })
}

fn required_u32_field(
    record_id: u32,
    child_index: u32,
    child: &QuillMcldChild,
    field_id: u8,
) -> Result<(u32, RawSpan), QuillMcldReadError> {
    let mut found = child.fields.iter().filter(|field| field.id == field_id);
    let field = found
        .next()
        .ok_or(QuillMcldReadError::MissingRequiredField {
            record_id,
            child_index,
            field_id,
        })?;
    if found.next().is_some() {
        return Err(QuillMcldReadError::DuplicateRequiredField {
            record_id,
            child_index,
            field_id,
        });
    }
    match field.value {
        QuillMcldFieldValue::U32(value) if field.wire_type == 0x22 => {
            Ok((value, field.source.clone()))
        }
        _ => Err(QuillMcldReadError::RequiredFieldWrongType {
            record_id,
            child_index,
            field_id,
            wire_type: field.wire_type,
        }),
    }
}

fn consensus(
    record_id: u32,
    field_id: u8,
    values: &[(u32, RawSpan)],
) -> Result<QuillMcldConsensusU32, QuillMcldReadError> {
    let Some((expected, _)) = values.first() else {
        return Err(QuillMcldReadError::MissingRequiredField {
            record_id,
            child_index: 0,
            field_id,
        });
    };
    for (child_index, (found, _)) in values.iter().enumerate().skip(1) {
        if found != expected {
            return Err(QuillMcldReadError::NonUniformRequiredField {
                record_id,
                field_id,
                expected: *expected,
                found: *found,
                child_index: u32::try_from(child_index).unwrap_or(u32::MAX),
            });
        }
    }

    Ok(QuillMcldConsensusU32 {
        value: *expected,
        sources: values.iter().map(|(_, source)| source.clone()).collect(),
    })
}

struct Cursor<'a> {
    stream: StreamPath,
    bytes: &'a [u8],
    base: usize,
    position: usize,
}

impl<'a> Cursor<'a> {
    fn new(stream: StreamPath, bytes: &'a [u8], base: usize) -> Self {
        Self {
            stream,
            bytes,
            base,
            position: 0,
        }
    }

    fn position(&self) -> usize {
        self.base + self.position
    }

    fn remaining(&self) -> usize {
        self.bytes.len().saturating_sub(self.position)
    }

    fn peek_u32(&self) -> Result<u32, QuillMcldReadError> {
        let bytes = self.bytes.get(self.position..self.position + 4).ok_or(
            QuillMcldReadError::TooShort {
                offset: self.position(),
                requested: 4,
                available: self.remaining(),
            },
        )?;
        Ok(u32::from_le_bytes(bytes.try_into().unwrap()))
    }

    fn read_u32(&mut self) -> Result<Decoded<u32>, QuillMcldReadError> {
        let start = self.position();
        let bytes = self.read_bytes(4)?;
        Ok(Decoded {
            value: u32::from_le_bytes(bytes.try_into().unwrap()),
            source: span(self.stream.clone(), start, 4),
            raw: bytes.to_vec(),
        })
    }

    fn skip_sized_block(&mut self, minimum_size: u32) -> Result<RawSpan, QuillMcldReadError> {
        let start = self.position();
        let declared_size = self.peek_u32()?;
        if declared_size < minimum_size {
            return Err(QuillMcldReadError::InvalidSizedBlock {
                offset: start as u64,
                declared_size,
                minimum_size,
            });
        }
        let size = usize::try_from(declared_size).map_err(|_| {
            QuillMcldReadError::SizedBlockOutOfBounds {
                offset: start as u64,
                declared_size,
                remaining: self.remaining(),
            }
        })?;
        if size > self.remaining() {
            return Err(QuillMcldReadError::SizedBlockOutOfBounds {
                offset: start as u64,
                declared_size,
                remaining: self.remaining(),
            });
        }
        self.read_bytes(size)?;
        Ok(span(self.stream.clone(), start, size))
    }

    fn read_u8_bounded(&mut self, end: usize) -> Result<u8, QuillMcldReadError> {
        Ok(self.read_bytes_bounded(end, 1)?[0])
    }

    fn read_u16_bounded(&mut self, end: usize) -> Result<u16, QuillMcldReadError> {
        let bytes = self.read_bytes_bounded(end, 2)?;
        Ok(u16::from_le_bytes(bytes.try_into().unwrap()))
    }

    fn read_u32_bounded(&mut self, end: usize) -> Result<u32, QuillMcldReadError> {
        let bytes = self.read_bytes_bounded(end, 4)?;
        Ok(u32::from_le_bytes(bytes.try_into().unwrap()))
    }

    fn read_bytes_bounded(
        &mut self,
        end: usize,
        len: usize,
    ) -> Result<&'a [u8], QuillMcldReadError> {
        let start = self.position();
        let absolute_end = start.checked_add(len).ok_or(QuillMcldReadError::TooShort {
            offset: start,
            requested: len,
            available: end.saturating_sub(start),
        })?;
        if absolute_end > end {
            return Err(QuillMcldReadError::TooShort {
                offset: start,
                requested: len,
                available: end.saturating_sub(start),
            });
        }
        self.read_bytes(len)
    }

    fn read_bytes(&mut self, len: usize) -> Result<&'a [u8], QuillMcldReadError> {
        let start = self.position;
        let end = start.checked_add(len).ok_or(QuillMcldReadError::TooShort {
            offset: self.position(),
            requested: len,
            available: self.remaining(),
        })?;
        let bytes = self
            .bytes
            .get(start..end)
            .ok_or(QuillMcldReadError::TooShort {
                offset: self.position(),
                requested: len,
                available: self.remaining(),
            })?;
        self.position = end;
        Ok(bytes)
    }
}

fn span(stream: StreamPath, offset: usize, len: usize) -> RawSpan {
    RawSpan {
        stream,
        offset: offset as u64,
        len: len as u64,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_span(offset: u64) -> RawSpan {
        RawSpan {
            stream: StreamPath("test".into()),
            offset,
            len: 1,
        }
    }

    fn decoded_u32(value: u32, offset: u64) -> Decoded<u32> {
        Decoded {
            value,
            source: test_span(offset),
            raw: value.to_le_bytes().to_vec(),
        }
    }

    fn inset_field(id: u8, value: u32, offset: u64) -> QuillMcldField {
        QuillMcldField {
            id,
            wire_type: 0x22,
            source: test_span(offset),
            value: QuillMcldFieldValue::U32(value),
        }
    }

    fn alignment_child(value: u32, child_index: u64) -> QuillMcldChild {
        let base = child_index * 10;
        QuillMcldChild {
            source: test_span(base),
            fields: vec![inset_field(0x18, value, base + 1)],
        }
    }

    fn inset_child(values: [u32; 4], child_index: u64) -> QuillMcldChild {
        let base = child_index * 10;
        QuillMcldChild {
            source: test_span(base),
            fields: vec![
                inset_field(0x06, values[0], base + 1),
                inset_field(0x07, values[1], base + 2),
                inset_field(0x08, values[2], base + 3),
                inset_field(0x09, values[3], base + 4),
            ],
        }
    }

    fn test_chunk(children: Vec<QuillMcldChild>) -> QuillMcldChunk {
        let child_count = u32::try_from(children.len()).unwrap();
        QuillMcldChunk {
            source: test_span(0),
            record_count: decoded_u32(1, 1),
            record_id_count: decoded_u32(1, 2),
            record_ids: vec![decoded_u32(77, 3)],
            records: vec![QuillMcldRecord {
                record_id: 77,
                source: test_span(4),
                header_source: test_span(5),
                child_count: decoded_u32(child_count, 6),
                children,
            }],
        }
    }

    #[test]
    fn table_uniform_text_inset_accepts_unanimous_multi_child_record() {
        let chunk = test_chunk(vec![
            inset_child([42, 42, 42, 42], 0),
            inset_child([42, 42, 42, 42], 1),
        ]);

        let inset =
            bounded_mcld_table_uniform_text_inset(&chunk, 77).expect("unanimous TABLE inset");
        assert_eq!(inset.inset_emu, 42);
        assert_eq!(inset.sources.len(), 8);
    }

    #[test]
    fn table_uniform_text_inset_rejects_asymmetric_child() {
        let chunk = test_chunk(vec![inset_child([42, 42, 41, 42], 0)]);

        assert!(matches!(
            bounded_mcld_table_uniform_text_inset(&chunk, 77),
            Err(QuillMcldReadError::NonUniformRequiredField {
                field_id: 0x08,
                child_index: 0,
                ..
            })
        ));
    }

    #[test]
    fn table_uniform_text_inset_rejects_cross_child_disagreement() {
        let chunk = test_chunk(vec![
            inset_child([42, 42, 42, 42], 0),
            inset_child([43, 43, 43, 43], 1),
        ]);

        assert!(matches!(
            bounded_mcld_table_uniform_text_inset(&chunk, 77),
            Err(QuillMcldReadError::NonUniformRequiredField {
                field_id: 0x06,
                child_index: 1,
                ..
            })
        ));
    }

    #[test]
    fn table_uniform_text_inset_rejects_missing_side() {
        let mut child = inset_child([42, 42, 42, 42], 0);
        child.fields.retain(|field| field.id != 0x09);
        let chunk = test_chunk(vec![child]);

        assert!(matches!(
            bounded_mcld_table_uniform_text_inset(&chunk, 77),
            Err(QuillMcldReadError::MissingRequiredField {
                field_id: 0x09,
                child_index: 0,
                ..
            })
        ));
    }

    #[test]
    fn table_vertical_alignment_accepts_unanimous_top_children() {
        let chunk = test_chunk(vec![alignment_child(0, 0), alignment_child(0, 1)]);

        let alignment = bounded_mcld_table_uniform_vertical_alignment(&chunk, 77)
            .expect("unanimous TABLE vertical alignment");
        assert_eq!(alignment.alignment, QuillMcldVerticalAlignment::Top);
        assert_eq!(alignment.sources.len(), 2);
    }

    #[test]
    fn table_vertical_alignment_rejects_mixed_children() {
        let chunk = test_chunk(vec![alignment_child(0, 0), alignment_child(1, 1)]);

        assert!(matches!(
            bounded_mcld_table_uniform_vertical_alignment(&chunk, 77),
            Err(QuillMcldReadError::NonUniformRequiredField {
                field_id: 0x18,
                child_index: 1,
                ..
            })
        ));
    }

    #[test]
    fn table_vertical_alignment_rejects_unsupported_value() {
        let chunk = test_chunk(vec![alignment_child(3, 0)]);

        assert!(matches!(
            bounded_mcld_table_uniform_vertical_alignment(&chunk, 77),
            Err(QuillMcldReadError::UnsupportedVerticalAlignmentValue { value: 3, .. })
        ));
    }
}
