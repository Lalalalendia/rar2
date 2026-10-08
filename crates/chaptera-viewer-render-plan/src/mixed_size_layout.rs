use super::*;
// Measurement-only base control for #2141; no product behavior change.\n
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
    lines: Vec<RenderResolvedTextLineV1>,
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
    let mut line_index = 0_u32;
    let mut lines = Vec::new();

    while cursor < fragment.scalar_end {
        let mut chosen = None;
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
            }
            if candidate.kind == BoundedBreakKind::Mandatory {
                break;
            }
        }

        let Some((chosen, chosen_boundary_safe_without_reshaping)) = chosen else {
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
    let evaluation =
        match evaluate_mixed_size_text_layout_v1(fragment, font, node_id, bounds, fingerprint) {
            Ok(value) => value,
            Err(reason) => return fallback_layout(reason),
        };

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

#[cfg(test)]
mod tests {
    use super::*;

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
