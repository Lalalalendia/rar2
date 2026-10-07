//! Plain Story text-session orchestration.
//!
//! This owner handles Story session capability and text replacement only.
//! Formatting, paragraph alignment, frame topology, CreateTextBox, export,
//! and generic history replay remain outside this module.

use super::*;

impl EditorSession {
    fn validate_story_text_session_capability(
        &self,
        story_id: StoryId,
        allow_character_format_history: bool,
        allow_paragraph_format_history: bool,
    ) -> Result<(), EditorError> {
        self.validate_source_identity()?;
        if !allow_paragraph_format_history
            && self.story_has_paragraph_alignment_history_v1(story_id)?
        {
            return Err(EditorError::ParagraphAlignmentLifecycleUnsupported { story_id });
        }

        let story = self
            .graph
            .stories
            .get(&story_id)
            .ok_or(EditorError::MissingStory { story_id })?;

        if !allow_character_format_history
            && self
                .undo
                .iter()
                .any(|operation| text_format_operation_story_id_v1(operation) == Some(story_id))
        {
            return Err(EditorError::TextFormatTextMutationConflict { story_id });
        }

        if !story.paragraphs.is_empty()
            || !story.runs.is_empty()
            || !story.fields.is_empty()
            || !story.hyperlinks.is_empty()
        {
            return Err(EditorError::RichStoryUnsupported { story_id });
        }

        if self.graph.nodes.values().any(|node| {
            node.payload
                .table_story
                .as_ref()
                .is_some_and(|owner| owner.story_id == Some(story_id))
                || node
                    .payload
                    .table
                    .as_ref()
                    .is_some_and(|table| table.story_id == Some(story_id))
        }) {
            return Err(EditorError::TableStoryUnsupported { story_id });
        }

        let frames = self
            .graph
            .nodes
            .iter()
            .filter_map(|(node_id, node)| {
                let frame = node.payload.story_frame.as_ref()?;
                (frame.story_id == Some(story_id)).then_some(StoryFrame {
                    story_id,
                    frame_id: *node_id,
                    ordinal: frame.ordinal,
                    previous: frame.previous_frame,
                    next: frame.next_frame,
                })
            })
            .collect::<Vec<StoryFrame<StoryId, NodeId>>>();

        if frames.is_empty() {
            return Err(EditorError::FrameCountUnsupported { story_id, found: 0 });
        }

        let topology_errors = validate_story_frames(&frames);
        if !topology_errors.is_empty() {
            return Err(EditorError::FrameTopologyUnsupported {
                story_id,
                errors: topology_errors.len(),
            });
        }

        Ok(())
    }

    pub fn can_enter_story_text_session(&self, story_id: StoryId) -> Result<(), EditorError> {
        self.validate_story_text_session_capability(story_id, true, true)
    }

    pub fn can_replace_story_text(&self, story_id: StoryId) -> Result<(), EditorError> {
        self.validate_story_text_session_capability(story_id, false, false)
    }

    pub fn replace_story_range(
        &mut self,
        story_id: StoryId,
        start_scalar: u32,
        end_scalar: u32,
        expected_before: impl Into<String>,
        replacement_text: impl Into<String>,
    ) -> Result<EditOperation, EditorError> {
        self.can_replace_story_text(story_id)?;

        let expected_before = expected_before.into();
        let replacement_text = replacement_text.into();
        let before = self
            .graph
            .stories
            .get(&story_id)
            .expect("capability check verified story presence")
            .text
            .clone();

        let after = replace_scalar_range_text(
            &before,
            start_scalar,
            end_scalar,
            &expected_before,
            &replacement_text,
        )
        .ok_or(EditorError::StaleOperation { story_id })?;

        if before == after {
            return Err(EditorError::NoChange { story_id });
        }

        let operation = EditOperation::ReplaceStoryRange {
            story_id,
            start_scalar,
            end_scalar,
            expected_before,
            replacement_text,
            before_story_state_id: story_state_id_v1(story_id, &before),
            after_story_state_id: story_state_id_v1(story_id, &after),
        };

        apply_forward(&mut self.graph, &operation)?;
        self.undo.push(operation.clone());
        self.redo.clear();
        self.validate_source_identity()?;
        Ok(operation)
    }

    pub fn replace_story_text(
        &mut self,
        story_id: StoryId,
        replacement: impl Into<String>,
    ) -> Result<EditOperation, EditorError> {
        self.can_replace_story_text(story_id)?;
        let before = self
            .graph
            .stories
            .get(&story_id)
            .expect("capability check verified story presence")
            .text
            .clone();
        let scalar_len = u32::try_from(before.chars().count())
            .map_err(|_| EditorError::StaleOperation { story_id })?;
        self.replace_story_range(story_id, 0, scalar_len, before, replacement)
    }
}
