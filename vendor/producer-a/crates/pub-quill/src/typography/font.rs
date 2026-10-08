// CI control: measure FONT leaf routing without changing its semantics.
use super::{
    BlockObservation, GENERAL_CONTAINER, QuillTypographyReadError, VARIABLE_BLOCK_TYPES,
    checked_end, parse_block, read_u16, read_u32, to_usize,
};
use pub_core::{QuillSyid, RawSpan, StreamPath};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

const FONT: [u8; 4] = *b"FONT";
pub(super) const FONT_INDEX_CONTAINER_ID: u16 = 0x0224;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuillScriptFontEntry {
    pub script_slot: u16,
    pub font_index: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_name: Option<String>,
    pub disposition: QuillScriptFontEntryDisposition,
    pub source: RawSpan,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuillScriptFontEntryDisposition {
    Resolved,
    UnresolvedSentinel,
    InvalidFontOrdinal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuillScriptFontMapObservation {
    pub story_index: u32,
    pub story_syid: QuillSyid,
    pub story_start_utf16: u32,
    pub story_end_utf16: u32,
    pub fdpc_descriptor_ordinal: u32,
    pub fdpc_style_ordinal: u32,
    pub fdpc_style_source: RawSpan,
    pub entries: Vec<QuillScriptFontEntry>,
}

pub(super) fn parse_font_catalog(
    bytes: &[u8],
    descriptors: &[(usize, &crate::QuillChunkDescriptor)],
) -> Result<Vec<String>, QuillTypographyReadError> {
    let mut names = Vec::new();
    for (_, descriptor) in descriptors
        .iter()
        .copied()
        .filter(|(_, descriptor)| descriptor.name.value == FONT)
    {
        let start = to_usize(descriptor.data_offset.value, "FONT offset")?;
        let len = to_usize(descriptor.data_length.value, "FONT length")?;
        let end = checked_end(start, len, bytes.len(), "FONT chunk")?;
        if start + 8 > end {
            return Err(QuillTypographyReadError::new(
                "FONT chunk is shorter than fixed prefix",
            ));
        }

        let count = read_u32(bytes, start + 4, end)?;
        let count_usize = to_usize(count, "FONT count")?;
        let index_bytes = count_usize
            .checked_mul(4)
            .ok_or_else(|| QuillTypographyReadError::new("FONT index table overflows usize"))?;
        let mut cursor = start
            .checked_add(20)
            .and_then(|value| value.checked_add(index_bytes))
            .ok_or_else(|| QuillTypographyReadError::new("FONT records offset overflows usize"))?;
        if cursor > end {
            return Err(QuillTypographyReadError::new(
                "FONT index table exceeds chunk",
            ));
        }

        for _ in 0..count {
            let name_units = usize::from(read_u16(bytes, cursor, end)?);
            cursor += 2;
            let name_bytes_len = name_units
                .checked_mul(2)
                .ok_or_else(|| QuillTypographyReadError::new("FONT name length overflows usize"))?;
            let name_end = cursor
                .checked_add(name_bytes_len)
                .ok_or_else(|| QuillTypographyReadError::new("FONT name end overflows usize"))?;
            let trailing_end = name_end
                .checked_add(4)
                .ok_or_else(|| QuillTypographyReadError::new("FONT record end overflows usize"))?;
            if trailing_end > end {
                return Err(QuillTypographyReadError::new("FONT record exceeds chunk"));
            }

            let mut units = Vec::with_capacity(name_units);
            for pair in bytes[cursor..name_end].chunks_exact(2) {
                units.push(u16::from_le_bytes([pair[0], pair[1]]));
            }
            let name = String::from_utf16(&units).map_err(|error| {
                QuillTypographyReadError::new(format!("FONT name is invalid UTF-16: {error}"))
            })?;
            names.push(name);
            cursor = trailing_end;
        }

        if cursor != end {
            return Err(QuillTypographyReadError::new(format!(
                "FONT records leave {} trailing bytes",
                end - cursor
            )));
        }
    }

    if names.is_empty() {
        return Err(QuillTypographyReadError::new("no FONT records"));
    }
    Ok(names)
}

pub(super) fn extract_script_font_map(
    bytes: &[u8],
    block: BlockObservation,
    stream: &StreamPath,
    font_names: &[String],
    unknown_block_types: &mut BTreeSet<u8>,
) -> Result<Vec<QuillScriptFontEntry>, QuillTypographyReadError> {
    if !VARIABLE_BLOCK_TYPES.contains(&block.block_type) {
        return Ok(Vec::new());
    }

    let mut cursor = block
        .data_offset
        .checked_add(4)
        .ok_or_else(|| QuillTypographyReadError::new("script-font map payload offset overflows"))?;
    let mut seen_slots = BTreeSet::new();
    let mut entries = Vec::new();

    while cursor < block.end {
        let entry_start = cursor;
        let (child, next) = parse_block(bytes, cursor, block.end, unknown_block_types)?;
        if child.block_type == GENERAL_CONTAINER {
            if !seen_slots.insert(child.id) {
                return Err(QuillTypographyReadError::new(format!(
                    "script-font map repeats raw script slot {}",
                    child.id
                )));
            }

            let inner_start = child.data_offset.checked_add(4).ok_or_else(|| {
                QuillTypographyReadError::new("script-font entry payload offset overflows")
            })?;
            if inner_start >= child.end {
                return Err(QuillTypographyReadError::new(format!(
                    "script-font slot {} has no FONT ordinal payload",
                    child.id
                )));
            }
            let (value_block, value_end) =
                parse_block(bytes, inner_start, child.end, unknown_block_types)?;
            if value_end != child.end {
                return Err(QuillTypographyReadError::new(format!(
                    "script-font slot {} contains trailing nested payload",
                    child.id
                )));
            }
            let font_index = value_block.value.ok_or_else(|| {
                QuillTypographyReadError::new(format!(
                    "script-font slot {} FONT ordinal is not scalar",
                    child.id
                ))
            })?;

            let (font_name, disposition) = if font_index == 0xFFFF {
                (None, QuillScriptFontEntryDisposition::UnresolvedSentinel)
            } else if let Ok(index) = usize::try_from(font_index) {
                if let Some(name) = font_names.get(index) {
                    (
                        Some(name.clone()),
                        QuillScriptFontEntryDisposition::Resolved,
                    )
                } else {
                    (None, QuillScriptFontEntryDisposition::InvalidFontOrdinal)
                }
            } else {
                (None, QuillScriptFontEntryDisposition::InvalidFontOrdinal)
            };

            entries.push(QuillScriptFontEntry {
                script_slot: child.id,
                font_index,
                font_name,
                disposition,
                source: RawSpan {
                    stream: stream.clone(),
                    offset: entry_start as u64,
                    len: (next - entry_start) as u64,
                },
            });
        }
        cursor = next;
    }

    Ok(entries)
}

pub(super) fn extract_primary_font_index(
    bytes: &[u8],
    block: BlockObservation,
    unknown_block_types: &mut BTreeSet<u8>,
) -> Result<Option<u32>, QuillTypographyReadError> {
    if !VARIABLE_BLOCK_TYPES.contains(&block.block_type) {
        return Ok(None);
    }

    let mut cursor = block
        .data_offset
        .checked_add(4)
        .ok_or_else(|| QuillTypographyReadError::new("font container payload offset overflows"))?;
    while cursor < block.end {
        let (child, next) = parse_block(bytes, cursor, block.end, unknown_block_types)?;
        if child.block_type == GENERAL_CONTAINER {
            let inner_start = child.data_offset.checked_add(4).ok_or_else(|| {
                QuillTypographyReadError::new("general container payload overflows")
            })?;
            if inner_start >= child.end {
                return Ok(None);
            }
            let (value_block, _) = parse_block(bytes, inner_start, child.end, unknown_block_types)?;
            return Ok(value_block.value);
        }
        cursor = next;
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::super::{GENERAL_CONTAINER, parse_block};
    use super::{
        FONT_INDEX_CONTAINER_ID, QuillScriptFontEntryDisposition, extract_script_font_map,
    };
    use pub_core::StreamPath;
    use std::collections::BTreeSet;

    fn script_font_map_bytes(entries: &[(u8, u32)]) -> Vec<u8> {
        let mut children = Vec::new();
        for (slot, font_index) in entries {
            let mut child_payload = Vec::new();
            child_payload.extend_from_slice(&[0x00, 0x20]);
            child_payload.extend_from_slice(&font_index.to_le_bytes());

            children.extend_from_slice(&[*slot, GENERAL_CONTAINER]);
            children.extend_from_slice(&(4_u32 + child_payload.len() as u32).to_le_bytes());
            children.extend_from_slice(&child_payload);
        }

        let mut bytes = Vec::new();
        bytes.extend_from_slice(&[0x24, 0x8A]);
        bytes.extend_from_slice(&(4_u32 + children.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&children);
        bytes
    }

    #[test]
    fn script_font_map_preserves_every_raw_slot_and_resolution_state() {
        let bytes = script_font_map_bytes(&[(1, 0), (7, 1), (44, 0xFFFF), (31, 9)]);
        let mut unknown = BTreeSet::new();
        let (block, next) =
            parse_block(&bytes, 0, bytes.len(), &mut unknown).expect("ScriptFonts block");
        assert_eq!(next, bytes.len());
        assert_eq!(block.id, FONT_INDEX_CONTAINER_ID);
        assert_eq!(block.block_type, GENERAL_CONTAINER);

        let stream = StreamPath("/Quill/QuillSub/CONTENTS".into());
        let entries = extract_script_font_map(
            &bytes,
            block,
            &stream,
            &["Arial".to_owned(), "Times New Roman".to_owned()],
            &mut unknown,
        )
        .expect("bounded script-font map");

        assert_eq!(entries.len(), 4);
        assert_eq!(entries[0].script_slot, 1);
        assert_eq!(entries[0].font_index, 0);
        assert_eq!(entries[0].font_name.as_deref(), Some("Arial"));
        assert_eq!(
            entries[0].disposition,
            QuillScriptFontEntryDisposition::Resolved
        );

        assert_eq!(entries[1].script_slot, 7);
        assert_eq!(entries[1].font_name.as_deref(), Some("Times New Roman"));

        assert_eq!(entries[2].script_slot, 44);
        assert_eq!(entries[2].font_index, 0xFFFF);
        assert_eq!(entries[2].font_name, None);
        assert_eq!(
            entries[2].disposition,
            QuillScriptFontEntryDisposition::UnresolvedSentinel
        );

        assert_eq!(entries[3].script_slot, 31);
        assert_eq!(entries[3].font_index, 9);
        assert_eq!(entries[3].font_name, None);
        assert_eq!(
            entries[3].disposition,
            QuillScriptFontEntryDisposition::InvalidFontOrdinal
        );
        assert!(unknown.is_empty());
    }

    #[test]
    fn script_font_map_rejects_duplicate_raw_slot_without_guessing() {
        let bytes = script_font_map_bytes(&[(7, 1), (7, 0)]);
        let mut unknown = BTreeSet::new();
        let (block, _) =
            parse_block(&bytes, 0, bytes.len(), &mut unknown).expect("ScriptFonts block");
        let error = extract_script_font_map(
            &bytes,
            block,
            &StreamPath("/Quill/QuillSub/CONTENTS".into()),
            &["Arial".to_owned(), "Times New Roman".to_owned()],
            &mut unknown,
        )
        .unwrap_err();
        assert!(error.to_string().contains("repeats raw script slot 7"));
    }
}
