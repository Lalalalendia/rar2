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

        let after = replace_scalar_range_text_v1(
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

pub(super) fn text_format_operation_story_id_v1(operation: &EditOperation) -> Option<StoryId> {
    match operation {
        EditOperation::SetTextFormatProperty { story_id, .. }
        | EditOperation::ClearTextFormatPropertyOverride { story_id, .. }
        | EditOperation::SetTextFormatPropertyScopedV1 { story_id, .. }
        | EditOperation::ClearTextFormatPropertyOverrideScopedV1 { story_id, .. } => {
            Some(*story_id)
        }
        _ => None,
    }
}

pub(super) fn text_format_operation_property_v1(operation: &EditOperation) -> Option<FormatPropertyV1> {
    match operation {
        EditOperation::SetTextFormatProperty { property, .. }
        | EditOperation::ClearTextFormatPropertyOverride { property, .. }
        | EditOperation::SetTextFormatPropertyScopedV1 { property, .. }
        | EditOperation::ClearTextFormatPropertyOverrideScopedV1 { property, .. } => {
            Some(*property)
        }
        _ => None,
    }
}

pub(super) fn is_scoped_text_format_operation_v1(operation: &EditOperation) -> bool {
    matches!(
        operation,
        EditOperation::SetTextFormatPropertyScopedV1 { .. }
            | EditOperation::ClearTextFormatPropertyOverrideScopedV1 { .. }
    )
}

pub(super) fn apply_text_format_history_operation_semantic_v1(
    state: &TextFormatOverlayStateV1,
    operation: &EditOperation,
) -> Result<TextFormatOverlayStateV1, EditorError> {
    let story_id = text_format_operation_story_id_v1(operation)
        .expect("semantic text-format replay receives only text-format operations");
    if state.story_id != story_id.as_canonical().to_string() {
        return Err(EditorError::TextFormatStateInvalid {
            story_id,
            message: "format operation Story does not match overlay Story".to_owned(),
        });
    }
    let current_hash =
        state_hash_v1(state).map_err(|error| EditorError::TextFormatStateInvalid {
            story_id,
            message: error.to_string(),
        })?;
    let receipt = match operation {
        EditOperation::SetTextFormatProperty {
            start_scalar,
            end_scalar,
            property,
            value,
            ..
        }
        | EditOperation::SetTextFormatPropertyScopedV1 {
            start_scalar,
            end_scalar,
            property,
            value,
            ..
        } => overlay_set_text_format_property_v1(
            state,
            *start_scalar,
            *end_scalar,
            *property,
            value.clone(),
            &current_hash,
        ),
        EditOperation::ClearTextFormatPropertyOverride {
            start_scalar,
            end_scalar,
            property,
            ..
        }
        | EditOperation::ClearTextFormatPropertyOverrideScopedV1 {
            start_scalar,
            end_scalar,
            property,
            ..
        } => overlay_clear_text_format_property_override_v1(
            state,
            *start_scalar,
            *end_scalar,
            *property,
            &current_hash,
        ),
        _ => unreachable!("semantic text-format replay receives only text-format operations"),
    }
    .map_err(|error| EditorError::TextFormatStateInvalid {
        story_id,
        message: error.to_string(),
    })?;
    Ok(receipt.after_state)
}

pub(super) fn apply_text_format_history_operation_v1(
    state: &TextFormatOverlayStateV1,
    operation: &EditOperation,
) -> Result<TextFormatOverlayStateV1, EditorError> {
    if is_scoped_text_format_operation_v1(operation) {
        let story_id = text_format_operation_story_id_v1(operation)
            .expect("scoped format operation has Story");
        return Err(EditorError::TextFormatStateInvalid {
            story_id,
            message: "property-scoped text-format operation cannot be validated in complete-overlay hash domain".to_owned(),
        });
    }

    let (story_id, before_state_hash, after_state_hash, receipt) = match operation {
        EditOperation::SetTextFormatProperty {
            story_id,
            start_scalar,
            end_scalar,
            property,
            value,
            before_state_hash,
            after_state_hash,
        } => (
            *story_id,
            before_state_hash,
            after_state_hash,
            overlay_set_text_format_property_v1(
                state,
                *start_scalar,
                *end_scalar,
                *property,
                value.clone(),
                before_state_hash,
            ),
        ),
        EditOperation::ClearTextFormatPropertyOverride {
            story_id,
            start_scalar,
            end_scalar,
            property,
            before_state_hash,
            after_state_hash,
        } => (
            *story_id,
            before_state_hash,
            after_state_hash,
            overlay_clear_text_format_property_override_v1(
                state,
                *start_scalar,
                *end_scalar,
                *property,
                before_state_hash,
            ),
        ),
        _ => unreachable!(
            "checked complete-overlay replay admits only legacy text-format operations"
        ),
    };

    if state.story_id != story_id.as_canonical().to_string() {
        return Err(EditorError::TextFormatStateInvalid {
            story_id,
            message: "format operation Story does not match overlay Story".to_owned(),
        });
    }

    let receipt = receipt.map_err(|error| EditorError::TextFormatStateInvalid {
        story_id,
        message: error.to_string(),
    })?;
    if receipt.command.before_state_hash != *before_state_hash
        || receipt.command.after_state_hash != *after_state_hash
    {
        return Err(EditorError::TextFormatStateInvalid {
            story_id,
            message: "persisted format operation hashes do not match deterministic replay"
                .to_owned(),
        });
    }
    Ok(receipt.after_state)
}
