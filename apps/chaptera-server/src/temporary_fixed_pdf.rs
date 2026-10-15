//! Temporary guest fixed-output adapter. This is not a PUB renderer.
//!
//! Replays are performed by product_replay_worker before this function is called,
//! under filesystem/network seccomp deny. We combine the *current* EditorSession
//! with the existing Viewer resolved resources and the source-neutral pub-pdf
//! renderer. The original PUB remains immutable.
use std::{
    collections::{BTreeMap, BTreeSet},
    str::FromStr,
};

use chaptera_viewer_render_plan::{
    ExplicitRenderTextFontResourceV1, RenderTextLayoutDispositionV1,
    build_page_render_plan_with_text_layout_v1, uniform_text_color_rgb_v1,
};

use pub_editor::{EditOperation, EditorSession, LengthEmu, NodeId, Sha256Digest, StoryId};
use pub_layout::{
    BoundedLayoutEnvironment, BoundedShapedFlowRuntime, BoundedShapedText, BoundedShapingRuntime,
    font_fingerprint_sha256, project_bounded, resolve_bounded_shaped_flow,
    shape_bounded_ltr_segment,
};
use pub_output::{
    ExplicitFontResource, FixedOutputFontProfile, FontIdentity, OutputFontRequest,
    PreferredEmbedding, plan_output_fonts, read_opentype_embedding_flags,
};
use pub_pdf::{
    FixedFontResource, FixedImagePlacement, FixedImageResource, FixedNodePaint, FixedPdfResources,
    FixedStroke, FixedTextRun, PdfRenderDisposition, PdfRenderError, PdfTargetProfile,
    render_bounded_pdf, render_bounded_pdf_a4_card_9up,
};
use pub_viewer::ViewerTypographyRun;
use serde_json::{Value, json};

const MAX_PRIVATE_PDF_BYTES: usize = 16 * 1024 * 1024;
const LOSS_REPORT_SCHEMA: &str = "chaptera.temporary-edited-fixed-pdf-loss.v1";

pub(crate) struct CurrentFixedPdf {
    pub bytes: Vec<u8>,
    pub loss_report: Value,
}

const EMU_PER_POINT: i64 = 12_700;

/// Only a uniform, source-hash-current typography range can authorize fixed
/// output paint/size. A canonical StoryId alone is not enough: an edited Story
/// invalidates source Quill spans even when the immutable PUB is unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SourcePdfLineStyleV1 {
    color_rgb: Option<[u8; 3]>,
    font_size_emu: Option<i64>,
}

fn admitted_source_line_style_v1(
    runs: &[ViewerTypographyRun],
    story_id: StoryId,
    current_story_text: &str,
    scalar_start: u32,
    scalar_end: u32,
) -> Option<SourcePdfLineStyleV1> {
    if scalar_start >= scalar_end {
        return None;
    }
    let mut candidates = runs
        .iter()
        .filter(|run| {
            run.story_id == story_id
                && run.applies_to_story_text(current_story_text)
                && run.scalar_start < scalar_end
                && run.scalar_end > scalar_start
        })
        .collect::<Vec<_>>();
    if candidates.is_empty() {
        return None;
    }
    candidates.sort_by_key(|run| (run.scalar_start, run.scalar_end));

    // Exactly cover the painted Story scalar interval; reject gaps, overlaps,
    // duplicates and stale input, rather than fabricating an inherited style.
    let mut cursor = scalar_start;
    for (index, run) in candidates.iter().enumerate() {
        if run.scalar_start > cursor
            || (index > 0 && run.scalar_start != cursor)
            || run.scalar_end <= cursor
        {
            return None;
        }
        cursor = run.scalar_end.min(scalar_end);
    }
    if cursor != scalar_end {
        return None;
    }

    let first = candidates[0];
    let color_rgb = candidates
        .iter()
        .all(|run| run.color_rgb == first.color_rgb)
        .then_some(first.color_rgb)
        .flatten();
    let font_size_emu = (first.text_size_emu > 0
        && candidates
            .iter()
            .all(|run| run.text_size_emu == first.text_size_emu))
    .then_some(i64::from(first.text_size_emu));
    Some(SourcePdfLineStyleV1 {
        color_rgb,
        font_size_emu,
    })
}

/// The only source-derived authority is the already-replayed EditorSession.
/// In particular, never call the PUB-to-PDF source CLI on original source bytes.
pub(crate) fn render_current_fixed_pdf(
    original: &[u8],
    expected_source_hash: &str,
    session: &EditorSession,
) -> Result<CurrentFixedPdf, &'static str> {
    render_current_fixed_pdf_with_sheet(original, expected_source_hash, session, false)
}

/// The one existing guest EditorSession current-revision PDF pipeline, with
/// nine vector copies on a physical A4 sheet. No new authoring or PUB parsing.
pub(crate) fn render_current_fixed_pdf_a4_card_9up(
    original: &[u8],
    expected_source_hash: &str,
    session: &EditorSession,
) -> Result<CurrentFixedPdf, &'static str> {
    render_current_fixed_pdf_with_sheet(original, expected_source_hash, session, true)
}

fn render_current_fixed_pdf_with_sheet(
    original: &[u8],
    expected_source_hash: &str,
    session: &EditorSession,
    nine_up: bool,
) -> Result<CurrentFixedPdf, &'static str> {
    let source_hash =
        Sha256Digest::from_str(expected_source_hash).map_err(|_| "invalid_source_hash")?;
    if session.source_hash() != source_hash {
        return Err("edited_pdf_source_identity_mismatch");
    }
    let mut bundle =
        pub_viewer::open_pub_bundle(original, pub_viewer::viewer_geometry_environment_v0_1())
            .map_err(|_| "edited_pdf_source_unsupported")?;
    if bundle.geometry.document.source.source_hash != source_hash
        || bundle.resolved_graph.source.source_hash != source_hash
    {
        return Err("edited_pdf_source_identity_mismatch");
    }

    // The Guest contract admits exactly MoveNode, ResizeNode and plain Story.
    // Unknown operations must never be silently discarded by a fixed exporter.
    let mut moved = BTreeSet::<NodeId>::new();
    let mut resized = BTreeSet::<NodeId>::new();
    let mut edited_stories = 0usize;
    for operation in session.operations() {
        match operation {
            EditOperation::MoveNode { node_id, .. } => {
                moved.insert(*node_id);
            }
            EditOperation::ResizeNode { node_id, .. } => {
                resized.insert(*node_id);
            }
            EditOperation::ReplaceStoryRange { .. } => edited_stories += 1,
            _ => return Err("edited_pdf_operation_unsupported"),
        }
    }

    // Preserve the Viewer page-role, paints and exact image-resource authority.
    // Only admitted direct-page MoveNode/ResizeNode geometry may be
    // projected. ResizeNode can keep x/y unchanged and must use its own
    // canonical eligibility gate rather than a synthetic move.
    for scene_node in &mut bundle.geometry.scene.nodes {
        if !moved.contains(&scene_node.origin) && !resized.contains(&scene_node.origin) {
            continue;
        }
        let authored = session
            .graph()
            .nodes
            .get(&scene_node.origin)
            .ok_or("edited_pdf_node_missing")?;
        let move_admitted = moved.contains(&scene_node.origin)
            && session
                .can_move_node_to(
                    scene_node.origin,
                    authored.header.bounds.x,
                    authored.header.bounds.y,
                )
                .is_ok();
        let resize_admitted = resized.contains(&scene_node.origin)
            && session.can_resize_node(scene_node.origin).is_ok();
        if authored.header.parent_id != scene_node.parent_origin
            || !(move_admitted || resize_admitted)
        {
            return Err("edited_pdf_node_not_page_local");
        }
        scene_node.bounds = authored.header.bounds;
    }
    for node_id in moved.iter().chain(resized.iter()) {
        let authored = session
            .graph()
            .nodes
            .get(node_id)
            .ok_or("edited_pdf_node_missing")?;
        if !bundle.geometry.scene.nodes.iter().any(|node| {
            node.origin == *node_id
                && node.parent_origin == authored.header.parent_id
                && node.bounds == authored.header.bounds
        }) {
            return Err("edited_pdf_changed_geometry_not_representable");
        }
    }

    let pages = bundle
        .geometry
        .document
        .pages
        .iter()
        .map(|page| page.id)
        .collect::<Vec<_>>();
    if pages.is_empty() {
        return Err("edited_pdf_no_customer_pages");
    }
    let authoring =
        pub_viewer::bounded_authoring_slice_from_resolved_pages(session.graph(), &pages)
            .map_err(|_| "edited_pdf_current_layout_unavailable")?;
    let projection = project_bounded(authoring);

    // The same pinned, license-disclosed fallback used by the Chaptera Reader.
    // Never discover or copy host/customer source fonts.
    chaptera_desktop_fallback_font_resource::validate()
        .map_err(|_| "edited_pdf_fallback_font_invalid")?;
    let font = chaptera_desktop_fallback_font_resource::bytes();
    let fingerprint = font_fingerprint_sha256(font);
    let font_size = LengthEmu::new(chaptera_desktop_fallback_font_resource::FONT_SIZE_EMU);
    let shaping = BoundedShapingRuntime {
        layout: BoundedLayoutEnvironment {
            engine_revision: LOSS_REPORT_SCHEMA.into(),
            font_set_fingerprint: fingerprint.clone(),
            resource_fingerprint: format!("pinned-ubuntu-light:{fingerprint}"),
        },
        face_index: 0,
        font_size_emu: font_size,
        font_bytes: font,
    };
    let flow = resolve_bounded_shaped_flow(
        &projection,
        &BoundedShapedFlowRuntime {
            shaping,
            line_height: LengthEmu::new(chaptera_desktop_fallback_font_resource::LINE_HEIGHT_EMU),
        },
    )
    .map_err(|_| "edited_pdf_current_text_layout_unavailable")?;

    // The Reader already shapes source-verified multi-line frames with the
    // pinned fallback font at their admitted typography size, including exact
    // line breaks, alignment offsets and frame-local vertical placement.
    // Consume that shared RenderPlan directly instead of independently breaking
    // an unchanged Story a second time at a different size/font policy.
    //
    // This deliberately only admits complete, one-frame, source-hash-current
    // Stories. Unresolved/mixed fonts, edited Story text, resized frames,
    // overset, missing glyphs, or unsupported resources keep the legacy path.
    // No source font is discovered or copied from the host/customer.
    const MAX_SHARED_STORIES: usize = 32;
    const MAX_SHARED_PAGES: usize = 8;
    const MAX_SHARED_LINES: usize = 512;
    const MAX_SHARED_SCALARS_PER_STORY: usize = 16_384;
    let scene_ids = bundle
        .geometry
        .scene
        .nodes
        .iter()
        .map(|node| node.origin)
        .collect::<BTreeSet<_>>();
    let mut shared_multiline = BTreeMap::<StoryId, Vec<FixedTextRun>>::new();
    // Only fixed reason labels and aggregate counts; never source text or IDs.
    let mut shared_reject_counts = BTreeMap::<&'static str, usize>::new();
    let mut shared_reject_lines = BTreeMap::<&'static str, usize>::new();
    macro_rules! reject_shared {
        ($reason:expr, $line_count:expr) => {{
            *shared_reject_counts.entry($reason).or_default() += 1;
            *shared_reject_lines.entry($reason).or_default() += $line_count;
        }};
    }
    let fallback = ExplicitRenderTextFontResourceV1 {
        resource_id: chaptera_desktop_fallback_font_resource::RESOURCE_ID,
        expected_sha256: chaptera_desktop_fallback_font_resource::EXPECTED_SHA256,
        face_index: 0,
        default_font_size_emu: chaptera_desktop_fallback_font_resource::FONT_SIZE_EMU,
        default_line_height_emu: chaptera_desktop_fallback_font_resource::LINE_HEIGHT_EMU,
        bytes: font,
    };
    if projection.stories.len() <= MAX_SHARED_STORIES
        && bundle.geometry.document.pages.len() <= MAX_SHARED_PAGES
    {
        for page_index in 0..bundle.geometry.document.pages.len() {
            let Ok(page_plan) =
                build_page_render_plan_with_text_layout_v1(&bundle.geometry, page_index, &fallback)
            else {
                continue;
            };
            for node in page_plan.nodes {
                if !scene_ids.contains(&node.node_id) || resized.contains(&node.node_id) {
                    continue;
                }
                let Some(fragment) = node.text else {
                    continue;
                };
                if shared_multiline.contains_key(&fragment.story_id) {
                    continue;
                }
                let Some(layout) = fragment.layout.as_ref() else {
                    reject_shared!("layout_absent", 0);
                    continue;
                };
                let RenderTextLayoutDispositionV1::SharedResolved {
                    font_resource_id,
                    font_fingerprint_sha256,
                    font_size_emu,
                    line_height_emu,
                } = &layout.disposition
                else {
                    reject_shared!("reader_layout_fallback", layout.lines.len());
                    continue;
                };
                let (font_size_emu, line_height_emu) = (*font_size_emu, *line_height_emu);
                if font_resource_id.as_str() != fallback.resource_id
                    || font_fingerprint_sha256.as_str() != fingerprint.as_str()
                {
                    reject_shared!("font_resource_mismatch", layout.lines.len());
                    continue;
                }
                if font_size_emu == font_size.get()
                    || !(6 * EMU_PER_POINT..=72 * EMU_PER_POINT).contains(&font_size_emu)
                    || line_height_emu <= 0
                    || !(2..=MAX_SHARED_LINES).contains(&layout.lines.len())
                {
                    reject_shared!("source_size_or_line_count_ineligible", layout.lines.len());
                    continue;
                }
                let Some(story) = projection
                    .stories
                    .iter()
                    .find(|story| story.origin == fragment.story_id)
                else {
                    reject_shared!("projection_story_missing", layout.lines.len());
                    continue;
                };
                let source_scalars = story.text.chars().collect::<Vec<_>>();
                let Ok(scalar_len) = u32::try_from(source_scalars.len()) else {
                    reject_shared!("scalar_count_overflow", layout.lines.len());
                    continue;
                };
                if source_scalars.len() > MAX_SHARED_SCALARS_PER_STORY {
                    reject_shared!("story_scalar_limit", layout.lines.len());
                    continue;
                }
                if fragment.scalar_start != 0
                    || fragment.scalar_end != scalar_len
                    || fragment.text != story.text
                {
                    reject_shared!("reader_fragment_not_whole_story", layout.lines.len());
                    continue;
                }
                if session
                    .graph()
                    .stories
                    .get(&story.origin)
                    .is_none_or(|current| current.text != story.text)
                {
                    reject_shared!("current_story_different", layout.lines.len());
                    continue;
                }
                // A detached prefix or multiple linked frames cannot inherit
                // the single-frame Reader viewport geometry by coincidence.
                let frames = projection
                    .story_frames
                    .iter()
                    .filter(|frame| frame.story_origin == story.origin)
                    .collect::<Vec<_>>();
                if frames.len() != 1 || frames[0].frame_origin != node.node_id {
                    reject_shared!("linked_or_other_frame", layout.lines.len());
                    continue;
                }
                let Some(source_style) = admitted_source_line_style_v1(
                    &bundle.geometry.typography_runs,
                    story.origin,
                    &story.text,
                    0,
                    scalar_len,
                ) else {
                    reject_shared!("source_style_unavailable", layout.lines.len());
                    continue;
                };
                if source_style.font_size_emu != Some(font_size_emu) {
                    reject_shared!("source_style_size_not_uniform", layout.lines.len());
                    continue;
                }

                let content_bounds = node.text_bounds.as_ref().unwrap_or(&node.bounds);
                let Some(origin_x) = content_bounds.x.get().checked_sub(node.bounds.x.get()) else {
                    reject_shared!("bounds_overflow", layout.lines.len());
                    continue;
                };
                let Some(origin_y) = content_bounds.y.get().checked_sub(node.bounds.y.get()) else {
                    reject_shared!("bounds_overflow", layout.lines.len());
                    continue;
                };
                if origin_x < 0
                    || origin_y < 0
                    || content_bounds.width.get() <= 0
                    || content_bounds.height.get() <= 0
                    || layout.vertical_offset_emu < 0
                {
                    reject_shared!("content_bounds_invalid", layout.lines.len());
                    continue;
                }
                let fill_rgb = uniform_text_color_rgb_v1(&fragment)
                    .or(source_style.color_rgb)
                    .unwrap_or([0, 0, 0]);
                let mut admitted = Vec::<FixedTextRun>::new();
                let mut consumed = 0_u32;
                let mut cursor_y = layout.vertical_offset_emu;
                let mut valid = true;
                let mut line_reject_reason = "line_sequence_or_text";
                for (index, line) in layout.lines.iter().enumerate() {
                    let Ok(index) = u32::try_from(index) else {
                        valid = false;
                        break;
                    };
                    let Some(start) = usize::try_from(line.scalar_start).ok() else {
                        valid = false;
                        break;
                    };
                    let Some(end) = usize::try_from(line.scalar_end).ok() else {
                        valid = false;
                        break;
                    };
                    if line.line_index != index
                        || line.scalar_start != consumed
                        || line.scalar_end < consumed
                        || line.consumed_scalar_end < line.scalar_end
                        || line.consumed_scalar_end > scalar_len
                        || source_scalars
                            .get(start..end)
                            .is_none_or(|slice| slice.iter().collect::<String>() != line.text)
                        || !line.spans.is_empty()
                        || line.measured_width_emu < 0
                        || line.x_offset_emu < 0
                        || line.line_height_emu != line_height_emu
                    {
                        valid = false;
                        break;
                    }
                    let (Some(next_y), Some(local_x), Some(local_y)) = (
                        cursor_y.checked_add(line.line_height_emu),
                        origin_x.checked_add(line.x_offset_emu),
                        origin_y
                            .checked_add(cursor_y)
                            .and_then(|y| y.checked_add(font_size_emu)),
                    ) else {
                        line_reject_reason = "line_coordinate_overflow";
                        valid = false;
                        break;
                    };
                    if next_y > content_bounds.height.get() {
                        line_reject_reason = "line_vertical_fit";
                        valid = false;
                        break;
                    }
                    if line
                        .x_offset_emu
                        .checked_add(line.measured_width_emu)
                        .is_none_or(|width| width > content_bounds.width.get())
                    {
                        line_reject_reason = "line_horizontal_fit";
                        valid = false;
                        break;
                    }
                    if !line.text.is_empty() {
                        let Some(shaping) = line.shaping.as_ref() else {
                            valid = false;
                            break;
                        };
                        if shaping.units_per_em == 0
                            || shaping.environment.face_index != fallback.face_index
                            || shaping.environment.font_size_emu.get() != font_size_emu
                            || shaping.environment.layout.font_set_fingerprint != fingerprint
                            || shaping.glyphs.is_empty()
                            || shaping.glyphs.iter().any(|glyph| {
                                glyph.glyph_id == 0
                                    || glyph.cluster < line.scalar_start
                                    || glyph.cluster >= line.scalar_end
                            })
                        {
                            line_reject_reason = "glyph_shaping_invalid";
                            valid = false;
                            break;
                        }
                        admitted.push(FixedTextRun {
                            node_id: node.node_id,
                            scalar_base: line.scalar_start,
                            logical_text: line.text.clone(),
                            shaped: BoundedShapedText {
                                environment: shaping.environment.clone(),
                                units_per_em: shaping.units_per_em,
                                glyphs: shaping.glyphs.clone(),
                                total_x_advance: LengthEmu::new(line.measured_width_emu),
                            },
                            baseline_x: LengthEmu::new(local_x),
                            baseline_y: LengthEmu::new(local_y),
                            fill_rgb,
                        });
                    }
                    consumed = line.consumed_scalar_end;
                    cursor_y = next_y;
                }
                let counted = shared_multiline.values().map(Vec::len).sum::<usize>();
                if valid
                    && consumed == scalar_len
                    && admitted.len() > 1
                    && counted + admitted.len() <= MAX_SHARED_LINES
                {
                    shared_multiline.insert(story.origin, admitted);
                } else if !valid {
                    reject_shared!(line_reject_reason, layout.lines.len());
                } else if consumed != scalar_len {
                    reject_shared!("incomplete_line_coverage", layout.lines.len());
                } else if admitted.len() <= 1 {
                    reject_shared!("less_than_two_painted_lines", layout.lines.len());
                } else {
                    reject_shared!("total_line_budget", layout.lines.len());
                }
            }
        }
    }
    let source_sized_multiline_stories = shared_multiline.len();
    let source_sized_multiline_lines = shared_multiline.values().map(Vec::len).sum::<usize>();

    let mut text_runs = Vec::new();
    let mut glyphs = BTreeSet::new();
    let mut unavailable_glyph_lines = 0usize;
    let mut source_color_lines = 0usize;
    let mut source_sized_single_lines = 0usize;
    for line in &flow.lines {
        if shared_multiline.contains_key(&line.story_origin) || line.text.is_empty() {
            continue;
        }
        if !scene_ids.contains(&line.frame_origin) {
            return Err("edited_pdf_current_text_frame_missing");
        }
        if line.glyphs.iter().any(|glyph| glyph.glyph_id == 0) {
            // The standard source-PDF CLI likewise omits unshapable runs with
            // an explicit side-channel loss. Never silently replace glyph 0.
            unavailable_glyph_lines += 1;
            continue;
        }
        let current_story = session.graph().stories.get(&line.story_origin);
        let source_style = current_story.and_then(|story| {
            admitted_source_line_style_v1(
                &bundle.geometry.typography_runs,
                line.story_origin,
                &story.text,
                line.scalar_start,
                line.scalar_end,
            )
        });
        let color_rgb = source_style
            .and_then(|style| style.color_rgb)
            .unwrap_or([0, 0, 0]);
        if source_style.is_some_and(|style| style.color_rgb.is_some()) {
            source_color_lines += 1;
        }

        let mut shaped = BoundedShapedText {
            environment: flow.environment.shaping.clone(),
            units_per_em: line.units_per_em,
            glyphs: line.glyphs.clone(),
            total_x_advance: line.measured_width,
        };
        let mut line_font_size = font_size;
        // Source size is only portable with our pinned fallback for complete,
        // one-line Stories whose independently re-shaped glyphs fit their exact
        // current frame. This must not silently reflow arbitrary paragraphs.
        if let (Some(style), Some(story)) = (source_style, current_story)
            && let Some(source_size) = style.font_size_emu
            && (6 * EMU_PER_POINT..=72 * EMU_PER_POINT).contains(&source_size)
            && source_size != font_size.get()
            && line.scalar_start == 0
            && line.frame_line_index == 0
            && u32::try_from(story.text.chars().count()).ok() == Some(line.consumed_scalar_end)
            && flow
                .lines
                .iter()
                .filter(|other| other.story_origin == line.story_origin)
                .count()
                == 1
            && let Some(frame) = bundle
                .geometry
                .scene
                .nodes
                .iter()
                .find(|node| node.origin == line.frame_origin)
            && source_size <= frame.bounds.height.get()
        {
            let source_runtime = BoundedShapingRuntime {
                layout: flow.environment.shaping.layout.clone(),
                face_index: 0,
                font_size_emu: LengthEmu::new(source_size),
                font_bytes: font,
            };
            if let Ok(candidate) =
                shape_bounded_ltr_segment(&line.text, line.scalar_start, &source_runtime)
                && candidate.total_x_advance.get() > 0
                && candidate.total_x_advance.get() <= frame.bounds.width.get()
                && candidate.glyphs.iter().all(|glyph| glyph.glyph_id != 0)
            {
                shaped = candidate;
                line_font_size = LengthEmu::new(source_size);
                source_sized_single_lines += 1;
            }
        }

        let row = i64::from(line.frame_line_index)
            .checked_mul(chaptera_desktop_fallback_font_resource::LINE_HEIGHT_EMU)
            .ok_or("edited_pdf_text_coordinate_overflow")?;
        let baseline_y = row
            .checked_add(line_font_size.get())
            .ok_or("edited_pdf_text_coordinate_overflow")?;
        glyphs.extend(shaped.glyphs.iter().map(|glyph| glyph.glyph_id));
        text_runs.push(FixedTextRun {
            node_id: line.frame_origin,
            scalar_base: line.scalar_start,
            logical_text: line.text.clone(),
            shaped,
            baseline_x: LengthEmu::ZERO,
            baseline_y: LengthEmu::new(baseline_y),
            fill_rgb: color_rgb,
        });
    }

    // Preserve the exact Reader-resolved per-line shaping and offsets when
    // admitted. This is still the pinned fallback font, not Publisher Calibri.
    for (_story_id, lines) in shared_multiline {
        for line in lines {
            if line.fill_rgb != [0, 0, 0] {
                source_color_lines += 1;
            }
            glyphs.extend(line.shaped.glyphs.iter().map(|glyph| glyph.glyph_id));
            text_runs.push(line);
        }
    }

    let (font_plan, fonts) = if text_runs.is_empty() {
        (None, Vec::new())
    } else {
        let embedding = read_opentype_embedding_flags(font, 0)
            .map_err(|_| "edited_pdf_fallback_font_embedding_unknown")?;
        let identity = FontIdentity {
            fingerprint_sha256: fingerprint.clone(),
            face_index: 0,
        };
        let mut profile = FixedOutputFontProfile::basic_pdf_v0_1();
        profile.preferred_embedding = PreferredEmbedding::Full;
        let plan = plan_output_fonts(
            &profile,
            vec![OutputFontRequest {
                source: identity.clone(),
                source_resource: Some(ExplicitFontResource {
                    identity: identity.clone(),
                    bytes: font,
                    embedding,
                }),
                fallback_resource: None,
                used_glyph_ids: glyphs,
            }],
        );
        if !plan.can_serialize() {
            return Err("edited_pdf_font_embedding_blocked");
        }
        (
            Some(plan),
            vec![FixedFontResource {
                identity,
                bytes: font.to_vec(),
            }],
        )
    };

    let paints = bundle
        .geometry
        .paints
        .iter()
        .filter(|paint| scene_ids.contains(&paint.node_id))
        .map(|paint| FixedNodePaint {
            node_id: paint.node_id,
            fill_rgb: paint.solid_fill_rgb,
            stroke: paint.solid_line.as_ref().map(|line| FixedStroke {
                rgb: line.rgb,
                width_emu: line.width_emu,
            }),
        })
        .collect();
    let images = bundle
        .geometry
        .images
        .iter()
        .filter_map(|image| {
            let node_ids = image
                .node_ids
                .iter()
                .copied()
                .filter(|id| scene_ids.contains(id))
                .collect::<Vec<_>>();
            if node_ids.is_empty() {
                return None;
            }
            let placements = image
                .placements
                .iter()
                .filter(|placement| {
                    node_ids.contains(&placement.node_id)
                        && (placement.content_rotation_degrees.is_some()
                            || (image.mime == "image/gif"
                                && (placement.source_window.is_some()
                                    || placement.recolor.is_some())))
                })
                .map(|placement| FixedImagePlacement {
                    node_id: placement.node_id,
                    content_rotation_degrees: placement.content_rotation_degrees,
                    source_window_present: placement.source_window.is_some(),
                    recolor_present: placement.recolor.is_some(),
                })
                .collect();
            Some(FixedImageResource {
                resource_id: image.resource_id,
                mime: image.mime.clone(),
                source_exact: image.source_exact,
                node_ids,
                placements,
                bytes: image.bytes.clone(),
            })
        })
        .collect();
    let resources = FixedPdfResources {
        page_order: pages,
        node_paints: paints,
        images,
        font_plan,
        fonts,
        text_runs,
    };
    let rendered = if nine_up {
        render_bounded_pdf_a4_card_9up(
            &bundle.geometry.scene,
            &resources,
            &PdfTargetProfile::basic_geometry_v0_1(),
        )
    } else {
        render_bounded_pdf(
            &bundle.geometry.scene,
            &resources,
            &PdfTargetProfile::basic_geometry_v0_1(),
        )
    }
    .map_err(|error| match error {
        PdfRenderError::NineUpCardGeometryUnsupported => "edited_pdf_nine_up_geometry_unsupported",
        _ => "edited_pdf_render_failed",
    })?;
    if rendered.bytes.len() <= 5
        || rendered.bytes.len() > MAX_PRIVATE_PDF_BYTES
        || !rendered.bytes.starts_with(b"%PDF-")
    {
        return Err("edited_pdf_output_invalid");
    }

    let painted = rendered
        .report
        .nodes
        .iter()
        .filter(|node| node.disposition == PdfRenderDisposition::Painted)
        .count();
    let partial = rendered
        .report
        .nodes
        .iter()
        .filter(|node| node.disposition == PdfRenderDisposition::Partial)
        .count();
    let unsupported = rendered
        .report
        .nodes
        .iter()
        .filter(|node| node.disposition == PdfRenderDisposition::Unsupported)
        .count();
    let layout_warnings = flow.diagnostics.len();
    let pdf_warnings = rendered.report.diagnostics.len();
    // Repeating a partial render nine times is not valid physical print
    // acceptance. Ordinary single-page PDF retains its existing explicit
    // LossReport, while 9-up refuses lossy or unsupported content.
    if nine_up
        && (rendered.report.pages.len() != 1
            || partial != 0
            || unsupported != 0
            || unavailable_glyph_lines != 0
            || layout_warnings != 0
            || pdf_warnings != 0)
    {
        return Err("edited_pdf_nine_up_visual_loss_unresolved");
    }
    // No original Story text or private font/PUB bytes appear in this report.
    let loss_report = json!({
        "schema_version": LOSS_REPORT_SCHEMA,
        "target": if nine_up { "pdf9up" } else { "pdf" },
        "renderer": "pub-pdf-v0.1",
        "current_editor_state": true,
        "can_serialize": true,
        "source_hash": expected_source_hash,
        "font": {
            "family": chaptera_desktop_fallback_font_resource::FAMILY_NAME,
            "disposition": chaptera_desktop_fallback_font_resource::DISPOSITION,
            "fingerprint_sha256": fingerprint,
            "license": chaptera_desktop_fallback_font_resource::LICENSE_ID,
            "embedding_policy": "full_if_technically_permitted"
        },
        "counts": {
            "blocking": 0,
            "pages": rendered.report.pages.len(),
            "painted": painted,
            "partial": partial,
            "unsupported": unsupported,
            "shaped_lines": resources.text_runs.len(),
            "font_missing_glyph_lines": unavailable_glyph_lines,
            "source_color_lines": source_color_lines,
            "source_sized_single_lines": source_sized_single_lines,
            "source_sized_multiline_stories": source_sized_multiline_stories,
            "source_sized_multiline_lines": source_sized_multiline_lines,
            "source_shared_rejections_by_reason": shared_reject_counts,
            "source_shared_rejected_lines_by_reason": shared_reject_lines,
            "moved_nodes": moved.len(),
            "edited_stories": edited_stories,
            "layout_warnings": layout_warnings,
            "pdf_warnings": pdf_warnings
        },
        "fidelity": if unavailable_glyph_lines > 0 || partial > 0 || unsupported > 0 || layout_warnings > 0
            || pdf_warnings > 0 || !resources.text_runs.is_empty() {
                "partial"
            } else {
                "bounded"
            },
        "limitations": [
            "fixed_pdf_bounded_visual_subset_not_publisher_parity",
            "fallback_ubuntu_light_not_original_source_font",
            "source_uniform_multiline_exact_reader_lines_only_for_current_quill_story",
            "source_size_single_line_only_if_fits_exact_frame",
            "mixed_or_stale_source_typography_keeps_fallback",
            "missing_fallback_glyph_lines_disclosed_not_serialized",
            "publisher_rich_vectors_and_effects_not_fully_supported"
        ]
    });
    Ok(CurrentFixedPdf {
        bytes: rendered.bytes,
        loss_report,
    })
}

#[cfg(test)]
mod source_pdf_style_tests {
    use super::*;

    fn story_id() -> StoryId {
        serde_json::from_str("\"1f1f1f1f-1f1f-1f1f-1f1f-1f1f1f1f1f1f\"")
            .expect("synthetic stable StoryId")
    }

    fn typography(
        range: (u32, u32),
        color_rgb: Option<[u8; 3]>,
        text_size_emu: u32,
        text: &str,
    ) -> ViewerTypographyRun {
        ViewerTypographyRun {
            story_id: story_id(),
            scalar_start: range.0,
            scalar_end: range.1,
            source_font_name: "Synthetic Family".into(),
            source_font_index: Some(3),
            text_size_emu,
            font_inherited: false,
            size_inherited: false,
            color_rgb,
            color_inherited: false,
            bold: None,
            italic: None,
            source_story_text_sha256: pub_viewer::viewer_story_text_sha256(text),
        }
    }

    #[test]
    fn current_uniform_source_color_and_size_admitted_for_inner_line() {
        let text = "Heading";
        let runs = vec![
            typography((0, 3), Some([211, 8, 81]), 28 * 12_700, text),
            typography((3, 7), Some([211, 8, 81]), 28 * 12_700, text),
        ];
        assert_eq!(
            admitted_source_line_style_v1(&runs, story_id(), text, 0, 7),
            Some(SourcePdfLineStyleV1 {
                color_rgb: Some([211, 8, 81]),
                font_size_emu: Some(28 * 12_700),
            })
        );
        assert_eq!(
            admitted_source_line_style_v1(&runs, story_id(), text, 1, 6),
            Some(SourcePdfLineStyleV1 {
                color_rgb: Some([211, 8, 81]),
                font_size_emu: Some(28 * 12_700),
            })
        );
    }

    #[test]
    fn stale_story_gaps_overlaps_and_mixed_styles_fail_closed() {
        let text = "Heading";
        let original = typography((0, 7), Some([211, 8, 81]), 28 * 12_700, text);
        assert_eq!(
            admitted_source_line_style_v1(
                std::slice::from_ref(&original),
                story_id(),
                "Changed",
                0,
                7,
            ),
            None,
            "a story mutation may never inherit stale Quill typography"
        );
        let gap = vec![
            typography((0, 3), Some([211, 8, 81]), 28 * 12_700, text),
            typography((4, 7), Some([211, 8, 81]), 28 * 12_700, text),
        ];
        assert_eq!(
            admitted_source_line_style_v1(&gap, story_id(), text, 0, 7),
            None
        );

        let overlaps = vec![
            typography((0, 4), Some([211, 8, 81]), 28 * 12_700, text),
            typography((3, 7), Some([211, 8, 81]), 28 * 12_700, text),
        ];
        assert_eq!(
            admitted_source_line_style_v1(&overlaps, story_id(), text, 0, 7),
            None
        );

        let mixed = vec![
            typography((0, 3), Some([211, 8, 81]), 28 * 12_700, text),
            typography((3, 7), Some([0, 0, 0]), 12 * 12_700, text),
        ];
        assert_eq!(
            admitted_source_line_style_v1(&mixed, story_id(), text, 0, 7),
            Some(SourcePdfLineStyleV1 {
                color_rgb: None,
                font_size_emu: None,
            }),
            "mixed color and size must not be flattened into a false uniform run"
        );
        assert_eq!(
            admitted_source_line_style_v1(&[original], story_id(), text, 5, 5),
            None
        );
    }
}
