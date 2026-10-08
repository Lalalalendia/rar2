use super::*;

pub(super) fn render_text_is_story_equivalent_for_layout_v1(
    visual: &ViewerGeometryDocument,
    page_id: PageId,
    node_id: NodeId,
    projected_target_frame_node_id: Option<NodeId>,
    fragment_text: &str,
    story_text: &str,
) -> bool {
    if fragment_text == story_text {
        return true;
    }

    #[cfg(feature = "projected-scene-instances")]
    {
        if projected_target_frame_node_id.is_some() {
            return false;
        }
        let target_page_id = page_id.as_canonical().to_string();
        let is_cmo_target_frame = visual.projected_instances.iter().any(|projected| {
            projected.target_frame_node_id == Some(node_id)
                && projected.scene_instance.target_page_id == target_page_id
                && projected.scene_instance.projection_kind == SceneProjectionKindV1::CmoStorySlot
        });
        if !is_cmo_target_frame {
            return false;
        }

        let mut saw_suppressed_marker = false;
        let mut story = story_text.chars();
        let mut fragment = fragment_text.chars();
        loop {
            match (story.next(), fragment.next()) {
                (None, None) => return saw_suppressed_marker,
                (Some(source), Some(rendered)) if source == rendered => {}
                (Some('\u{FFFC}'), Some('\u{200B}')) => saw_suppressed_marker = true,
                _ => return false,
            }
        }
    }

    #[cfg(not(feature = "projected-scene-instances"))]
    {
        let _ = (visual, page_id, node_id, projected_target_frame_node_id);
        false
    }
}

pub(super) fn exact_direct_story_slice_scalar_base_v1(
    visual: &ViewerGeometryDocument,
    page_id: PageId,
    node_id: NodeId,
    projected_target_frame_node_id: Option<NodeId>,
    fragment: &RenderTextFragmentV1,
    story_text: &str,
) -> Option<u32> {
    if projected_target_frame_node_id.is_some() || fragment.scalar_start >= fragment.scalar_end {
        return None;
    }

    #[cfg(feature = "projected-scene-instances")]
    {
        let target_page_id = page_id.as_canonical().to_string();
        if visual.projected_instances.iter().any(|projected| {
            projected.target_frame_node_id == Some(node_id)
                && projected.scene_instance.target_page_id == target_page_id
        }) {
            return None;
        }
    }

    let story_scalars = story_text.chars().collect::<Vec<_>>();
    let start = usize::try_from(fragment.scalar_start).ok()?;
    let end = usize::try_from(fragment.scalar_end).ok()?;
    if start >= end || end > story_scalars.len() {
        return None;
    }
    let fragment_scalar_len =
        usize::try_from(fragment.scalar_end.checked_sub(fragment.scalar_start)?).ok()?;
    if fragment.text.chars().count() != fragment_scalar_len
        || story_scalars[start..end].iter().collect::<String>() != fragment.text
    {
        return None;
    }

    let mut matching_frame = None;
    let mut story_frame_count = 0_usize;
    for frame in visual
        .story_frames
        .iter()
        .filter(|frame| frame.story_id == fragment.story_id)
    {
        story_frame_count += 1;
        if frame.frame_id == node_id {
            if matching_frame.is_some() {
                return None;
            }
            matching_frame = Some(frame);
        }
    }
    let frame = matching_frame?;
    if story_frame_count < 2 {
        return None;
    }
    if (frame.ordinal == 0) != (fragment.scalar_start == 0) {
        return None;
    }

    Some(fragment.scalar_start)
}

pub(super) fn promote_direct_single_frame_prefix_to_whole_story_v1(
    visual: &ViewerGeometryDocument,
    node_id: NodeId,
    fragment: &mut RenderTextFragmentV1,
) -> bool {
    let Some(story) = visual
        .document
        .stories
        .iter()
        .find(|story| story.id == fragment.story_id)
    else {
        return false;
    };
    let Ok(story_scalar_len) = u32::try_from(story.text.chars().count()) else {
        return false;
    };
    if fragment.scalar_start != 0
        || fragment.scalar_end >= story_scalar_len
        || u32::try_from(fragment.text.chars().count()).ok() != Some(fragment.scalar_end)
    {
        return false;
    }

    let story_scalars = story.text.chars().collect::<Vec<_>>();
    let Ok(prefix_end) = usize::try_from(fragment.scalar_end) else {
        return false;
    };
    if story_scalars
        .get(..prefix_end)
        .map(|scalars| scalars.iter().collect::<String>())
        .as_deref()
        != Some(fragment.text.as_str())
    {
        return false;
    }

    let mut frames = visual
        .story_frames
        .iter()
        .filter(|frame| frame.story_id == fragment.story_id);
    let Some(frame) = frames.next() else {
        return false;
    };
    if frames.next().is_some() || frame.frame_id != node_id {
        return false;
    }

    fragment.scalar_end = story_scalar_len;
    fragment.text = story.text.clone();
    fragment.line_count = 0;
    fragment.typography = visual
        .typography_runs
        .iter()
        .filter(|run| run.story_id == fragment.story_id)
        .filter(|run| run.applies_to_story_text(&story.text))
        .filter_map(|run| {
            let scalar_start = run.scalar_start.min(story_scalar_len);
            let scalar_end = run.scalar_end.min(story_scalar_len);
            (scalar_start < scalar_end).then(|| RenderTypographyRunV1 {
                scalar_start,
                scalar_end,
                source_font_name: run.source_font_name.clone(),
                text_size_emu: run.text_size_emu,
                font_inherited: run.font_inherited,
                size_inherited: run.size_inherited,
                color_rgb: run.color_rgb,
                color_inherited: run.color_inherited,
                bold: run.bold.map(|value| value.effective_value),
                italic: run.italic.map(|value| value.effective_value),
            })
        })
        .collect();
    fragment.paragraph_alignments = render_paragraph_alignment_runs_v1(
        visual,
        fragment.story_id,
        &story.text,
        0,
        story_scalar_len,
    );
    fragment.backend_font_resource_id = None;
    fragment.layout = None;
    true
}

pub(super) fn admitted_layout_frame_ordinal(
    visual: &ViewerGeometryDocument,
    story_id: StoryId,
    node_id: NodeId,
    projected_target_frame_node_id: Option<NodeId>,
) -> Result<u32, RenderTextLayoutFallbackReasonV1> {
    let Some(target_frame_node_id) = projected_target_frame_node_id else {
        let mut frames = visual
            .story_frames
            .iter()
            .filter(|frame| frame.story_id == story_id);
        let Some(frame) = frames.next() else {
            return Err(RenderTextLayoutFallbackReasonV1::SingleFrameRequired);
        };
        if frames.next().is_some() || frame.frame_id != node_id {
            return Err(RenderTextLayoutFallbackReasonV1::SingleFrameRequired);
        }
        return Ok(frame.ordinal);
    };

    let mut target_frame_matches = visual
        .story_frames
        .iter()
        .filter(|candidate| candidate.frame_id == target_frame_node_id);
    let Some(target_frame) = target_frame_matches.next() else {
        return Err(RenderTextLayoutFallbackReasonV1::SingleFrameRequired);
    };
    if target_frame_matches.next().is_some() {
        return Err(RenderTextLayoutFallbackReasonV1::SingleFrameRequired);
    }

    let mut target_story_frames = visual
        .story_frames
        .iter()
        .filter(|candidate| candidate.story_id == target_frame.story_id);
    let Some(single_target_frame) = target_story_frames.next() else {
        return Err(RenderTextLayoutFallbackReasonV1::SingleFrameRequired);
    };
    if target_story_frames.next().is_some() || single_target_frame.frame_id != target_frame_node_id
    {
        return Err(RenderTextLayoutFallbackReasonV1::SingleFrameRequired);
    }

    Ok(0)
}

fn incomplete_layout_is_explicit_story_overset(
    diagnostics: &[pub_layout::ResolveDiagnostic],
    story_id: StoryId,
) -> bool {
    diagnostics.len() == 1
        && diagnostics[0].code == "story_overset"
        && diagnostics[0].origin == story_id.into_canonical()
}

pub(super) fn projected_incomplete_layout_is_explicit_overset(
    projected_target_frame_node_id: Option<NodeId>,
    diagnostics: &[pub_layout::ResolveDiagnostic],
    story_id: StoryId,
) -> bool {
    projected_target_frame_node_id.is_some()
        && incomplete_layout_is_explicit_story_overset(diagnostics, story_id)
}

pub(super) fn ordinary_incomplete_layout_is_admitted_partial_story_overset(
    projected_target_frame_node_id: Option<NodeId>,
    diagnostics: &[pub_layout::ResolveDiagnostic],
    story_id: StoryId,
    has_visible_resolved_line: bool,
    last_consumed_scalar_end: Option<u32>,
    story_scalar_len: u32,
) -> bool {
    projected_target_frame_node_id.is_none()
        && has_visible_resolved_line
        && last_consumed_scalar_end.is_some_and(|end| end < story_scalar_len)
        && incomplete_layout_is_explicit_story_overset(diagnostics, story_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pub_layout::{
        BoundedLayoutEnvironment, BoundedResolvedScene, ResolvedPhysicalNode, ResolvedSurface,
    };
    use pub_model::{Affine2D, CanonicalId, Sha256Digest};
    use pub_viewer::{ViewerDocument, ViewerPage, ViewerSource};

    fn canonical(byte: u8) -> CanonicalId {
        CanonicalId::from_bytes([byte; 16])
    }

    fn slice_visual(first_node_id: NodeId, story_id: StoryId) -> ViewerGeometryDocument {
        let page_id = PageId::from_canonical(canonical(1));
        let second_node_id = NodeId::from_canonical(canonical(10));
        ViewerGeometryDocument {
            schema_version: "viewer.v1".into(),
            document: ViewerDocument {
                schema_version: "viewer.document.v1".into(),
                source: ViewerSource {
                    format: "pub".into(),
                    format_version: Some("0x2c".into()),
                    source_hash: Sha256Digest::from_bytes([0x11; 32]),
                    byte_len: 12,
                },
                pages: vec![ViewerPage {
                    index: 1,
                    id: page_id,
                    width_emu: 10_000_000,
                    height_emu: 10_000_000,
                }],
                stories: vec![pub_viewer::ViewerStory {
                    id: story_id,
                    text: "hello world".into(),
                }],
                diagnostics: Vec::new(),
            },
            scene: BoundedResolvedScene {
                environment: BoundedLayoutEnvironment {
                    engine_revision: "test".into(),
                    font_set_fingerprint: "fonts:test".into(),
                    resource_fingerprint: "resources:test".into(),
                },
                surfaces: vec![ResolvedSurface {
                    origin: page_id,
                    size: Size2D::new(LengthEmu::new(10_000_000), LengthEmu::new(10_000_000)),
                    bleed: None,
                    margins: None,
                }],
                nodes: vec![ResolvedPhysicalNode {
                    origin: first_node_id,
                    parent_origin: page_id.into_canonical(),
                    bounds: RectEmu::new(
                        LengthEmu::ZERO,
                        LengthEmu::ZERO,
                        LengthEmu::new(5_000_000),
                        LengthEmu::new(5_000_000),
                    ),
                    transform: Affine2D::identity(),
                }],
                origin_mapping: Vec::new(),
                diagnostics: Vec::new(),
            },
            paints: Vec::new(),
            story_frames: vec![
                pub_viewer::ViewerStoryFrame {
                    story_id,
                    frame_id: first_node_id,
                    ordinal: 0,
                    text_content_bounds: None,
                    vertical_alignment: None,
                },
                pub_viewer::ViewerStoryFrame {
                    story_id,
                    frame_id: second_node_id,
                    ordinal: 1,
                    text_content_bounds: None,
                    vertical_alignment: None,
                },
            ],
            text_fragments: Vec::new(),
            #[cfg(feature = "projected-scene-instances")]
            projected_instances: Vec::new(),
            typography_runs: Vec::new(),
            paragraph_alignments: Vec::new(),
            paragraph_line_spacings: Vec::new(),
            paragraph_flow_runs: Vec::new(),
            script_font_maps: Vec::new(),
            tables: Vec::new(),
            images: Vec::new(),
            decorative_borders: Vec::new(),
            decorative_border_resources: Vec::new(),
        }
    }

    fn fragment(story_id: StoryId, start: u32, end: u32, text: &str) -> RenderTextFragmentV1 {
        RenderTextFragmentV1 {
            story_id,
            scalar_start: start,
            scalar_end: end,
            text: text.to_owned(),
            line_count: 1,
            typography: Vec::new(),
            paragraph_alignments: Vec::new(),
            backend_font_resource_id: None,
            layout: None,
        }
    }

    #[test]
    fn exact_first_frame_prefix_is_admitted_as_zero_based_slice() {
        let node_id = NodeId::from_canonical(canonical(2));
        let story_id = StoryId::from_canonical(canonical(3));
        let visual = slice_visual(node_id, story_id);
        let fragment = fragment(story_id, 0, 5, "hello");

        assert_eq!(
            exact_direct_story_slice_scalar_base_v1(
                &visual,
                visual.document.pages[0].id,
                node_id,
                None,
                &fragment,
                "hello world",
            ),
            Some(0)
        );
    }

    #[test]
    fn modified_first_frame_prefix_stays_fail_closed() {
        let node_id = NodeId::from_canonical(canonical(2));
        let story_id = StoryId::from_canonical(canonical(3));
        let visual = slice_visual(node_id, story_id);
        let fragment = fragment(story_id, 0, 5, "hullo");

        assert_eq!(
            exact_direct_story_slice_scalar_base_v1(
                &visual,
                visual.document.pages[0].id,
                node_id,
                None,
                &fragment,
                "hello world",
            ),
            None
        );
    }

    #[test]
    fn first_and_later_frame_scalar_bases_must_match_frame_ordinal() {
        let first_node_id = NodeId::from_canonical(canonical(2));
        let second_node_id = NodeId::from_canonical(canonical(10));
        let story_id = StoryId::from_canonical(canonical(3));
        let visual = slice_visual(first_node_id, story_id);

        let later = fragment(story_id, 6, 11, "world");
        assert_eq!(
            exact_direct_story_slice_scalar_base_v1(
                &visual,
                visual.document.pages[0].id,
                second_node_id,
                None,
                &later,
                "hello world",
            ),
            Some(6)
        );

        let wrong_first = fragment(story_id, 6, 11, "world");
        assert_eq!(
            exact_direct_story_slice_scalar_base_v1(
                &visual,
                visual.document.pages[0].id,
                first_node_id,
                None,
                &wrong_first,
                "hello world",
            ),
            None
        );

        let wrong_later = fragment(story_id, 0, 5, "hello");
        assert_eq!(
            exact_direct_story_slice_scalar_base_v1(
                &visual,
                visual.document.pages[0].id,
                second_node_id,
                None,
                &wrong_later,
                "hello world",
            ),
            None
        );
    }
}
