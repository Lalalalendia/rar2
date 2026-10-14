//! Bounded Quill text-color authority.
//!
//! This owner contains PL color-reference parsing, direct/scheme color
//! resolution, effective explicit/inherited color selection, and nested color
//! index extraction. Generic FDPC/STSH framing and typography inheritance stay
//! in the parent module.

use super::{
    BlockObservation, CharacterDefaultObservation, QuillTypographyRange, QuillTypographyReadError,
    QuillTypographyValueSource, VARIABLE_BLOCK_TYPES, checked_end, parse_block, read_u32, to_usize,
};
use std::collections::BTreeSet;

const PL: [u8; 4] = *b"PL  ";

pub(super) const BARE_COLOR_INDEX_ID: u16 = 0x002E;
pub(super) const BARE_COLOR_INDEX_EXTENDED_ID: u16 = 0x022E;
pub(super) const COLOR_INDEX_CONTAINER_ID: u16 = 0x0044;
pub(super) const COLOR_INDEX_CONTAINER_EXTENDED_ID: u16 = 0x0244;
const COLOR_INDEX_ID: u16 = 0x0200;
const PL_COLOR_REFERENCE_ID: u16 = 0x0201;

pub(super) fn parse_text_color_references(
    bytes: &[u8],
    descriptors: &[(usize, &crate::QuillChunkDescriptor)],
) -> Result<Vec<u32>, QuillTypographyReadError> {
    let mut references = Vec::new();
    let mut unknown_block_types = BTreeSet::new();
    let color_descriptors = descriptors
        .iter()
        .copied()
        .filter(|(_, descriptor)| descriptor.name.value == PL)
        .collect::<Vec<_>>();
    if color_descriptors.len() > 1 {
        return Err(QuillTypographyReadError::new(format!(
            "multiple PL text-color tables are ambiguous: {}",
            color_descriptors.len()
        )));
    }

    for (_, descriptor) in color_descriptors {
        let start = to_usize(descriptor.data_offset.value, "PL offset")?;
        let len = to_usize(descriptor.data_length.value, "PL length")?;
        let end = checked_end(start, len, bytes.len(), "PL chunk")?;
        if start + 12 > end {
            return Err(QuillTypographyReadError::new(
                "PL chunk is shorter than fixed prefix",
            ));
        }

        let count = to_usize(read_u32(bytes, start, end)?, "PL color count")?;
        let mut cursor = start + 12;
        for ordinal in 0..count {
            if cursor + 4 > end {
                return Err(QuillTypographyReadError::new(format!(
                    "PL color entry {ordinal} header exceeds chunk"
                )));
            }
            let entry_len = to_usize(read_u32(bytes, cursor, end)?, "PL color entry length")?;
            if entry_len < 4 {
                return Err(QuillTypographyReadError::new(format!(
                    "PL color entry {ordinal} length is smaller than header"
                )));
            }
            let entry_end = checked_end(cursor, entry_len, end, "PL color entry")?;
            let mut block_cursor = cursor + 4;
            let mut values = Vec::new();
            while block_cursor < entry_end {
                let (block, next) =
                    parse_block(bytes, block_cursor, entry_end, &mut unknown_block_types)?;
                if block.id == PL_COLOR_REFERENCE_ID {
                    if let Some(value) = block.value {
                        values.push(value);
                    }
                }
                block_cursor = next;
            }
            if block_cursor != entry_end {
                return Err(QuillTypographyReadError::new(format!(
                    "PL color entry {ordinal} did not close exactly"
                )));
            }
            let [value] = values.as_slice() else {
                return Err(QuillTypographyReadError::new(format!(
                    "PL color entry {ordinal} contains {} color-reference values",
                    values.len()
                )));
            };
            references.push(*value);
            cursor = entry_end;
        }
        if cursor != end {
            return Err(QuillTypographyReadError::new(format!(
                "PL color table leaves {} trailing bytes",
                end - cursor
            )));
        }
    }

    Ok(references)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BoundedQuillTextColor {
    DirectRgb([u8; 3]),
    PublisherSchemeSlot(u8),
}

fn resolve_bounded_quill_text_color(raw: u32) -> Option<BoundedQuillTextColor> {
    let high = (raw >> 24) as u8;
    if high == 0 {
        return Some(BoundedQuillTextColor::DirectRgb([
            (raw & 0xFF) as u8,
            ((raw >> 8) & 0xFF) as u8,
            ((raw >> 16) & 0xFF) as u8,
        ]));
    }

    // QUILL-SCHEME-TEXT-COLOR-AUTH-01 native Publisher 2019 evidence proves
    // this exact form only: fSchemeIndex high byte 0x08, zero middle payload,
    // and low-byte slot 0..7. COM SchemeColor roles 1..8 persist as slots
    // 0..7 and the slot stays stable across a document ColorScheme switch.
    if high == 0x08 && (raw & 0x00FF_FF00) == 0 {
        let slot = (raw & 0xFF) as u8;
        if slot < 8 {
            return Some(BoundedQuillTextColor::PublisherSchemeSlot(slot));
        }
    }

    // Palette/system/auto/procedural and wider scheme forms remain fail-closed.
    None
}

pub(super) fn split_bounded_quill_text_color(
    color: Option<BoundedQuillTextColor>,
) -> (Option<[u8; 3]>, Option<u8>) {
    match color {
        Some(BoundedQuillTextColor::DirectRgb(rgb)) => (Some(rgb), None),
        Some(BoundedQuillTextColor::PublisherSchemeSlot(slot)) => (None, Some(slot)),
        None => (None, None),
    }
}

#[cfg(test)]
fn resolve_direct_quill_text_color(raw: u32) -> Option<[u8; 3]> {
    match resolve_bounded_quill_text_color(raw) {
        Some(BoundedQuillTextColor::DirectRgb(rgb)) => Some(rgb),
        _ => None,
    }
}

pub(super) fn resolve_effective_text_color(
    fdpc: &QuillTypographyRange,
    default: Option<&CharacterDefaultObservation>,
    references: &[u32],
) -> Option<(BoundedQuillTextColor, QuillTypographyValueSource)> {
    let mut explicit = fdpc.color_indices.clone();
    explicit.sort_unstable();
    explicit.dedup();
    if let [index] = explicit.as_slice() {
        let raw = references.get(*index as usize)?;
        return resolve_bounded_quill_text_color(*raw)
            .map(|color| (color, QuillTypographyValueSource::ExplicitFdpc));
    }
    if !explicit.is_empty() {
        return None;
    }

    let default = default?;
    let mut inherited = default.color_indices.clone();
    inherited.sort_unstable();
    inherited.dedup();
    let [index] = inherited.as_slice() else {
        return None;
    };
    let raw = references.get(*index as usize)?;
    resolve_bounded_quill_text_color(*raw)
        .map(|color| (color, QuillTypographyValueSource::InheritedStsh1))
}

pub(super) fn extract_color_index(
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
        .ok_or_else(|| QuillTypographyReadError::new("color container payload offset overflows"))?;
    while cursor < block.end {
        let (child, next) = parse_block(bytes, cursor, block.end, unknown_block_types)?;
        if child.id == COLOR_INDEX_ID {
            return Ok(child.value);
        }
        cursor = next;
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quill_text_color_carrier_tags_decode_to_extended_0x2xx_fields() {
        assert_eq!(
            super::super::decode_quill_style_tag([0x00, 0x22]),
            (COLOR_INDEX_ID, 0x20)
        );
        assert_eq!(
            super::super::decode_quill_style_tag([0x01, 0x22]),
            (PL_COLOR_REFERENCE_ID, 0x20)
        );
    }

    #[test]
    fn direct_quill_text_color_decodes_bgr_word_and_fails_closed_on_indirection() {
        assert_eq!(
            resolve_direct_quill_text_color(0x0011_2233),
            Some([0x33, 0x22, 0x11])
        );
        assert_eq!(resolve_direct_quill_text_color(0x0800_0001), None);
        assert_eq!(resolve_direct_quill_text_color(0x1001_8000), None);
        assert_eq!(resolve_direct_quill_text_color(0xFF11_2233), None);
    }

    #[test]
    fn publisher_scheme_text_color_accepts_only_native_grounded_eight_slot_form() {
        assert_eq!(
            resolve_bounded_quill_text_color(0x0800_0000),
            Some(BoundedQuillTextColor::PublisherSchemeSlot(0))
        );
        assert_eq!(
            resolve_bounded_quill_text_color(0x0800_0007),
            Some(BoundedQuillTextColor::PublisherSchemeSlot(7))
        );
        assert_eq!(resolve_bounded_quill_text_color(0x0800_0008), None);
        assert_eq!(resolve_bounded_quill_text_color(0x0800_0100), None);
        assert_eq!(resolve_bounded_quill_text_color(0x1000_0000), None);
        assert_eq!(resolve_bounded_quill_text_color(0xFF00_0000), None);
    }
}
