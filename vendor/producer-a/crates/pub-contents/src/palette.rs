use crate::{
    BLOCK_TYPE_CONTAINER_88, BLOCK_TYPE_CONTAINER_A0, BLOCK_TYPE_DUMMY, BLOCK_TYPE_U32,
    BlockReadError, Contents0x2cChunk, ContentsCursor, ContentsReadError, RawContentsBlock,
    RawContentsBlockBody, decode_packed_field_tag, parse_confirmed_block,
};
use pub_core::RawSpan;
use serde::{Deserialize, Serialize};
use std::fmt;

pub const COLOR_SCHEME_COUNT_ID: u16 = 0x01;
pub const COLOR_SCHEME_ENTRIES_ID: u16 = 0x02;
pub const COLOR_SCHEME_NAME_ID: u16 = 0x06;
const COLOR_SCHEME_ENTRY_RGB_ID: u16 = 0x01;
const COLOR_SCHEME_NAME_WIRE_TYPE: u8 = 0xC0;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MatureColorSchemeSlot {
    pub ordinal: usize,
    /// Effective RGB for this scheme ordinal.
    ///
    /// Publisher encodes the default black slot as a DUMMY block. That still
    /// denotes black; rgb_source == None records that the RGB is implicit
    /// rather than backed by an explicit field0x01 value.
    pub rgb: Option<[u8; 3]>,
    pub source: RawSpan,
    pub rgb_source: Option<RawSpan>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MatureColorScheme {
    pub source: RawSpan,
    pub declared_count: u32,
    pub declared_count_source: RawSpan,
    pub slots: Vec<MatureColorSchemeSlot>,
    pub name: Option<String>,
    pub name_source: Option<RawSpan>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MatureColorSchemeReadError {
    Contents(ContentsReadError),
    Block(BlockReadError),
    SpanTooLarge { source: RawSpan },
    UnsupportedTail { source: RawSpan },
    MissingField { id: u16 },
    DuplicateField { id: u16 },
    UnexpectedType { id: u16, block_type: u8 },
    InconsistentBody { id: u16 },
    UnexpectedEntryType { ordinal: usize, block_type: u8 },
    MissingEntryRgb { ordinal: usize },
    DuplicateEntryRgb { ordinal: usize },
    InvalidNameUtf16,
    CountMismatch { declared: u32, observed: usize },
}

impl fmt::Display for MatureColorSchemeReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Contents(error) => error.fmt(f),
            Self::Block(error) => error.fmt(f),
            Self::SpanTooLarge { source } => write!(
                f,
                "OplSccm span does not fit address space: offset={}, len={}",
                source.offset, source.len
            ),
            Self::UnsupportedTail { source } => write!(
                f,
                "OplSccm contains unsupported tail at offset {} ({} bytes)",
                source.offset, source.len
            ),
            Self::MissingField { id } => write!(f, "OplSccm missing required field 0x{id:02X}"),
            Self::DuplicateField { id } => write!(f, "OplSccm repeats field 0x{id:02X}"),
            Self::UnexpectedType { id, block_type } => write!(
                f,
                "OplSccm field 0x{id:02X} has unexpected wire type 0x{block_type:02X}"
            ),
            Self::InconsistentBody { id } => {
                write!(f, "OplSccm field 0x{id:02X} has inconsistent decoded body")
            }
            Self::UnexpectedEntryType {
                ordinal,
                block_type,
            } => write!(
                f,
                "OplSccm slot {ordinal} has unexpected wire type 0x{block_type:02X}"
            ),
            Self::MissingEntryRgb { ordinal } => {
                write!(
                    f,
                    "OplSccm slot {ordinal} is a non-dummy entry without RGB field0x01"
                )
            }
            Self::DuplicateEntryRgb { ordinal } => {
                write!(f, "OplSccm slot {ordinal} repeats RGB field0x01")
            }
            Self::InvalidNameUtf16 => write!(f, "OplSccm scheme name is not valid UTF-16LE"),
            Self::CountMismatch { declared, observed } => write!(
                f,
                "OplSccm declared {declared} color slots but decoded {observed}"
            ),
        }
    }
}

impl std::error::Error for MatureColorSchemeReadError {}

impl From<ContentsReadError> for MatureColorSchemeReadError {
    fn from(value: ContentsReadError) -> Self {
        Self::Contents(value)
    }
}

impl From<BlockReadError> for MatureColorSchemeReadError {
    fn from(value: BlockReadError) -> Self {
        Self::Block(value)
    }
}

pub fn parse_confirmed_mature_color_scheme(
    bytes: &[u8],
    chunk: &Contents0x2cChunk,
) -> Result<MatureColorScheme, MatureColorSchemeReadError> {
    let count = unique_field(chunk, COLOR_SCHEME_COUNT_ID)?;
    if count.block_type != BLOCK_TYPE_U32 {
        return Err(MatureColorSchemeReadError::UnexpectedType {
            id: COLOR_SCHEME_COUNT_ID,
            block_type: count.block_type,
        });
    }
    let (declared_count, declared_count_source) = match &count.body {
        RawContentsBlockBody::U32 {
            value,
            value_source,
        } => (*value, value_source.clone()),
        _ => {
            return Err(MatureColorSchemeReadError::InconsistentBody {
                id: COLOR_SCHEME_COUNT_ID,
            });
        }
    };

    let entries = unique_field(chunk, COLOR_SCHEME_ENTRIES_ID)?;
    if entries.block_type != BLOCK_TYPE_CONTAINER_A0 {
        return Err(MatureColorSchemeReadError::UnexpectedType {
            id: COLOR_SCHEME_ENTRIES_ID,
            block_type: entries.block_type,
        });
    }
    let entries_source = match &entries.body {
        RawContentsBlockBody::Container { content_source, .. } => content_source,
        _ => {
            return Err(MatureColorSchemeReadError::InconsistentBody {
                id: COLOR_SCHEME_ENTRIES_ID,
            });
        }
    };

    let entry_blocks = parse_blocks_in_span(bytes, entries_source)?;
    let mut slots = Vec::with_capacity(entry_blocks.len());
    for (ordinal, entry) in entry_blocks.into_iter().enumerate() {
        match entry.block_type {
            BLOCK_TYPE_DUMMY => slots.push(MatureColorSchemeSlot {
                ordinal,
                rgb: Some([0, 0, 0]),
                source: entry.source,
                rgb_source: None,
            }),
            BLOCK_TYPE_CONTAINER_88 => {
                let source = entry.source.clone();
                let content_source = match &entry.body {
                    RawContentsBlockBody::Container { content_source, .. } => content_source,
                    _ => {
                        return Err(MatureColorSchemeReadError::InconsistentBody {
                            id: COLOR_SCHEME_ENTRIES_ID,
                        });
                    }
                };
                let fields = parse_blocks_in_span(bytes, content_source)?;
                let mut rgbs = fields
                    .iter()
                    .filter(|field| {
                        field.id == COLOR_SCHEME_ENTRY_RGB_ID && field.block_type == BLOCK_TYPE_U32
                    })
                    .collect::<Vec<_>>();
                if rgbs.is_empty() {
                    return Err(MatureColorSchemeReadError::MissingEntryRgb { ordinal });
                }
                if rgbs.len() != 1 {
                    return Err(MatureColorSchemeReadError::DuplicateEntryRgb { ordinal });
                }
                let rgb_field = rgbs.pop().expect("exactly one RGB field");
                let (value, rgb_source) = match &rgb_field.body {
                    RawContentsBlockBody::U32 {
                        value,
                        value_source,
                    } => (*value, value_source.clone()),
                    _ => {
                        return Err(MatureColorSchemeReadError::InconsistentBody {
                            id: COLOR_SCHEME_ENTRY_RGB_ID,
                        });
                    }
                };
                let raw = value.to_le_bytes();
                slots.push(MatureColorSchemeSlot {
                    ordinal,
                    rgb: Some([raw[0], raw[1], raw[2]]),
                    source,
                    rgb_source: Some(rgb_source),
                });
            }
            block_type => {
                return Err(MatureColorSchemeReadError::UnexpectedEntryType {
                    ordinal,
                    block_type,
                });
            }
        }
    }

    if usize::try_from(declared_count).ok() != Some(slots.len()) {
        return Err(MatureColorSchemeReadError::CountMismatch {
            declared: declared_count,
            observed: slots.len(),
        });
    }

    let (name, name_source) = parse_optional_scheme_name(bytes, chunk)?;

    Ok(MatureColorScheme {
        source: chunk.source.clone(),
        declared_count,
        declared_count_source,
        slots,
        name,
        name_source,
    })
}

fn unique_field(
    chunk: &Contents0x2cChunk,
    id: u16,
) -> Result<&RawContentsBlock, MatureColorSchemeReadError> {
    optional_unique_field(chunk, id)?.ok_or(MatureColorSchemeReadError::MissingField { id })
}

fn optional_unique_field(
    chunk: &Contents0x2cChunk,
    id: u16,
) -> Result<Option<&RawContentsBlock>, MatureColorSchemeReadError> {
    let mut fields = chunk.fields.iter().filter(|field| field.id == id);
    let first = fields.next();
    if fields.next().is_some() {
        return Err(MatureColorSchemeReadError::DuplicateField { id });
    }
    Ok(first)
}

fn parse_optional_scheme_name(
    bytes: &[u8],
    chunk: &Contents0x2cChunk,
) -> Result<(Option<String>, Option<RawSpan>), MatureColorSchemeReadError> {
    if let Some(field) = optional_unique_field(chunk, COLOR_SCHEME_NAME_ID)? {
        return Err(MatureColorSchemeReadError::UnexpectedType {
            id: COLOR_SCHEME_NAME_ID,
            block_type: field.block_type,
        });
    }

    let Some(source) = chunk.unsupported_tail.as_ref() else {
        return Ok((None, None));
    };
    let start =
        usize::try_from(source.offset).map_err(|_| MatureColorSchemeReadError::SpanTooLarge {
            source: source.clone(),
        })?;
    let len =
        usize::try_from(source.len).map_err(|_| MatureColorSchemeReadError::SpanTooLarge {
            source: source.clone(),
        })?;
    let mut cursor = ContentsCursor::bounded(source.stream.clone(), bytes, start, len)?;
    let (tag0, _) = cursor.read_u8()?;
    let (tag1, _) = cursor.read_u8()?;
    let (id, block_type) = decode_packed_field_tag([tag0, tag1]);
    if id != COLOR_SCHEME_NAME_ID || block_type != COLOR_SCHEME_NAME_WIRE_TYPE {
        return Err(MatureColorSchemeReadError::UnsupportedTail {
            source: source.clone(),
        });
    }

    let (declared_length, _) = cursor.read_u32_le()?;
    if declared_length < 4 {
        return Err(BlockReadError::InvalidDeclaredLength {
            block_type,
            offset: start,
            declared_length,
        }
        .into());
    }
    let content_len =
        usize::try_from(declared_length - 4).map_err(|_| BlockReadError::LengthTooLarge {
            block_type,
            declared_length,
        })?;
    let (raw, value_source) = cursor.take(content_len)?;
    if cursor.remaining() != 0 {
        return Err(MatureColorSchemeReadError::UnsupportedTail {
            source: RawSpan {
                stream: source.stream.clone(),
                offset: cursor.position() as u64,
                len: cursor.remaining() as u64,
            },
        });
    }

    Ok((Some(decode_utf16le_name(raw)?), Some(value_source)))
}

fn parse_blocks_in_span(
    bytes: &[u8],
    source: &RawSpan,
) -> Result<Vec<RawContentsBlock>, MatureColorSchemeReadError> {
    let start =
        usize::try_from(source.offset).map_err(|_| MatureColorSchemeReadError::SpanTooLarge {
            source: source.clone(),
        })?;
    let len =
        usize::try_from(source.len).map_err(|_| MatureColorSchemeReadError::SpanTooLarge {
            source: source.clone(),
        })?;
    let mut cursor = ContentsCursor::bounded(source.stream.clone(), bytes, start, len)?;
    let mut blocks = Vec::new();
    while cursor.remaining() > 0 {
        blocks.push(parse_confirmed_block(&mut cursor)?);
    }
    Ok(blocks)
}

fn decode_utf16le_name(raw: &[u8]) -> Result<String, MatureColorSchemeReadError> {
    if raw.len() % 2 != 0 {
        return Err(MatureColorSchemeReadError::InvalidNameUtf16);
    }
    let mut units = raw
        .chunks_exact(2)
        .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
        .collect::<Vec<_>>();
    if units.last() == Some(&0) {
        units.pop();
    }
    String::from_utf16(&units).map_err(|_| MatureColorSchemeReadError::InvalidNameUtf16)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_confirmed_0x2c_chunk;
    use pub_core::StreamPath;

    fn sample_chunk_bytes() -> Vec<u8> {
        vec![
            0x28, 0x00, 0x00, 0x00, 0x01, 0x20, 0x02, 0x00, 0x00, 0x00, 0x02, 0xA0, 0x12, 0x00,
            0x00, 0x00, 0x00, 0x88, 0x0A, 0x00, 0x00, 0x00, 0x01, 0x20, 0x33, 0x22, 0x11, 0x00,
            0x00, 0x78, 0x06, 0xC0, 0x08, 0x00, 0x00, 0x00, b'A', 0x00, 0x00, 0x00,
        ]
    }

    #[test]
    fn parses_ordered_scheme_slots_and_resolves_dummy_as_implicit_black() {
        let bytes = sample_chunk_bytes();
        let chunk = parse_confirmed_0x2c_chunk(StreamPath("/Contents".into()), &bytes, 0)
            .expect("synthetic OplSccm chunk must parse");
        assert!(chunk.unsupported_tail.is_some());
        let scheme = parse_confirmed_mature_color_scheme(&bytes, &chunk)
            .expect("synthetic OplSccm semantics must parse");

        assert_eq!(scheme.declared_count, 2);
        assert_eq!(scheme.name.as_deref(), Some("A"));
        assert_eq!(scheme.slots.len(), 2);
        assert_eq!(scheme.slots[0].ordinal, 0);
        assert_eq!(scheme.slots[0].rgb, Some([0x33, 0x22, 0x11]));
        assert_eq!(scheme.slots[1].ordinal, 1);
        assert_eq!(scheme.slots[1].rgb, Some([0, 0, 0]));
        assert!(scheme.slots[0].rgb_source.is_some());
        assert!(scheme.slots[1].rgb_source.is_none());
    }

    #[test]
    fn rejects_declared_count_that_would_shift_slot_identity() {
        let mut bytes = sample_chunk_bytes();
        bytes[6] = 0x03;
        let chunk = parse_confirmed_0x2c_chunk(StreamPath("/Contents".into()), &bytes, 0)
            .expect("wire remains parseable");
        assert!(matches!(
            parse_confirmed_mature_color_scheme(&bytes, &chunk),
            Err(MatureColorSchemeReadError::CountMismatch {
                declared: 3,
                observed: 2
            })
        ));
    }
}
