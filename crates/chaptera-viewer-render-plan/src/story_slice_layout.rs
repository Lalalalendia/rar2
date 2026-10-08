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
#[path = "story_slice_layout_tests.rs"]
mod tests;
