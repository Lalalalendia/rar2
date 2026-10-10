//! Plain Story text-session orchestration.
//!
//! This owner handles Story session capability and text replacement only.
//! Formatting, paragraph alignment, frame topology, CreateTextBox, export,
//! and generic history replay remain outside this module.

use super::*;
use chaptera_text_format_overlay::{
    FontAuthoringScopeV1, FontReplacementCandidateV1, FontResourceIdentityV1, ServerFontResourceV1,
    replay_admitted_font_resource_v1 as overlay_readmit_font_v1,
    replay_recorded_font_resource_v1 as replay_font_history_v1,
    set_admitted_font_resource_v1 as overlay_admit_font_v1,
};

/// Trusted server-side reopen grant, never deserialized from EditorProject.
/// A caller must independently obtain original full bytes and authoring
/// permission for this exact source/project; file and browser metadata cannot
/// populate this struct as a policy authority.
#[derive(Debug)]
pub struct EditorProjectFontReopenGrantV1<'a> {
    pub source_hash: Sha256Digest,
    pub project_document_id: &'a str,
    pub resource: ServerFontResourceV1<'a>,
}

fn denied_project_font_reopen(index: usize, story_id: StoryId, reason: &str) -> EditorProjectError {
    EditorProjectError::Operation {
        index,
        error: EditorError::TextFormatStateInvalid {
            story_id,
            message: reason.to_owned(),
        },
    }
}

pub(super) fn replay_project_operation_with_font_grants_v1(
    session: &mut EditorSession,
    expected: &EditOperation,
    index: usize,
    project: &EditorProject,
    grants: &[EditorProjectFontReopenGrantV1<'_>],
) -> Result<EditOperation, EditorProjectError> {
    let EditOperation::SetTextFormatProperty {
        story_id,
        start_scalar,
        end_scalar,
        property: FormatPropertyV1::FontResource,
        value: FormatValueV1::FontResource(identity),
        before_state_hash,
        ..
    } = expected
    else {
        return replay_canonical_operation(session, expected, index);
    };

    // Resource IDs and face indices select only exact entries. A mismatched
    // physical identity is rejected, never substituted by family display name.
    let mut matching = grants.iter().filter(|grant| {
        grant.resource.identity.resource_id == identity.resource_id
            && grant.resource.identity.face_index == identity.face_index
    });
    let grant = matching.next().ok_or_else(|| {
        denied_project_font_reopen(index, *story_id, "trusted physical font grant unavailable")
    })?;
    if matching.next().is_some() {
        return Err(denied_project_font_reopen(
            index,
            *story_id,
            "ambiguous duplicate physical font grants",
        ));
    }
    if grant.source_hash != project.source_hash
        || project
            .identity
            .as_ref()
            .is_none_or(|identity| identity.document_id != grant.project_document_id)
    {
        return Err(denied_project_font_reopen(
            index,
            *story_id,
            "physical font grant is bound to another source or EditorProject",
        ));
    }
    session
        .replay_admitted_project_font_v1(
            *story_id,
            *start_scalar,
            *end_scalar,
            identity,
            &grant.resource,
            before_state_hash,
        )
        .map_err(|error| EditorProjectError::Operation { index, error })
}

impl EditorSession {
    /// Standard reopen remains fail-closed for font overrides when the caller
    /// has no independently admitted complete resource.
    pub fn apply_project_with_assets(
        &mut self,
        project: &EditorProject,
        asset_bytes: &BTreeMap<Sha256Digest, Vec<u8>>,
    ) -> Result<(), EditorProjectError> {
        self.apply_project_with_admitted_font_resources_v1(project, asset_bytes, &[])
    }

    /// Replay a recorded font override only after exact physical bytes, face
    /// and document-bound authoring permission were independently re-admitted.
    pub(super) fn replay_admitted_project_font_v1(
        &mut self,
        story_id: StoryId,
        start_scalar: u32,
        end_scalar: u32,
        identity: &FontResourceIdentityV1,
        resource: &ServerFontResourceV1<'_>,
        expected_state_hash: &str,
    ) -> Result<EditOperation, EditorError> {
        let before = self.current_text_format_overlay_v1(story_id)?;
        let current_hash =
            state_hash_v1(&before).map_err(|error| EditorError::TextFormatStateInvalid {
                story_id,
                message: error.to_string(),
            })?;
        if current_hash != expected_state_hash {
            return Err(EditorError::StaleOperation { story_id });
        }
        let receipt = overlay_readmit_font_v1(
            &before,
            start_scalar,
            end_scalar,
            identity,
            resource,
            expected_state_hash,
        )
        .map_err(|error| EditorError::TextFormatStateInvalid {
            story_id,
            message: error.to_string(),
        })?;
        if receipt.command.before_state_hash == receipt.command.after_state_hash {
            return Err(EditorError::NoChange { story_id });
        }
        let operation = EditOperation::SetTextFormatProperty {
            story_id,
            start_scalar,
            end_scalar,
            property: FormatPropertyV1::FontResource,
            value: FormatValueV1::FontResource(identity.clone()),
            before_state_hash: receipt.command.before_state_hash,
            after_state_hash: receipt.command.after_state_hash,
        };
        let replayed = apply_text_format_history_operation_v1(&before, &operation)?;
        if replayed != receipt.after_state {
            return Err(EditorError::TextFormatStateInvalid {
                story_id,
                message: "re-admitted font history differs from canonical receipt".to_owned(),
            });
        }
        self.undo.push(operation.clone());
        self.redo.clear();
        self.validate_source_identity()?;
        Ok(operation)
    }

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

    /// Admit an exact physical font into the canonical Story format history.
    ///
    /// The caller supplies independent, policy-approved full font bytes from
    /// its trusted resource registry; a browser candidate alone has no write
    /// authority. This commits text-format state only. Layout/PDF and fresh
    /// EditorProject re-admission require their own authoritative consumers.
    // Exact scope, trusted bytes and story/hash inputs intentionally remain
    // separate; callers cannot forge server authorization from a browser token.
    #[expect(clippy::too_many_arguments, reason = "admission trust inputs must remain explicit")]
    pub fn set_admitted_font_resource_v1(
        &mut self,
        story_id: StoryId,
        start_scalar: u32,
        end_scalar: u32,
        candidate: &FontReplacementCandidateV1,
        scope: &FontAuthoringScopeV1,
        server_resource: &ServerFontResourceV1<'_>,
        expected_state_hash: &str,
    ) -> Result<EditOperation, EditorError> {
        let before = self.current_text_format_overlay_v1(story_id)?;
        let before_hash =
            state_hash_v1(&before).map_err(|error| EditorError::TextFormatStateInvalid {
                story_id,
                message: error.to_string(),
            })?;
        if before_hash != expected_state_hash {
            return Err(EditorError::StaleOperation { story_id });
        }
        let receipt = overlay_admit_font_v1(
            &before,
            start_scalar,
            end_scalar,
            candidate,
            scope,
            server_resource,
            expected_state_hash,
        )
        .map_err(|error| EditorError::TextFormatStateInvalid {
            story_id,
            message: error.to_string(),
        })?;
        if receipt.command.before_state_hash == receipt.command.after_state_hash {
            return Err(EditorError::NoChange { story_id });
        }
        let operation = EditOperation::SetTextFormatProperty {
            story_id,
            start_scalar,
            end_scalar,
            property: FormatPropertyV1::FontResource,
            value: receipt
                .command
                .value
                .clone()
                .expect("admitted font set always carries its exact identity"),
            before_state_hash: receipt.command.before_state_hash,
            after_state_hash: receipt.command.after_state_hash,
        };
        let replayed = apply_text_format_history_operation_v1(&before, &operation)?;
        if replayed != receipt.after_state {
            return Err(EditorError::TextFormatStateInvalid {
                story_id,
                message: "admitted font history differs from canonical receipt".to_owned(),
            });
        }
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

pub(super) fn text_format_operation_property_v1(
    operation: &EditOperation,
) -> Option<FormatPropertyV1> {
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

/// This validates an operation already in admitted session history, never a
/// new client edit. The generic editor setter and fresh project replay still
/// reject an unsigned font-resource candidate without trusted original bytes.
fn replay_existing_text_format_set_v1(
    state: &TextFormatOverlayStateV1,
    start_scalar: u32,
    end_scalar: u32,
    property: FormatPropertyV1,
    value: FormatValueV1,
    expected_state_hash: &str,
) -> chaptera_text_format_overlay::Result<chaptera_text_format_overlay::TextFormatOperationReceiptV1>
{
    if let (FormatPropertyV1::FontResource, FormatValueV1::FontResource(identity)) =
        (property, &value)
    {
        replay_font_history_v1(
            state,
            start_scalar,
            end_scalar,
            identity,
            expected_state_hash,
        )
    } else {
        overlay_set_text_format_property_v1(
            state,
            start_scalar,
            end_scalar,
            property,
            value,
            expected_state_hash,
        )
    }
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
        } => replay_existing_text_format_set_v1(
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
            replay_existing_text_format_set_v1(
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

#[cfg(test)]
mod font_resource_session_tests {
    use super::*;
    use chaptera_text_format_overlay::{
        FontAuthoringScopeV1, FontReplacementCandidateV1, FontResourceIdentityV1, FormatPropertyV1,
        FormatValueV1, ServerFontResourceV1,
    };
    use pub_model::{
        Affine2D, CanonicalId, Document, DocumentId, LengthEmu, Node, NodeHeader, NodeId, NodeKind,
        Page, PageId, RectEmu, ResolvedGraph, Sha256Digest, Size2D, SourceDescriptor, Story,
        StoryId,
    };
    use pub_reader::{
        PubExplicitShapePaintSource, PubResolvedGraph, PubResolvedNodePayload,
        PubResolvedStoryFrame, PubTypographyBooleanV1, PubTypographyRun,
    };
    use std::collections::BTreeMap;

    fn canonical(byte: u8) -> CanonicalId {
        CanonicalId::from_bytes([byte; 16])
    }

    fn document_id() -> DocumentId {
        DocumentId::from_canonical(canonical(0x10))
    }

    fn page_id() -> PageId {
        PageId::from_canonical(canonical(0x20))
    }

    fn frame_id() -> NodeId {
        NodeId::from_canonical(canonical(0x30))
    }

    fn story_id() -> StoryId {
        StoryId::from_canonical(canonical(0x40))
    }

    fn source_hash() -> Sha256Digest {
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
            .parse()
            .expect("valid source hash")
    }

    fn graph() -> PubResolvedGraph {
        let page_id = page_id();
        let frame_id = frame_id();
        let story_id = story_id();
        let source_hash = source_hash();

        let frame = Node {
            kind: NodeKind::TextFrame,
            header: NodeHeader {
                id: frame_id,
                parent_id: page_id.into_canonical(),
                bounds: RectEmu::new(
                    LengthEmu::new(100_000),
                    LengthEmu::new(100_000),
                    LengthEmu::new(800_000),
                    LengthEmu::new(300_000),
                ),
                transform: Affine2D::identity(),
                source_refs: Vec::new(),
                extensions: Vec::new(),
            },
            payload: PubResolvedNodePayload {
                contents_seq_num: 1,
                officeart_shape_type: Some(202),
                officeart_spid: Some(1),
                image_slot: None,
                legacy_ole: None,
                explicit_image_crop: None,
                explicit_image_cardinal_rotation_degrees: None,
                explicit_paint: PubExplicitShapePaintSource::default(),
                effective_paint: None,
                story_frame: Some(PubResolvedStoryFrame {
                    story_id: Some(story_id),
                    ordinal: 0,
                    previous_frame: None,
                    next_frame: None,
                    vertical_alignment: None,
                }),
                text_frame_inset: None,
                table_story: None,
                table: None,
            },
        };

        ResolvedGraph {
            cdm_version: "0.1".into(),
            resolver_version: "story-text-session-test".into(),
            source: SourceDescriptor {
                format: "pub".into(),
                format_version: Some("0x2c".into()),
                adapter_version: "pub-rs/test".into(),
                source_hash,
            },
            document: Document {
                id: document_id(),
                format_origin: "pub".into(),
                source_hash,
                pages: vec![page_id],
                resources: Vec::new(),
                styles: Vec::new(),
            },
            pages: BTreeMap::from([(
                page_id,
                Page {
                    id: page_id,
                    size: Size2D::new(LengthEmu::new(4_000_000), LengthEmu::new(2_000_000)),
                    bleed: None,
                    margins: None,
                    children: vec![frame_id],
                    extensions: Vec::new(),
                },
            )]),
            nodes: BTreeMap::from([(frame_id, frame)]),
            stories: BTreeMap::from([(
                story_id,
                Story {
                    id: story_id,
                    text: "Hello world".into(),
                    paragraphs: Vec::new(),
                    runs: Vec::new(),
                    fields: Vec::new(),
                    hyperlinks: Vec::new(),
                    source_refs: Vec::new(),
                },
            )]),
            paragraphs: BTreeMap::new(),
            text_runs: BTreeMap::new(),
            resources: BTreeMap::new(),
            styles: BTreeMap::new(),
            extensions: BTreeMap::new(),
        }
    }

    fn session() -> EditorSession {
        let mut editor = EditorSession::new(graph()).expect("source-backed EditorSession");
        let normal = PubTypographyBooleanV1 {
            local_toggle: false,
            inherited_value: false,
            effective_value: false,
        };
        editor.source_typography_runs.push(PubTypographyRun {
            story_id: story_id(),
            story_utf16_start: 0,
            story_utf16_end: 11,
            story_scalar_start: 0,
            story_scalar_end: 11,
            source_font_index: 1,
            source_font_name: "Source Family".to_owned(),
            text_size_emu: 12000,
            font_inherited: false,
            size_inherited: false,
            color_rgb: Some([0, 0, 0]),
            color_scheme_slot: None,
            color_inherited: false,
            bold: Some(normal),
            italic: Some(normal),
        });
        editor
    }

    fn scoped_identity(
        bytes: &[u8],
    ) -> (
        FontAuthoringScopeV1,
        FontResourceIdentityV1,
        FontReplacementCandidateV1,
    ) {
        let scope = FontAuthoringScopeV1 {
            document_id: document_id().as_canonical().to_string(),
            revision_id: format!("sha256:{}", "1".repeat(64)),
            scene_snapshot_id: format!("sha256:{}", "2".repeat(64)),
            layout_environment_id: format!("sha256:{}", "3".repeat(64)),
            font_set_fingerprint: format!("sha256:{}", "4".repeat(64)),
        };
        let identity = FontResourceIdentityV1 {
            resource_id: "82222222-2222-4222-8222-222222222222".to_owned(),
            font_fingerprint: format!("sha256:{}", "b".repeat(64)),
            content_hash: Sha256::digest(bytes)
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect(),
            face_index: 0,
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
        (scope, identity, candidate)
    }

    #[test]
    fn admitted_font_resource_enters_real_editor_history_with_undo_redo_and_clear() {
        // Synthetic bytes prove the admission/EditorSession plumbing, NOT that
        // the bytes parse as a real font or are permitted for fixed output.
        let bytes = b"synthetic-server-owned-font-resource";
        let (scope, id, candidate) = scoped_identity(bytes);
        let resource = ServerFontResourceV1 {
            identity: &id,
            full_font_bytes: bytes,
            face_count: 1,
            is_full_resource: true,
            authoring_admitted: true,
        };
        let mut editor = session();
        let before = editor
            .current_text_format_overlay_v1(story_id())
            .expect("source format state");
        let hash = state_hash_v1(&before).expect("source format hash");

        // Existing generic history cannot be used by an untrusted client.
        assert!(
            editor
                .set_text_format_property_v1(
                    story_id(),
                    1,
                    5,
                    FormatPropertyV1::FontResource,
                    FormatValueV1::FontResource(id.clone()),
                    &hash,
                )
                .is_err()
        );
        assert!(editor.operations().is_empty());

        let operation = editor
            .set_admitted_font_resource_v1(story_id(), 1, 5, &candidate, &scope, &resource, &hash)
            .expect("independently admitted exact-byte font operation");
        assert!(matches!(
            &operation,
            EditOperation::SetTextFormatProperty {
                property: FormatPropertyV1::FontResource,
                value: FormatValueV1::FontResource(actual),
                ..
            } if actual == &id
        ));
        assert_eq!(editor.operations().len(), 1);
        assert_eq!(editor.graph().stories[&story_id()].text, "Hello world");

        let replaced = editor
            .current_text_format_overlay_v1(story_id())
            .expect("effective font override");
        assert_ne!(replaced, before);
        assert_eq!(replaced.overrides.len(), 1);
        editor.undo().expect("undo admitted font");
        assert_eq!(
            editor.current_text_format_overlay_v1(story_id()).unwrap(),
            before
        );
        editor.redo().expect("redo admitted font");
        assert_eq!(
            editor.current_text_format_overlay_v1(story_id()).unwrap(),
            replaced
        );

        // Project persistence must not silently admit a font without trusted
        // original bytes at the next host. Current project replay fails closed.
        let project = editor.project();
        assert_eq!(project.operations.len(), 1);
        let serialized = serde_json::to_vec(&project).unwrap();
        let loaded: EditorProject = serde_json::from_slice(&serialized).unwrap();
        assert_eq!(project, loaded);
        let mut fresh = session();
        assert!(fresh.apply_project(&loaded).is_err());
        assert!(fresh.operations().is_empty());
        assert_eq!(
            fresh.current_text_format_overlay_v1(story_id()).unwrap(),
            before
        );

        let cleared = editor
            .clear_text_format_property_override_v1(
                story_id(),
                1,
                5,
                FormatPropertyV1::FontResource,
                &state_hash_v1(&replaced).unwrap(),
            )
            .expect("clear font override reveals immutable source");
        assert!(matches!(
            cleared,
            EditOperation::ClearTextFormatPropertyOverride {
                property: FormatPropertyV1::FontResource,
                ..
            }
        ));
        assert_eq!(
            editor.current_text_format_overlay_v1(story_id()).unwrap(),
            before
        );
        editor.undo().expect("undo clear restores admitted font");
        assert_eq!(
            editor.current_text_format_overlay_v1(story_id()).unwrap(),
            replaced
        );
    }

    #[test]
    fn exact_admitted_project_font_survives_serialized_fresh_reopen_and_undo_redo() {
        let bytes = b"independently-server-owned-full-font-bytes-for-reopen";
        let (scope, identity, candidate) = scoped_identity(bytes);
        let resource = ServerFontResourceV1 {
            identity: &identity,
            full_font_bytes: bytes,
            face_count: 1,
            is_full_resource: true,
            authoring_admitted: true,
        };
        let mut editor = session();
        let source = editor.current_text_format_overlay_v1(story_id()).unwrap();
        editor
            .set_admitted_font_resource_v1(
                story_id(),
                0,
                11,
                &candidate,
                &scope,
                &resource,
                &state_hash_v1(&source).unwrap(),
            )
            .expect("original font admission");
        let edited = editor.current_text_format_overlay_v1(story_id()).unwrap();
        let project = editor.project();
        let disk = serde_json::to_vec(&project).unwrap();
        let loaded: EditorProject = serde_json::from_slice(&disk).unwrap();
        let grant = EditorProjectFontReopenGrantV1 {
            source_hash: loaded.source_hash,
            project_document_id: &loaded.identity.as_ref().unwrap().document_id,
            resource: ServerFontResourceV1 {
                identity: &identity,
                full_font_bytes: bytes,
                face_count: 1,
                is_full_resource: true,
                authoring_admitted: true,
            },
        };
        let mut fresh = session();
        fresh
            .apply_project_with_admitted_font_resources_v1(&loaded, &BTreeMap::new(), &[grant])
            .expect("trusted resource re-admission on a fresh EditorSession");
        assert_eq!(
            fresh.current_text_format_overlay_v1(story_id()).unwrap(),
            edited
        );
        assert_eq!(fresh.project().state_id_v1(), project.state_id_v1());
        assert_eq!(fresh.source_hash(), editor.source_hash());
        assert_eq!(fresh.graph().stories[&story_id()].text, "Hello world");
        fresh.undo().expect("fresh Undo");
        assert_eq!(
            fresh.current_text_format_overlay_v1(story_id()).unwrap(),
            source
        );
        fresh.redo().expect("fresh Redo");
        assert_eq!(
            fresh.current_text_format_overlay_v1(story_id()).unwrap(),
            edited
        );
    }

    #[test]
    fn recorded_font_reopen_rejects_wrong_context_missing_bytes_and_duplicate_grants_atomically() {
        let bytes = b"exact-reopen-resource";
        let (scope, identity, candidate) = scoped_identity(bytes);
        let mut author = session();
        let original_hash = author
            .current_text_format_state_hash_v1(story_id())
            .unwrap();
        author
            .set_admitted_font_resource_v1(
                story_id(),
                2,
                8,
                &candidate,
                &scope,
                &ServerFontResourceV1 {
                    identity: &identity,
                    full_font_bytes: bytes,
                    face_count: 1,
                    is_full_resource: true,
                    authoring_admitted: true,
                },
                &original_hash,
            )
            .expect("author canonical resource");
        let project = author.project();
        let project_doc_id = project.identity.as_ref().unwrap().document_id.as_str();
        let original_source = session()
            .current_text_format_overlay_v1(story_id())
            .unwrap();

        macro_rules! rejected_without_mutation {
            ($grants:expr) => {{
                let mut reopened = session();
                assert!(
                    reopened
                        .apply_project_with_admitted_font_resources_v1(
                            &project,
                            &BTreeMap::new(),
                            $grants
                        )
                        .is_err()
                );
                assert!(reopened.operations().is_empty());
                assert_eq!(
                    reopened.current_text_format_overlay_v1(story_id()).unwrap(),
                    original_source
                );
            }};
        }
        rejected_without_mutation!(&[]);
        let valid = || EditorProjectFontReopenGrantV1 {
            source_hash: project.source_hash,
            project_document_id: project_doc_id,
            resource: ServerFontResourceV1 {
                identity: &identity,
                full_font_bytes: bytes,
                face_count: 1,
                is_full_resource: true,
                authoring_admitted: true,
            },
        };
        rejected_without_mutation!(&[valid(), valid()]);
        rejected_without_mutation!(&[EditorProjectFontReopenGrantV1 {
            source_hash: Sha256Digest::from_bytes([0x99; 32]),
            ..valid()
        }]);
        rejected_without_mutation!(&[EditorProjectFontReopenGrantV1 {
            project_document_id: "other-project-document",
            ..valid()
        }]);
        rejected_without_mutation!(&[EditorProjectFontReopenGrantV1 {
            resource: ServerFontResourceV1 {
                full_font_bytes: b"different-font-bytes",
                ..valid().resource
            },
            ..valid()
        }]);
        rejected_without_mutation!(&[EditorProjectFontReopenGrantV1 {
            resource: ServerFontResourceV1 {
                authoring_admitted: false,
                ..valid().resource
            },
            ..valid()
        }]);
        rejected_without_mutation!(&[EditorProjectFontReopenGrantV1 {
            resource: ServerFontResourceV1 {
                is_full_resource: false,
                ..valid().resource
            },
            ..valid()
        }]);
        let other_identity = FontResourceIdentityV1 {
            content_hash: "a".repeat(64),
            ..identity.clone()
        };
        rejected_without_mutation!(&[EditorProjectFontReopenGrantV1 {
            resource: ServerFontResourceV1 {
                identity: &other_identity,
                ..valid().resource
            },
            ..valid()
        }]);
    }

    #[test]
    fn stale_scope_untrusted_bytes_and_wrong_face_do_not_commit_history() {
        let bytes = b"synthetic-resource-for-negative-controls";
        let (scope, id, candidate) = scoped_identity(bytes);
        let mut editor = session();
        let hash = editor
            .current_text_format_state_hash_v1(story_id())
            .unwrap();
        let good = ServerFontResourceV1 {
            identity: &id,
            full_font_bytes: bytes,
            face_count: 1,
            is_full_resource: true,
            authoring_admitted: true,
        };
        let mut stale = candidate.clone();
        stale.layout_environment_id = format!("sha256:{}", "9".repeat(64));
        assert!(
            editor
                .set_admitted_font_resource_v1(story_id(), 0, 11, &stale, &scope, &good, &hash)
                .is_err()
        );
        let corrupt = ServerFontResourceV1 {
            identity: &id,
            full_font_bytes: b"different bytes",
            face_count: 1,
            is_full_resource: true,
            authoring_admitted: true,
        };
        assert!(
            editor
                .set_admitted_font_resource_v1(
                    story_id(),
                    0,
                    11,
                    &candidate,
                    &scope,
                    &corrupt,
                    &hash
                )
                .is_err()
        );
        let denied = ServerFontResourceV1 {
            identity: &id,
            full_font_bytes: bytes,
            face_count: 1,
            is_full_resource: true,
            authoring_admitted: false,
        };
        assert!(
            editor
                .set_admitted_font_resource_v1(
                    story_id(),
                    0,
                    11,
                    &candidate,
                    &scope,
                    &denied,
                    &hash
                )
                .is_err()
        );
        let mut wrong_face = candidate.clone();
        wrong_face.face_index = 1;
        assert!(
            editor
                .set_admitted_font_resource_v1(story_id(), 0, 11, &wrong_face, &scope, &good, &hash)
                .is_err()
        );
        assert!(editor.operations().is_empty());
        assert_eq!(
            editor
                .current_text_format_state_hash_v1(story_id())
                .unwrap(),
            hash
        );
    }
}
