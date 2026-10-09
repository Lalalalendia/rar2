use super::*;

#[derive(Debug, Clone, Copy)]
pub(super) struct AdmittedTypographyRunV1 {
    pub(super) scalar_start: u32,
    pub(super) scalar_end: u32,
    pub(super) font_size_emu: i64,
}

#[derive(Debug)]
pub(super) struct PreparedTypographyRunV1 {
    run: AdmittedTypographyRunV1,
    advance_prefix_emu: Vec<i64>,
    shaping: RenderResolvedShapingV1,
}

#[derive(Debug)]
pub(super) struct MixedLineCandidateV1 {
    pub(super) scalar_end: u32,
    pub(super) consumed_scalar_end: u32,
    pub(super) text: String,
    pub(super) measured_width_emu: i64,
    pub(super) line_height_emu: i64,
    pub(super) spans: Vec<RenderResolvedTextSpanV1>,
}

pub(super) fn admitted_typography_runs_v1(
    fragment: &RenderTextFragmentV1,
    default_font_size_emu: i64,
) -> Result<Vec<AdmittedTypographyRunV1>, RenderTextLayoutFallbackReasonV1> {
    if fragment.typography.is_empty() {
        if default_font_size_emu <= 0 {
            return Err(RenderTextLayoutFallbackReasonV1::FontResourceInvalid);
        }
        return Ok(vec![AdmittedTypographyRunV1 {
            scalar_start: fragment.scalar_start,
            scalar_end: fragment.scalar_end,
            font_size_emu: default_font_size_emu,
        }]);
    }

    let mut cursor = fragment.scalar_start;
    let mut admitted = Vec::with_capacity(fragment.typography.len());
    for run in &fragment.typography {
        if run.scalar_start != cursor
            || run.scalar_end <= run.scalar_start
            || run.scalar_end > fragment.scalar_end
            || run.text_size_emu == 0
        {
            return Err(RenderTextLayoutFallbackReasonV1::TypographyCoverageGap);
        }
        admitted.push(AdmittedTypographyRunV1 {
            scalar_start: run.scalar_start,
            scalar_end: run.scalar_end,
            font_size_emu: i64::from(run.text_size_emu),
        });
        cursor = run.scalar_end;
    }
    if cursor != fragment.scalar_end {
        return Err(RenderTextLayoutFallbackReasonV1::TypographyCoverageGap);
    }
    Ok(admitted)
}

pub(super) fn admitted_font_size_emu(
    fragment: &RenderTextFragmentV1,
    default_font_size_emu: i64,
) -> Result<i64, RenderTextLayoutFallbackReasonV1> {
    let admitted = admitted_typography_runs_v1(fragment, default_font_size_emu)?;
    let Some(first) = admitted.first().map(|run| run.font_size_emu) else {
        return Err(RenderTextLayoutFallbackReasonV1::TypographyCoverageGap);
    };
    if admitted.iter().all(|run| run.font_size_emu == first) {
        Ok(first)
    } else {
        Err(RenderTextLayoutFallbackReasonV1::MixedTypographySize)
    }
}

pub(super) fn scalar_text_range_v1(scalars: &[char], start: u32, end: u32) -> Option<String> {
    let start = usize::try_from(start).ok()?;
    let end = usize::try_from(end).ok()?;
    (start <= end && end <= scalars.len()).then(|| scalars[start..end].iter().collect())
}

pub(super) fn prepare_typography_run_v1(
    run: AdmittedTypographyRunV1,
    shaped: &pub_layout::BoundedShapedText,
) -> Result<PreparedTypographyRunV1, RenderTextLayoutFallbackReasonV1> {
    let glyphs = &shaped.glyphs;
    let scalar_len = usize::try_from(
        run.scalar_end
            .checked_sub(run.scalar_start)
            .ok_or(RenderTextLayoutFallbackReasonV1::SharedLayoutFailed)?,
    )
    .map_err(|_| RenderTextLayoutFallbackReasonV1::SharedLayoutFailed)?;
    let mut advances = vec![0_i64; scalar_len.saturating_add(1)];
    for glyph in glyphs {
        if glyph.cluster < run.scalar_start || glyph.cluster >= run.scalar_end {
            return Err(RenderTextLayoutFallbackReasonV1::SharedLayoutFailed);
        }
        let local = usize::try_from(glyph.cluster - run.scalar_start)
            .map_err(|_| RenderTextLayoutFallbackReasonV1::SharedLayoutFailed)?;
        let slot = local
            .checked_add(1)
            .ok_or(RenderTextLayoutFallbackReasonV1::SharedLayoutFailed)?;
        advances[slot] = advances[slot]
            .checked_add(glyph.x_advance.get())
            .ok_or(RenderTextLayoutFallbackReasonV1::SharedLayoutFailed)?;
    }
    for index in 1..advances.len() {
        advances[index] = advances[index - 1]
            .checked_add(advances[index])
            .ok_or(RenderTextLayoutFallbackReasonV1::SharedLayoutFailed)?;
    }
    Ok(PreparedTypographyRunV1 {
        run,
        advance_prefix_emu: advances,
        shaping: RenderResolvedShapingV1 {
            environment: shaped.environment.clone(),
            units_per_em: shaped.units_per_em,
            glyphs: shaped.glyphs.clone(),
        },
    })
}

fn prepared_run_width_v1(
    prepared: &PreparedTypographyRunV1,
    scalar_start: u32,
    scalar_end: u32,
) -> Result<i64, RenderTextLayoutFallbackReasonV1> {
    if scalar_start < prepared.run.scalar_start
        || scalar_end > prepared.run.scalar_end
        || scalar_start > scalar_end
    {
        return Err(RenderTextLayoutFallbackReasonV1::SharedLayoutFailed);
    }
    let start = usize::try_from(scalar_start - prepared.run.scalar_start)
        .map_err(|_| RenderTextLayoutFallbackReasonV1::SharedLayoutFailed)?;
    let end = usize::try_from(scalar_end - prepared.run.scalar_start)
        .map_err(|_| RenderTextLayoutFallbackReasonV1::SharedLayoutFailed)?;
    prepared.advance_prefix_emu[end]
        .checked_sub(prepared.advance_prefix_emu[start])
        .ok_or(RenderTextLayoutFallbackReasonV1::SharedLayoutFailed)
}

pub(super) fn reuse_mixed_line_candidate_v1(
    scalars: &[char],
    cursor: u32,
    consumed_scalar_end: u32,
    prepared_runs: &[PreparedTypographyRunV1],
    font: &ExplicitRenderTextFontResourceV1<'_>,
) -> Result<MixedLineCandidateV1, RenderTextLayoutFallbackReasonV1> {
    let scalar_end = consumed_scalar_end;
    let text = scalar_text_range_v1(scalars, cursor, scalar_end)
        .ok_or(RenderTextLayoutFallbackReasonV1::SharedLayoutFailed)?;
    let mut spans = Vec::new();
    let mut measured_width_emu = 0_i64;
    let mut line_height_emu = font.default_line_height_emu;

    for prepared in prepared_runs {
        let run = prepared.run;
        let span_start = run.scalar_start.max(cursor);
        let span_end = run.scalar_end.min(scalar_end);
        if span_start >= span_end {
            continue;
        }
        let span_text = scalar_text_range_v1(scalars, span_start, span_end)
            .ok_or(RenderTextLayoutFallbackReasonV1::SharedLayoutFailed)?;
        let Some(span_line_height_emu) = scaled_line_height_emu(
            run.font_size_emu,
            font.default_font_size_emu,
            font.default_line_height_emu,
        ) else {
            return Err(RenderTextLayoutFallbackReasonV1::FontResourceInvalid);
        };
        let span_width_emu = prepared_run_width_v1(prepared, span_start, span_end)?;
        let span_glyphs = prepared
            .shaping
            .glyphs
            .iter()
            .filter(|glyph| glyph.cluster >= span_start && glyph.cluster < span_end)
            .cloned()
            .collect::<Vec<_>>();
        spans.push(RenderResolvedTextSpanV1 {
            scalar_start: span_start,
            scalar_end: span_end,
            text: span_text,
            x_offset_emu: measured_width_emu,
            measured_width_emu: span_width_emu,
            font_size_emu: run.font_size_emu,
            font_resource_id: None,
            font_fingerprint_sha256: None,
            shaping: Some(RenderResolvedShapingV1 {
                environment: prepared.shaping.environment.clone(),
                units_per_em: prepared.shaping.units_per_em,
                glyphs: span_glyphs,
            }),
        });
        measured_width_emu = measured_width_emu
            .checked_add(span_width_emu)
            .ok_or(RenderTextLayoutFallbackReasonV1::SharedLayoutFailed)?;
        line_height_emu = line_height_emu.max(span_line_height_emu);
    }

    Ok(MixedLineCandidateV1 {
        scalar_end,
        consumed_scalar_end,
        text,
        measured_width_emu,
        line_height_emu,
        spans,
    })
}

pub(super) fn shape_mixed_line_candidate_v1(
    scalars: &[char],
    cursor: u32,
    consumed_scalar_end: u32,
    kind: BoundedBreakKind,
    runs: &[AdmittedTypographyRunV1],
    font: &ExplicitRenderTextFontResourceV1<'_>,
    fingerprint: &str,
) -> Result<MixedLineCandidateV1, RenderTextLayoutFallbackReasonV1> {
    let mut scalar_end = consumed_scalar_end;
    if kind == BoundedBreakKind::Mandatory {
        while scalar_end > cursor {
            let index = usize::try_from(scalar_end - 1)
                .map_err(|_| RenderTextLayoutFallbackReasonV1::SharedLayoutFailed)?;
            if !matches!(scalars.get(index).copied(), Some('\r' | '\n')) {
                break;
            }
            scalar_end -= 1;
        }
    }

    let text = scalar_text_range_v1(scalars, cursor, scalar_end)
        .ok_or(RenderTextLayoutFallbackReasonV1::SharedLayoutFailed)?;
    let mut spans = Vec::new();
    let mut measured_width_emu = 0_i64;
    let mut line_height_emu = font.default_line_height_emu;

    for run in runs {
        let span_start = run.scalar_start.max(cursor);
        let span_end = run.scalar_end.min(scalar_end);
        if span_start >= span_end {
            continue;
        }
        let span_text = scalar_text_range_v1(scalars, span_start, span_end)
            .ok_or(RenderTextLayoutFallbackReasonV1::SharedLayoutFailed)?;
        let Some(span_line_height_emu) = scaled_line_height_emu(
            run.font_size_emu,
            font.default_font_size_emu,
            font.default_line_height_emu,
        ) else {
            return Err(RenderTextLayoutFallbackReasonV1::FontResourceInvalid);
        };
        let runtime = BoundedShapingRuntime {
            layout: BoundedLayoutEnvironment {
                engine_revision: SHARED_TEXT_LAYOUT_REVISION_V1.to_owned(),
                font_set_fingerprint: fingerprint.to_owned(),
                resource_fingerprint: font.resource_id.to_owned(),
            },
            face_index: font.face_index,
            font_size_emu: LengthEmu::new(run.font_size_emu),
            font_bytes: font.bytes,
        };
        let shaped = shape_bounded_ltr_segment(&span_text, span_start, &runtime)
            .map_err(|_| RenderTextLayoutFallbackReasonV1::SharedLayoutFailed)?;
        let span_width_emu = shaped.total_x_advance.get();
        spans.push(RenderResolvedTextSpanV1 {
            scalar_start: span_start,
            scalar_end: span_end,
            text: span_text,
            x_offset_emu: measured_width_emu,
            measured_width_emu: span_width_emu,
            font_size_emu: run.font_size_emu,
            font_resource_id: None,
            font_fingerprint_sha256: None,
            shaping: Some(RenderResolvedShapingV1 {
                environment: shaped.environment,
                units_per_em: shaped.units_per_em,
                glyphs: shaped.glyphs,
            }),
        });
        measured_width_emu = measured_width_emu
            .checked_add(span_width_emu)
            .ok_or(RenderTextLayoutFallbackReasonV1::SharedLayoutFailed)?;
        line_height_emu = line_height_emu.max(span_line_height_emu);
    }

    Ok(MixedLineCandidateV1 {
        scalar_end,
        consumed_scalar_end,
        text,
        measured_width_emu,
        line_height_emu,
        spans,
    })
}

#[derive(Debug)]
struct MixedSizeLayoutEvaluationV1 {
    runs: Vec<AdmittedTypographyRunV1>,
    cursor: u32,
    used_height_emu: i64,
    terminal_mandatory_stop: Option<MixedLineCandidateV1>,
    lines: Vec<RenderResolvedTextLineV1>,
}

fn mixed_line_physical_extent_emu_v1(
    line: &RenderResolvedTextLineV1,
    font: &ExplicitRenderTextFontResourceV1<'_>,
) -> Option<i64> {
    let extent = line
        .spans
        .iter()
        .map(|span| {
            compatible_natural_line_height_emu_v1(
                font.bytes,
                font.face_index,
                LengthEmu::new(span.font_size_emu),
            )
            .map(LengthEmu::get)
        })
        .collect::<Option<Vec<_>>>()?
        .into_iter()
        .max()?;
    (extent > 0).then_some(extent.min(line.line_height_emu))
}

fn frozen_mixed_prefix_height_emu_v1(
    lines: &[RenderResolvedTextLineV1],
    font: &ExplicitRenderTextFontResourceV1<'_>,
) -> Option<i64> {
    let (first, rest) = lines.split_first()?;
    let mut height = mixed_line_physical_extent_emu_v1(first, font)?;
    for line in rest {
        height = height.checked_add(line.line_height_emu)?;
    }
    Some(height)
}

fn evaluate_mixed_size_text_layout_v1(
    fragment: &RenderTextFragmentV1,
    font: &ExplicitRenderTextFontResourceV1<'_>,
    node_id: NodeId,
    bounds: &RectEmu,
    fingerprint: &str,
) -> Result<MixedSizeLayoutEvaluationV1, RenderTextLayoutFallbackReasonV1> {
    let runs = admitted_typography_runs_v1(fragment, font.default_font_size_emu)?;
    if runs.len() < 2
        || runs
            .iter()
            .all(|run| run.font_size_emu == runs[0].font_size_emu)
    {
        return Err(RenderTextLayoutFallbackReasonV1::MixedTypographySize);
    }

    let scalars: Vec<char> = fragment.text.chars().collect();
    let scalar_count = u32::try_from(scalars.len())
        .map_err(|_| RenderTextLayoutFallbackReasonV1::SharedLayoutFailed)?;
    if fragment.scalar_start != 0 || fragment.scalar_end != scalar_count {
        return Err(RenderTextLayoutFallbackReasonV1::StoryExtentMismatch);
    }

    let mut policy_glyphs = Vec::new();
    let mut prepared_runs = Vec::with_capacity(runs.len());
    for run in &runs {
        let run_text = scalar_text_range_v1(&scalars, run.scalar_start, run.scalar_end)
            .ok_or(RenderTextLayoutFallbackReasonV1::SharedLayoutFailed)?;
        let runtime = BoundedShapingRuntime {
            layout: BoundedLayoutEnvironment {
                engine_revision: SHARED_TEXT_LAYOUT_REVISION_V1.to_owned(),
                font_set_fingerprint: fingerprint.to_owned(),
                resource_fingerprint: font.resource_id.to_owned(),
            },
            face_index: font.face_index,
            font_size_emu: LengthEmu::new(run.font_size_emu),
            font_bytes: font.bytes,
        };
        let shaped = shape_bounded_ltr_segment(&run_text, run.scalar_start, &runtime)
            .map_err(|_| RenderTextLayoutFallbackReasonV1::SharedLayoutFailed)?;
        prepared_runs.push(prepare_typography_run_v1(*run, &shaped)?);
        policy_glyphs.extend(shaped.glyphs);
    }
    let policy = break_policy_for_shaped_text(&fragment.text, &policy_glyphs)
        .map_err(|_| RenderTextLayoutFallbackReasonV1::SharedLayoutFailed)?;

    let mut cursor = fragment.scalar_start;
    let mut cursor_safe_without_reshaping = true;
    let mut used_height_emu = 0_i64;
    let mut terminal_mandatory_stop = None;
    let mut line_index = 0_u32;
    let mut lines = Vec::new();

    while cursor < fragment.scalar_end {
        let mut chosen = None;
        let mut rejected_terminal_mandatory = None;
        for candidate in policy
            .candidates
            .iter()
            .filter(|candidate| candidate.scalar_boundary > cursor)
        {
            let evaluated = if cursor_safe_without_reshaping
                && candidate.safe_without_reshaping
                && candidate.kind == BoundedBreakKind::Allowed
            {
                reuse_mixed_line_candidate_v1(
                    &scalars,
                    cursor,
                    candidate.scalar_boundary,
                    &prepared_runs,
                    font,
                )?
            } else {
                shape_mixed_line_candidate_v1(
                    &scalars,
                    cursor,
                    candidate.scalar_boundary,
                    candidate.kind,
                    &runs,
                    font,
                    fingerprint,
                )?
            };
            let fits_width = evaluated.measured_width_emu <= bounds.width.get();
            let fits_height = used_height_emu
                .checked_add(evaluated.line_height_emu)
                .is_some_and(|height| height <= bounds.height.get());
            if fits_width && fits_height {
                chosen = Some((evaluated, candidate.safe_without_reshaping));
            } else if fits_width
                && candidate.kind == BoundedBreakKind::Mandatory
                && evaluated.scalar_end == cursor
                && evaluated.consumed_scalar_end == fragment.scalar_end
                && evaluated.text.is_empty()
                && evaluated.spans.is_empty()
            {
                rejected_terminal_mandatory = Some(evaluated);
            }
            if candidate.kind == BoundedBreakKind::Mandatory {
                break;
            }
        }

        let Some((chosen, chosen_boundary_safe_without_reshaping)) = chosen else {
            terminal_mandatory_stop = rejected_terminal_mandatory;
            break;
        };
        used_height_emu = used_height_emu
            .checked_add(chosen.line_height_emu)
            .ok_or(RenderTextLayoutFallbackReasonV1::SharedLayoutFailed)?;
        let x_offset_emu = resolved_line_x_offset_emu_v1(
            fragment,
            node_id,
            bounds,
            line_index,
            cursor..chosen.scalar_end,
            chosen.measured_width_emu,
            fingerprint,
        );
        lines.push(RenderResolvedTextLineV1 {
            line_index,
            scalar_start: cursor,
            scalar_end: chosen.scalar_end,
            consumed_scalar_end: chosen.consumed_scalar_end,
            text: chosen.text,
            measured_width_emu: chosen.measured_width_emu,
            line_height_emu: chosen.line_height_emu,
            x_offset_emu,
            spans: chosen.spans,
            shaping: None,
        });
        cursor = chosen.consumed_scalar_end;
        cursor_safe_without_reshaping = chosen_boundary_safe_without_reshaping;
        line_index = line_index
            .checked_add(1)
            .ok_or(RenderTextLayoutFallbackReasonV1::SharedLayoutFailed)?;
    }

    Ok(MixedSizeLayoutEvaluationV1 {
        runs,
        cursor,
        used_height_emu,
        terminal_mandatory_stop,
        lines,
    })
}

pub(super) fn resolve_mixed_size_text_layout_v1(
    fragment: &RenderTextFragmentV1,
    font: &ExplicitRenderTextFontResourceV1<'_>,
    node_id: NodeId,
    bounds: &RectEmu,
    fingerprint: &str,
    vertical_alignment: Option<ViewerTextVerticalAlignment>,
) -> RenderTextLayoutV1 {
    let mut evaluation =
        match evaluate_mixed_size_text_layout_v1(fragment, font, node_id, bounds, fingerprint) {
            Ok(value) => value,
            Err(reason) => return fallback_layout(reason),
        };

    if evaluation.cursor != fragment.scalar_end {
        let Some(terminal) = evaluation.terminal_mandatory_stop.take() else {
            return fallback_layout(RenderTextLayoutFallbackReasonV1::SharedLayoutIncomplete);
        };
        let Some(prefix_height_emu) = frozen_mixed_prefix_height_emu_v1(&evaluation.lines, font)
        else {
            return fallback_layout(RenderTextLayoutFallbackReasonV1::SharedLayoutIncomplete);
        };
        let Some(completed_height_emu) = prefix_height_emu.checked_add(terminal.line_height_emu)
        else {
            return fallback_layout(RenderTextLayoutFallbackReasonV1::SharedLayoutIncomplete);
        };
        if completed_height_emu > bounds.height.get()
            || terminal.scalar_end != evaluation.cursor
            || terminal.consumed_scalar_end != fragment.scalar_end
        {
            return fallback_layout(RenderTextLayoutFallbackReasonV1::SharedLayoutIncomplete);
        }

        let Ok(line_index) = u32::try_from(evaluation.lines.len()) else {
            return fallback_layout(RenderTextLayoutFallbackReasonV1::SharedLayoutFailed);
        };
        evaluation.lines.push(RenderResolvedTextLineV1 {
            line_index,
            scalar_start: evaluation.cursor,
            scalar_end: terminal.scalar_end,
            consumed_scalar_end: terminal.consumed_scalar_end,
            text: terminal.text,
            measured_width_emu: terminal.measured_width_emu,
            line_height_emu: terminal.line_height_emu,
            x_offset_emu: 0,
            spans: terminal.spans,
            shaping: None,
        });
        evaluation.cursor = terminal.consumed_scalar_end;
        evaluation.used_height_emu = completed_height_emu;
    }

    if evaluation.cursor != fragment.scalar_end {
        return fallback_layout(RenderTextLayoutFallbackReasonV1::SharedLayoutIncomplete);
    }

    let max_font_size_emu = evaluation
        .runs
        .iter()
        .map(|run| run.font_size_emu)
        .max()
        .unwrap_or(font.default_font_size_emu);
    let max_line_height_emu = evaluation
        .lines
        .iter()
        .map(|line| line.line_height_emu)
        .max()
        .unwrap_or(font.default_line_height_emu);

    RenderTextLayoutV1 {
        disposition: RenderTextLayoutDispositionV1::SharedResolved {
            font_resource_id: font.resource_id.to_owned(),
            font_fingerprint_sha256: fingerprint.to_owned(),
            font_size_emu: max_font_size_emu,
            line_height_emu: max_line_height_emu,
        },
        vertical_offset_emu: resolved_vertical_offset_emu_v1(
            vertical_alignment,
            bounds.height.get(),
            evaluation.used_height_emu,
        ),
        lines: evaluation.lines,
    }
}

// Exact mixed-family source text is still explicitly LTR. The only newly
// admitted non-ASCII scalars are bounded ordinary Unicode punctuation: no
// bidi controls, combining marks, non-Latin scripts or ambient font lookup.
// The independent exact per-run face/SHA/shaping and spacing gates still apply.
pub(super) fn source_text_simple_ltr_v1(text: &str) -> bool {
    !text.is_empty()
        && text.chars().all(|ch| {
            ch.is_ascii()
                || matches!(
                    ch as u32,
                    0x00A1 | 0x00AB | 0x00BB | 0x00BF | 0x2010..=0x2027 | 0x2030..=0x203F
                )
        })
}

// Exact source-backed mixed-family terminal admission, preserving all earlier
// bounded break choices and accepted physical shaping.
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
        if span.scalar_start != cursor || span.scalar_end <= cursor || span.scalar_end > scalar_end
        {
            return None;
        }
        let mut covering = runs.iter().filter(|run| {
            run.scalar_start <= span.scalar_start && run.scalar_end >= span.scalar_end
        });
        let run = covering.next()?;
        if covering.next().is_some()
            || span.font_resource_id.as_deref() != Some(run.font.resource_id)
            || span.font_fingerprint_sha256.as_deref() != Some(run.font_fingerprint_sha256.as_str())
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
    let physical =
        physical_spans_extent_emu_v1(&first.spans, first.scalar_start, first.scalar_end, runs)?;
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
    let story = visual
        .document
        .stories
        .iter()
        .find(|story| story.id == story_id)?;
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
    let natural =
        physical_spans_extent_emu_v1(&terminal.spans, scalar_start, terminal.scalar_end, runs)?;
    let scaled = (i128::from(natural) * 3 + 2) / 4;
    let height = i64::try_from(scaled).ok()?;
    (height > 0).then_some(height)
}

pub(super) fn resolve_mixed_family_text_layout_v1<'a, G>(
    visual: &ViewerGeometryDocument,
    target: RenderTextLayoutTargetV1,
    fragment: &RenderTextFragmentV1,
    resolve_span_font: &mut G,
) -> Option<RenderTextLayoutV1>
where
    G: FnMut(
        &RenderTextFragmentV1,
        &RenderTypographyRunV1,
    ) -> Option<ExplicitRenderTextFontResourceV1<'a>>,
{
    if target.projected_target_frame_node_id.is_some()
        || target.bounds.width.get() <= 0
        || target.bounds.height.get() <= 0
    {
        return None;
    }

    let story = visual
        .document
        .stories
        .iter()
        .find(|story| story.id == fragment.story_id)?;
    let story_scalar_len = u32::try_from(story.text.chars().count()).ok()?;
    if fragment.scalar_start != 0
        || fragment.scalar_end != story_scalar_len
        || !render_text_is_story_equivalent_for_layout_v1(
            visual,
            target.page_id,
            target.node_id,
            target.projected_target_frame_node_id,
            &fragment.text,
            &story.text,
        )
    {
        return None;
    }
    admitted_layout_frame_ordinal(
        visual,
        fragment.story_id,
        target.node_id,
        target.projected_target_frame_node_id,
    )
    .ok()?;

    let runs = admitted_mixed_family_typography_runs_v1(fragment, resolve_span_font)?;
    let scalars: Vec<char> = fragment.text.chars().collect();

    let mut policy_glyphs = Vec::new();
    for run in &runs {
        let run_text = scalar_text_range_v1(&scalars, run.scalar_start, run.scalar_end)?;
        let runtime = BoundedShapingRuntime {
            layout: BoundedLayoutEnvironment {
                engine_revision: SHARED_TEXT_LAYOUT_REVISION_V1.to_owned(),
                font_set_fingerprint: run.font_fingerprint_sha256.clone(),
                resource_fingerprint: run.font.resource_id.to_owned(),
            },
            face_index: run.font.face_index,
            font_size_emu: LengthEmu::new(run.font_size_emu),
            font_bytes: run.font.bytes,
        };
        let shaped = shape_bounded_ltr_segment(&run_text, run.scalar_start, &runtime).ok()?;
        policy_glyphs.extend(shaped.glyphs);
    }
    let policy = break_policy_for_shaped_text(&fragment.text, &policy_glyphs).ok()?;
    let layout_fingerprint = mixed_family_layout_fingerprint_v1(&runs);

    let mut cursor = fragment.scalar_start;
    let mut used_height_emu = 0_i64;
    let mut line_index = 0_u32;
    let mut lines = Vec::new();

    while cursor < fragment.scalar_end {
        let mut chosen = None;
        // Retain only an unambiguous candidate selected by the existing
        // bounded break policy, never an invented terminal text range.
        let mut rejected_terminal = None;
        let mut ambiguous_terminal = false;
        for candidate in policy
            .candidates
            .iter()
            .filter(|candidate| candidate.scalar_boundary > cursor)
        {
            let evaluated = shape_mixed_family_line_candidate_v1(
                &scalars,
                cursor,
                candidate.scalar_boundary,
                candidate.kind,
                &runs,
            )
            .ok()?;
            let fits_width = evaluated.measured_width_emu <= target.bounds.width.get();
            let fits_height = used_height_emu
                .checked_add(evaluated.line_height_emu)
                .is_some_and(|height| height <= target.bounds.height.get());
            if fits_width && fits_height {
                chosen = Some(evaluated);
            } else if fits_width
                && evaluated.scalar_end > cursor
                && evaluated.consumed_scalar_end == fragment.scalar_end
                && !evaluated.text.is_empty()
                && !evaluated.spans.is_empty()
            {
                if rejected_terminal.is_some() {
                    ambiguous_terminal = true;
                } else {
                    rejected_terminal = Some(evaluated);
                }
            }
            if candidate.kind == BoundedBreakKind::Mandatory {
                break;
            }
        }

        let Some(chosen) = chosen else {
            if ambiguous_terminal {
                return None;
            }
            let terminal = rejected_terminal?;
            // Regular-only font packets authorize only explicitly proven
            // effective Regular source runs. Unknown style is not false.
            // Never substitute Regular for Bold, Italic, or missing authority.
            if fragment
                .typography
                .iter()
                .any(|run| run.bold != Some(false) || run.italic != Some(false))
            {
                return None;
            }
            let prefix_height = mixed_family_frozen_prefix_height_emu_v1(&lines, &runs)?;
            let terminal_advance = mixed_family_terminal_source_advance_emu_v1(
                visual,
                fragment.story_id,
                cursor,
                fragment.scalar_end,
                &terminal,
                &runs,
            )?;
            let completed = prefix_height.checked_add(terminal_advance)?;
            if completed > target.bounds.height.get() {
                return None;
            }
            let x_offset_emu = resolved_line_x_offset_emu_v1(
                fragment,
                target.node_id,
                &target.bounds,
                line_index,
                cursor..terminal.scalar_end,
                terminal.measured_width_emu,
                &layout_fingerprint,
            );
            lines.push(RenderResolvedTextLineV1 {
                line_index,
                scalar_start: cursor,
                scalar_end: terminal.scalar_end,
                consumed_scalar_end: terminal.consumed_scalar_end,
                text: terminal.text,
                measured_width_emu: terminal.measured_width_emu,
                line_height_emu: terminal_advance,
                x_offset_emu,
                spans: terminal.spans,
                shaping: None,
            });
            cursor = terminal.consumed_scalar_end;
            used_height_emu = completed;
            break;
        };
        used_height_emu = used_height_emu.checked_add(chosen.line_height_emu)?;
        let x_offset_emu = resolved_line_x_offset_emu_v1(
            fragment,
            target.node_id,
            &target.bounds,
            line_index,
            cursor..chosen.scalar_end,
            chosen.measured_width_emu,
            &layout_fingerprint,
        );
        lines.push(RenderResolvedTextLineV1 {
            line_index,
            scalar_start: cursor,
            scalar_end: chosen.scalar_end,
            consumed_scalar_end: chosen.consumed_scalar_end,
            text: chosen.text,
            measured_width_emu: chosen.measured_width_emu,
            line_height_emu: chosen.line_height_emu,
            x_offset_emu,
            spans: chosen.spans,
            shaping: None,
        });
        cursor = chosen.consumed_scalar_end;
        line_index = line_index.checked_add(1)?;
    }

    if cursor != fragment.scalar_end {
        return None;
    }

    let first = runs.first()?;
    let max_font_size_emu = runs
        .iter()
        .map(|run| run.font_size_emu)
        .max()
        .unwrap_or(first.font_size_emu);
    let max_line_height_emu = lines
        .iter()
        .map(|line| line.line_height_emu)
        .max()
        .unwrap_or(first.font.default_line_height_emu);

    Some(RenderTextLayoutV1 {
        disposition: RenderTextLayoutDispositionV1::SharedResolved {
            font_resource_id: first.font.resource_id.to_owned(),
            font_fingerprint_sha256: first.font_fingerprint_sha256.clone(),
            font_size_emu: max_font_size_emu,
            line_height_emu: max_line_height_emu,
        },
        vertical_offset_emu: resolved_vertical_offset_emu_v1(
            target.vertical_alignment,
            target.bounds.height.get(),
            used_height_emu,
        ),
        lines,
    })
}

#[cfg(test)]
mod tests {
    use super::super::tests::{fixture, render_fragment};
    use super::*;
    use pub_viewer::{ViewerParagraphLineSpacingRun, viewer_story_text_sha256};

    #[test]
    fn mixed_family_source_text_only_admits_bounded_ltr_punctuation() {
        assert!(source_text_simple_ltr_v1("AB\rCD"));
        assert!(source_text_simple_ltr_v1("A\u{2019}B\rCD"));
        assert!(source_text_simple_ltr_v1("A\u{2013}B"));
        assert!(source_text_simple_ltr_v1("A\u{00AB}B"));

        assert!(!source_text_simple_ltr_v1(""));
        assert!(!source_text_simple_ltr_v1("A\u{00A0}B"));
        assert!(!source_text_simple_ltr_v1("A\u{0301}B"));
        assert!(!source_text_simple_ltr_v1("A\u{05D0}B"));
        assert!(!source_text_simple_ltr_v1("A\u{4E2D}B"));
        assert!(!source_text_simple_ltr_v1("A\u{200F}B"));
    }

    #[test]
    fn mixed_family_terminal_114300_needs_exact_fonts_and_fresh_unique_spacing() {
        let mut visual = fixture();
        let story_id = visual.document.stories[0].id;
        let page_id = visual.document.pages[0].id;
        let node_id = visual.scene.nodes[0].origin;
        let text = "AB\rCD";
        visual.document.stories[0].text = text.to_owned();
        visual.story_frames.push(pub_viewer::ViewerStoryFrame {
            story_id,
            frame_id: node_id,
            ordinal: 0,
            text_content_bounds: None,
            vertical_alignment: None,
        });

        let typography = vec![
            RenderTypographyRunV1 {
                scalar_start: 0,
                scalar_end: 3,
                source_font_name: "Family A".to_owned(),
                text_size_emu: 12 * 12_700,
                font_inherited: false,
                size_inherited: false,
                color_rgb: None,
                color_inherited: false,
                bold: Some(false),
                italic: Some(false),
            },
            RenderTypographyRunV1 {
                scalar_start: 3,
                scalar_end: 5,
                source_font_name: "Family B".to_owned(),
                text_size_emu: 18 * 12_700,
                font_inherited: false,
                size_inherited: false,
                color_rgb: None,
                color_inherited: false,
                bold: Some(false),
                italic: Some(false),
            },
        ];
        let fragment = render_fragment(story_id, text, typography);
        assert_eq!(effective_source_font_family_v1(&visual, &fragment), None);

        let first_bytes: &[u8] = font_test_data::AHEM;
        let second_bytes: &[u8] = font_test_data::TINOS_SUBSET;
        let first_sha = font_fingerprint_sha256(first_bytes);
        let second_sha = font_fingerprint_sha256(second_bytes);
        let mut resolve = |_: &RenderTextFragmentV1, run: &RenderTypographyRunV1| match run
            .source_font_name
            .as_str()
        {
            "Family A" => Some(ExplicitRenderTextFontResourceV1 {
                resource_id: "exact-a",
                expected_sha256: &first_sha,
                face_index: 0,
                default_font_size_emu: 12 * 12_700,
                default_line_height_emu: 30 * 12_700,
                bytes: first_bytes,
            }),
            "Family B" => Some(ExplicitRenderTextFontResourceV1 {
                resource_id: "exact-b",
                expected_sha256: &second_sha,
                face_index: 0,
                default_font_size_emu: 12 * 12_700,
                default_line_height_emu: 30 * 12_700,
                bytes: second_bytes,
            }),
            _ => None,
        };
        let first_extent =
            compatible_natural_line_height_emu_v1(first_bytes, 0, LengthEmu::new(12 * 12_700))
                .map(LengthEmu::get)
                .expect("exact first physical metric");
        let terminal_extent =
            compatible_natural_line_height_emu_v1(second_bytes, 0, LengthEmu::new(18 * 12_700))
                .map(LengthEmu::get)
                .expect("exact terminal physical metric");
        let expected_terminal_advance = (terminal_extent * 3 + 2) / 4;
        // Admit the first line at its normal baseline, then require the
        // bounded physical-prefix retry only for the terminal visible line.
        let frame_height =
            (30 * 12_700).max(first_extent.min(30 * 12_700) + expected_terminal_advance);
        assert!(
            30 * 12_700 + 45 * 12_700 > frame_height,
            "ordinary baseline must overflow the selected narrow frame"
        );
        let target = RenderTextLayoutTargetV1 {
            page_id,
            page_size: Size2D::new(LengthEmu::new(10_000_000), LengthEmu::new(10_000_000)),
            node_id,
            projected_target_frame_node_id: None,
            vertical_alignment: None,
            bounds: RectEmu::new(
                LengthEmu::ZERO,
                LengthEmu::ZERO,
                LengthEmu::new(10_000_000),
                LengthEmu::new(frame_height),
            ),
            transform: Affine2D::identity(),
        };

        // Missing spacing is a negative even though both exact fonts exist.
        assert!(
            resolve_mixed_family_text_layout_v1(&visual, target.clone(), &fragment, &mut resolve,)
                .is_none()
        );

        visual.paragraph_line_spacings = vec![ViewerParagraphLineSpacingRun {
            story_id,
            scalar_start: 3,
            scalar_end: 5,
            line_spacing: ViewerParagraphLineSpacing::Proportional {
                point_equivalent_emu: 9 * 12_700,
            },
            source_value: Some(914_402),
            source_story_text_sha256: viewer_story_text_sha256(text),
        }];
        let layout =
            resolve_mixed_family_text_layout_v1(&visual, target.clone(), &fragment, &mut resolve)
                .expect("one uniquely sourced mixed-family terminal line must fit");
        assert!(matches!(
            layout.disposition,
            RenderTextLayoutDispositionV1::SharedResolved { .. }
        ));
        assert_eq!(layout.lines.len(), 2);
        assert_eq!(layout.lines[0].text, "AB");
        assert_eq!(layout.lines[1].text, "CD");
        assert_eq!(layout.lines[1].scalar_start, 3);
        assert_eq!(layout.lines[1].scalar_end, 5);
        assert_eq!(layout.lines[1].consumed_scalar_end, 5);
        assert_eq!(layout.lines[1].line_height_emu, expected_terminal_advance);
        assert_eq!(
            layout.lines[1].spans[0].font_resource_id.as_deref(),
            Some("exact-b")
        );
        assert!(layout.lines[1].spans[0].shaping.is_some());

        // The source predicate is fail-closed, not a numeric/page heuristic.
        visual.paragraph_line_spacings[0].source_story_text_sha256 =
            viewer_story_text_sha256("stale");
        assert!(
            resolve_mixed_family_text_layout_v1(&visual, target.clone(), &fragment, &mut resolve,)
                .is_none()
        );
        visual.paragraph_line_spacings[0].source_story_text_sha256 = viewer_story_text_sha256(text);
        visual
            .paragraph_line_spacings
            .push(visual.paragraph_line_spacings[0].clone());
        assert!(
            resolve_mixed_family_text_layout_v1(&visual, target.clone(), &fragment, &mut resolve,)
                .is_none()
        );
        visual.paragraph_line_spacings.truncate(1);
        visual.paragraph_line_spacings[0].line_spacing = ViewerParagraphLineSpacing::Proportional {
            point_equivalent_emu: 12 * 12_700,
        };
        assert!(
            resolve_mixed_family_text_layout_v1(&visual, target.clone(), &fragment, &mut resolve,)
                .is_none()
        );
        visual.paragraph_line_spacings[0].line_spacing = ViewerParagraphLineSpacing::Proportional {
            point_equivalent_emu: 9 * 12_700,
        };
        let mut missing_face = |_: &RenderTextFragmentV1, run: &RenderTypographyRunV1| {
            (run.source_font_name == "Family A").then_some(ExplicitRenderTextFontResourceV1 {
                resource_id: "exact-a",
                expected_sha256: &first_sha,
                face_index: 0,
                default_font_size_emu: 12 * 12_700,
                default_line_height_emu: 30 * 12_700,
                bytes: first_bytes,
            })
        };
        assert!(
            resolve_mixed_family_text_layout_v1(
                &visual,
                target.clone(),
                &fragment,
                &mut missing_face,
            )
            .is_none()
        );

        let mut styled_fragment = fragment.clone();
        styled_fragment.typography[1].bold = Some(true);
        assert!(
            resolve_mixed_family_text_layout_v1(
                &visual,
                target.clone(),
                &styled_fragment,
                &mut resolve,
            )
            .is_none(),
            "Regular family packet cannot authorize Bold source-face retry"
        );

        // Unknown effective Bold/Italic is not proof of Regular. The
        // Regular-only font packet must fail closed on either missing boolean.
        let mut unknown_bold = fragment.clone();
        unknown_bold.typography[0].bold = None;
        assert!(
            resolve_mixed_family_text_layout_v1(
                &visual,
                target.clone(),
                &unknown_bold,
                &mut resolve,
            )
            .is_none(),
            "unknown Bold cannot silently become effective Regular"
        );

        let mut unknown_italic = fragment.clone();
        unknown_italic.typography[1].italic = None;
        assert!(
            resolve_mixed_family_text_layout_v1(&visual, target, &unknown_italic, &mut resolve,)
                .is_none(),
            "unknown Italic cannot silently become effective Regular"
        );
    }

    fn mixed_fragment(text: &str) -> RenderTextFragmentV1 {
        let scalar_end = u32::try_from(text.chars().count()).expect("bounded fixture");
        RenderTextFragmentV1 {
            story_id: StoryId::from_canonical(pub_model::CanonicalId::from_bytes([3; 16])),
            scalar_start: 0,
            scalar_end,
            text: text.to_owned(),
            line_count: 0,
            typography: vec![
                RenderTypographyRunV1 {
                    scalar_start: 0,
                    scalar_end: 3,
                    source_font_name: "Noto Serif".to_owned(),
                    text_size_emu: 12 * 12_700,
                    font_inherited: false,
                    size_inherited: false,
                    color_rgb: None,
                    color_inherited: false,
                    bold: None,
                    italic: None,
                },
                RenderTypographyRunV1 {
                    scalar_start: 3,
                    scalar_end,
                    source_font_name: "Noto Serif".to_owned(),
                    text_size_emu: 18 * 12_700,
                    font_inherited: false,
                    size_inherited: false,
                    color_rgb: None,
                    color_inherited: false,
                    bold: None,
                    italic: None,
                },
            ],
            paragraph_alignments: Vec::new(),
            backend_font_resource_id: None,
            layout: None,
        }
    }

    #[test]
    fn mixed_size_terminal_mandatory_break_completes_without_relayouting_visible_prefix() {
        let bytes = font_test_data::NOTOSERIF_AUTOHINT_SHAPING;
        let fingerprint = font_fingerprint_sha256(bytes);
        let default_font_size_emu = 12 * 12_700;
        let default_line_height_emu = 30 * 12_700;
        let font = ExplicitRenderTextFontResourceV1 {
            resource_id: "test:noto-serif",
            expected_sha256: &fingerprint,
            face_index: 0,
            default_font_size_emu,
            default_line_height_emu,
            bytes,
        };
        let fragment = mixed_fragment("aa\r\r");
        let node_id = NodeId::from_canonical(pub_model::CanonicalId::from_bytes([4; 16]));
        let first_physical_extent_emu =
            compatible_natural_line_height_emu_v1(bytes, 0, LengthEmu::new(default_font_size_emu))
                .map(LengthEmu::get)
                .expect("test font physical extent")
                .min(default_line_height_emu);
        assert!(first_physical_extent_emu < default_line_height_emu);

        let bounds = RectEmu::new(
            LengthEmu::ZERO,
            LengthEmu::ZERO,
            LengthEmu::new(10_000_000),
            LengthEmu::new(
                first_physical_extent_emu
                    .checked_add(default_line_height_emu)
                    .expect("bounded terminal completion height"),
            ),
        );

        let baseline =
            evaluate_mixed_size_text_layout_v1(&fragment, &font, node_id, &bounds, &fingerprint)
                .expect("baseline mixed-size evaluation");
        assert_eq!(baseline.cursor, 3);
        assert_eq!(baseline.lines.len(), 1);
        assert_eq!(baseline.lines[0].text, "aa");
        let terminal = baseline
            .terminal_mandatory_stop
            .as_ref()
            .expect("terminal paragraph break is the only rejected remainder");
        assert_eq!(terminal.scalar_end, baseline.cursor);
        assert_eq!(terminal.consumed_scalar_end, fragment.scalar_end);
        assert!(terminal.text.is_empty());
        assert!(terminal.spans.is_empty());
        assert!(
            baseline
                .used_height_emu
                .checked_add(terminal.line_height_emu)
                .is_some_and(|height| height > bounds.height.get()),
            "legacy all-baseline accounting must reject the terminal line"
        );
        assert!(
            frozen_mixed_prefix_height_emu_v1(&baseline.lines, &font)
                .and_then(|height| height.checked_add(terminal.line_height_emu))
                .is_some_and(|height| height <= bounds.height.get()),
            "frozen visible prefix plus terminal baseline must fit"
        );

        let resolved = resolve_mixed_size_text_layout_v1(
            &fragment,
            &font,
            node_id,
            &bounds,
            &fingerprint,
            None,
        );
        assert!(matches!(
            resolved.disposition,
            RenderTextLayoutDispositionV1::SharedResolved { .. }
        ));
        assert_eq!(resolved.lines.len(), 2);
        assert_eq!(resolved.lines[0].text, "aa");
        assert_eq!(resolved.lines[0].scalar_start, 0);
        assert_eq!(resolved.lines[0].scalar_end, 2);
        assert_eq!(resolved.lines[0].consumed_scalar_end, 3);
        assert!(resolved.lines[1].text.is_empty());
        assert!(resolved.lines[1].spans.is_empty());
        assert_eq!(resolved.lines[1].scalar_start, 3);
        assert_eq!(resolved.lines[1].scalar_end, 3);
        assert_eq!(resolved.lines[1].consumed_scalar_end, 4);
    }

    #[test]
    fn mixed_size_partial_layout_stays_fail_closed_after_visible_height_exhaustion() {
        let bytes = font_test_data::NOTOSERIF_AUTOHINT_SHAPING;
        let fingerprint = font_fingerprint_sha256(bytes);
        let font = ExplicitRenderTextFontResourceV1 {
            resource_id: "test:noto-serif",
            expected_sha256: &fingerprint,
            face_index: 0,
            default_font_size_emu: 12 * 12_700,
            default_line_height_emu: 14 * 12_700,
            bytes,
        };
        let fragment = mixed_fragment("aa\rbb");
        let node_id = NodeId::from_canonical(pub_model::CanonicalId::from_bytes([4; 16]));

        let one_line_bounds = RectEmu::new(
            LengthEmu::ZERO,
            LengthEmu::ZERO,
            LengthEmu::new(10_000_000),
            LengthEmu::new(14 * 12_700),
        );
        let partial = resolve_mixed_size_text_layout_v1(
            &fragment,
            &font,
            node_id,
            &one_line_bounds,
            &fingerprint,
            None,
        );
        assert_eq!(
            partial.disposition,
            RenderTextLayoutDispositionV1::BackendFallback {
                reason: RenderTextLayoutFallbackReasonV1::SharedLayoutIncomplete,
            }
        );
        assert!(partial.lines.is_empty());

        let zero_line_bounds = RectEmu::new(
            LengthEmu::ZERO,
            LengthEmu::ZERO,
            LengthEmu::new(10_000_000),
            LengthEmu::new(14 * 12_700 - 1),
        );
        let zero_line = resolve_mixed_size_text_layout_v1(
            &fragment,
            &font,
            node_id,
            &zero_line_bounds,
            &fingerprint,
            None,
        );
        assert_eq!(
            zero_line.disposition,
            RenderTextLayoutDispositionV1::BackendFallback {
                reason: RenderTextLayoutFallbackReasonV1::SharedLayoutIncomplete,
            }
        );
        assert!(zero_line.lines.is_empty());
    }
}
