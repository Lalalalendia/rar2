//! Bounded paragraph semantics projected from mature Quill typography.
//!
//! This owner contains paragraph alignment, line-spacing and flow DTOs plus
//! their run-building laws. Shared FDPP/STSH framing, paragraph-range
//! materialization and effective font/size/color inheritance remain in the
//! parent typography module.

use super::{
    PARAGRAPH_FLOW_NATIVE_ON_VALUE, PARAGRAPH_LINE_SPACING_ID,
    PARAGRAPH_LINE_SPACING_RAW_UNITS_PER_EMU, ParagraphTypographyRange, QuillTypographyReadError,
    StoryExtent, checked_end, parse_fdpp_block,
};
use pub_core::{QuillSyid, RawSpan};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuillParagraphAlignment {
    Center,
    Right,
    InterWord,
    Distribute,
}

impl QuillParagraphAlignment {
    pub(super) fn from_persisted_value(value: u32) -> Option<Self> {
        match value {
            1 => Some(Self::Center),
            2 => Some(Self::Right),
            3 => Some(Self::InterWord),
            4 => Some(Self::Distribute),
            _ => None,
        }
    }

    pub(super) fn from_explicit_fdpp_value(value: u32) -> Option<Self> {
        Self::from_persisted_value(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuillParagraphAlignmentRun {
    pub story_index: u32,
    pub story_syid: QuillSyid,
    pub story_start_utf16: u32,
    pub story_end_utf16: u32,
    pub alignment: QuillParagraphAlignment,
    pub source_value: u16,
    pub fdpp_descriptor_ordinal: u32,
    pub fdpp_style_ordinal: u32,
    pub fdpp_style_source: RawSpan,
}

/// Bounded explicit Publisher paragraph line-spacing semantics proven for
/// mature FDPP packed property 0x234. Omission remains absence here because
/// selected/default paragraph styles can supply additional semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum QuillParagraphLineSpacing {
    Proportional { point_equivalent_emu: u32 },
    Absolute { spacing_emu: u32 },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuillParagraphLineSpacingRun {
    pub story_index: u32,
    pub story_syid: QuillSyid,
    pub story_start_utf16: u32,
    pub story_end_utf16: u32,
    pub line_spacing: QuillParagraphLineSpacing,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_value: Option<u32>,
    pub fdpp_descriptor_ordinal: u32,
    pub fdpp_style_ordinal: u32,
    pub fdpp_style_source: RawSpan,
}

/// Explicit paragraph-flow switch observed in a bounded Publisher FDPP write.
/// Only the native ON form is currently promoted. Absence is not flattened
/// into false because style/default inheritance and explicit clear remain
/// outside the closed carrier slice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuillParagraphFlowConstraint {
    StartInNextTextBox,
    KeepLinesTogether,
    KeepWithNext,
    WidowControl,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuillParagraphFlowRun {
    pub story_index: u32,
    pub story_syid: QuillSyid,
    pub story_start_utf16: u32,
    pub story_end_utf16: u32,
    pub constraint: QuillParagraphFlowConstraint,
    pub source_value: u32,
    pub property_source: RawSpan,
    pub fdpp_descriptor_ordinal: u32,
    pub fdpp_style_ordinal: u32,
    pub fdpp_style_source: RawSpan,
}

pub(super) fn build_paragraph_alignment_runs(
    ranges: &[ParagraphTypographyRange],
    stories: &[StoryExtent],
) -> Vec<QuillParagraphAlignmentRun> {
    let mut runs = Vec::new();
    for range in ranges {
        let (Some(alignment), Some(source_value)) = (range.alignment, range.alignment_source_value)
        else {
            continue;
        };
        for story in stories {
            let start = range.global_start_utf16.max(story.global_start_utf16);
            let end = range.global_end_utf16.min(story.global_end_utf16);
            if start >= end {
                continue;
            }
            runs.push(QuillParagraphAlignmentRun {
                story_index: story.story_index,
                story_syid: story.story_syid,
                story_start_utf16: start - story.global_start_utf16,
                story_end_utf16: end - story.global_start_utf16,
                alignment,
                source_value,
                fdpp_descriptor_ordinal: range.fdpp_descriptor_ordinal,
                fdpp_style_ordinal: range.fdpp_style_ordinal,
                fdpp_style_source: range
                    .alignment_source
                    .clone()
                    .unwrap_or_else(|| range.style_source.clone()),
            });
        }
    }
    runs
}

pub(super) fn decode_explicit_paragraph_line_spacing(
    raw_value: u32,
) -> Option<QuillParagraphLineSpacing> {
    let mode = raw_value & 0x3;
    let magnitude = raw_value & !0x3;
    if magnitude == 0 || magnitude % PARAGRAPH_LINE_SPACING_RAW_UNITS_PER_EMU != 0 {
        return None;
    }
    let point_equivalent_emu = magnitude / PARAGRAPH_LINE_SPACING_RAW_UNITS_PER_EMU;
    match mode {
        0x1 => Some(QuillParagraphLineSpacing::Absolute {
            spacing_emu: point_equivalent_emu,
        }),
        0x2 => Some(QuillParagraphLineSpacing::Proportional {
            point_equivalent_emu,
        }),
        _ => None,
    }
}

fn explicit_line_spacing_value_for_range(
    bytes: &[u8],
    range: &ParagraphTypographyRange,
) -> Result<Option<u32>, QuillTypographyReadError> {
    let start = usize::try_from(range.style_source.offset)
        .map_err(|_| QuillTypographyReadError::new("FDPP style offset exceeds usize"))?;
    let len = usize::try_from(range.style_source.len)
        .map_err(|_| QuillTypographyReadError::new("FDPP style length exceeds usize"))?;
    let end = checked_end(start, len, bytes.len(), "FDPP line-spacing style")?;
    if len < 4 {
        return Err(QuillTypographyReadError::new(
            "FDPP line-spacing style is shorter than header",
        ));
    }
    let mut cursor = start + 4;
    let mut unknown = BTreeSet::new();
    let mut values = Vec::new();
    while cursor < end {
        let (block, next) = parse_fdpp_block(bytes, cursor, end, &mut unknown)?;
        if block.id == PARAGRAPH_LINE_SPACING_ID {
            if block.block_type != 0x20 {
                return Ok(None);
            }
            if let Some(value) = block.value {
                values.push(value);
            }
        }
        cursor = next;
    }
    if cursor != end || !unknown.is_empty() {
        return Ok(None);
    }
    values.sort_unstable();
    values.dedup();
    match values.as_slice() {
        [] => Ok(None),
        [value] => Ok(Some(*value)),
        _ => Ok(None),
    }
}

pub(super) fn build_paragraph_line_spacing_runs(
    bytes: &[u8],
    ranges: &[ParagraphTypographyRange],
    stories: &[StoryExtent],
) -> Result<Vec<QuillParagraphLineSpacingRun>, QuillTypographyReadError> {
    let mut runs = Vec::new();
    for range in ranges {
        let source_value = explicit_line_spacing_value_for_range(bytes, range)?;
        let Some(raw_value) = source_value else {
            continue;
        };
        let Some(line_spacing) = decode_explicit_paragraph_line_spacing(raw_value) else {
            continue;
        };

        for story in stories {
            let start = range.global_start_utf16.max(story.global_start_utf16);
            let end = range.global_end_utf16.min(story.global_end_utf16);
            if start >= end {
                continue;
            }
            runs.push(QuillParagraphLineSpacingRun {
                story_index: story.story_index,
                story_syid: story.story_syid,
                story_start_utf16: start - story.global_start_utf16,
                story_end_utf16: end - story.global_start_utf16,
                line_spacing,
                source_value,
                fdpp_descriptor_ordinal: range.fdpp_descriptor_ordinal,
                fdpp_style_ordinal: range.fdpp_style_ordinal,
                fdpp_style_source: range.style_source.clone(),
            });
        }
    }
    Ok(runs)
}

pub(super) fn paragraph_flow_constraint_from_native_id(
    id: u16,
) -> Option<QuillParagraphFlowConstraint> {
    match id {
        0x0A0A => Some(QuillParagraphFlowConstraint::StartInNextTextBox),
        0x0A17 => Some(QuillParagraphFlowConstraint::KeepLinesTogether),
        0x0A18 => Some(QuillParagraphFlowConstraint::KeepWithNext),
        0x0A1D => Some(QuillParagraphFlowConstraint::WidowControl),
        _ => None,
    }
}

pub(super) fn build_paragraph_flow_runs(
    bytes: &[u8],
    ranges: &[ParagraphTypographyRange],
    stories: &[StoryExtent],
) -> Result<Vec<QuillParagraphFlowRun>, QuillTypographyReadError> {
    let mut runs = Vec::new();

    for range in ranges {
        let start = usize::try_from(range.style_source.offset).map_err(|_| {
            QuillTypographyReadError::new("FDPP paragraph-flow style offset exceeds usize")
        })?;
        let len = usize::try_from(range.style_source.len).map_err(|_| {
            QuillTypographyReadError::new("FDPP paragraph-flow style length exceeds usize")
        })?;
        let end = checked_end(start, len, bytes.len(), "FDPP paragraph-flow style")?;
        if len < 4 {
            return Err(QuillTypographyReadError::new(
                "FDPP paragraph-flow style is shorter than header",
            ));
        }

        let mut cursor = start + 4;
        let mut unknown = BTreeSet::new();
        let mut observations = Vec::new();
        while cursor < end {
            let property_start = cursor;
            let (block, next) = parse_fdpp_block(bytes, cursor, end, &mut unknown)?;
            if let (Some(constraint), Some(value)) = (
                paragraph_flow_constraint_from_native_id(block.id),
                block.value,
            ) {
                if block.block_type == 0x20 && value == PARAGRAPH_FLOW_NATIVE_ON_VALUE {
                    observations.push((
                        constraint,
                        value,
                        RawSpan {
                            stream: range.style_source.stream.clone(),
                            offset: property_start as u64,
                            len: (next - property_start) as u64,
                        },
                    ));
                }
            }
            cursor = next;
        }
        if cursor != end || !unknown.is_empty() {
            continue;
        }

        // Duplicate instances of the same native switch are ambiguous. Do not
        // guess which one wins; simply suppress that constraint for this range.
        for constraint in [
            QuillParagraphFlowConstraint::StartInNextTextBox,
            QuillParagraphFlowConstraint::KeepLinesTogether,
            QuillParagraphFlowConstraint::KeepWithNext,
            QuillParagraphFlowConstraint::WidowControl,
        ] {
            let matches = observations
                .iter()
                .filter(|(candidate, _, _)| *candidate == constraint)
                .collect::<Vec<_>>();
            let [(_, source_value, property_source)] = matches.as_slice() else {
                continue;
            };

            for story in stories {
                let story_start = range.global_start_utf16.max(story.global_start_utf16);
                let story_end = range.global_end_utf16.min(story.global_end_utf16);
                if story_start >= story_end {
                    continue;
                }
                runs.push(QuillParagraphFlowRun {
                    story_index: story.story_index,
                    story_syid: story.story_syid,
                    story_start_utf16: story_start - story.global_start_utf16,
                    story_end_utf16: story_end - story.global_start_utf16,
                    constraint,
                    source_value: *source_value,
                    property_source: (*property_source).clone(),
                    fdpp_descriptor_ordinal: range.fdpp_descriptor_ordinal,
                    fdpp_style_ordinal: range.fdpp_style_ordinal,
                    fdpp_style_source: range.style_source.clone(),
                });
            }
        }
    }

    runs.sort_by_key(|run| {
        (
            run.story_index,
            run.story_start_utf16,
            run.story_end_utf16,
            run.constraint,
            run.fdpp_descriptor_ordinal,
            run.fdpp_style_ordinal,
        )
    });
    Ok(runs)
}
