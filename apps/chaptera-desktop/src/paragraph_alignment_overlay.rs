use chaptera_viewer_render_plan::{PageRenderPlanV1, RenderTextLayoutDispositionV1};
use pub_editor::{EditorSession, EffectiveParagraphAlignmentValueV1, ImportedParagraphV1, StoryId};
use pub_line_placement::{
    LayoutPlacementContextV1, ParagraphAlignmentV1, ParagraphLinePlacementInputV1,
    ResolvedLineInputV1, resolve_paragraph_line_placement_v1,
};
use std::collections::{BTreeMap, BTreeSet};

fn effective_alignment_v1(
    editor: &EditorSession,
    paragraph: &ImportedParagraphV1,
) -> Result<ParagraphAlignmentV1, String> {
    let effective = editor
        .effective_paragraph_alignment_v1(paragraph.paragraph_id)
        .map_err(|error| error.to_string())?;
    match effective.effective {
        Some(EffectiveParagraphAlignmentValueV1::Left) => Ok(ParagraphAlignmentV1::Left),
        Some(EffectiveParagraphAlignmentValueV1::Center) => Ok(ParagraphAlignmentV1::Center),
        Some(EffectiveParagraphAlignmentValueV1::Right) => Ok(ParagraphAlignmentV1::Right),
        Some(EffectiveParagraphAlignmentValueV1::InterWord) => Err(format!(
            "Story {} Paragraph {} has effective InterWord alignment; live authored layout stays read-only",
            paragraph.story_id.as_canonical(),
            paragraph.paragraph_id.as_canonical()
        )),
        Some(EffectiveParagraphAlignmentValueV1::Distribute) => Err(format!(
            "Story {} Paragraph {} has effective Distribute alignment; live authored layout stays read-only",
            paragraph.story_id.as_canonical(),
            paragraph.paragraph_id.as_canonical()
        )),
        None => Err(format!(
            "Story {} Paragraph {} has unknown effective alignment; live authored layout cannot synthesize Left",
            paragraph.story_id.as_canonical(),
            paragraph.paragraph_id.as_canonical()
        )),
    }
}

fn paragraph_for_line_v1(
    paragraphs: &[ImportedParagraphV1],
    story_id: StoryId,
    scalar_start: u32,
    consumed_scalar_end: u32,
) -> Option<&ImportedParagraphV1> {
    let start = u64::from(scalar_start);
    let end = u64::from(consumed_scalar_end);
    paragraphs.iter().find(|paragraph| {
        paragraph.story_id == story_id
            && paragraph.range.start <= start
            && start < paragraph.range.end
            && end <= paragraph.range.end
    })
}

pub(super) fn apply_editor_paragraph_alignment_layout_v1(
    plan: &mut PageRenderPlanV1,
    editor: &EditorSession,
) -> Result<(), String> {
    let paragraphs = match editor.imported_paragraphs_v1() {
        Ok(paragraphs) => paragraphs,
        Err(_) => return Ok(()),
    };
    let mut by_story = BTreeMap::<StoryId, Vec<ImportedParagraphV1>>::new();
    let mut overridden_stories = BTreeSet::new();
    for paragraph in paragraphs {
        if editor
            .authored_paragraph_alignment_override_v1(paragraph.paragraph_id)
            .map_err(|error| error.to_string())?
            .is_some()
        {
            overridden_stories.insert(paragraph.story_id);
        }
        by_story
            .entry(paragraph.story_id)
            .or_default()
            .push(paragraph);
    }
    if overridden_stories.is_empty() {
        return Ok(());
    }
    for paragraphs in by_story.values_mut() {
        paragraphs.sort_by_key(|paragraph| {
            (
                paragraph.range.start,
                paragraph.range.end,
                paragraph.paragraph_id,
            )
        });
    }

    let revision = editor.project().state_id_v1();
    for node in &mut plan.nodes {
        let content_width_emu = node.text_bounds.unwrap_or(node.bounds).width.get();
        let Some(text) = node.text.as_mut() else {
            continue;
        };
        if !overridden_stories.contains(&text.story_id) {
            continue;
        }
        let story_paragraphs = by_story.get(&text.story_id).ok_or_else(|| {
            format!(
                "active paragraph override Story {} has no canonical ParagraphId projection",
                text.story_id.as_canonical()
            )
        })?;
        let layout = text.layout.as_mut().ok_or_else(|| {
            format!(
                "active paragraph override Story {} has no resolved Desktop text layout",
                text.story_id.as_canonical()
            )
        })?;
        if !matches!(
            &layout.disposition,
            RenderTextLayoutDispositionV1::SharedResolved { .. }
        ) {
            return Err(format!(
                "active paragraph override Story {} is on backend fallback; live paragraph layout fails closed",
                text.story_id.as_canonical()
            ));
        }

        for line in &mut layout.lines {
            let paragraph = paragraph_for_line_v1(
                story_paragraphs,
                text.story_id,
                line.scalar_start,
                line.consumed_scalar_end,
            )
            .ok_or_else(|| {
                format!(
                    "resolved line {}..{} in Story {} does not map to one canonical ParagraphId",
                    line.scalar_start,
                    line.consumed_scalar_end,
                    text.story_id.as_canonical()
                )
            })?;
            let alignment = effective_alignment_v1(editor, paragraph)?;
            let placement = resolve_paragraph_line_placement_v1(&ParagraphLinePlacementInputV1 {
                context: LayoutPlacementContextV1 {
                    authoring_revision: revision.clone(),
                    layout_environment_fingerprint: "chaptera.desktop.current-paragraph-layout.v1"
                        .to_owned(),
                },
                alignment,
                story_overset: false,
                lines: vec![ResolvedLineInputV1 {
                    line_index: 0,
                    story_id: text.story_id.as_canonical().to_string(),
                    frame_node_id: node.node_id.as_canonical().to_string(),
                    frame_line_index: line.line_index,
                    scalar_start: line.scalar_start,
                    scalar_end: line.scalar_end,
                    content_leading_x_emu: 0,
                    content_width_emu,
                    measured_width_emu: line.measured_width_emu,
                }],
            })
            .map_err(|error| error.to_string())?;
            line.x_offset_emu = placement
                .lines
                .first()
                .expect("one-line placement returns one line")
                .line_origin_x_emu;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_line_mapping_keeps_cr_with_preceding_paragraph() {
        use pub_editor::ParagraphId;
        use pub_model::{CanonicalId, TextRange};

        fn id(byte: u8) -> CanonicalId {
            CanonicalId::from_bytes([byte; 16])
        }
        let story_id = StoryId::from_canonical(id(1));
        let paragraphs = vec![
            ImportedParagraphV1 {
                paragraph_id: ParagraphId::from_canonical(id(2)),
                story_id,
                range: TextRange::new(0, 4).unwrap(),
            },
            ImportedParagraphV1 {
                paragraph_id: ParagraphId::from_canonical(id(4)),
                story_id,
                range: TextRange::new(4, 7).unwrap(),
            },
        ];
        assert_eq!(
            paragraph_for_line_v1(&paragraphs, story_id, 0, 4)
                .unwrap()
                .paragraph_id,
            paragraphs[0].paragraph_id
        );
        assert_eq!(
            paragraph_for_line_v1(&paragraphs, story_id, 4, 7)
                .unwrap()
                .paragraph_id,
            paragraphs[1].paragraph_id
        );
    }
}
