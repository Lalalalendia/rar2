use crate::{
    BoundedLayoutEnvironment, BoundedLayoutProjection, BoundedParagraphFlowConstraint,
    BoundedParagraphFlowRun, ProjectionSeverity, ResolveBlocked, ResolveDiagnostic,
    ResolveSeverity, ResolvedPhysicalNode, ResolvedSurface, SceneOriginMapping,
    resolve_bounded_geometry,
};
use pub_model::{LengthEmu, NodeId, StoryId};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoundedTextMetrics {
    pub font_fingerprint: String,
    pub scalar_advance: LengthEmu,
    pub line_height: LengthEmu,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoundedTextFlowEnvironment {
    pub layout: BoundedLayoutEnvironment,
    pub text_metrics: Option<BoundedTextMetrics>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedTextFragment {
    pub story_origin: StoryId,
    pub frame_origin: NodeId,
    pub scalar_start: u32,
    pub scalar_end: u32,
    pub text: String,
    pub line_count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextFragmentOriginMapping {
    pub story_origin: StoryId,
    pub frame_origin: NodeId,
    pub scalar_start: u32,
    pub scalar_end: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoundedTextFlowScene {
    pub environment: BoundedTextFlowEnvironment,
    pub surfaces: Vec<ResolvedSurface>,
    pub nodes: Vec<ResolvedPhysicalNode>,
    pub text_fragments: Vec<ResolvedTextFragment>,
    pub origin_mapping: Vec<SceneOriginMapping>,
    pub text_origin_mapping: Vec<TextFragmentOriginMapping>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<ResolveDiagnostic>,
}

/// LAYOUT-RESOLVE-01B0: deterministic linked-story flow using explicit,
/// environment-fenced scalar metrics.
///
/// This deliberately does not claim real font shaping. It proves source-free
/// flow semantics, explicit-chain ownership, overset reporting and fragment
/// origin mapping before a shaping engine is introduced.
pub fn resolve_bounded_text_flow(
    projection: &BoundedLayoutProjection,
    environment: BoundedTextFlowEnvironment,
) -> Result<BoundedTextFlowScene, ResolveBlocked> {
    resolve_bounded_text_flow_with_paragraph_flow(projection, environment, &[])
}

pub fn resolve_bounded_text_flow_with_paragraph_flow(
    projection: &BoundedLayoutProjection,
    environment: BoundedTextFlowEnvironment,
    paragraph_flow: &[BoundedParagraphFlowRun],
) -> Result<BoundedTextFlowScene, ResolveBlocked> {
    let projection_errors: Vec<_> = projection
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.severity == ProjectionSeverity::Error)
        .cloned()
        .collect();

    if !projection_errors.is_empty() {
        return Err(ResolveBlocked { projection_errors });
    }

    let base = resolve_bounded_geometry(projection, environment.layout.clone())?;
    let mut diagnostics: Vec<_> = base
        .diagnostics
        .into_iter()
        .filter(|diagnostic| diagnostic.code != "story_text_layout_not_implemented")
        .collect();

    let Some(metrics) = environment.text_metrics.as_ref() else {
        for story in &projection.stories {
            diagnostics.push(ResolveDiagnostic {
                code: "text_metrics_missing".into(),
                severity: ResolveSeverity::FidelityWarning,
                origin: story.origin.into_canonical(),
                message: "text flow requires explicit environment-fenced text metrics".into(),
            });
        }

        return Ok(BoundedTextFlowScene {
            environment,
            surfaces: base.surfaces,
            nodes: base.nodes,
            text_fragments: Vec::new(),
            origin_mapping: base.origin_mapping,
            text_origin_mapping: Vec::new(),
            diagnostics,
        });
    };

    if metrics.font_fingerprint != environment.layout.font_set_fingerprint {
        for story in &projection.stories {
            diagnostics.push(ResolveDiagnostic {
                code: "text_metrics_font_mismatch".into(),
                severity: ResolveSeverity::FidelityWarning,
                origin: story.origin.into_canonical(),
                message: "text metrics fingerprint does not match the declared font set".into(),
            });
        }

        return Ok(BoundedTextFlowScene {
            environment,
            surfaces: base.surfaces,
            nodes: base.nodes,
            text_fragments: Vec::new(),
            origin_mapping: base.origin_mapping,
            text_origin_mapping: Vec::new(),
            diagnostics,
        });
    }

    if metrics.scalar_advance.get() <= 0 || metrics.line_height.get() <= 0 {
        for story in &projection.stories {
            diagnostics.push(ResolveDiagnostic {
                code: "invalid_text_metrics".into(),
                severity: ResolveSeverity::FidelityWarning,
                origin: story.origin.into_canonical(),
                message: "text metrics must use positive scalar advance and line height".into(),
            });
        }

        return Ok(BoundedTextFlowScene {
            environment,
            surfaces: base.surfaces,
            nodes: base.nodes,
            text_fragments: Vec::new(),
            origin_mapping: base.origin_mapping,
            text_origin_mapping: Vec::new(),
            diagnostics,
        });
    }

    let geometry: BTreeMap<_, _> = projection
        .node_geometry
        .iter()
        .map(|node| (node.origin, node.bounds))
        .collect();

    let mut text_fragments = Vec::new();
    let mut text_origin_mapping = Vec::new();

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

        let scalars: Vec<char> = story.text.chars().collect();
        let mut cursor = 0usize;
        let mut consumed_start_next = BTreeSet::<u32>::new();

        for (chain_index, frame_origin) in chain.iter().copied().enumerate() {
            let Some(bounds) = geometry.get(&frame_origin) else {
                diagnostics.push(ResolveDiagnostic {
                    code: "text_frame_geometry_missing".into(),
                    severity: ResolveSeverity::FidelityWarning,
                    origin: frame_origin.into_canonical(),
                    message: "text frame has no geometry available to the resolver".into(),
                });
                continue;
            };

            let Some((columns, _rows, capacity)) = fixed_text_capacity_v1(bounds, metrics) else {
                diagnostics.push(ResolveDiagnostic {
                    code: "text_frame_has_no_capacity".into(),
                    severity: ResolveSeverity::FidelityWarning,
                    origin: frame_origin.into_canonical(),
                    message: "text frame geometry cannot fit one measured scalar".into(),
                });
                continue;
            };

            let provisional_end = cursor.saturating_add(capacity).min(scalars.len());
            let successor_capacity = chain
                .get(chain_index + 1)
                .and_then(|next_frame| geometry.get(next_frame))
                .and_then(|next_bounds| fixed_text_capacity_v1(next_bounds, metrics))
                .map(|(_, _, capacity)| capacity);
            let flow_break = paragraph_flow_break_before_fixed_v1(
                paragraph_flow,
                story.origin,
                &scalars,
                cursor,
                provisional_end,
                columns,
                capacity,
                successor_capacity,
                &consumed_start_next,
            );
            if let Some(break_before) = flow_break {
                if let Ok(break_before_u32) = u32::try_from(break_before) {
                    if paragraph_flow.iter().any(|run| {
                        run.story_origin == story.origin
                            && run.scalar_start == break_before_u32
                            && run.constraint == BoundedParagraphFlowConstraint::StartInNextTextBox
                    }) {
                        consumed_start_next.insert(break_before_u32);
                    }
                }
            }
            let end = flow_break.unwrap_or(provisional_end);
            if end == cursor {
                continue;
            }

            let text: String = scalars[cursor..end].iter().collect();
            let scalar_start = u32::try_from(cursor).unwrap_or(u32::MAX);
            let scalar_end = u32::try_from(end).unwrap_or(u32::MAX);
            let line_count = u32::try_from((end - cursor).div_ceil(columns)).unwrap_or(u32::MAX);

            text_fragments.push(ResolvedTextFragment {
                story_origin: story.origin,
                frame_origin,
                scalar_start,
                scalar_end,
                text,
                line_count,
            });
            text_origin_mapping.push(TextFragmentOriginMapping {
                story_origin: story.origin,
                frame_origin,
                scalar_start,
                scalar_end,
            });

            cursor = end;
            if cursor == scalars.len() {
                break;
            }
        }

        if cursor < scalars.len() {
            diagnostics.push(ResolveDiagnostic {
                code: "story_overset".into(),
                severity: ResolveSeverity::FidelityWarning,
                origin: story.origin.into_canonical(),
                message: format!(
                    "{} measured scalars remain after the explicit frame chain",
                    scalars.len() - cursor
                ),
            });
        }
    }

    text_fragments.sort_by_key(|fragment| {
        (
            fragment.story_origin,
            fragment.scalar_start,
            fragment.frame_origin,
        )
    });
    text_origin_mapping.sort_by_key(|mapping| {
        (
            mapping.story_origin,
            mapping.scalar_start,
            mapping.frame_origin,
        )
    });

    Ok(BoundedTextFlowScene {
        environment,
        surfaces: base.surfaces,
        nodes: base.nodes,
        text_fragments,
        origin_mapping: base.origin_mapping,
        text_origin_mapping,
        diagnostics,
    })
}

fn fixed_text_capacity_v1(
    bounds: &pub_model::RectEmu,
    metrics: &BoundedTextMetrics,
) -> Option<(usize, usize, usize)> {
    let columns = bounds.width.get() / metrics.scalar_advance.get();
    let rows = bounds.height.get() / metrics.line_height.get();
    if columns <= 0 || rows <= 0 {
        return None;
    }
    let columns = usize::try_from(columns).ok()?;
    let rows = usize::try_from(rows).ok()?;
    let capacity = columns.checked_mul(rows)?;
    Some((columns, rows, capacity))
}

fn next_paragraph_end_fixed_v1(scalars: &[char], start: usize) -> Option<usize> {
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

// This helper deliberately receives the complete fixed-metric allocation
// snapshot so policy decisions cannot mix state from different frames.
#[allow(clippy::too_many_arguments)]
fn paragraph_flow_break_before_fixed_v1(
    paragraph_flow: &[BoundedParagraphFlowRun],
    story_origin: StoryId,
    scalars: &[char],
    cursor: usize,
    provisional_end: usize,
    columns: usize,
    capacity: usize,
    successor_capacity: Option<usize>,
    consumed_start_next: &BTreeSet<u32>,
) -> Option<usize> {
    let cursor_u32 = u32::try_from(cursor).ok()?;
    let provisional_end_u32 = u32::try_from(provisional_end).ok()?;

    let mut starts = paragraph_flow
        .iter()
        .filter(|run| {
            run.story_origin == story_origin
                && run.scalar_start >= cursor_u32
                && run.scalar_start < provisional_end_u32
                && run.scalar_end > run.scalar_start
        })
        .map(|run| run.scalar_start)
        .collect::<Vec<_>>();
    starts.sort_unstable();
    starts.dedup();

    for start_u32 in starts {
        let start = usize::try_from(start_u32).ok()?;
        let group = paragraph_flow
            .iter()
            .filter(|run| run.story_origin == story_origin && run.scalar_start == start_u32)
            .collect::<Vec<_>>();
        let end_u32 = group.first()?.scalar_end;
        if group.iter().any(|run| run.scalar_end != end_u32) {
            continue;
        }
        let end = usize::try_from(end_u32).ok()?;
        if end <= start || end > scalars.len() {
            continue;
        }

        let used = start.checked_sub(cursor)?;
        if used >= capacity {
            continue;
        }
        let remaining = capacity - used;
        let paragraph_len = end - start;

        if !consumed_start_next.contains(&start_u32)
            && group
                .iter()
                .any(|run| run.constraint == BoundedParagraphFlowConstraint::StartInNextTextBox)
        {
            return Some(start);
        }

        let Some(successor_capacity) = successor_capacity else {
            continue;
        };

        if group
            .iter()
            .any(|run| run.constraint == BoundedParagraphFlowConstraint::KeepLinesTogether)
            && paragraph_len > remaining
            && paragraph_len <= successor_capacity
        {
            return Some(start);
        }

        if group
            .iter()
            .any(|run| run.constraint == BoundedParagraphFlowConstraint::KeepWithNext)
        {
            if let Some(pair_end) = next_paragraph_end_fixed_v1(scalars, end) {
                let pair_len = pair_end.saturating_sub(start);
                if pair_end > end && pair_len > remaining && pair_len <= successor_capacity {
                    return Some(start);
                }
            }
        }

        if group
            .iter()
            .any(|run| run.constraint == BoundedParagraphFlowConstraint::WidowControl)
            && remaining <= columns
            && paragraph_len > remaining
            && paragraph_len <= successor_capacity
        {
            return Some(start);
        }
    }

    None
}

/// Reuse the single existing reciprocal linked-frame chain law in current
/// physical-font flow; ordinal-only frames never imply a document connection.

pub fn validated_projected_story_frame_chain_v1(
    frames: &[&crate::ProjectedStoryFrame],
) -> Result<Vec<NodeId>, &'static str> {
    explicit_chain(frames)
}

pub(crate) fn explicit_chain(
    frames: &[&crate::ProjectedStoryFrame],
) -> Result<Vec<NodeId>, &'static str> {
    if frames.len() == 1 {
        return Ok(vec![frames[0].frame_origin]);
    }

    if frames
        .iter()
        .all(|frame| frame.previous_frame_origin.is_none() && frame.next_frame_origin.is_none())
    {
        return Err("shared_story_without_explicit_flow");
    }

    let by_id: BTreeMap<_, _> = frames
        .iter()
        .map(|frame| (frame.frame_origin, *frame))
        .collect();
    let heads: Vec<_> = frames
        .iter()
        .filter(|frame| frame.previous_frame_origin.is_none())
        .collect();

    if heads.len() != 1 {
        return Err("ambiguous_story_flow");
    }

    let mut chain = Vec::with_capacity(frames.len());
    let mut seen = BTreeSet::new();
    let mut current = heads[0].frame_origin;

    loop {
        if !seen.insert(current) {
            return Err("cyclic_story_flow");
        }
        chain.push(current);

        let Some(frame) = by_id.get(&current) else {
            return Err("broken_story_flow");
        };
        let Some(next) = frame.next_frame_origin else {
            break;
        };

        let Some(next_frame) = by_id.get(&next) else {
            return Err("broken_story_flow");
        };
        if next_frame.previous_frame_origin != Some(current) {
            return Err("non_reciprocal_story_flow");
        }

        current = next;
    }

    if chain.len() != frames.len() {
        return Err("disconnected_story_flow");
    }

    Ok(chain)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        BoundedAuthoringSlice, BoundedNodeGeometryInput, ProjectedStoryFrame, project_bounded,
    };
    use pub_model::{
        Affine2D, CanonicalId, LengthEmu, NodeId, Page, PageId, RectEmu, Size2D, Story, StoryFrame,
        StoryId,
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

    fn environment() -> BoundedTextFlowEnvironment {
        BoundedTextFlowEnvironment {
            layout: BoundedLayoutEnvironment {
                engine_revision: "layout-resolve-01b0".into(),
                font_set_fingerprint: "font-fixture-v1".into(),
                resource_fingerprint: "resources:none".into(),
            },
            text_metrics: Some(BoundedTextMetrics {
                font_fingerprint: "font-fixture-v1".into(),
                scalar_advance: LengthEmu::new(10),
                line_height: LengthEmu::new(10),
            }),
        }
    }

    fn projection(linked: bool, text: &str) -> BoundedLayoutProjection {
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

        project_bounded(BoundedAuthoringSlice {
            pages: vec![Page {
                id: page_id(1),
                size: Size2D::new(LengthEmu::new(500), LengthEmu::new(500)),
                bleed: None,
                margins: None,
                children: vec![node_id(10), node_id(11)],
                extensions: Vec::new(),
            }],
            node_geometry: vec![
                BoundedNodeGeometryInput {
                    node_id: node_id(10),
                    parent_origin: page_id(1).into_canonical(),
                    bounds: RectEmu::new(
                        LengthEmu::ZERO,
                        LengthEmu::ZERO,
                        LengthEmu::new(30),
                        LengthEmu::new(20),
                    ),
                    transform: Affine2D::identity(),
                },
                BoundedNodeGeometryInput {
                    node_id: node_id(11),
                    parent_origin: page_id(1).into_canonical(),
                    bounds: RectEmu::new(
                        LengthEmu::new(40),
                        LengthEmu::ZERO,
                        LengthEmu::new(30),
                        LengthEmu::new(20),
                    ),
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
        })
    }

    #[test]
    fn explicit_links_control_deterministic_story_flow() {
        let scene = resolve_bounded_text_flow(&projection(true, "ABCDEFGHIJ"), environment())
            .expect("projection should resolve");

        assert_eq!(scene.text_fragments.len(), 2);
        assert_eq!(scene.text_fragments[0].frame_origin, node_id(10));
        assert_eq!(scene.text_fragments[0].text, "ABCDEF");
        assert_eq!(scene.text_fragments[1].frame_origin, node_id(11));
        assert_eq!(scene.text_fragments[1].text, "GHIJ");
        assert!(
            !scene
                .diagnostics
                .iter()
                .any(|diagnostic| { diagnostic.code == "story_overset" })
        );
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
    fn fallback_start_next_moves_noninitial_paragraph_to_successor() {
        let projection = projection(true, "ABC\rDEF");
        let flow = [flow_run(
            4,
            7,
            BoundedParagraphFlowConstraint::StartInNextTextBox,
        )];

        let scene =
            resolve_bounded_text_flow_with_paragraph_flow(&projection, environment(), &flow)
                .expect("flow");

        assert_eq!(scene.text_fragments.len(), 2);
        assert_eq!(scene.text_fragments[0].frame_origin, node_id(10));
        assert_eq!(scene.text_fragments[0].text, "ABC\r");
        assert_eq!(scene.text_fragments[1].frame_origin, node_id(11));
        assert_eq!(scene.text_fragments[1].scalar_start, 4);
        assert_eq!(scene.text_fragments[1].text, "DEF");
    }

    #[test]
    fn fallback_keep_lines_together_moves_whole_paragraph() {
        let projection = projection(true, "ABC\rDEFGHI");
        let flow = [flow_run(
            4,
            10,
            BoundedParagraphFlowConstraint::KeepLinesTogether,
        )];

        let off = resolve_bounded_text_flow(&projection, environment()).expect("off");
        let on = resolve_bounded_text_flow_with_paragraph_flow(&projection, environment(), &flow)
            .expect("on");

        assert_eq!(off.text_fragments[0].text, "ABC\rDE");
        assert_eq!(on.text_fragments[0].text, "ABC\r");
        assert_eq!(on.text_fragments[1].text, "DEFGHI");
    }

    #[test]
    fn fallback_keep_with_next_moves_paragraph_pair() {
        let projection = projection(true, "ABC\rD\rE");
        let flow = [flow_run(4, 6, BoundedParagraphFlowConstraint::KeepWithNext)];

        let off = resolve_bounded_text_flow(&projection, environment()).expect("off");
        let on = resolve_bounded_text_flow_with_paragraph_flow(&projection, environment(), &flow)
            .expect("on");

        assert_eq!(off.text_fragments[0].text, "ABC\rD\r");
        assert_eq!(on.text_fragments[0].text, "ABC\r");
        assert_eq!(on.text_fragments[1].text, "D\rE");
    }

    #[test]
    fn fallback_widow_control_moves_one_row_orphan_case() {
        let projection = projection(true, "ABC\rDEFG");
        let flow = [flow_run(4, 8, BoundedParagraphFlowConstraint::WidowControl)];

        let off = resolve_bounded_text_flow(&projection, environment()).expect("off");
        let on = resolve_bounded_text_flow_with_paragraph_flow(&projection, environment(), &flow)
            .expect("on");

        assert_eq!(off.text_fragments[0].text, "ABC\rDE");
        assert_eq!(on.text_fragments[0].text, "ABC\r");
        assert_eq!(on.text_fragments[1].text, "DEFG");
    }

    #[test]
    fn fallback_start_next_without_successor_leaves_target_overset() {
        let mut projection = projection(true, "ABC\rDEF");
        projection
            .story_frames
            .retain(|frame| frame.frame_origin == node_id(10));
        projection.story_frames[0].next_frame_origin = None;
        let flow = [flow_run(
            4,
            7,
            BoundedParagraphFlowConstraint::StartInNextTextBox,
        )];

        let scene =
            resolve_bounded_text_flow_with_paragraph_flow(&projection, environment(), &flow)
                .expect("flow");

        assert_eq!(scene.text_fragments.len(), 1);
        assert_eq!(scene.text_fragments[0].text, "ABC\r");
        assert!(scene.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "story_overset" && diagnostic.origin == story_id(7).into_canonical()
        }));
    }

    #[test]
    fn shared_story_without_links_is_not_silently_chained() {
        let scene = resolve_bounded_text_flow(&projection(false, "ABCDEFG"), environment())
            .expect("projection should resolve");

        assert!(scene.text_fragments.is_empty());
        assert!(
            scene
                .diagnostics
                .iter()
                .any(|diagnostic| { diagnostic.code == "shared_story_without_explicit_flow" })
        );
    }

    #[test]
    fn overset_is_explicit() {
        let scene = resolve_bounded_text_flow(&projection(true, "ABCDEFGHIJKLM"), environment())
            .expect("projection should resolve");

        assert_eq!(scene.text_fragments.len(), 2);
        assert!(
            scene
                .diagnostics
                .iter()
                .any(|diagnostic| { diagnostic.code == "story_overset" })
        );
    }

    #[test]
    fn missing_metrics_is_explicit() {
        let mut environment = environment();
        environment.text_metrics = None;

        let scene = resolve_bounded_text_flow(&projection(true, "ABC"), environment)
            .expect("projection should resolve");

        assert!(scene.text_fragments.is_empty());
        assert!(
            scene
                .diagnostics
                .iter()
                .any(|diagnostic| { diagnostic.code == "text_metrics_missing" })
        );
    }

    #[test]
    fn fixed_input_and_environment_are_deterministic() {
        let projection = projection(true, "ABCDEFGHIJ");
        let left = resolve_bounded_text_flow(&projection, environment()).unwrap();
        let right = resolve_bounded_text_flow(&projection, environment()).unwrap();

        assert_eq!(left, right);
    }

    #[test]
    fn explicit_chain_helper_rejects_ordinal_only_flow() {
        let first = ProjectedStoryFrame {
            story_origin: story_id(1),
            frame_origin: node_id(1),
            ordinal: 0,
            previous_frame_origin: None,
            next_frame_origin: None,
        };
        let second = ProjectedStoryFrame {
            story_origin: story_id(1),
            frame_origin: node_id(2),
            ordinal: 1,
            previous_frame_origin: None,
            next_frame_origin: None,
        };

        assert_eq!(
            explicit_chain(&[&first, &second]),
            Err("shared_story_without_explicit_flow")
        );
    }
}
