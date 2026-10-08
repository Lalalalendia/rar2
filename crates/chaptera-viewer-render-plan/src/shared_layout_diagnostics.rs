//! Diagnostic-only replay of SharedLayoutIncomplete causes.
//!
//! This owner classifies already-failed shared layouts. It must not change
//! render admission, Story content, paint, or backend execution.

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SharedLayoutIncompleteCauseV1 {
    pub path: &'static str,
    pub consumption: &'static str,
    pub cause: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MixedSizeLayoutCapacityDiagnosticV1 {
    pub accepted_lines: usize,
    pub frame_height_emu: i64,
    pub used_height_emu: i64,
    pub physical_first_then_baseline_height_emu: Option<i64>,
    pub next_width_fit_line_height_emu: Option<i64>,
    pub next_width_fit_physical_extent_emu: Option<i64>,
    pub current_next_fits_height: Option<bool>,
    pub physical_first_then_baseline_next_fits_height: Option<bool>,
}

pub fn classify_mixed_size_layout_capacity_v1(
    fragment: &RenderTextFragmentV1,
    font: &ExplicitRenderTextFontResourceV1<'_>,
    node_id: NodeId,
    bounds: &RectEmu,
) -> Option<MixedSizeLayoutCapacityDiagnosticV1> {
    if bounds.width.get() <= 0
        || bounds.height.get() <= 0
        || font.resource_id.is_empty()
        || font.bytes.is_empty()
        || font.default_font_size_emu <= 0
        || font.default_line_height_emu <= 0
    {
        return None;
    }
    let fingerprint = font_fingerprint_sha256(font.bytes);
    if font.expected_sha256.is_empty() || fingerprint != font.expected_sha256 {
        return None;
    }

    let evaluation =
        evaluate_mixed_size_text_layout_v1(fragment, font, node_id, bounds, &fingerprint).ok()?;
    let next_line_height = evaluation.stop_width_fit_min_line_height_emu;
    let current_next_fits_height = next_line_height.map(|height| {
        evaluation
            .used_height_emu
            .checked_add(height)
            .is_some_and(|total| total <= bounds.height.get())
    });
    let physical_first_then_baseline_next_fits_height =
        match (evaluation.physical_first_then_baseline_height_emu, next_line_height) {
            (Some(used), Some(height)) if !evaluation.lines.is_empty() => used
                .checked_add(height)
                .map(|total| total <= bounds.height.get()),
            (Some(_), Some(_)) => evaluation
                .stop_width_fit_min_physical_extent_emu
                .map(|extent| extent <= bounds.height.get()),
            _ => None,
        };

    Some(MixedSizeLayoutCapacityDiagnosticV1 {
        accepted_lines: evaluation.lines.len(),
        frame_height_emu: bounds.height.get(),
        used_height_emu: evaluation.used_height_emu,
        physical_first_then_baseline_height_emu: evaluation
            .physical_first_then_baseline_height_emu,
        next_width_fit_line_height_emu: next_line_height,
        next_width_fit_physical_extent_emu: evaluation
            .stop_width_fit_min_physical_extent_emu,
        current_next_fits_height,
        physical_first_then_baseline_next_fits_height,
    })
}

/// Replays one already-classified SharedLayoutIncomplete fragment through the
/// same uniform shared-flow law and returns only source-safe causal classes.
///
/// This helper is diagnostic only. It does not change layout admission,
/// fallback disposition, Story content, or paint.
pub fn classify_shared_layout_incomplete_cause_v1(
    visual: &ViewerGeometryDocument,
    page: &PageRenderPlanV1,
    node: &NodeRenderPlanV1,
    fragment: &RenderTextFragmentV1,
    font: &ExplicitRenderTextFontResourceV1<'_>,
    font_is_source_resolved: bool,
) -> SharedLayoutIncompleteCauseV1 {
    #[cfg(feature = "projected-scene-instances")]
    if node.projected_scene_instance.is_some() {
        return SharedLayoutIncompleteCauseV1 {
            path: "projected_path",
            consumption: "unknown",
            cause: "projected_fail_closed",
        };
    }

    let font_size_emu = match admitted_font_size_emu(fragment, font.default_font_size_emu) {
        Ok(value) => value,
        Err(RenderTextLayoutFallbackReasonV1::MixedTypographySize) => {
            return SharedLayoutIncompleteCauseV1 {
                path: "mixed_size_path",
                consumption: "unknown",
                cause: "mixed_size_fail_closed",
            };
        }
        Err(_) => {
            return SharedLayoutIncompleteCauseV1 {
                path: "uniform_path",
                consumption: "unknown",
                cause: "typography_fail_closed",
            };
        }
    };

    let Some(story) = visual
        .document
        .stories
        .iter()
        .find(|story| story.id == fragment.story_id)
    else {
        return SharedLayoutIncompleteCauseV1 {
            path: "uniform_path",
            consumption: "unknown",
            cause: "story_missing",
        };
    };

    let exact_slice_scalar_base = exact_direct_story_slice_scalar_base_v1(
        visual,
        page.page_id,
        node.node_id,
        None,
        fragment,
        &story.text,
    );
    let diagnostic_path = if exact_slice_scalar_base.is_some() {
        "exact_slice_uniform_path"
    } else {
        "uniform_path"
    };
    let frame_ordinal = if exact_slice_scalar_base.is_some() {
        0
    } else {
        match admitted_layout_frame_ordinal(visual, fragment.story_id, node.node_id, None) {
            Ok(value) => value,
            Err(_) => {
                return SharedLayoutIncompleteCauseV1 {
                    path: diagnostic_path,
                    consumption: "unknown",
                    cause: "frame_fail_closed",
                };
            }
        }
    };

    let bounds = node.text_bounds.unwrap_or(node.bounds);
    if bounds.width.get() <= 0 || bounds.height.get() <= 0 {
        return SharedLayoutIncompleteCauseV1 {
            path: diagnostic_path,
            consumption: "unknown",
            cause: "frame_geometry_invalid",
        };
    }

    if font.resource_id.is_empty()
        || font.bytes.is_empty()
        || font.default_font_size_emu <= 0
        || font.default_line_height_emu <= 0
    {
        return SharedLayoutIncompleteCauseV1 {
            path: diagnostic_path,
            consumption: "unknown",
            cause: "font_resource_invalid",
        };
    }
    let fingerprint = font_fingerprint_sha256(font.bytes);
    if font.expected_sha256.is_empty() || fingerprint != font.expected_sha256 {
        return SharedLayoutIncompleteCauseV1 {
            path: diagnostic_path,
            consumption: "unknown",
            cause: "font_fingerprint_mismatch",
        };
    }

    let Some(line_height_emu) = resolved_uniform_line_height_emu_v1(
        visual,
        fragment,
        font_size_emu,
        font,
        font_is_source_resolved,
    ) else {
        return SharedLayoutIncompleteCauseV1 {
            path: diagnostic_path,
            consumption: "unknown",
            cause: "line_height_unavailable",
        };
    };

    let projection = BoundedLayoutProjection {
        pages: vec![ProjectedPage {
            origin: page.page_id,
            size: page.page_size,
            bleed: None,
            margins: None,
        }],
        node_geometry: vec![ProjectedNodeGeometry {
            origin: node.node_id,
            parent_origin: page.page_id.into_canonical(),
            bounds,
            transform: node.transform.clone(),
        }],
        stories: vec![ProjectedStory {
            origin: story.id,
            text: fragment.text.clone(),
            paragraph_origins: Vec::new(),
            run_origins: Vec::new(),
        }],
        story_frames: vec![ProjectedStoryFrame {
            story_origin: story.id,
            frame_origin: node.node_id,
            ordinal: frame_ordinal,
            previous_frame_origin: None,
            next_frame_origin: None,
        }],
        tables: Vec::new(),
        guides: Vec::new(),
        diagnostics: Vec::new(),
    };
    let runtime = BoundedShapedFlowRuntime {
        shaping: BoundedShapingRuntime {
            layout: BoundedLayoutEnvironment {
                engine_revision: SHARED_TEXT_LAYOUT_REVISION_V1.to_owned(),
                font_set_fingerprint: fingerprint,
                resource_fingerprint: font.resource_id.to_owned(),
            },
            face_index: font.face_index,
            font_size_emu: LengthEmu::new(font_size_emu),
            font_bytes: font.bytes,
        },
        line_height: LengthEmu::new(line_height_emu),
    };

    let Ok(scene) = resolve_bounded_shaped_flow(&projection, &runtime) else {
        return SharedLayoutIncompleteCauseV1 {
            path: diagnostic_path,
            consumption: "unknown",
            cause: "shared_layout_error",
        };
    };
    let mut source_lines = scene
        .lines
        .iter()
        .filter(|line| line.story_origin == story.id && line.frame_origin == node.node_id)
        .collect::<Vec<_>>();
    source_lines.sort_by_key(|line| line.frame_line_index);
    let consumption = if source_lines.is_empty() {
        "zero_lines"
    } else {
        "partial_lines"
    };
    let cursor = source_lines
        .last()
        .map(|line| line.consumed_scalar_end)
        .unwrap_or_default()
        .checked_add(exact_slice_scalar_base.unwrap_or_default())
        .unwrap_or(fragment.scalar_start);

    let has_no_capacity = scene
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "text_frame_has_no_capacity");
    let has_unbreakable = scene
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "unbreakable_shaped_line");
    let has_overset = scene
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "story_overset");

    let mandatory_after_cursor = if has_unbreakable {
        shape_bounded_ltr_segment(&fragment.text, fragment.scalar_start, &runtime.shaping)
            .ok()
            .and_then(|shaped| break_policy_for_shaped_text(&fragment.text, &shaped.glyphs).ok())
            .is_some_and(|policy| {
                policy
                    .candidates
                    .iter()
                    .filter(|candidate| candidate.scalar_boundary > cursor)
                    .any(|candidate| candidate.kind == BoundedBreakKind::Mandatory)
            })
    } else {
        false
    };

    let cause = if has_no_capacity && source_lines.is_empty() {
        "first_line_height_rejection"
    } else if has_unbreakable && mandatory_after_cursor {
        "mandatory_boundary_no_fit"
    } else if has_unbreakable {
        "width_no_legal_break"
    } else if has_overset && !source_lines.is_empty() {
        "pure_story_overset_partial"
    } else if has_overset {
        "overset_without_lines"
    } else {
        "other_fail_closed"
    };

    SharedLayoutIncompleteCauseV1 {
        path: diagnostic_path,
        consumption,
        cause,
    }
}
