//! Physical-font-gated, nonempty mixed-family terminal retry.
//! Source and exact face are established per typography span, never by
//! making a mixed-family Story masquerade as a single-family fragment.

use super::{
    LengthEmu, MixedLineCandidateV1, RenderResolvedTextLineV1, RenderResolvedTextSpanV1,
    ResolvedFamilyTypographyRunV1, StoryId, ViewerGeometryDocument, ViewerParagraphLineSpacing,
    compatible_natural_line_height_emu_v1,
};

const NATIVE_PROVEN_THREE_QUARTER_SPACING_EMU_V1: u32 = 9 * 12_700;

/// A span must match exactly one source typography run and that run's
/// previously admitted exact bytes, SHA, face and point size.
fn physical_spans_extent_emu_v1(
    spans: &[RenderResolvedTextSpanV1],
    scalar_start: u32,
    scalar_end: u32,
    runs: &[ResolvedFamilyTypographyRunV1<'_>],
) -> Option<i64> {
    if scalar_start >= scalar_end || spans.is_empty() {
        return None;
    }
    let mut cursor = scalar_start;
    let mut max_extent = 0_i64;
    for span in spans {
        if span.scalar_start != cursor || span.scalar_end <= cursor || span.scalar_end > scalar_end {
            return None;
        }
        let mut covering = runs.iter().filter(|run| {
            run.scalar_start <= span.scalar_start && run.scalar_end >= span.scalar_end
        });
        let run = covering.next()?;
        if covering.next().is_some()
            || span.font_resource_id.as_deref() != Some(run.font.resource_id)
            || span.font_fingerprint_sha256.as_deref()
                != Some(run.font_fingerprint_sha256.as_str())
            || span.font_size_emu != run.font_size_emu
            || span.shaping.is_none()
        {
            return None;
        }
        let physical = compatible_natural_line_height_emu_v1(
            run.font.bytes,
            run.font.face_index,
            LengthEmu::new(span.font_size_emu),
        )
        .map(LengthEmu::get)?;
        if physical <= 0 {
            return None;
        }
        max_extent = max_extent.max(physical);
        cursor = span.scalar_end;
    }
    (cursor == scalar_end && max_extent > 0).then_some(max_extent)
}

/// Preserve accepted lines and their advances, replacing only the first
/// baseline-height contribution by its exact visible physical extent.
pub(super) fn mixed_family_frozen_prefix_height_emu_v1(
    lines: &[RenderResolvedTextLineV1],
    runs: &[ResolvedFamilyTypographyRunV1<'_>],
) -> Option<i64> {
    let (first, rest) = lines.split_first()?;
    let physical = physical_spans_extent_emu_v1(
        &first.spans, first.scalar_start, first.scalar_end, runs,
    )?;
    let mut height = physical.min(first.line_height_emu);
    for line in rest {
        height = height.checked_add(line.line_height_emu)?;
    }
    (height > 0).then_some(height)
}

/// A single fresh and explicitly sourced 0.75 paragraph spacing run must
/// uniquely cover the exact width-fitting, height-rejected terminal line.
pub(super) fn mixed_family_terminal_source_advance_emu_v1(
    visual: &ViewerGeometryDocument,
    story_id: StoryId,
    scalar_start: u32,
    fragment_scalar_end: u32,
    terminal: &MixedLineCandidateV1,
    runs: &[ResolvedFamilyTypographyRunV1<'_>],
) -> Option<i64> {
    if terminal.scalar_end <= scalar_start
        || terminal.consumed_scalar_end != fragment_scalar_end
        || terminal.text.is_empty()
    {
        return None;
    }
    let story = visual.document.stories.iter().find(|story| story.id == story_id)?;
    let mut intersecting = visual
        .paragraph_line_spacings
        .iter()
        .filter(|run| run.story_id == story_id)
        .filter(|run| run.applies_to_story_text(&story.text))
        .filter(|run| run.scalar_end > scalar_start && run.scalar_start < terminal.scalar_end);
    let spacing = intersecting.next()?;
    if intersecting.next().is_some()
        || spacing.source_value.is_none()
        || spacing.scalar_start > scalar_start
        || spacing.scalar_end < terminal.scalar_end
        || !matches!(
            spacing.line_spacing,
            ViewerParagraphLineSpacing::Proportional { point_equivalent_emu }
                if point_equivalent_emu == NATIVE_PROVEN_THREE_QUARTER_SPACING_EMU_V1
        )
    {
        return None;
    }
    let natural = physical_spans_extent_emu_v1(
        &terminal.spans, scalar_start, terminal.scalar_end, runs,
    )?;
    let scaled = (i128::from(natural) * 3 + 2) / 4;
    let height = i64::try_from(scaled).ok()?;
    (height > 0).then_some(height)
}
