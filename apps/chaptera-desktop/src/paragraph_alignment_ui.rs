use crate::text_session::{self, DesktopTextMode};
use pub_editor::{
    AuthoredParagraphAlignmentValueV1, EditOperation, EditorSession,
    EffectiveParagraphAlignmentValueV1, ImportedParagraphV1, ParagraphAlignmentAuthorityV1,
    ParagraphId, StoryId,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DesktopParagraphAlignmentEffectiveStateV1 {
    Uniform(AuthoredParagraphAlignmentValueV1),
    Mixed,
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DesktopParagraphAlignmentProvenanceStateV1 {
    Base,
    ChapteraOverride,
    Mixed,
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DesktopParagraphAlignmentSelectionStateV1 {
    pub paragraph_ids: Vec<ParagraphId>,
    pub effective: DesktopParagraphAlignmentEffectiveStateV1,
    pub provenance: DesktopParagraphAlignmentProvenanceStateV1,
    pub has_chaptera_override: bool,
}

impl DesktopParagraphAlignmentSelectionStateV1 {
    pub const fn is_editable(&self) -> bool {
        !matches!(
            self.effective,
            DesktopParagraphAlignmentEffectiveStateV1::Unsupported
        )
    }

    pub fn is_uniform(&self, value: AuthoredParagraphAlignmentValueV1) -> bool {
        matches!(
            self.effective,
            DesktopParagraphAlignmentEffectiveStateV1::Uniform(current) if current == value
        )
    }
}

fn paragraph_targets_for_selection_v1(
    paragraphs: &[ImportedParagraphV1],
    story_id: StoryId,
    anchor_scalar: u32,
    focus_scalar: u32,
) -> Vec<ParagraphId> {
    let start = u64::from(anchor_scalar.min(focus_scalar));
    let end = u64::from(anchor_scalar.max(focus_scalar));
    let mut story_paragraphs = paragraphs
        .iter()
        .filter(|paragraph| paragraph.story_id == story_id)
        .collect::<Vec<_>>();
    story_paragraphs.sort_by_key(|paragraph| {
        (
            paragraph.range.start,
            paragraph.range.end,
            paragraph.paragraph_id,
        )
    });

    if start == end {
        if let Some(paragraph) = story_paragraphs
            .iter()
            .find(|paragraph| paragraph.range.start <= start && start < paragraph.range.end)
        {
            return vec![paragraph.paragraph_id];
        }
        if let Some(paragraph) = story_paragraphs
            .last()
            .filter(|paragraph| paragraph.range.end == start)
        {
            return vec![paragraph.paragraph_id];
        }
        return Vec::new();
    }

    story_paragraphs
        .into_iter()
        .filter(|paragraph| paragraph.range.start < end && start < paragraph.range.end)
        .map(|paragraph| paragraph.paragraph_id)
        .collect()
}

pub(super) fn paragraph_alignment_selection_state_v1(
    editor: &EditorSession,
    mode: &DesktopTextMode,
) -> Result<DesktopParagraphAlignmentSelectionStateV1, String> {
    let paragraphs = editor
        .imported_paragraphs_v1()
        .map_err(|error| error.to_string())?;
    let paragraph_ids = paragraph_targets_for_selection_v1(
        &paragraphs,
        mode.story_id,
        mode.session.selection.anchor_scalar,
        mode.session.selection.focus_scalar,
    );
    if paragraph_ids.is_empty() {
        return Err("caret/selection does not intersect a canonical ParagraphId".to_owned());
    }

    let mut effective_value = None;
    let mut effective_mixed = false;
    let mut unsupported_effective = false;
    let mut provenance = None;
    let mut provenance_mixed = false;
    let mut unsupported_provenance = false;
    let mut has_chaptera_override = false;

    for paragraph_id in &paragraph_ids {
        let state = editor
            .effective_paragraph_alignment_v1(*paragraph_id)
            .map_err(|error| error.to_string())?;

        let current = match state.effective {
            Some(EffectiveParagraphAlignmentValueV1::Left) => {
                Some(AuthoredParagraphAlignmentValueV1::Left)
            }
            Some(EffectiveParagraphAlignmentValueV1::Center) => {
                Some(AuthoredParagraphAlignmentValueV1::Center)
            }
            Some(EffectiveParagraphAlignmentValueV1::Right) => {
                Some(AuthoredParagraphAlignmentValueV1::Right)
            }
            Some(EffectiveParagraphAlignmentValueV1::Justify)
            | Some(EffectiveParagraphAlignmentValueV1::InterWord)
            | Some(EffectiveParagraphAlignmentValueV1::Distribute)
            | None => {
                unsupported_effective = true;
                None
            }
        };
        if let Some(current) = current {
            if effective_value.is_some_and(|previous| previous != current) {
                effective_mixed = true;
            } else if effective_value.is_none() {
                effective_value = Some(current);
            }
        }

        match state.authority {
            Some(ParagraphAlignmentAuthorityV1::ImportedBase) => {
                if provenance.is_some_and(|previous| {
                    previous != DesktopParagraphAlignmentProvenanceStateV1::Base
                }) {
                    provenance_mixed = true;
                } else if provenance.is_none() {
                    provenance = Some(DesktopParagraphAlignmentProvenanceStateV1::Base);
                }
            }
            Some(ParagraphAlignmentAuthorityV1::ChapteraOverride) => {
                has_chaptera_override = true;
                if provenance.is_some_and(|previous| {
                    previous != DesktopParagraphAlignmentProvenanceStateV1::ChapteraOverride
                }) {
                    provenance_mixed = true;
                } else if provenance.is_none() {
                    provenance = Some(DesktopParagraphAlignmentProvenanceStateV1::ChapteraOverride);
                }
            }
            None => {
                unsupported_provenance = true;
            }
        }
    }

    let effective = if unsupported_effective {
        DesktopParagraphAlignmentEffectiveStateV1::Unsupported
    } else if effective_mixed {
        DesktopParagraphAlignmentEffectiveStateV1::Mixed
    } else {
        DesktopParagraphAlignmentEffectiveStateV1::Uniform(
            effective_value
                .ok_or_else(|| "missing paragraph alignment effective state".to_owned())?,
        )
    };
    let provenance = if unsupported_provenance {
        DesktopParagraphAlignmentProvenanceStateV1::Unsupported
    } else if provenance_mixed {
        DesktopParagraphAlignmentProvenanceStateV1::Mixed
    } else {
        provenance.unwrap_or(DesktopParagraphAlignmentProvenanceStateV1::Unsupported)
    };

    Ok(DesktopParagraphAlignmentSelectionStateV1 {
        paragraph_ids,
        effective,
        provenance,
        has_chaptera_override,
    })
}

pub(super) fn apply_paragraph_alignment_v1(
    editor: &mut EditorSession,
    mode: &mut DesktopTextMode,
    value: AuthoredParagraphAlignmentValueV1,
) -> Result<Option<EditOperation>, String> {
    let selection = paragraph_alignment_selection_state_v1(editor, mode)?;
    if !selection.is_editable() {
        return Err(
            "selected paragraph alignment is unsupported/read-only; Chaptera will not coerce it"
                .to_owned(),
        );
    }
    if selection.is_uniform(value) {
        return Ok(None);
    }

    let operation = editor
        .set_paragraph_alignment_override_v1(selection.paragraph_ids, value)
        .map_err(|error| error.to_string())?;
    text_session::rebind_after_non_text_document_change(editor, mode)?;
    Ok(Some(operation))
}

pub(super) fn clear_paragraph_alignment_override_v1(
    editor: &mut EditorSession,
    mode: &mut DesktopTextMode,
) -> Result<EditOperation, String> {
    let selection = paragraph_alignment_selection_state_v1(editor, mode)?;
    if !selection.has_chaptera_override {
        return Err("selected paragraphs have no Chaptera alignment override to clear".to_owned());
    }

    let operation = editor
        .clear_paragraph_alignment_override_v1(selection.paragraph_ids)
        .map_err(|error| error.to_string())?;
    text_session::rebind_after_non_text_document_change(editor, mode)?;
    Ok(operation)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pub_editor::{Sha256Digest, open_mature_0x2c_editor};
    use pub_model::TextRange;
    use std::{env, fs};

    #[test]
    fn paragraph_target_mapping_uses_canonical_ranges_not_visual_lines() {
        let story_id: StoryId =
            serde_json::from_str("\"33000000-0000-4000-8000-000000000001\"").expect("story id");
        let first: ParagraphId =
            serde_json::from_str("\"33000000-0000-4000-8000-000000000002\"").expect("paragraph 1");
        let second: ParagraphId =
            serde_json::from_str("\"33000000-0000-4000-8000-000000000003\"").expect("paragraph 2");
        let paragraphs = vec![
            ImportedParagraphV1 {
                paragraph_id: first,
                story_id,
                range: TextRange::new(0, 4).expect("range 1"),
            },
            ImportedParagraphV1 {
                paragraph_id: second,
                story_id,
                range: TextRange::new(4, 8).expect("range 2"),
            },
        ];

        assert_eq!(
            paragraph_targets_for_selection_v1(&paragraphs, story_id, 0, 3),
            vec![first]
        );
        assert_eq!(
            paragraph_targets_for_selection_v1(&paragraphs, story_id, 3, 5),
            vec![first, second]
        );
        assert_eq!(
            paragraph_targets_for_selection_v1(&paragraphs, story_id, 4, 4),
            vec![second],
            "caret exactly at a canonical paragraph boundary belongs to the following paragraph"
        );
        assert_eq!(
            paragraph_targets_for_selection_v1(&paragraphs, story_id, 8, 8),
            vec![second],
            "terminal caret remains owned by the final paragraph"
        );
    }

    #[test]
    fn paragraph_ui_state_keeps_unsupported_read_only() {
        let unsupported = DesktopParagraphAlignmentSelectionStateV1 {
            paragraph_ids: Vec::new(),
            effective: DesktopParagraphAlignmentEffectiveStateV1::Unsupported,
            provenance: DesktopParagraphAlignmentProvenanceStateV1::Unsupported,
            has_chaptera_override: false,
        };
        assert!(!unsupported.is_editable());
        assert!(!unsupported.is_uniform(AuthoredParagraphAlignmentValueV1::Left));

        let mixed = DesktopParagraphAlignmentSelectionStateV1 {
            paragraph_ids: Vec::new(),
            effective: DesktopParagraphAlignmentEffectiveStateV1::Mixed,
            provenance: DesktopParagraphAlignmentProvenanceStateV1::Mixed,
            has_chaptera_override: true,
        };
        assert!(mixed.is_editable());
        assert!(!mixed.is_uniform(AuthoredParagraphAlignmentValueV1::Center));
    }

    #[test]
    #[ignore = "requires the pinned public Carlton March PUB path"]
    fn real_carlton_paragraph_alignment_ui_state_set_clear_and_left() {
        let Some(path) = env::var_os("CHAPTERA_CARLTON_PUB").map(std::path::PathBuf::from) else {
            eprintln!(
                "CHAPTERA_CARLTON_PUB not set; dedicated paragraph UI gate owns real evidence"
            );
            return;
        };
        let original = fs::read(&path).expect("read pinned Carlton March PUB");
        let source_hash = "bf9cda0f632b5820ab9dbdbe1b838b2a988b2f3fdd69253c22b4fc3aef9f11c3"
            .parse::<Sha256Digest>()
            .expect("pinned Carlton source hash");
        let mut editor =
            open_mature_0x2c_editor(&original, source_hash).expect("open Carlton EditorSession");
        let base = editor
            .imported_paragraph_base_alignments_v1()
            .expect("bind Carlton imported paragraph base alignment");
        let paragraph = base
            .iter()
            .find(|item| item.alignment == pub_editor::ImportedParagraphAlignmentValueV1::Right)
            .expect("Carlton must expose one grounded Right paragraph base")
            .clone();
        let visual = pub_viewer::open_mature_0x2c_geometry(
            &original,
            pub_viewer::viewer_geometry_environment_v0_1(),
        )
        .expect("open Carlton geometry");
        let fragment = visual
            .text_fragments
            .iter()
            .find(|fragment| fragment.story_id == paragraph.story_id)
            .expect("grounded Right Story must be projected into one real TextFrame");
        let mut mode =
            text_session::enter_explicit_text_mode(&editor, paragraph.story_id, fragment.frame_id)
                .expect("enter grounded paragraph Story");

        let stop = mode
            .layout
            .caret_map
            .caret_stops
            .iter()
            .find(|stop| {
                u64::from(stop.scalar_boundary) >= paragraph.range.start
                    && u64::from(stop.scalar_boundary) < paragraph.range.end
            })
            .cloned()
            .expect("grounded paragraph must expose one projected caret stop");
        text_session::reposition_pointer(
            &mut mode,
            &stop.page_id,
            stop.page_x_emu,
            stop.page_y_top_emu + (stop.page_y_bottom_emu - stop.page_y_top_emu) / 2,
        )
        .expect("position caret inside grounded paragraph");

        let source_text = editor.graph().stories[&paragraph.story_id].text.clone();
        let source = paragraph_alignment_selection_state_v1(&editor, &mode)
            .expect("read source paragraph toolbar state");
        assert!(source.paragraph_ids.contains(&paragraph.paragraph_id));
        assert_eq!(
            source.effective,
            DesktopParagraphAlignmentEffectiveStateV1::Uniform(
                AuthoredParagraphAlignmentValueV1::Right
            )
        );
        assert_eq!(
            source.provenance,
            DesktopParagraphAlignmentProvenanceStateV1::Base
        );
        assert!(!source.has_chaptera_override);

        let center = apply_paragraph_alignment_v1(
            &mut editor,
            &mut mode,
            AuthoredParagraphAlignmentValueV1::Center,
        )
        .expect("commit Center through Desktop paragraph command")
        .expect("Center must append one operation");
        assert!(matches!(
            center,
            EditOperation::SetParagraphAlignmentOverride { value, .. }
                if value == AuthoredParagraphAlignmentValueV1::Center
        ));
        let centered = paragraph_alignment_selection_state_v1(&editor, &mode)
            .expect("read centered toolbar state");
        assert_eq!(
            centered.effective,
            DesktopParagraphAlignmentEffectiveStateV1::Uniform(
                AuthoredParagraphAlignmentValueV1::Center
            )
        );
        assert_eq!(
            centered.provenance,
            DesktopParagraphAlignmentProvenanceStateV1::ChapteraOverride
        );
        assert!(centered.has_chaptera_override);

        let clear = clear_paragraph_alignment_override_v1(&mut editor, &mut mode)
            .expect("clear paragraph override through Desktop command");
        assert!(matches!(
            clear,
            EditOperation::ClearParagraphAlignmentOverride { .. }
        ));
        let cleared = paragraph_alignment_selection_state_v1(&editor, &mode)
            .expect("read cleared toolbar state");
        assert_eq!(
            cleared.effective,
            DesktopParagraphAlignmentEffectiveStateV1::Uniform(
                AuthoredParagraphAlignmentValueV1::Right
            )
        );
        assert_eq!(
            cleared.provenance,
            DesktopParagraphAlignmentProvenanceStateV1::Base
        );
        assert!(!cleared.has_chaptera_override);

        let left = apply_paragraph_alignment_v1(
            &mut editor,
            &mut mode,
            AuthoredParagraphAlignmentValueV1::Left,
        )
        .expect("commit Left through Desktop paragraph command")
        .expect("Left must append one operation");
        assert!(matches!(
            left,
            EditOperation::SetParagraphAlignmentOverride { value, .. }
                if value == AuthoredParagraphAlignmentValueV1::Left
        ));
        let left_state = paragraph_alignment_selection_state_v1(&editor, &mode)
            .expect("read Left toolbar state");
        assert_eq!(
            left_state.effective,
            DesktopParagraphAlignmentEffectiveStateV1::Uniform(
                AuthoredParagraphAlignmentValueV1::Left
            )
        );
        assert_eq!(
            left_state.provenance,
            DesktopParagraphAlignmentProvenanceStateV1::ChapteraOverride
        );

        assert_eq!(
            editor.graph().stories[&paragraph.story_id].text,
            source_text
        );
        assert_eq!(
            fs::read(&path).expect("re-read Carlton source"),
            original,
            "Desktop paragraph controls must not mutate source PUB bytes"
        );
    }
}
