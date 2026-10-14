use crate::text_flow::explicit_chain;
use crate::{
    BOUNDED_SHAPER_REVISION, BoundedBreakCandidate, BoundedBreakKind, BoundedBreakPolicyError,
    BoundedLayoutProjection, BoundedShapeError, BoundedShapedGlyph, BoundedShapingDescriptor,
    BoundedShapingRuntime, ResolveBlocked, ResolveDiagnostic, ResolveSeverity,
    ResolvedPhysicalNode, ResolvedSurface, SceneOriginMapping, break_policy_for_shaped_text,
    resolve_bounded_geometry, shape_bounded_ltr, shape_bounded_ltr_segment,
};
use pub_model::{LengthEmu, NodeId, RectEmu, StoryId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

#[derive(Debug, Clone)]
pub struct BoundedShapedFlowRuntime<'a> {
    pub shaping: BoundedShapingRuntime<'a>,
    pub line_height: LengthEmu,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoundedShapedFlowDescriptor {
    pub shaping: BoundedShapingDescriptor,
    pub line_height: LengthEmu,
}

/// Source-neutral paragraph-flow constraints consumed during Story -> frame
/// allocation. These carry no Publisher/Quill bytes or inheritance claims.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BoundedParagraphFlowConstraint {
    StartInNextTextBox,
    KeepLinesTogether,
    KeepWithNext,
    WidowControl,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoundedParagraphFlowRun {
    pub story_origin: StoryId,
    pub scalar_start: u32,
    pub scalar_end: u32,
    pub constraint: BoundedParagraphFlowConstraint,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoundedShapedLine {
    pub story_origin: StoryId,
    pub frame_origin: NodeId,
    pub frame_line_index: u32,
    pub scalar_start: u32,
    /// End of visible shaped text, excluding a trailing CR/LF mandatory delimiter.
    pub scalar_end: u32,
    /// Logical Story position consumed by this line, including a hard-break delimiter.
    pub consumed_scalar_end: u32,
    pub text: String,
    /// OpenType units-per-em from the exact shaping result that produced this line.
    ///
    /// Fixed-output consumers use this with the already-resolved glyphs and
    /// must not reshape merely to recover font metrics.
    pub units_per_em: u32,
    pub measured_width: LengthEmu,
    pub glyphs: Vec<BoundedShapedGlyph>,
    pub break_kind: BoundedBreakKind,
    pub reshaped_for_break: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShapedLineOriginMapping {
    pub story_origin: StoryId,
    pub frame_origin: NodeId,
    pub scalar_start: u32,
    pub scalar_end: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoundedShapedFlowScene {
    pub environment: BoundedShapedFlowDescriptor,
    pub surfaces: Vec<ResolvedSurface>,
    pub nodes: Vec<ResolvedPhysicalNode>,
    pub lines: Vec<BoundedShapedLine>,
    pub origin_mapping: Vec<SceneOriginMapping>,
    pub line_origin_mapping: Vec<ShapedLineOriginMapping>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<ResolveDiagnostic>,
}

impl BoundedShapedFlowScene {
    /// Losslessly expose the physical geometry already carried by shaped flow.
    pub fn geometry_scene(&self) -> crate::BoundedResolvedScene {
        crate::BoundedResolvedScene {
            environment: self.environment.shaping.layout.clone(),
            surfaces: self.surfaces.clone(),
            nodes: self.nodes.clone(),
            origin_mapping: self.origin_mapping.clone(),
            diagnostics: self.diagnostics.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BoundedShapedFlowError {
    ProjectionBlocked(ResolveBlocked),
    Shape(BoundedShapeError),
    BreakPolicy(BoundedBreakPolicyError),
    NonPositiveLineHeight { line_height_emu: i64 },
    MetricOverflow,
}

impl fmt::Display for BoundedShapedFlowError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ProjectionBlocked(error) => write!(
                f,
                "projection blocked shaped flow with {} errors",
                error.projection_errors.len()
            ),
            Self::Shape(error) => write!(f, "text shaping failed: {error}"),
            Self::BreakPolicy(error) => write!(f, "line-break policy failed: {error}"),
            Self::NonPositiveLineHeight { line_height_emu } => {
                write!(f, "line height must be positive, got {line_height_emu} EMU")
            }
            Self::MetricOverflow => write!(f, "shaped line metric accumulation overflowed"),
        }
    }
}

impl std::error::Error for BoundedShapedFlowError {}

#[derive(Debug, Clone)]
struct EvaluatedBreak {
    visible_end: usize,
    consumed_end: usize,
    text: String,
    units_per_em: u32,
    measured_width: LengthEmu,
    glyphs: Vec<BoundedShapedGlyph>,
    kind: BoundedBreakKind,
    reshaped: bool,
}

#[derive(Debug, Clone, Copy, Default)]
struct ParagraphFlowState {
    scalar_end: usize,
    start_in_next_text_box: bool,
    keep_lines_together: bool,
    keep_with_next: bool,
    widow_control: bool,
}

fn paragraph_flow_state_at_cursor(
    runs: &[BoundedParagraphFlowRun],
    story_origin: StoryId,
    cursor: usize,
    scalar_count: usize,
) -> Option<ParagraphFlowState> {
    let cursor_u32 = u32::try_from(cursor).ok()?;
    let mut state = ParagraphFlowState::default();
    let mut observed_end = None;

    for run in runs
        .iter()
        .filter(|run| run.story_origin == story_origin && run.scalar_start == cursor_u32)
    {
        let end = usize::try_from(run.scalar_end).ok()?;
        if end <= cursor || end > scalar_count {
            return None;
        }
        if observed_end.is_some_and(|current| current != end) {
            return None;
        }
        observed_end = Some(end);
        match run.constraint {
            BoundedParagraphFlowConstraint::StartInNextTextBox => {
                state.start_in_next_text_box = true;
            }
            BoundedParagraphFlowConstraint::KeepLinesTogether => {
                state.keep_lines_together = true;
            }
            BoundedParagraphFlowConstraint::KeepWithNext => {
                state.keep_with_next = true;
            }
            BoundedParagraphFlowConstraint::WidowControl => {
                state.widow_control = true;
            }
        }
    }

    state.scalar_end = observed_end?;
    Some(state)
}

fn next_paragraph_end_v1(scalars: &[char], start: usize) -> Option<usize> {
    if start >= scalars.len() {
        return None;
    }
    for (offset, scalar) in scalars[start..].iter().enumerate() {
        if matches!(scalar, '\r' | '\n') {
            return start.checked_add(offset)?.checked_add(1);
        }
    }
    Some(scalars.len())
}

fn frame_row_capacity_for_bounds_v1(
    bounds: &RectEmu,
    runtime: &BoundedShapedFlowRuntime<'_>,
) -> Result<usize, BoundedShapedFlowError> {
    if bounds.width.get() <= 0 || bounds.height.get() <= 0 {
        return Ok(0);
    }
    let first_line_extent_emu = crate::compatible_natural_line_height_emu_v1(
        runtime.shaping.font_bytes,
        runtime.shaping.face_index,
        runtime.shaping.font_size_emu,
    )
    .map(LengthEmu::get)
    .map(|extent| extent.min(runtime.line_height.get()))
    .unwrap_or(runtime.line_height.get());
    shaped_line_capacity_v1(
        bounds.height.get(),
        first_line_extent_emu,
        runtime.line_height.get(),
    )
    .ok_or(BoundedShapedFlowError::MetricOverflow)
}

fn line_count_to_boundary_v1(
    scalars: &[char],
    shaped: &crate::BoundedShapedText,
    policy: &crate::BoundedBreakPolicy,
    start: usize,
    target_end: usize,
    width: LengthEmu,
    runtime: &BoundedShapedFlowRuntime<'_>,
) -> Result<Option<usize>, BoundedShapedFlowError> {
    if start >= target_end || target_end > scalars.len() {
        return Ok(None);
    }

    let scalar_count_u32 =
        u32::try_from(scalars.len()).map_err(|_| BoundedShapedFlowError::MetricOverflow)?;
    let mut cursor = start;
    let mut count = 0usize;

    while cursor < target_end {
        let cursor_u32 =
            u32::try_from(cursor).map_err(|_| BoundedShapedFlowError::MetricOverflow)?;
        let start_safe = boundary_safe(&policy.candidates, cursor_u32, scalar_count_u32);
        let mut chosen: Option<EvaluatedBreak> = None;

        for candidate in policy.candidates.iter().filter(|candidate| {
            usize::try_from(candidate.scalar_boundary)
                .is_ok_and(|end| end > cursor && end <= target_end)
        }) {
            let evaluated = evaluate_candidate(
                scalars,
                &shaped.glyphs,
                shaped.units_per_em,
                cursor,
                start_safe,
                candidate,
                &runtime.shaping,
            )?;
            if evaluated.measured_width.get() <= width.get() {
                chosen = Some(evaluated);
            }
            if candidate.kind == BoundedBreakKind::Mandatory {
                break;
            }
        }

        let Some(chosen) = chosen else {
            return Ok(None);
        };
        if chosen.consumed_end <= cursor {
            return Ok(None);
        }
        cursor = chosen.consumed_end;
        count = count
            .checked_add(1)
            .ok_or(BoundedShapedFlowError::MetricOverflow)?;
    }

    if cursor == target_end {
        Ok(Some(count))
    } else {
        Ok(None)
    }
}

// This helper intentionally takes the complete local allocation context so
// every decision is made from the same shaped Story/frame snapshot.
#[allow(clippy::too_many_arguments)]
fn should_defer_paragraph_flow_v1(
    state: ParagraphFlowState,
    scalars: &[char],
    shaped: &crate::BoundedShapedText,
    policy: &crate::BoundedBreakPolicy,
    cursor: usize,
    remaining_rows: usize,
    current_bounds: &RectEmu,
    successor_bounds: Option<&RectEmu>,
    runtime: &BoundedShapedFlowRuntime<'_>,
) -> Result<bool, BoundedShapedFlowError> {
    // Native Publisher evidence also covers the no-next-frame arm: once a
    // non-initial StartInNextTextBox paragraph is reached, the paragraph and
    // following Story content remain in overflow when no successor exists.
    if state.start_in_next_text_box {
        return Ok(true);
    }

    let Some(successor_bounds) = successor_bounds else {
        return Ok(false);
    };
    let successor_rows = frame_row_capacity_for_bounds_v1(successor_bounds, runtime)?;
    if successor_rows == 0 {
        return Ok(false);
    }

    let current_paragraph_lines = line_count_to_boundary_v1(
        scalars,
        shaped,
        policy,
        cursor,
        state.scalar_end,
        current_bounds.width,
        runtime,
    )?;
    let successor_paragraph_lines = line_count_to_boundary_v1(
        scalars,
        shaped,
        policy,
        cursor,
        state.scalar_end,
        successor_bounds.width,
        runtime,
    )?;

    if state.keep_lines_together
        && current_paragraph_lines.is_some_and(|lines| lines > remaining_rows)
        && successor_paragraph_lines.is_some_and(|lines| lines <= successor_rows)
    {
        return Ok(true);
    }

    if state.widow_control
        && remaining_rows == 1
        && current_paragraph_lines.is_some_and(|lines| lines > 1)
        && successor_paragraph_lines.is_some_and(|lines| lines <= successor_rows)
    {
        return Ok(true);
    }

    if state.keep_with_next {
        if let Some(pair_end) = next_paragraph_end_v1(scalars, state.scalar_end) {
            if pair_end > state.scalar_end {
                let current_pair_lines = line_count_to_boundary_v1(
                    scalars,
                    shaped,
                    policy,
                    cursor,
                    pair_end,
                    current_bounds.width,
                    runtime,
                )?;
                let successor_pair_lines = line_count_to_boundary_v1(
                    scalars,
                    shaped,
                    policy,
                    cursor,
                    pair_end,
                    successor_bounds.width,
                    runtime,
                )?;
                if current_pair_lines.is_some_and(|lines| lines > remaining_rows)
                    && successor_pair_lines.is_some_and(|lines| lines <= successor_rows)
                {
                    return Ok(true);
                }
            }
        }
    }

    Ok(false)
}

fn shaped_line_capacity_v1(
    frame_height_emu: i64,
    first_line_extent_emu: i64,
    baseline_advance_emu: i64,
) -> Option<usize> {
    if frame_height_emu <= 0 || first_line_extent_emu <= 0 || baseline_advance_emu <= 0 {
        return Some(0);
    }
    if frame_height_emu < first_line_extent_emu {
        return Some(0);
    }

    let remaining = frame_height_emu.checked_sub(first_line_extent_emu)?;
    let subsequent = remaining / baseline_advance_emu;
    usize::try_from(subsequent.checked_add(1)?).ok()
}

/// LAYOUT-RESOLVE-01B2C: execute Unicode break policy over real shaping and
/// explicit linked-frame flow.
///
/// The Story is shaped once to derive UAX #14 policy and safe-reuse boundaries.
/// When either the line start or chosen end boundary cannot safely split that
/// full-run shape, the exact visible line segment is shaped independently and
/// its clusters are rebased to Story-global Unicode-scalar indices.
///
/// This remains an LTR bounded proof. Bidi, fallback, hyphenation,
/// justification and language-specific tailoring are separate gates.
pub fn resolve_bounded_shaped_flow(
    projection: &BoundedLayoutProjection,
    runtime: &BoundedShapedFlowRuntime<'_>,
) -> Result<BoundedShapedFlowScene, BoundedShapedFlowError> {
    resolve_bounded_shaped_flow_with_paragraph_flow(projection, runtime, &[])
}

pub fn resolve_bounded_shaped_flow_with_paragraph_flow(
    projection: &BoundedLayoutProjection,
    runtime: &BoundedShapedFlowRuntime<'_>,
    paragraph_flow: &[BoundedParagraphFlowRun],
) -> Result<BoundedShapedFlowScene, BoundedShapedFlowError> {
    if runtime.line_height.get() <= 0 {
        return Err(BoundedShapedFlowError::NonPositiveLineHeight {
            line_height_emu: runtime.line_height.get(),
        });
    }

    let base = resolve_bounded_geometry(projection, runtime.shaping.layout.clone())
        .map_err(BoundedShapedFlowError::ProjectionBlocked)?;
    let mut diagnostics: Vec<_> = base
        .diagnostics
        .into_iter()
        .filter(|diagnostic| diagnostic.code != "story_text_layout_not_implemented")
        .collect();

    let geometry: BTreeMap<_, _> = projection
        .node_geometry
        .iter()
        .map(|node| (node.origin, node.bounds))
        .collect();

    let mut lines = Vec::new();
    let mut line_origin_mapping = Vec::new();

    for story in &projection.stories {
        let frames: Vec<_> = projection
            .story_frames
            .iter()
            .filter(|frame| frame.story_origin == story.origin)
            .collect();

        if frames.is_empty() {
            diagnostics.push(ResolveDiagnostic {
                code: "story_has_no_frames".into(),
                severity: ResolveSeverity::FidelityWarning,
                origin: story.origin.into_canonical(),
                message: "story content has no projected text frame".into(),
            });
            continue;
        }

        let chain = match explicit_chain(&frames) {
            Ok(chain) => chain,
            Err(code) => {
                diagnostics.push(ResolveDiagnostic {
                    code: code.into(),
                    severity: ResolveSeverity::FidelityWarning,
                    origin: story.origin.into_canonical(),
                    message: "story flow is not a single explicit linked-frame chain".into(),
                });
                continue;
            }
        };

        let shaped = match shape_bounded_ltr(&story.text, &runtime.shaping) {
            Ok(shaped) => shaped,
            Err(error) => {
                diagnostics.push(shape_failure_diagnostic(story.origin, &error));
                continue;
            }
        };
        let policy = break_policy_for_shaped_text(&story.text, &shaped.glyphs)
            .map_err(BoundedShapedFlowError::BreakPolicy)?;
        let scalars: Vec<char> = story.text.chars().collect();
        let scalar_count = scalars.len();
        let scalar_count_u32 =
            u32::try_from(scalar_count).map_err(|_| BoundedShapedFlowError::MetricOverflow)?;
        let mut cursor = 0usize;

        for (chain_index, frame_origin) in chain.iter().copied().enumerate() {
            if cursor == scalar_count {
                break;
            }

            let Some(bounds) = geometry.get(&frame_origin) else {
                diagnostics.push(ResolveDiagnostic {
                    code: "text_frame_geometry_missing".into(),
                    severity: ResolveSeverity::FidelityWarning,
                    origin: frame_origin.into_canonical(),
                    message: "text frame has no geometry available to the resolver".into(),
                });
                continue;
            };

            if bounds.width.get() <= 0 || bounds.height.get() <= 0 {
                diagnostics.push(ResolveDiagnostic {
                    code: "text_frame_has_no_capacity".into(),
                    severity: ResolveSeverity::FidelityWarning,
                    origin: frame_origin.into_canonical(),
                    message: "text frame must have positive width and height".into(),
                });
                continue;
            }

            let row_count = frame_row_capacity_for_bounds_v1(bounds, runtime)?;
            if row_count == 0 {
                diagnostics.push(ResolveDiagnostic {
                    code: "text_frame_has_no_capacity".into(),
                    severity: ResolveSeverity::FidelityWarning,
                    origin: frame_origin.into_canonical(),
                    message: "text frame cannot fit one physical first-line extent".into(),
                });
                continue;
            }
            let mut frame_stalled = false;

            for frame_line_index in 0..row_count {
                if cursor == scalar_count {
                    break;
                }

                if frame_line_index > 0 {
                    if let Some(state) = paragraph_flow_state_at_cursor(
                        paragraph_flow,
                        story.origin,
                        cursor,
                        scalar_count,
                    ) {
                        let successor_bounds = chain
                            .get(chain_index + 1)
                            .and_then(|next_frame| geometry.get(next_frame));
                        let remaining_rows = row_count - frame_line_index;
                        if should_defer_paragraph_flow_v1(
                            state,
                            &scalars,
                            &shaped,
                            &policy,
                            cursor,
                            remaining_rows,
                            bounds,
                            successor_bounds,
                            runtime,
                        )? {
                            break;
                        }
                    }
                }

                let cursor_u32 =
                    u32::try_from(cursor).map_err(|_| BoundedShapedFlowError::MetricOverflow)?;
                let start_safe = boundary_safe(&policy.candidates, cursor_u32, scalar_count_u32);
                let mut chosen: Option<EvaluatedBreak> = None;

                for candidate in policy.candidates.iter().filter(|candidate| {
                    usize::try_from(candidate.scalar_boundary).is_ok_and(|end| end > cursor)
                }) {
                    let evaluated = evaluate_candidate(
                        &scalars,
                        &shaped.glyphs,
                        shaped.units_per_em,
                        cursor,
                        start_safe,
                        candidate,
                        &runtime.shaping,
                    )?;

                    if evaluated.measured_width.get() <= bounds.width.get() {
                        chosen = Some(evaluated);
                    }

                    if candidate.kind == BoundedBreakKind::Mandatory {
                        break;
                    }
                }

                let Some(chosen) = chosen else {
                    diagnostics.push(ResolveDiagnostic {
                        code: "unbreakable_shaped_line".into(),
                        severity: ResolveSeverity::FidelityWarning,
                        origin: frame_origin.into_canonical(),
                        message: format!(
                            "no Unicode break candidate from scalar {cursor} fits frame width {} EMU",
                            bounds.width.get()
                        ),
                    });
                    frame_stalled = true;
                    break;
                };

                let scalar_start =
                    u32::try_from(cursor).map_err(|_| BoundedShapedFlowError::MetricOverflow)?;
                let scalar_end = u32::try_from(chosen.visible_end)
                    .map_err(|_| BoundedShapedFlowError::MetricOverflow)?;
                let consumed_scalar_end = u32::try_from(chosen.consumed_end)
                    .map_err(|_| BoundedShapedFlowError::MetricOverflow)?;
                let frame_line_index = u32::try_from(frame_line_index)
                    .map_err(|_| BoundedShapedFlowError::MetricOverflow)?;

                lines.push(BoundedShapedLine {
                    story_origin: story.origin,
                    frame_origin,
                    frame_line_index,
                    scalar_start,
                    scalar_end,
                    consumed_scalar_end,
                    text: chosen.text,
                    units_per_em: chosen.units_per_em,
                    measured_width: chosen.measured_width,
                    glyphs: chosen.glyphs,
                    break_kind: chosen.kind,
                    reshaped_for_break: chosen.reshaped,
                });
                line_origin_mapping.push(ShapedLineOriginMapping {
                    story_origin: story.origin,
                    frame_origin,
                    scalar_start,
                    scalar_end: consumed_scalar_end,
                });

                cursor = chosen.consumed_end;
            }

            if frame_stalled {
                continue;
            }
        }

        if cursor < scalar_count {
            diagnostics.push(ResolveDiagnostic {
                code: "story_overset".into(),
                severity: ResolveSeverity::FidelityWarning,
                origin: story.origin.into_canonical(),
                message: format!(
                    "{} Unicode scalars remain after shaped flow",
                    scalar_count - cursor
                ),
            });
        }
    }

    lines.sort_by_key(|line| (line.story_origin, line.scalar_start, line.frame_origin));
    line_origin_mapping.sort_by_key(|mapping| {
        (
            mapping.story_origin,
            mapping.scalar_start,
            mapping.frame_origin,
        )
    });

    Ok(BoundedShapedFlowScene {
        environment: shaping_descriptor(runtime),
        surfaces: base.surfaces,
        nodes: base.nodes,
        lines,
        origin_mapping: base.origin_mapping,
        line_origin_mapping,
        diagnostics,
    })
}

fn shape_failure_diagnostic(story_origin: StoryId, error: &BoundedShapeError) -> ResolveDiagnostic {
    let code = match error {
        BoundedShapeError::NonPositiveFontSize { .. } => "invalid_font_size",
        BoundedShapeError::FontFingerprintMismatch { .. } => "font_fingerprint_mismatch",
        BoundedShapeError::InvalidFont { .. } => "invalid_font",
        BoundedShapeError::ScalarIndexOverflow => "text_scalar_index_overflow",
        BoundedShapeError::MetricScaleOverflow => "text_metric_scale_overflow",
    };

    ResolveDiagnostic {
        code: code.into(),
        severity: ResolveSeverity::FidelityWarning,
        origin: story_origin.into_canonical(),
        message: error.to_string(),
    }
}

fn evaluate_candidate(
    scalars: &[char],
    full_glyphs: &[BoundedShapedGlyph],
    full_units_per_em: u32,
    cursor: usize,
    start_safe: bool,
    candidate: &BoundedBreakCandidate,
    runtime: &BoundedShapingRuntime<'_>,
) -> Result<EvaluatedBreak, BoundedShapedFlowError> {
    let consumed_end = usize::try_from(candidate.scalar_boundary)
        .map_err(|_| BoundedShapedFlowError::MetricOverflow)?;
    let visible_end = visible_end_for_candidate(scalars, cursor, consumed_end, candidate.kind);

    let text: String = scalars[cursor..visible_end].iter().collect();
    let reshaped = !start_safe || candidate.requires_reshaping;

    let (glyphs, measured_width, units_per_em) = if reshaped {
        let scalar_base =
            u32::try_from(cursor).map_err(|_| BoundedShapedFlowError::MetricOverflow)?;
        let shaped = shape_bounded_ltr_segment(&text, scalar_base, runtime)
            .map_err(BoundedShapedFlowError::Shape)?;
        (shaped.glyphs, shaped.total_x_advance, shaped.units_per_em)
    } else {
        let glyphs = glyphs_for_scalar_range(full_glyphs, cursor, visible_end)?;
        let measured_width = glyphs.iter().try_fold(LengthEmu::ZERO, |width, glyph| {
            width
                .checked_add(glyph.x_advance)
                .ok_or(BoundedShapedFlowError::MetricOverflow)
        })?;
        (glyphs, measured_width, full_units_per_em)
    };

    Ok(EvaluatedBreak {
        visible_end,
        consumed_end,
        text,
        units_per_em,
        measured_width,
        glyphs,
        kind: candidate.kind,
        reshaped,
    })
}

fn visible_end_for_candidate(
    scalars: &[char],
    cursor: usize,
    consumed_end: usize,
    kind: BoundedBreakKind,
) -> usize {
    if kind != BoundedBreakKind::Mandatory {
        return consumed_end;
    }

    let mut visible_end = consumed_end;
    while visible_end > cursor && matches!(scalars[visible_end - 1], '\r' | '\n') {
        visible_end -= 1;
    }
    visible_end
}

fn boundary_safe(candidates: &[BoundedBreakCandidate], boundary: u32, scalar_count: u32) -> bool {
    if boundary == 0 || boundary == scalar_count {
        return true;
    }

    candidates
        .iter()
        .find(|candidate| candidate.scalar_boundary == boundary)
        .is_some_and(|candidate| candidate.safe_without_reshaping)
}

fn glyphs_for_scalar_range(
    glyphs: &[BoundedShapedGlyph],
    start: usize,
    end: usize,
) -> Result<Vec<BoundedShapedGlyph>, BoundedShapedFlowError> {
    glyphs
        .iter()
        .filter_map(|glyph| {
            let cluster = usize::try_from(glyph.cluster).ok()?;
            (cluster >= start && cluster < end).then_some(Ok(glyph.clone()))
        })
        .collect()
}

fn shaping_descriptor(runtime: &BoundedShapedFlowRuntime<'_>) -> BoundedShapedFlowDescriptor {
    BoundedShapedFlowDescriptor {
        shaping: BoundedShapingDescriptor {
            layout: runtime.shaping.layout.clone(),
            face_index: runtime.shaping.face_index,
            font_size_emu: runtime.shaping.font_size_emu,
            shaper_revision: BOUNDED_SHAPER_REVISION.into(),
        },
        line_height: runtime.line_height,
    }
}

#[cfg(test)]
fn width_for_scalar_range(
    glyphs: &[BoundedShapedGlyph],
    start: usize,
    end: usize,
) -> Result<LengthEmu, BoundedShapedFlowError> {
    glyphs_for_scalar_range(glyphs, start, end)?
        .iter()
        .try_fold(LengthEmu::ZERO, |width, glyph| {
            width
                .checked_add(glyph.x_advance)
                .ok_or(BoundedShapedFlowError::MetricOverflow)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        BoundedAuthoringSlice, BoundedNodeGeometryInput, font_fingerprint_sha256, project_bounded,
    };
    use pub_model::{
        Affine2D, CanonicalId, EMU_PER_POINT, NodeId, Page, PageId, RectEmu, Size2D, Story,
        StoryFrame,
    };

    fn id(byte: u8) -> CanonicalId {
        CanonicalId::from_bytes([byte; 16])
    }

    fn page_id(byte: u8) -> PageId {
        PageId::from_canonical(id(byte))
    }

    fn node_id(byte: u8) -> NodeId {
        NodeId::from_canonical(id(byte))
    }

    fn story_id(byte: u8) -> StoryId {
        StoryId::from_canonical(id(byte))
    }

    fn runtime(font: &[u8], line_height: i64) -> BoundedShapedFlowRuntime<'_> {
        BoundedShapedFlowRuntime {
            shaping: BoundedShapingRuntime {
                layout: crate::BoundedLayoutEnvironment {
                    engine_revision: "layout-resolve-01b2c".into(),
                    font_set_fingerprint: font_fingerprint_sha256(font),
                    resource_fingerprint: "resources:none".into(),
                },
                face_index: 0,
                font_size_emu: LengthEmu::new(12 * EMU_PER_POINT),
                font_bytes: font,
            },
            line_height: LengthEmu::new(line_height),
        }
    }

    fn authoring_slice(
        linked: bool,
        text: &str,
        width: LengthEmu,
        height: LengthEmu,
    ) -> BoundedAuthoringSlice {
        let first = StoryFrame {
            story_id: story_id(7),
            frame_id: node_id(10),
            ordinal: 0,
            previous: None,
            next: linked.then_some(node_id(11)),
        };
        let second = StoryFrame {
            story_id: story_id(7),
            frame_id: node_id(11),
            ordinal: 1,
            previous: linked.then_some(node_id(10)),
            next: None,
        };

        BoundedAuthoringSlice {
            pages: vec![Page {
                id: page_id(1),
                size: Size2D::new(LengthEmu::new(2_000_000), LengthEmu::new(2_000_000)),
                bleed: None,
                margins: None,
                children: vec![node_id(10), node_id(11)],
                extensions: Vec::new(),
            }],
            node_geometry: vec![
                BoundedNodeGeometryInput {
                    node_id: node_id(10),
                    parent_origin: page_id(1).into_canonical(),
                    bounds: RectEmu::new(LengthEmu::ZERO, LengthEmu::ZERO, width, height),
                    transform: Affine2D::identity(),
                },
                BoundedNodeGeometryInput {
                    node_id: node_id(11),
                    parent_origin: page_id(1).into_canonical(),
                    bounds: RectEmu::new(LengthEmu::new(500_000), LengthEmu::ZERO, width, height),
                    transform: Affine2D::identity(),
                },
            ],
            stories: vec![Story {
                id: story_id(7),
                text: text.into(),
                paragraphs: Vec::new(),
                runs: Vec::new(),
                fields: Vec::new(),
                hyperlinks: Vec::new(),
                source_refs: Vec::new(),
            }],
            story_frames: vec![second, first],
            tables: Vec::new(),
            guides: Vec::new(),
            unknown_layout_state: Vec::new(),
        }
    }

    fn projection(
        linked: bool,
        text: &str,
        width: LengthEmu,
        height: LengthEmu,
    ) -> BoundedLayoutProjection {
        project_bounded(authoring_slice(linked, text, width, height))
    }

    #[test]
    fn first_line_extent_is_distinct_from_subsequent_baseline_advance() {
        let first_line_extent = 198_000_i64;
        let baseline_advance = 222_250_i64;

        assert_eq!(
            shaped_line_capacity_v1(first_line_extent - 1, first_line_extent, baseline_advance),
            Some(0)
        );
        assert_eq!(
            shaped_line_capacity_v1(first_line_extent, first_line_extent, baseline_advance),
            Some(1)
        );
        assert_eq!(
            shaped_line_capacity_v1(
                first_line_extent + baseline_advance - 1,
                first_line_extent,
                baseline_advance,
            ),
            Some(1)
        );
        assert_eq!(
            shaped_line_capacity_v1(
                first_line_extent + baseline_advance,
                first_line_extent,
                baseline_advance,
            ),
            Some(2)
        );
    }

    #[test]
    fn real_shaped_width_flows_across_explicit_frames_and_oversets() {
        let font = font_test_data::NOTOSERIF_AUTOHINT_SHAPING;
        let runtime = runtime(font, 200_000);
        let shaped = shape_bounded_ltr("Hfi ", &runtime.shaping).unwrap();
        let one_word_width = width_for_scalar_range(&shaped.glyphs, 0, 4).unwrap();
        let projection = projection(true, "Hfi Hfi Hfi", one_word_width, runtime.line_height);

        let scene = resolve_bounded_shaped_flow(&projection, &runtime).unwrap();

        assert_eq!(scene.lines.len(), 2);
        assert_eq!(scene.lines[0].frame_origin, node_id(10));
        assert_eq!(scene.lines[0].text, "Hfi ");
        assert_eq!(scene.lines[1].frame_origin, node_id(11));
        assert_eq!(scene.lines[1].text, "Hfi ");
        assert!(scene.lines.iter().all(|line| line.units_per_em > 0));

        let geometry = scene.geometry_scene();
        assert_eq!(geometry.surfaces, scene.surfaces);
        assert_eq!(geometry.nodes, scene.nodes);
        assert_eq!(geometry.origin_mapping, scene.origin_mapping);
        assert_eq!(geometry.diagnostics, scene.diagnostics);
        assert_eq!(geometry.environment, scene.environment.shaping.layout);
        assert!(
            scene
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "story_overset")
        );
    }

    #[test]
    fn mandatory_newline_breaks_even_when_frame_has_more_width() {
        let font = font_test_data::NOTOSERIF_AUTOHINT_SHAPING;
        let runtime = runtime(font, 200_000);
        let projection = projection(
            true,
            "Hfi\nHfi",
            LengthEmu::new(2_000_000),
            LengthEmu::new(400_000),
        );

        let scene = resolve_bounded_shaped_flow(&projection, &runtime).unwrap();

        assert_eq!(scene.lines.len(), 2);
        assert_eq!(scene.lines[0].text, "Hfi");
        assert_eq!(scene.lines[0].break_kind, BoundedBreakKind::Mandatory);
        assert_eq!(scene.lines[0].scalar_end, 3);
        assert_eq!(scene.lines[0].consumed_scalar_end, 4);
        assert_eq!(scene.lines[1].text, "Hfi");
    }

    #[test]
    fn unsafe_mandatory_candidate_is_independently_reshaped() {
        let font = font_test_data::NOTOSERIF_AUTOHINT_SHAPING;
        let runtime = runtime(font, 200_000);
        let scalars: Vec<_> = "Hfi\nHfi".chars().collect();
        let full = shape_bounded_ltr("Hfi\nHfi", &runtime.shaping).unwrap();
        let candidate = BoundedBreakCandidate {
            scalar_boundary: 4,
            kind: BoundedBreakKind::Mandatory,
            safe_without_reshaping: false,
            requires_reshaping: true,
        };

        let evaluated = evaluate_candidate(
            &scalars,
            &full.glyphs,
            full.units_per_em,
            0,
            true,
            &candidate,
            &runtime.shaping,
        )
        .unwrap();
        let independently_shaped = shape_bounded_ltr_segment("Hfi", 0, &runtime.shaping).unwrap();

        assert!(evaluated.reshaped);
        assert_eq!(evaluated.text, "Hfi");
        assert_eq!(evaluated.visible_end, 3);
        assert_eq!(evaluated.consumed_end, 4);
        assert_eq!(evaluated.glyphs, independently_shaped.glyphs);
        assert_eq!(
            evaluated.measured_width,
            independently_shaped.total_x_advance
        );
        assert_eq!(evaluated.units_per_em, independently_shaped.units_per_em);
    }

    #[test]
    fn unsafe_start_boundary_forces_next_segment_reshaping() {
        let font = font_test_data::NOTOSERIF_AUTOHINT_SHAPING;
        let runtime = runtime(font, 200_000);
        let scalars: Vec<_> = "Hfi Hfi".chars().collect();
        let full = shape_bounded_ltr("Hfi Hfi", &runtime.shaping).unwrap();
        let candidate = BoundedBreakCandidate {
            scalar_boundary: 7,
            kind: BoundedBreakKind::Allowed,
            safe_without_reshaping: true,
            requires_reshaping: false,
        };

        let evaluated = evaluate_candidate(
            &scalars,
            &full.glyphs,
            full.units_per_em,
            4,
            false,
            &candidate,
            &runtime.shaping,
        )
        .unwrap();

        assert!(evaluated.reshaped);
        assert!(evaluated.glyphs.iter().all(|glyph| glyph.cluster >= 4));
        assert!(evaluated.units_per_em > 0);
    }

    fn flow_run(
        start: u32,
        end: u32,
        constraint: BoundedParagraphFlowConstraint,
    ) -> BoundedParagraphFlowRun {
        BoundedParagraphFlowRun {
            story_origin: story_id(7),
            scalar_start: start,
            scalar_end: end,
            constraint,
        }
    }

    #[test]
    fn start_in_next_text_box_defers_noninitial_paragraph_to_successor_frame() {
        let font = font_test_data::NOTOSERIF_AUTOHINT_SHAPING;
        let runtime = runtime(font, 200_000);
        let projection = projection(
            true,
            "Hfi\rHfi",
            LengthEmu::new(2_000_000),
            LengthEmu::new(400_000),
        );
        let flow = [flow_run(
            4,
            7,
            BoundedParagraphFlowConstraint::StartInNextTextBox,
        )];

        let scene =
            resolve_bounded_shaped_flow_with_paragraph_flow(&projection, &runtime, &flow).unwrap();

        assert_eq!(scene.lines.len(), 2);
        assert_eq!(scene.lines[0].frame_origin, node_id(10));
        assert_eq!(scene.lines[0].text, "Hfi");
        assert_eq!(scene.lines[1].frame_origin, node_id(11));
        assert_eq!(scene.lines[1].text, "Hfi");
        assert_eq!(scene.lines[1].scalar_start, 4);
    }

    #[test]
    fn start_in_next_text_box_without_successor_leaves_target_in_overset() {
        let font = font_test_data::NOTOSERIF_AUTOHINT_SHAPING;
        let runtime = runtime(font, 200_000);
        let mut authoring = authoring_slice(
            true,
            "Hfi\rHfi",
            LengthEmu::new(2_000_000),
            LengthEmu::new(400_000),
        );
        authoring
            .story_frames
            .retain(|frame| frame.frame_id == node_id(10));
        authoring.story_frames[0].next = None;
        let projection = project_bounded(authoring);
        let flow = [flow_run(
            4,
            7,
            BoundedParagraphFlowConstraint::StartInNextTextBox,
        )];

        let scene =
            resolve_bounded_shaped_flow_with_paragraph_flow(&projection, &runtime, &flow).unwrap();

        assert_eq!(scene.lines.len(), 1);
        assert_eq!(scene.lines[0].frame_origin, node_id(10));
        assert_eq!(scene.lines[0].text, "Hfi");
        assert!(scene.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "story_overset" && diagnostic.origin == story_id(7).into_canonical()
        }));
    }

    #[test]
    fn keep_lines_together_moves_whole_paragraph_when_successor_can_fit_it() {
        let font = font_test_data::NOTOSERIF_AUTOHINT_SHAPING;
        let runtime = runtime(font, 200_000);
        let shaped = shape_bounded_ltr("Hfi ", &runtime.shaping).unwrap();
        let width = width_for_scalar_range(&shaped.glyphs, 0, 4).unwrap();
        let projection = projection(true, "Hfi\rHfi Hfi", width, LengthEmu::new(400_000));
        let flow = [flow_run(
            4,
            11,
            BoundedParagraphFlowConstraint::KeepLinesTogether,
        )];

        let off = resolve_bounded_shaped_flow(&projection, &runtime).unwrap();
        let on =
            resolve_bounded_shaped_flow_with_paragraph_flow(&projection, &runtime, &flow).unwrap();

        assert_eq!(off.lines[1].frame_origin, node_id(10));
        assert_eq!(off.lines[1].scalar_start, 4);
        assert_eq!(on.lines[1].frame_origin, node_id(11));
        assert_eq!(on.lines[1].scalar_start, 4);
        assert_eq!(on.lines[2].frame_origin, node_id(11));
    }

    #[test]
    fn keep_with_next_moves_current_and_following_paragraph_as_one_pair() {
        let font = font_test_data::NOTOSERIF_AUTOHINT_SHAPING;
        let runtime = runtime(font, 200_000);
        let projection = projection(
            true,
            "Hfi\rHfi\rHfi",
            LengthEmu::new(2_000_000),
            LengthEmu::new(400_000),
        );
        let flow = [flow_run(4, 8, BoundedParagraphFlowConstraint::KeepWithNext)];

        let off = resolve_bounded_shaped_flow(&projection, &runtime).unwrap();
        let on =
            resolve_bounded_shaped_flow_with_paragraph_flow(&projection, &runtime, &flow).unwrap();

        assert_eq!(off.lines[1].frame_origin, node_id(10));
        assert_eq!(on.lines[1].frame_origin, node_id(11));
        assert_eq!(on.lines[1].scalar_start, 4);
        assert_eq!(on.lines[2].frame_origin, node_id(11));
        assert_eq!(on.lines[2].scalar_start, 8);
    }

    #[test]
    fn widow_control_avoids_one_line_paragraph_orphan_at_frame_bottom() {
        let font = font_test_data::NOTOSERIF_AUTOHINT_SHAPING;
        let runtime = runtime(font, 200_000);
        let shaped = shape_bounded_ltr("Hfi ", &runtime.shaping).unwrap();
        let width = width_for_scalar_range(&shaped.glyphs, 0, 4).unwrap();
        let projection = projection(true, "Hfi\rHfi Hfi", width, LengthEmu::new(400_000));
        let flow = [flow_run(
            4,
            11,
            BoundedParagraphFlowConstraint::WidowControl,
        )];

        let off = resolve_bounded_shaped_flow(&projection, &runtime).unwrap();
        let on =
            resolve_bounded_shaped_flow_with_paragraph_flow(&projection, &runtime, &flow).unwrap();

        assert_eq!(off.lines[1].frame_origin, node_id(10));
        assert_eq!(off.lines[1].scalar_start, 4);
        assert_eq!(on.lines[1].frame_origin, node_id(11));
        assert_eq!(on.lines[1].scalar_start, 4);
        assert_eq!(on.lines[2].frame_origin, node_id(11));
    }

    #[test]
    fn ordinal_only_membership_never_creates_flow() {
        let font = font_test_data::NOTOSERIF_AUTOHINT_SHAPING;
        let runtime = runtime(font, 200_000);
        let shaped = shape_bounded_ltr("Hfi ", &runtime.shaping).unwrap();
        let width = width_for_scalar_range(&shaped.glyphs, 0, 4).unwrap();
        let projection = projection(false, "Hfi Hfi", width, runtime.line_height);

        let scene = resolve_bounded_shaped_flow(&projection, &runtime).unwrap();

        assert!(scene.lines.is_empty());
        assert!(
            scene
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "shared_story_without_explicit_flow")
        );
    }

    #[test]
    fn unbreakable_word_is_explicit_and_not_force_split() {
        let font = font_test_data::NOTOSERIF_AUTOHINT_SHAPING;
        let runtime = runtime(font, 200_000);
        let shaped = shape_bounded_ltr("Hfix", &runtime.shaping).unwrap();
        let width = LengthEmu::new(shaped.total_x_advance.get() - 1);
        let projection = projection(true, "Hfix", width, runtime.line_height);

        let scene = resolve_bounded_shaped_flow(&projection, &runtime).unwrap();

        assert!(scene.lines.is_empty());
        assert!(
            scene
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "unbreakable_shaped_line")
        );
        assert!(
            scene
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "story_overset")
        );
    }

    #[test]
    fn font_dependency_failures_are_diagnostics_without_substitution() {
        let font = font_test_data::NOTOSERIF_AUTOHINT_SHAPING;
        let mut mismatch_runtime = runtime(font, 200_000);
        mismatch_runtime.shaping.layout.font_set_fingerprint = "00".repeat(32);
        let projection = projection(
            true,
            "Hfi",
            LengthEmu::new(2_000_000),
            mismatch_runtime.line_height,
        );

        let mismatch_scene = resolve_bounded_shaped_flow(&projection, &mismatch_runtime).unwrap();

        assert!(mismatch_scene.lines.is_empty());
        assert!(!mismatch_scene.nodes.is_empty());
        assert!(
            mismatch_scene
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "font_fingerprint_mismatch")
        );

        let invalid_runtime = runtime(b"", 200_000);
        let invalid_scene = resolve_bounded_shaped_flow(&projection, &invalid_runtime).unwrap();

        assert!(invalid_scene.lines.is_empty());
        assert!(
            invalid_scene
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "invalid_font")
        );
    }

    #[test]
    fn authoring_vector_order_does_not_change_resolved_scene() {
        let font = font_test_data::NOTOSERIF_AUTOHINT_SHAPING;
        let runtime = runtime(font, 200_000);
        let shaped = shape_bounded_ltr("Hfi ", &runtime.shaping).unwrap();
        let width = width_for_scalar_range(&shaped.glyphs, 0, 4).unwrap();

        let left_input = authoring_slice(true, "Hfi Hfi", width, runtime.line_height);
        let mut right_input = authoring_slice(true, "Hfi Hfi", width, runtime.line_height);
        right_input.pages.reverse();
        right_input.node_geometry.reverse();
        right_input.stories.reverse();
        right_input.story_frames.reverse();
        right_input.tables.reverse();
        right_input.guides.reverse();
        right_input.unknown_layout_state.reverse();

        let left_projection = project_bounded(left_input);
        let right_projection = project_bounded(right_input);
        assert_eq!(left_projection, right_projection);

        let left = resolve_bounded_shaped_flow(&left_projection, &runtime).unwrap();
        let right = resolve_bounded_shaped_flow(&right_projection, &runtime).unwrap();

        assert_eq!(left, right);
    }

    #[test]
    fn fixed_input_and_environment_are_deterministic() {
        let font = font_test_data::NOTOSERIF_AUTOHINT_SHAPING;
        let runtime = runtime(font, 200_000);
        let shaped = shape_bounded_ltr("Hfi ", &runtime.shaping).unwrap();
        let width = width_for_scalar_range(&shaped.glyphs, 0, 4).unwrap();
        let projection = projection(true, "Hfi Hfi", width, runtime.line_height);

        let left = resolve_bounded_shaped_flow(&projection, &runtime).unwrap();
        let right = resolve_bounded_shaped_flow(&projection, &runtime).unwrap();

        assert_eq!(left, right);
    }
}
