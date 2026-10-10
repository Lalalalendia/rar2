//! Fail-closed current-Story physical-font line-fit preview.
//!
//! Consumes canonical FontResource spans from EditorSession, *never* a browser
//! supplied family name or host font. Line candidates are independently
//! reshaped per effective physical span: OpenType metrics, kerning and cluster
//! provenance remain attached to their exact resource. UAX #14 supplies break
//! opportunities; the existing pub-layout reciprocal linked-frame law orders
//! frames. Publisher source font bindings without trusted full bytes yield NO
//! pretend reflow. Fixed line advance is an explicit preview model, not proof
//! of native Publisher baseline, overset, or PDF equivalence.

use super::{
    CURRENT_FONT_SPANS_V1, CurrentPhysicalFontSpanV1, CurrentPhysicalFontSpansV1,
    DesktopShapedFlowRuntimeError, shape_current_exact_font_override_spans_v1,
};
use chaptera_text_format_overlay::{FontResourceIdentityV1, ServerFontResourceV1};
use pub_editor::EditorSession;
use pub_layout::{
    BoundedBreakKind, BoundedLayoutEnvironment, BoundedLayoutProjection, BoundedShapedGlyph,
    BoundedShapedText, BoundedShapingRuntime, break_policy_for_shaped_text,
    font_fingerprint_sha256, project_bounded, shape_bounded_ltr_segment,
    validated_projected_story_frame_chain_v1,
};
use pub_model::{LengthEmu, NodeId, StoryId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const CURRENT_MIXED_FONT_FLOW_V1: &str = "chaptera.current-mixed-font-line-fit.v1";
const MAX_SCALARS: usize = 8192;
const MAX_FRAMES: usize = 64;
const MAX_EVALUATIONS: usize = 20000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CurrentMixedFontFlowStateV1 {
    /// All current scalars have exact admitted full physical font bytes, and
    /// bounded LTR line-break/linked-frame fit was calculated. NOT PDF authority.
    PhysicalLineFitPreview,
    /// Source font binding lacks physical byte authorization. No lines are
    /// fabricated and no source font is borrowed from the replacement.
    SourceFontUnresolved,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CurrentMissingFontRangeV1 {
    pub start_scalar: u32,
    pub end_scalar: u32,
    pub source_font_binding_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CurrentMixedFontLineFragmentV1 {
    pub scalar_start: u32,
    pub scalar_end: u32,
    pub resource: FontResourceIdentityV1,
    pub font_size_emu: LengthEmu,
    pub units_per_em: u32,
    pub glyphs: Vec<BoundedShapedGlyph>,
    pub measured_width: LengthEmu,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CurrentMixedFontLineV1 {
    pub frame_origin: NodeId,
    pub frame_line_index: u32,
    pub scalar_start: u32,
    pub visible_scalar_end: u32,
    pub consumed_scalar_end: u32,
    pub measured_width: LengthEmu,
    pub break_kind: BoundedBreakKind,
    pub fragments: Vec<CurrentMixedFontLineFragmentV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CurrentMixedFontFlowPreviewV1 {
    pub protocol_version: String,
    pub story_id: String,
    pub story_format_state_hash: String,
    pub story_scalar_len: u32,
    pub state: CurrentMixedFontFlowStateV1,
    pub source_gaps: Vec<CurrentMissingFontRangeV1>,
    pub explicit_line_advance_emu: LengthEmu,
    pub lines: Vec<CurrentMixedFontLineV1>,
    pub overset_start_scalar: Option<u32>,
    pub unicode_breaks_evaluated: bool,
    /// Fixed line advance + bounded LTR shaping is not native Publisher
    /// baseline placement. Never route this preview into fixed output.
    pub native_publisher_layout_authoritative: bool,
    pub fixed_pdf_allowed: bool,
}

#[derive(Debug, Clone)]
struct CandidateLine {
    end: usize,
    visible_end: usize,
    width: LengthEmu,
    fragments: Vec<CurrentMixedFontLineFragmentV1>,
    kind: BoundedBreakKind,
}

fn blocked(code: &'static str, reason: impl Into<String>) -> DesktopShapedFlowRuntimeError {
    DesktopShapedFlowRuntimeError::new(code, reason)
}

fn checked_exact_resource(
    resource: &ServerFontResourceV1<'_>,
) -> Result<String, DesktopShapedFlowRuntimeError> {
    let identity = resource.identity;
    if !resource.is_full_resource
        || !resource.authoring_admitted
        || resource.full_font_bytes.is_empty()
        || resource.face_count == 0
        || u32::from(identity.face_index) >= resource.face_count
    {
        return Err(blocked(
            "mixed_font_exact_resource_not_admitted",
            "a complete independently admitted physical font resource is mandatory",
        ));
    }
    let actual = font_fingerprint_sha256(resource.full_font_bytes);
    if identity.content_hash != actual || identity.font_fingerprint != format!("sha256:{actual}") {
        return Err(blocked(
            "mixed_font_exact_resource_changed",
            "physical font fingerprint differs from trusted authoring bytes",
        ));
    }
    Ok(actual)
}

/// Authoritative *input*, bounded *preview*: current EditorSession and trusted
/// original physical bytes, never untrusted serialized glyphs, drive this call.
pub fn build_current_story_mixed_font_flow_preview_v1(
    editor: &EditorSession,
    story_id: StoryId,
    trusted: &ServerFontResourceV1<'_>,
    explicit_line_advance_emu: LengthEmu,
) -> Result<CurrentMixedFontFlowPreviewV1, DesktopShapedFlowRuntimeError> {
    let story = editor.graph().stories.get(&story_id).ok_or_else(|| {
        blocked(
            "mixed_font_story_missing",
            "Story is absent in current EditorSession",
        )
    })?;
    let spans = shape_current_exact_font_override_spans_v1(editor, story_id, trusted)?;
    // The same bounded viewer Story/frames projection is used by current
    // shaped-flow; do not manufacture a parallel geometric document model.
    let authoring =
        pub_viewer::bounded_authoring_slice_from_resolved_story_payload(editor.graph(), story_id)
            .map_err(|error| blocked("mixed_font_projection_blocked", error.to_string()))?;
    let projection = project_bounded(authoring);
    place_current_exact_mixed_font_spans_v1(
        &story.text,
        story_id,
        &spans,
        trusted,
        &projection,
        explicit_line_advance_emu,
    )
}

/// This pure internal planner accepts only spans from the canonical current
/// font consumer; it checks the entire scalar partition and grant again.
/// It intentionally never emits partial lines for unresolved source fonts.
fn place_current_exact_mixed_font_spans_v1(
    text: &str,
    story_id: StoryId,
    spans: &CurrentPhysicalFontSpansV1,
    trusted: &ServerFontResourceV1<'_>,
    projection: &BoundedLayoutProjection,
    line_advance: LengthEmu,
) -> Result<CurrentMixedFontFlowPreviewV1, DesktopShapedFlowRuntimeError> {
    let chars: Vec<char> = text.chars().collect();
    let story_len = u32::try_from(chars.len()).map_err(|_| {
        blocked(
            "mixed_font_story_extent_overflow",
            "Story scalar extent exceeds u32",
        )
    })?;
    if spans.protocol_version != CURRENT_FONT_SPANS_V1
        || spans.story_id != story_id.as_canonical().to_string()
        || spans.story_scalar_len != story_len
        || line_advance.get() <= 0
        || chars.len() > MAX_SCALARS
    {
        return Err(blocked(
            "mixed_font_input_out_of_scope",
            "canonical Story spans, positive explicit line advance and bounded extent are required",
        ));
    }
    if projection
        .stories
        .iter()
        .filter(|candidate| candidate.origin == story_id)
        .count()
        != 1
        || projection
            .stories
            .iter()
            .find(|candidate| candidate.origin == story_id)
            .is_none_or(|candidate| candidate.text != text)
    {
        return Err(blocked(
            "mixed_font_projection_stale",
            "projected Story content is not the current canonical Story text",
        ));
    }

    let mut source_gaps = Vec::new();
    let mut cursor = 0_u32;
    for part in &spans.spans {
        let (start, end) = match part {
            CurrentPhysicalFontSpanV1::SourceUnresolved {
                start_scalar,
                end_scalar,
                source_font_binding_id,
            } => {
                if source_font_binding_id.is_empty() {
                    return Err(blocked(
                        "mixed_font_invalid_source_binding",
                        "empty source font binding",
                    ));
                }
                source_gaps.push(CurrentMissingFontRangeV1 {
                    start_scalar: *start_scalar,
                    end_scalar: *end_scalar,
                    source_font_binding_id: source_font_binding_id.clone(),
                });
                (*start_scalar, *end_scalar)
            }
            CurrentPhysicalFontSpanV1::AdmittedExact {
                start_scalar,
                end_scalar,
                identity,
                shaped,
                font_size_emu,
            } => {
                if identity != trusted.identity || font_size_emu.get() <= 0 {
                    return Err(blocked(
                        "mixed_font_untrusted_span",
                        "current physical glyph span is not the trusted resource",
                    ));
                }
                if shaped
                    .glyphs
                    .iter()
                    .any(|glyph| glyph.cluster < *start_scalar || glyph.cluster >= *end_scalar)
                {
                    return Err(blocked(
                        "mixed_font_clusters_invalid",
                        "physical span glyph clusters escape canonical Story range",
                    ));
                }
                (*start_scalar, *end_scalar)
            }
        };
        if start != cursor || end <= start || end > story_len {
            return Err(blocked(
                "mixed_font_partition_invalid",
                "font spans must form one contiguous exact Unicode scalar partition",
            ));
        }
        cursor = end;
    }
    if cursor != story_len || spans.all_scalars_shaped != source_gaps.is_empty() {
        return Err(blocked(
            "mixed_font_partition_invalid",
            "font span coverage or full-admission flag contradicts current Story extent",
        ));
    }

    let mut result = CurrentMixedFontFlowPreviewV1 {
        protocol_version: CURRENT_MIXED_FONT_FLOW_V1.to_owned(),
        story_id: spans.story_id.clone(),
        story_format_state_hash: spans.story_format_state_hash.clone(),
        story_scalar_len: story_len,
        state: if source_gaps.is_empty() {
            CurrentMixedFontFlowStateV1::PhysicalLineFitPreview
        } else {
            CurrentMixedFontFlowStateV1::SourceFontUnresolved
        },
        source_gaps,
        explicit_line_advance_emu: line_advance,
        lines: Vec::new(),
        overset_start_scalar: None,
        unicode_breaks_evaluated: false,
        native_publisher_layout_authoritative: false,
        fixed_pdf_allowed: false,
    };
    if result.state == CurrentMixedFontFlowStateV1::SourceFontUnresolved || chars.is_empty() {
        // Source metrics are unknown: even the first line could move, so
        // synthesizing downstream line/frame placement would be dishonest.
        return Ok(result);
    }
    let fingerprint = checked_exact_resource(trusted)?;

    let frames: Vec<_> = projection
        .story_frames
        .iter()
        .filter(|frame| frame.story_origin == story_id)
        .collect();
    if frames.is_empty() || frames.len() > MAX_FRAMES {
        return Err(blocked(
            "mixed_font_frames_out_of_scope",
            "real Story needs one to 64 projected text frames",
        ));
    }
    let chain = validated_projected_story_frame_chain_v1(&frames)
        .map_err(|reason| blocked("mixed_font_invalid_frame_chain", reason))?;
    let geom: BTreeMap<_, _> = projection
        .node_geometry
        .iter()
        .map(|node| (node.origin, node.bounds))
        .collect();

    let full_glyphs = spans
        .spans
        .iter()
        .flat_map(|span| match span {
            CurrentPhysicalFontSpanV1::AdmittedExact { shaped, .. } => shaped.glyphs.clone(),
            CurrentPhysicalFontSpanV1::SourceUnresolved { .. } => Vec::new(),
        })
        .collect::<Vec<_>>();
    if full_glyphs.iter().any(|glyph| {
        glyph.glyph_id == 0
            && !chars
                .get(glyph.cluster as usize)
                .is_some_and(|ch| matches!(*ch, '\r' | '\n'))
    }) {
        return Err(blocked(
            "mixed_font_glyph_unavailable",
            "exact physical font lacks at least one required glyph; no fallback is authorized",
        ));
    }
    let policy = break_policy_for_shaped_text(text, &full_glyphs)
        .map_err(|error| blocked("mixed_font_unicode_break_policy_blocked", error.to_string()))?;
    let mut current = 0_usize;
    let mut evaluations = 0_usize;
    for frame_id in chain {
        if current == chars.len() {
            break;
        }
        let bounds = geom.get(&frame_id).ok_or_else(|| {
            blocked(
                "mixed_font_frame_geometry_missing",
                "linked text frame has no projected geometry",
            )
        })?;
        if bounds.width.get() <= 0 || bounds.height.get() <= 0 {
            continue;
        }
        let rows = usize::try_from(bounds.height.get() / line_advance.get())
            .map_err(|_| {
                blocked(
                    "mixed_font_capacity_overflow",
                    "frame row count exceeds usize",
                )
            })?
            .min(chars.len());
        for row in 0..rows {
            if current == chars.len() {
                break;
            }
            let mut chosen: Option<CandidateLine> = None;
            for candidate in policy.candidates.iter().filter(|candidate| {
                usize::try_from(candidate.scalar_boundary).is_ok_and(|end| end > current)
            }) {
                evaluations = evaluations.checked_add(1).ok_or_else(|| {
                    blocked("mixed_font_candidate_limit", "candidate counter overflow")
                })?;
                if evaluations > MAX_EVALUATIONS {
                    return Err(blocked(
                        "mixed_font_candidate_limit",
                        "too many independent physical-font break evaluations",
                    ));
                }
                let next = usize::try_from(candidate.scalar_boundary)
                    .map_err(|_| blocked("mixed_font_break_extent", "break scalar overflow"))?;
                let visible = if candidate.kind == BoundedBreakKind::Mandatory {
                    let mut end = next;
                    while end > current && matches!(chars[end - 1], '\r' | '\n') {
                        end -= 1;
                    }
                    end
                } else {
                    next
                };
                let fragments = reshape_physical_line_fragments_v1(
                    &chars,
                    current,
                    visible,
                    &spans.spans,
                    trusted,
                    &fingerprint,
                )?;
                let width = fragments
                    .iter()
                    .try_fold(LengthEmu::ZERO, |width, frag| {
                        width.checked_add(frag.measured_width)
                    })
                    .ok_or_else(|| {
                        blocked(
                            "mixed_font_width_overflow",
                            "glyph width summation overflow",
                        )
                    })?;
                if width.get() <= bounds.width.get() {
                    chosen = Some(CandidateLine {
                        end: next,
                        visible_end: visible,
                        width,
                        fragments,
                        kind: candidate.kind,
                    });
                }
                if candidate.kind == BoundedBreakKind::Mandatory {
                    break;
                }
            }
            let Some(line) = chosen else {
                // Do not split an unbreakable word or invent hyphenation.
                // A later, wider linked frame may still fit the same word.
                break;
            };
            let start = u32::try_from(current)
                .map_err(|_| blocked("mixed_font_break_extent", "line start overflow"))?;
            result.lines.push(CurrentMixedFontLineV1 {
                frame_origin: frame_id,
                frame_line_index: u32::try_from(row).map_err(|_| {
                    blocked("mixed_font_capacity_overflow", "frame line index overflow")
                })?,
                scalar_start: start,
                visible_scalar_end: u32::try_from(line.visible_end)
                    .map_err(|_| blocked("mixed_font_break_extent", "visible end overflow"))?,
                consumed_scalar_end: u32::try_from(line.end)
                    .map_err(|_| blocked("mixed_font_break_extent", "consumed end overflow"))?,
                measured_width: line.width,
                break_kind: line.kind,
                fragments: line.fragments,
            });
            current = line.end;
        }
    }
    result.unicode_breaks_evaluated = true;
    if current < chars.len() {
        result.overset_start_scalar = Some(u32::try_from(current).map_err(|_| {
            blocked(
                "mixed_font_story_extent_overflow",
                "overset cursor overflow",
            )
        })?);
    }
    Ok(result)
}

/// Always shape each *candidate line segment*, not only the original full
/// Story. This enforces exact shaping at newly selected line boundaries and at
/// formatting changes, including unsafe HarfRust ligature break points.
fn reshape_physical_line_fragments_v1(
    chars: &[char],
    start: usize,
    visible_end: usize,
    spans: &[CurrentPhysicalFontSpanV1],
    trusted: &ServerFontResourceV1<'_>,
    fingerprint: &str,
) -> Result<Vec<CurrentMixedFontLineFragmentV1>, DesktopShapedFlowRuntimeError> {
    let mut result = Vec::new();
    for part in spans {
        let CurrentPhysicalFontSpanV1::AdmittedExact {
            start_scalar,
            end_scalar,
            font_size_emu,
            identity,
            ..
        } = part
        else {
            return Err(blocked(
                "mixed_font_source_byte_gap",
                "source font gaps cannot be reshaped by borrowing alternate font bytes",
            ));
        };
        if identity != trusted.identity {
            return Err(blocked(
                "mixed_font_resource_changed",
                "font resource identity mismatch",
            ));
        }
        let lo = usize::try_from(*start_scalar)
            .map_err(|_| blocked("mixed_font_span_overflow", "start scalar overflow"))?
            .max(start);
        let hi = usize::try_from(*end_scalar)
            .map_err(|_| blocked("mixed_font_span_overflow", "end scalar overflow"))?
            .min(visible_end);
        if hi <= lo {
            continue;
        }
        let text: String = chars[lo..hi].iter().collect();
        let runtime = BoundedShapingRuntime {
            layout: BoundedLayoutEnvironment {
                engine_revision: CURRENT_MIXED_FONT_FLOW_V1.to_owned(),
                font_set_fingerprint: fingerprint.to_owned(),
                resource_fingerprint: identity.resource_id.clone(),
            },
            face_index: u32::from(identity.face_index),
            font_size_emu: *font_size_emu,
            font_bytes: trusted.full_font_bytes,
        };
        let shaped: BoundedShapedText = shape_bounded_ltr_segment(
            &text,
            u32::try_from(lo)
                .map_err(|_| blocked("mixed_font_span_overflow", "cluster base overflow"))?,
            &runtime,
        )
        .map_err(|error| blocked("mixed_font_break_reshaping_failed", error.to_string()))?;
        if shaped.glyphs.iter().any(|glyph| glyph.glyph_id == 0) {
            return Err(blocked(
                "mixed_font_glyph_unavailable",
                "a line-boundary reshape produced a missing glyph; no fallback is authorized",
            ));
        }
        let end = u32::try_from(hi)
            .map_err(|_| blocked("mixed_font_span_overflow", "fragment end overflow"))?;
        let start = u32::try_from(lo)
            .map_err(|_| blocked("mixed_font_span_overflow", "fragment start overflow"))?;
        if shaped
            .glyphs
            .iter()
            .any(|glyph| glyph.cluster < start || glyph.cluster >= end)
        {
            return Err(blocked(
                "mixed_font_break_cluster_invalid",
                "line fragment glyph clusters escape global Story scalar boundaries",
            ));
        }
        result.push(CurrentMixedFontLineFragmentV1 {
            scalar_start: start,
            scalar_end: end,
            resource: identity.clone(),
            font_size_emu: *font_size_emu,
            units_per_em: shaped.units_per_em,
            glyphs: shaped.glyphs,
            measured_width: shaped.total_x_advance,
        });
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chaptera_text_format_overlay::{FontAuthoringScopeV1, FontReplacementCandidateV1};
    use pub_layout::{BoundedAuthoringSlice, BoundedNodeGeometryInput, project_bounded};
    use pub_model::{Affine2D, CanonicalId, Page, PageId, RectEmu, Size2D, Story, StoryFrame};
    use std::{collections::BTreeMap, env, fs};

    fn id(b: u8) -> CanonicalId {
        CanonicalId::from_bytes([b; 16])
    }
    fn story_id() -> StoryId {
        StoryId::from_canonical(id(7))
    }
    fn node_id(n: u8) -> NodeId {
        NodeId::from_canonical(id(n))
    }
    fn page_id() -> PageId {
        PageId::from_canonical(id(1))
    }

    fn identity(bytes: &[u8]) -> FontResourceIdentityV1 {
        let content_hash = font_fingerprint_sha256(bytes);
        FontResourceIdentityV1 {
            resource_id: "f27a8036-8492-480f-8fa6-d2e775cc9f12".to_owned(),
            font_fingerprint: format!("sha256:{content_hash}"),
            content_hash,
            face_index: 0,
        }
    }

    fn projected(text: &str, width1: i64, width2: i64, linked: bool) -> BoundedLayoutProjection {
        let page = page_id();
        let first = node_id(10);
        let second = node_id(11);
        project_bounded(BoundedAuthoringSlice {
            pages: vec![Page {
                id: page,
                size: Size2D::new(LengthEmu::new(20_000_000), LengthEmu::new(20_000_000)),
                bleed: None,
                margins: None,
                children: vec![first, second],
                extensions: Vec::new(),
            }],
            node_geometry: vec![
                BoundedNodeGeometryInput {
                    node_id: first,
                    parent_origin: page.into_canonical(),
                    bounds: RectEmu::new(
                        LengthEmu::ZERO,
                        LengthEmu::ZERO,
                        LengthEmu::new(width1),
                        LengthEmu::new(120_000),
                    ),
                    transform: Affine2D::identity(),
                },
                BoundedNodeGeometryInput {
                    node_id: second,
                    parent_origin: page.into_canonical(),
                    bounds: RectEmu::new(
                        LengthEmu::new(1_000_000),
                        LengthEmu::ZERO,
                        LengthEmu::new(width2),
                        LengthEmu::new(120_000),
                    ),
                    transform: Affine2D::identity(),
                },
            ],
            stories: vec![Story {
                id: story_id(),
                text: text.to_owned(),
                paragraphs: Vec::new(),
                runs: Vec::new(),
                fields: Vec::new(),
                hyperlinks: Vec::new(),
                source_refs: Vec::new(),
            }],
            // Reverse source vector intentionally. Reciprocal links, not
            // array position or guessed ordinal, own this Story's topology.
            story_frames: vec![
                StoryFrame {
                    story_id: story_id(),
                    frame_id: second,
                    ordinal: 1,
                    previous: linked.then_some(first),
                    next: None,
                },
                StoryFrame {
                    story_id: story_id(),
                    frame_id: first,
                    ordinal: 0,
                    previous: None,
                    next: linked.then_some(second),
                },
            ],
            tables: Vec::new(),
            guides: Vec::new(),
            unknown_layout_state: Vec::new(),
        })
    }

    fn exact_spans(
        text: &str,
        font_bytes: &[u8],
        cuts: &[(u32, u32, i64)],
    ) -> CurrentPhysicalFontSpansV1 {
        let ident = identity(font_bytes);
        let partitions = cuts
            .iter()
            .map(|(start, end, size)| {
                let chars = text.chars().collect::<Vec<_>>();
                let piece: String = chars[*start as usize..*end as usize].iter().collect();
                let runtime = BoundedShapingRuntime {
                    layout: BoundedLayoutEnvironment {
                        engine_revision: CURRENT_MIXED_FONT_FLOW_V1.to_owned(),
                        font_set_fingerprint: ident.content_hash.clone(),
                        resource_fingerprint: ident.resource_id.clone(),
                    },
                    face_index: 0,
                    font_size_emu: LengthEmu::new(*size),
                    font_bytes,
                };
                let shaped = shape_bounded_ltr_segment(&piece, *start, &runtime).unwrap();
                CurrentPhysicalFontSpanV1::AdmittedExact {
                    start_scalar: *start,
                    end_scalar: *end,
                    identity: ident.clone(),
                    font_size_emu: LengthEmu::new(*size),
                    shaped: Box::new(shaped),
                }
            })
            .collect();
        CurrentPhysicalFontSpansV1 {
            protocol_version: CURRENT_FONT_SPANS_V1.to_owned(),
            story_id: story_id().as_canonical().to_string(),
            story_format_state_hash: "sha256:synthetic-case".to_owned(),
            story_scalar_len: u32::try_from(text.chars().count()).unwrap(),
            spans: partitions,
            all_scalars_shaped: true,
            authoritative_line_breaks: false,
            fixed_pdf_allowed: false,
        }
    }

    #[test]
    fn unicode_breaks_reflow_exact_widths_across_reciprocal_linked_frames_and_report_overset() {
        let bytes = include_bytes!("../../../assets/fonts/ofl/abel/Abel-Regular.ttf");
        let identity = identity(bytes);
        let resource = ServerFontResourceV1 {
            identity: &identity,
            full_font_bytes: bytes,
            face_count: 1,
            is_full_resource: true,
            authoring_admitted: true,
        };
        let text = "Hfi Hfi Hfi";
        let spans = exact_spans(text, bytes, &[(0, 11, 120_000)]);
        let runtime = BoundedShapingRuntime {
            layout: BoundedLayoutEnvironment {
                engine_revision: CURRENT_MIXED_FONT_FLOW_V1.to_owned(),
                font_set_fingerprint: identity.content_hash.clone(),
                resource_fingerprint: identity.resource_id.clone(),
            },
            face_index: 0,
            font_size_emu: LengthEmu::new(120_000),
            font_bytes: bytes,
        };
        let word_width = shape_bounded_ltr_segment("Hfi ", 0, &runtime)
            .unwrap()
            .total_x_advance
            .get();
        let projection = projected(text, word_width, word_width, true);
        let a = place_current_exact_mixed_font_spans_v1(
            text,
            story_id(),
            &spans,
            &resource,
            &projection,
            LengthEmu::new(120_000),
        )
        .unwrap();
        assert_eq!(a.state, CurrentMixedFontFlowStateV1::PhysicalLineFitPreview);
        assert!(a.unicode_breaks_evaluated);
        assert!(!a.native_publisher_layout_authoritative);
        assert!(!a.fixed_pdf_allowed);
        assert_eq!(a.lines.len(), 2);
        assert_eq!(a.lines[0].frame_origin, node_id(10));
        assert_eq!(a.lines[1].frame_origin, node_id(11));
        assert_eq!(a.lines[0].scalar_start, 0);
        assert_eq!(a.lines[0].consumed_scalar_end, 4);
        assert_eq!(a.lines[1].scalar_start, 4);
        assert_eq!(a.lines[1].consumed_scalar_end, 8);
        assert_eq!(a.overset_start_scalar, Some(8));
        for line in &a.lines {
            assert!(line.measured_width.get() <= word_width);
            assert_eq!(line.fragments.len(), 1);
            assert!(line.fragments[0].glyphs.iter().all(|glyph| {
                line.scalar_start <= glyph.cluster && glyph.cluster < line.visible_scalar_end
            }));
        }
        assert_eq!(
            a,
            place_current_exact_mixed_font_spans_v1(
                text,
                story_id(),
                &spans,
                &resource,
                &projection,
                LengthEmu::new(120_000)
            )
            .unwrap()
        );

        let broken = projected(text, word_width, word_width, false);
        let denied = place_current_exact_mixed_font_spans_v1(
            text,
            story_id(),
            &spans,
            &resource,
            &broken,
            LengthEmu::new(120_000),
        )
        .unwrap_err();
        assert_eq!(denied.code, "mixed_font_invalid_frame_chain");
    }

    #[test]
    fn physical_spans_with_different_effective_sizes_remain_distinct_inside_one_line() {
        let bytes = include_bytes!("../../../assets/fonts/ofl/abel/Abel-Regular.ttf");
        let identity = identity(bytes);
        let trust = ServerFontResourceV1 {
            identity: &identity,
            full_font_bytes: bytes,
            face_count: 1,
            is_full_resource: true,
            authoring_admitted: true,
        };
        let text = "Hfi Hfi";
        let spans = exact_spans(text, bytes, &[(0, 4, 120_000), (4, 7, 240_000)]);
        let projection = projected(text, 10_000_000, 10_000_000, true);
        let a = place_current_exact_mixed_font_spans_v1(
            text,
            story_id(),
            &spans,
            &trust,
            &projection,
            LengthEmu::new(120_000),
        )
        .unwrap();
        assert_eq!(a.lines.len(), 1);
        let line = &a.lines[0];
        assert_eq!(line.fragments.len(), 2);
        assert_eq!(line.fragments[0].font_size_emu.get(), 120_000);
        assert_eq!(line.fragments[1].font_size_emu.get(), 240_000);
        assert_eq!(line.fragments[0].scalar_start, 0);
        assert_eq!(line.fragments[1].scalar_start, 4);
        assert!(
            line.fragments[1]
                .glyphs
                .iter()
                .all(|glyph| glyph.cluster >= 4)
        );
        let sum = line
            .fragments
            .iter()
            .map(|f| f.measured_width.get())
            .sum::<i64>();
        assert_eq!(sum, line.measured_width.get());
        assert_eq!(a.overset_start_scalar, None);
        assert!(!a.fixed_pdf_allowed);
    }

    #[test]
    fn missing_source_font_and_altered_full_bytes_never_fabricate_line_flow() {
        let bytes = include_bytes!("../../../assets/fonts/ofl/abel/Abel-Regular.ttf");
        let identity = identity(bytes);
        let resource = ServerFontResourceV1 {
            identity: &identity,
            full_font_bytes: bytes,
            face_count: 1,
            is_full_resource: true,
            authoring_admitted: true,
        };
        let text = "Hfi Hfi";
        let projected = projected(text, 10_000_000, 10_000_000, true);
        let mut spans = exact_spans(text, bytes, &[(0, 7, 120_000)]);
        spans.all_scalars_shaped = false;
        spans.spans = vec![
            CurrentPhysicalFontSpanV1::SourceUnresolved {
                start_scalar: 0,
                end_scalar: 1,
                source_font_binding_id: "pub-source-font:missing-original".to_owned(),
            },
            exact_spans(text, bytes, &[(1, 7, 120_000)]).spans.remove(0),
        ];
        let blocked = place_current_exact_mixed_font_spans_v1(
            text,
            story_id(),
            &spans,
            &resource,
            &projected,
            LengthEmu::new(120_000),
        )
        .unwrap();
        assert_eq!(
            blocked.state,
            CurrentMixedFontFlowStateV1::SourceFontUnresolved
        );
        assert_eq!(blocked.source_gaps.len(), 1);
        assert!(!blocked.unicode_breaks_evaluated);
        assert!(blocked.lines.is_empty());
        assert_eq!(blocked.overset_start_scalar, None);
        assert!(!blocked.fixed_pdf_allowed);

        let wrong = ServerFontResourceV1 {
            identity: &identity,
            full_font_bytes: b"wrong-original-bytes",
            face_count: 1,
            is_full_resource: true,
            authoring_admitted: true,
        };
        let all = exact_spans(text, bytes, &[(0, 7, 120_000)]);
        assert_eq!(
            place_current_exact_mixed_font_spans_v1(
                text,
                story_id(),
                &all,
                &wrong,
                &projected,
                LengthEmu::new(120_000),
            )
            .unwrap_err()
            .code,
            "mixed_font_exact_resource_changed"
        );
        // A syntactically valid but unavailable glyph must not silently inherit
        // an ambient host font or claim valid frame allocation.
        let mut unavailable = all.clone();
        if let CurrentPhysicalFontSpanV1::AdmittedExact { shaped, .. } = &mut unavailable.spans[0] {
            shaped.glyphs[0].glyph_id = 0;
        }
        assert_eq!(
            place_current_exact_mixed_font_spans_v1(
                text,
                story_id(),
                &unavailable,
                &resource,
                &projected,
                LengthEmu::new(120_000),
            )
            .unwrap_err()
            .code,
            "mixed_font_glyph_unavailable"
        );
    }

    #[test]
    fn real_publisher_full_physical_override_reflows_and_survives_fresh_project_reopen() {
        use pub_editor::{EditorProjectFontReopenGrantV1, Sha256Digest, open_mature_0x2c_editor};
        use sha2::{Digest, Sha256};

        let Some(path) = env::var_os("CHAPTERA_SAMPLE_NEWSLETTER") else {
            eprintln!("pinned SampleNewsletter only runs in its dedicated real-PUB workflow");
            return;
        };
        let source = fs::read(path).expect("real pinned source PUB");
        let source_hash = Sha256Digest::from_bytes(Sha256::digest(&source).into());
        let mut editor = open_mature_0x2c_editor(&source, source_hash)
            .expect("real source-backed EditorSession");
        let story_id = editor
            .graph()
            .stories
            .keys()
            .copied()
            .find(|id| {
                let story = &editor.graph().stories[id];
                let len = story.text.chars().count();
                len > 3
                    && len <= MAX_SCALARS
                    && editor.current_text_format_overlay_v1(*id).is_ok()
                    && pub_viewer::bounded_authoring_slice_from_resolved_story_payload(
                        editor.graph(),
                        *id,
                    )
                    .ok()
                    .is_some_and(|slice| {
                        let projection = project_bounded(slice);
                        let frames = projection
                            .story_frames
                            .iter()
                            .filter(|f| f.story_origin == *id)
                            .collect::<Vec<_>>();
                        !frames.is_empty()
                            && validated_projected_story_frame_chain_v1(&frames).is_ok()
                            && projection.node_geometry.iter().any(|node| {
                                frames.iter().any(|f| f.frame_origin == node.origin)
                                    && node.bounds.width.get() > 0
                                    && node.bounds.height.get() > 0
                            })
                    })
            })
            .expect("real pinned PUB with fully typed Story format and reciprocal frames");

        let font_bytes = include_bytes!("../../../assets/fonts/ofl/abel/Abel-Regular.ttf");
        let identity = identity(font_bytes);
        let trust = ServerFontResourceV1 {
            identity: &identity,
            full_font_bytes: font_bytes,
            face_count: 1,
            is_full_resource: true,
            authoring_admitted: true,
        };
        let line_advance = LengthEmu::new(152_400);
        let original =
            build_current_story_mixed_font_flow_preview_v1(&editor, story_id, &trust, line_advance)
                .expect("source font still unresolved without independently admitted bytes");
        assert_eq!(
            original.state,
            CurrentMixedFontFlowStateV1::SourceFontUnresolved
        );
        assert!(original.lines.is_empty());

        let current = editor.project();
        let scope = FontAuthoringScopeV1 {
            document_id: current.identity.as_ref().unwrap().document_id.clone(),
            revision_id: "sha256:".to_owned() + &"1".repeat(64),
            scene_snapshot_id: "sha256:".to_owned() + &"2".repeat(64),
            layout_environment_id: "sha256:".to_owned() + &"3".repeat(64),
            font_set_fingerprint: "sha256:".to_owned() + &"4".repeat(64),
        };
        let candidate = FontReplacementCandidateV1 {
            protocol_version: "chaptera.font-replacement-candidate.v1".to_owned(),
            document_id: scope.document_id.clone(),
            expected_revision_id: scope.revision_id.clone(),
            scene_snapshot_id: scope.scene_snapshot_id.clone(),
            layout_environment_id: scope.layout_environment_id.clone(),
            font_set_fingerprint: scope.font_set_fingerprint.clone(),
            resource_id: identity.resource_id.clone(),
            font_fingerprint: identity.font_fingerprint.clone(),
            content_hash: identity.content_hash.clone(),
            face_index: identity.face_index,
            authority: "candidate_only_server_validation_required".to_owned(),
        };
        let scalar_len =
            u32::try_from(editor.graph().stories[&story_id].text.chars().count()).unwrap();
        let before_hash = editor.current_text_format_state_hash_v1(story_id).unwrap();
        editor
            .set_admitted_font_resource_v1(
                story_id,
                0,
                scalar_len,
                &candidate,
                &scope,
                &trust,
                &before_hash,
            )
            .expect("bounded whole-Story test resource admission by Rust");
        let flow =
            build_current_story_mixed_font_flow_preview_v1(&editor, story_id, &trust, line_advance)
                .expect("current exact full physical font should calculate frame-fit preview");
        assert_eq!(
            flow.state,
            CurrentMixedFontFlowStateV1::PhysicalLineFitPreview
        );
        assert!(flow.unicode_breaks_evaluated);
        assert!(flow.source_gaps.is_empty());
        assert!(!flow.lines.is_empty() || flow.overset_start_scalar.is_some());
        assert!(!flow.native_publisher_layout_authoritative);
        assert!(!flow.fixed_pdf_allowed);
        assert_eq!(flow, build_current_story_mixed_font_flow_preview_v1(
            &editor, story_id, &trust, line_advance,
        ).unwrap());

        let project = editor.project();
        editor.undo().expect("exact-font Undo");
        assert_eq!(
            build_current_story_mixed_font_flow_preview_v1(
                &editor, story_id, &trust, line_advance,
            ).unwrap(),
            original,
        );
        editor.redo().expect("exact-font Redo");
        assert_eq!(
            build_current_story_mixed_font_flow_preview_v1(
                &editor, story_id, &trust, line_advance,
            ).unwrap(),
            flow,
        );

        let disk = serde_json::to_vec(&project).unwrap();
        let loaded: pub_editor::EditorProject = serde_json::from_slice(&disk).unwrap();
        let fresh_grant = EditorProjectFontReopenGrantV1 {
            source_hash,
            project_document_id: &loaded.identity.as_ref().unwrap().document_id,
            resource: ServerFontResourceV1 {
                identity: &identity,
                full_font_bytes: font_bytes,
                face_count: 1,
                is_full_resource: true,
                authoring_admitted: true,
            },
        };
        let mut fresh = open_mature_0x2c_editor(&source, source_hash)
            .expect("independently reopen original real PUB");
        fresh
            .apply_project_with_admitted_font_resources_v1(
                &loaded,
                &BTreeMap::new(),
                &[fresh_grant],
            )
            .expect("re-admit original full Abel bytes before replay");
        assert_eq!(
            build_current_story_mixed_font_flow_preview_v1(&fresh, story_id, &trust, line_advance,)
                .expect("fresh current state must compute the same line-fit preview"),
            flow,
        );
        assert_eq!(fresh.source_hash(), source_hash);
        println!(
            "REAL_PUB_MIXED_FONT_FLOW_OK {}",
            serde_json::json!({
                "publisher_source": "SampleNewsletter.pub",
                "current_editor_project": true,
                "real_exact_byte_glyphs": true,
                "unicode_17_line_breaks": true,
                "reciprocal_frame_fit": true,
                "undo_redo": true,
                "fresh_reopen": true,
                "missing_source_fallback": false,
                "publisher_layout_authoritative": false,
                "fixed_pdf_allowed": false,
            })
        );
    }
}
