use std::{fmt, str::FromStr, sync::Arc};

use pub_editor::{
    EDITOR_PROJECT_VERSION_V0_2, EDITOR_PROJECT_VERSION_V0_3, EDITOR_PROJECT_VERSION_V0_4,
    EDITOR_PROJECT_VERSION_V0_5, EDITOR_PROJECT_VERSION_V0_6, EDITOR_PROJECT_VERSION_V0_7,
    EDITOR_PROJECT_VERSION_V0_8, EDITOR_PROJECT_VERSION_V0_9, EDITOR_PROJECT_VERSION_V0_10,
    EDITOR_PROJECT_VERSION_V0_11, EDITOR_PROJECT_VERSION_V0_12, EDITOR_PROJECT_VERSION_V0_13,
    EDITOR_PROJECT_VERSION_V0_14, EDITOR_PROJECT_VERSION_V0_15, EDITOR_PROJECT_VERSION_V0_16,
    EDITOR_PROJECT_VERSION_V0_17, EDITOR_PROJECT_VERSION_V0_18, EDITOR_PROJECT_VERSION_V0_19,
    EDITOR_PROJECT_VERSION_V0_20, EDITOR_PROJECT_VERSION_V0_21, EDITOR_PROJECT_VERSION_V0_22,
    EDITOR_PROJECT_VERSION_V0_23, EDITOR_PROJECT_VERSION_V0_24, EDITOR_PROJECT_VERSION_V0_25,
    EDITOR_PROJECT_VERSION_V0_26, EDITOR_PROJECT_VERSION_V0_27, EDITOR_PROJECT_VERSION_V0_28,
    EDITOR_PROJECT_VERSION_V0_29, EDITOR_PROJECT_VERSION_V0_30, EDITOR_PROJECT_VERSION_V0_31,
    EditOperation, EditorProject, Sha256Digest, open_mature_0x2c_editor,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    blob_store::BlobStoreService,
    source_baseline::{derive_authoring_state_identity, derive_history_revision_identities},
    sqlite_store::{
        RevisionEdge, SqliteRevisionStore, decode_canonical_event, encode_canonical_event,
    },
};

pub const EDITOR_REVISION_EVENT_SCHEMA_V1: &str = "chaptera.editor-revision-event.v1";
pub const EDITOR_HISTORY_EVENT_SCHEMA_V1: &str = "chaptera.editor-history-event.v1";
pub const MATERIALIZATION_RECEIPT_SCHEMA_V1: &str = "chaptera.exact-revision-materialization.v1";
pub const EDITOR_REVISION_EVENT_SEMANTIC_SCHEMA_VERSION: i64 = 1;
pub const EDITOR_HISTORY_EVENT_SEMANTIC_SCHEMA_VERSION: i64 = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevisionMaterializerError {
    pub code: &'static str,
    pub message: String,
}

impl RevisionMaterializerError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl fmt::Display for RevisionMaterializerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for RevisionMaterializerError {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthorizedDocumentSource {
    pub tenant_id: String,
    pub document_id: String,
    pub binding_id: String,
    pub source_sha256: String,
    pub byte_len: u64,
    pub baseline_revision_id: String,
    pub baseline_cursor: i64,
}

#[async_trait::async_trait]
pub trait DocumentSourceAuthority: Send + Sync {
    async fn resolve_document_source(
        &self,
        tenant_id: &str,
        document_id: &str,
    ) -> Result<AuthorizedDocumentSource, RevisionMaterializerError>;
}

#[async_trait::async_trait]
pub trait ExactSourceLoader: Send + Sync {
    async fn load_exact_source(
        &self,
        source: &AuthorizedDocumentSource,
    ) -> Result<Vec<u8>, RevisionMaterializerError>;
}

#[derive(Clone)]
pub struct BlobStoreExactSourceLoader {
    blob_store: BlobStoreService,
}

impl BlobStoreExactSourceLoader {
    pub fn new(blob_store: BlobStoreService) -> Self {
        Self { blob_store }
    }
}

#[async_trait::async_trait]
impl ExactSourceLoader for BlobStoreExactSourceLoader {
    async fn load_exact_source(
        &self,
        source: &AuthorizedDocumentSource,
    ) -> Result<Vec<u8>, RevisionMaterializerError> {
        let mut bytes = Vec::new();
        self.blob_store
            .stream_binding_verified(&source.tenant_id, &source.binding_id, &mut bytes)
            .await
            .map_err(|error| RevisionMaterializerError::new(error.code, error.message))?;

        if bytes.len() as u64 != source.byte_len {
            return Err(RevisionMaterializerError::new(
                "source_length_mismatch",
                "authorized source byte length differs from the verified binding bytes",
            ));
        }
        let actual_sha256 = sha256_hex(&bytes);
        if actual_sha256 != source.source_sha256 {
            return Err(RevisionMaterializerError::new(
                "source_hash_mismatch",
                "authorized source hash differs from the verified binding bytes",
            ));
        }
        Ok(bytes)
    }
}

pub trait EditorReplayEngine: Send + Sync {
    fn baseline_project(
        &self,
        source_bytes: &[u8],
        source_sha256: &str,
    ) -> Result<EditorProject, RevisionMaterializerError>;

    fn replay_project(
        &self,
        source_bytes: &[u8],
        source_sha256: &str,
        project: &EditorProject,
    ) -> Result<EditorProject, RevisionMaterializerError>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct PubEditorReplayEngine;

impl PubEditorReplayEngine {
    fn open(
        source_bytes: &[u8],
        source_sha256: &str,
    ) -> Result<pub_editor::EditorSession, RevisionMaterializerError> {
        let source_hash = Sha256Digest::from_str(source_sha256).map_err(|error| {
            RevisionMaterializerError::new(
                "invalid_source_hash",
                format!("authorized source hash is not canonical SHA-256: {error}"),
            )
        })?;
        open_mature_0x2c_editor(source_bytes, source_hash).map_err(|error| {
            RevisionMaterializerError::new(
                "editor_source_unsupported",
                format!("canonical editor could not open the immutable source: {error}"),
            )
        })
    }
}

impl EditorReplayEngine for PubEditorReplayEngine {
    fn baseline_project(
        &self,
        source_bytes: &[u8],
        source_sha256: &str,
    ) -> Result<EditorProject, RevisionMaterializerError> {
        let session = Self::open(source_bytes, source_sha256)?;
        let project = cloud_revision_project(&session.project());
        require_project_source(&project, source_sha256)?;
        if !project.assets.is_empty() {
            return Err(RevisionMaterializerError::new(
                "editor_asset_replay_unsupported",
                "baseline EditorProject unexpectedly requires external asset bytes",
            ));
        }
        Ok(project)
    }

    fn replay_project(
        &self,
        source_bytes: &[u8],
        source_sha256: &str,
        project: &EditorProject,
    ) -> Result<EditorProject, RevisionMaterializerError> {
        require_project_source(project, source_sha256)?;
        if !project.assets.is_empty() {
            return Err(RevisionMaterializerError::new(
                "editor_asset_replay_unsupported",
                "exact revision materialization does not yet resolve EditorProject asset bytes",
            ));
        }

        let mut session = Self::open(source_bytes, source_sha256)?;
        let mut local_replay = project.clone();
        if local_replay.identity.is_none()
            && cloud_replay_requires_local_identity(&local_replay.schema_version)
        {
            local_replay.identity = session.project().identity;
        }
        session.apply_project(&local_replay).map_err(|error| {
            RevisionMaterializerError::new(
                "editor_replay_rejected",
                format!("canonical EditorSession rejected persisted project replay: {error}"),
            )
        })?;
        let replayed = cloud_revision_project(&session.project());
        if replayed != *project {
            return Err(RevisionMaterializerError::new(
                "editor_replay_mismatch",
                "canonical EditorSession replay did not reproduce the exact Cloud semantic EditorProject",
            ));
        }
        Ok(replayed)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EditorRevisionEventV1 {
    pub schema_version: String,
    pub source_sha256: String,
    pub before_project_sha256: String,
    pub after_project_sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authoring_root_hash: Option<String>,
    pub operation: EditOperation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EditorHistoryEventV1 {
    pub schema_version: String,
    pub source_sha256: String,
    pub transition_kind: String,
    pub before_project_sha256: String,
    pub after_project_sha256: String,
    pub base_state_id: String,
    pub resulting_state_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authoring_root_hash: Option<String>,
    pub operation: EditOperation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExactRevisionMaterializationReceipt {
    pub schema_version: String,
    pub tenant_id: String,
    pub document_id: String,
    pub source_binding_id: String,
    pub source_sha256: String,
    pub baseline_revision_id: String,
    pub baseline_cursor: i64,
    pub requested_revision_id: String,
    pub canonical_revision_schema_version: String,
    pub canonical_authoring_revision_id: String,
    pub replayed_edges: usize,
    pub project_sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authoring_root_hash: Option<String>,
    pub project: EditorProject,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExactRevisionMaterializedState {
    pub receipt: ExactRevisionMaterializationReceipt,
    pub source_bytes: Vec<u8>,
}

pub struct ExactRevisionMaterializer {
    source_authority: Arc<dyn DocumentSourceAuthority>,
    source_loader: Arc<dyn ExactSourceLoader>,
    revision_store: SqliteRevisionStore,
    editor: Arc<dyn EditorReplayEngine>,
}

impl ExactRevisionMaterializer {
    pub fn new(
        source_authority: Arc<dyn DocumentSourceAuthority>,
        source_loader: Arc<dyn ExactSourceLoader>,
        revision_store: SqliteRevisionStore,
        editor: Arc<dyn EditorReplayEngine>,
    ) -> Self {
        Self {
            source_authority,
            source_loader,
            revision_store,
            editor,
        }
    }

    pub async fn materialize(
        &self,
        tenant_id: &str,
        document_id: &str,
        requested_revision_id: &str,
    ) -> Result<ExactRevisionMaterializationReceipt, RevisionMaterializerError> {
        Ok(self
            .materialize_state(tenant_id, document_id, requested_revision_id)
            .await?
            .receipt)
    }

    pub async fn materialize_state(
        &self,
        tenant_id: &str,
        document_id: &str,
        requested_revision_id: &str,
    ) -> Result<ExactRevisionMaterializedState, RevisionMaterializerError> {
        require_identifier(tenant_id, "tenant_id")?;
        require_identifier(document_id, "document_id")?;
        require_identifier(requested_revision_id, "requested_revision_id")?;

        let source = self
            .source_authority
            .resolve_document_source(tenant_id, document_id)
            .await?;
        validate_authorized_source(&source, tenant_id, document_id)?;

        let source_bytes = self.source_loader.load_exact_source(&source).await?;
        if source_bytes.len() as u64 != source.byte_len {
            return Err(RevisionMaterializerError::new(
                "source_length_mismatch",
                "loaded immutable source does not match the authorized byte length",
            ));
        }
        let actual_source_hash = sha256_hex(&source_bytes);
        if actual_source_hash != source.source_sha256 {
            return Err(RevisionMaterializerError::new(
                "source_hash_mismatch",
                "loaded immutable source does not match the authorized SHA-256",
            ));
        }

        let mut current_project = self
            .editor
            .baseline_project(&source_bytes, &source.source_sha256)?;
        require_project_source(&current_project, &source.source_sha256)?;
        if !current_project.assets.is_empty() {
            return Err(RevisionMaterializerError::new(
                "editor_asset_replay_unsupported",
                "baseline project contains asset metadata without a materialization asset resolver",
            ));
        }

        let edges = self
            .revision_store
            .load_chain_to_revision(
                document_id,
                &source.baseline_revision_id,
                source.baseline_cursor,
                requested_revision_id,
            )
            .await
            .map_err(|error| RevisionMaterializerError::new(error.code, error.message))?;

        let mut authoring_root_hash = None;
        for edge in &edges {
            current_project = self.replay_edge(&source_bytes, &source, current_project, edge)?;
            authoring_root_hash = edge.authoring_root_hash.clone();
        }

        let project_sha256 = project_sha256(&current_project)?;
        let identity = self
            .revision_store
            .require_revision_identity(document_id, requested_revision_id)
            .await
            .map_err(|error| RevisionMaterializerError::new(error.code, error.message))?;
        let receipt = ExactRevisionMaterializationReceipt {
            schema_version: MATERIALIZATION_RECEIPT_SCHEMA_V1.to_owned(),
            tenant_id: tenant_id.to_owned(),
            document_id: document_id.to_owned(),
            source_binding_id: source.binding_id,
            source_sha256: source.source_sha256,
            baseline_revision_id: source.baseline_revision_id,
            baseline_cursor: source.baseline_cursor,
            requested_revision_id: requested_revision_id.to_owned(),
            canonical_revision_schema_version: identity.canonical_schema_version,
            canonical_authoring_revision_id: identity.canonical_revision_id,
            replayed_edges: edges.len(),
            project_sha256,
            authoring_root_hash,
            project: current_project,
        };
        Ok(ExactRevisionMaterializedState {
            receipt,
            source_bytes,
        })
    }

    fn replay_edge(
        &self,
        source_bytes: &[u8],
        source: &AuthorizedDocumentSource,
        current_project: EditorProject,
        edge: &RevisionEdge,
    ) -> Result<EditorProject, RevisionMaterializerError> {
        if edge.document_id != source.document_id {
            return Err(RevisionMaterializerError::new(
                "revision_document_mismatch",
                "RevisionStream edge belongs to a different document",
            ));
        }
        if edge.semantic_schema_version == EDITOR_HISTORY_EVENT_SEMANTIC_SCHEMA_VERSION {
            return self.replay_undo_edge_v2(source_bytes, source, current_project, edge);
        }
        if edge.semantic_schema_version != EDITOR_REVISION_EVENT_SEMANTIC_SCHEMA_VERSION {
            return Err(RevisionMaterializerError::new(
                "unsupported_event_schema",
                format!(
                    "semantic schema version {} is not supported by exact materialization",
                    edge.semantic_schema_version
                ),
            ));
        }

        let event = decode_editor_revision_event_v1(edge)?;
        if event.source_sha256 != source.source_sha256 {
            return Err(RevisionMaterializerError::new(
                "event_source_mismatch",
                "revision event is bound to a different immutable source",
            ));
        }
        if event.authoring_root_hash != edge.authoring_root_hash {
            return Err(RevisionMaterializerError::new(
                "authoring_root_mismatch",
                "revision event authoring root does not match the durable RevisionStream edge",
            ));
        }
        if matches!(event.operation, EditOperation::ReplaceImage { .. }) {
            return Err(RevisionMaterializerError::new(
                "unsupported_event_operation",
                "ReplaceImage replay requires immutable asset-byte resolution and is fail-closed in materialization V1",
            ));
        }

        let before_hash = project_sha256(&current_project)?;
        if event.before_project_sha256 != before_hash {
            return Err(RevisionMaterializerError::new(
                "before_state_mismatch",
                "revision event before-project hash does not match the materialized predecessor state",
            ));
        }

        let candidate = append_event_operation(current_project, event.operation)?;
        let replayed =
            self.editor
                .replay_project(source_bytes, &source.source_sha256, &candidate)?;
        if replayed != candidate {
            return Err(RevisionMaterializerError::new(
                "editor_replay_mismatch",
                "canonical editor replay returned a different project than the durable event sequence",
            ));
        }

        let after_hash = project_sha256(&replayed)?;
        if event.after_project_sha256 != after_hash {
            return Err(RevisionMaterializerError::new(
                "event_state_hash_mismatch",
                "revision event after-project hash does not match canonical replay",
            ));
        }
        if edge.resulting_state_hash != after_hash {
            return Err(RevisionMaterializerError::new(
                "revision_state_hash_mismatch",
                "durable RevisionStream resulting_state_hash does not match canonical replay",
            ));
        }

        Ok(replayed)
    }

    fn replay_undo_edge_v2(
        &self,
        source_bytes: &[u8],
        source: &AuthorizedDocumentSource,
        current_project: EditorProject,
        edge: &RevisionEdge,
    ) -> Result<EditorProject, RevisionMaterializerError> {
        let event = decode_editor_history_event_v1(edge)?;
        if event.source_sha256 != source.source_sha256 {
            return Err(RevisionMaterializerError::new(
                "event_source_mismatch",
                "history event is bound to a different immutable source",
            ));
        }
        if event.authoring_root_hash != edge.authoring_root_hash {
            return Err(RevisionMaterializerError::new(
                "authoring_root_mismatch",
                "history event authoring root does not match the durable RevisionStream edge",
            ));
        }

        let before_hash = project_sha256(&current_project)?;
        if event.before_project_sha256 != before_hash {
            return Err(RevisionMaterializerError::new(
                "before_state_mismatch",
                "history event before-project hash does not match the materialized predecessor state",
            ));
        }
        let base_state = derive_authoring_state_identity(
            &edge.document_id,
            &source.source_sha256,
            &current_project.schema_version,
            &current_project,
        )
        .map_err(|error| RevisionMaterializerError::new(error.code, error.message))?;
        if event.base_state_id != base_state.state_id {
            return Err(RevisionMaterializerError::new(
                "history_base_state_mismatch",
                "history event base_state_id does not match the materialized predecessor state",
            ));
        }

        let candidate = undo_event_operation(current_project, &event.operation)?;
        let replayed =
            self.editor
                .replay_project(source_bytes, &source.source_sha256, &candidate)?;
        if replayed != candidate {
            return Err(RevisionMaterializerError::new(
                "editor_replay_mismatch",
                "canonical editor replay returned a different project after durable undo",
            ));
        }

        let after_hash = project_sha256(&replayed)?;
        if event.after_project_sha256 != after_hash {
            return Err(RevisionMaterializerError::new(
                "event_state_hash_mismatch",
                "history event after-project hash does not match canonical replay",
            ));
        }
        if edge.resulting_state_hash != after_hash {
            return Err(RevisionMaterializerError::new(
                "revision_state_hash_mismatch",
                "durable RevisionStream resulting_state_hash does not match canonical undo replay",
            ));
        }

        let resulting_state = derive_authoring_state_identity(
            &edge.document_id,
            &source.source_sha256,
            &replayed.schema_version,
            &replayed,
        )
        .map_err(|error| RevisionMaterializerError::new(error.code, error.message))?;
        if event.resulting_state_id != resulting_state.state_id {
            return Err(RevisionMaterializerError::new(
                "history_resulting_state_mismatch",
                "history event resulting_state_id does not match canonical undo replay",
            ));
        }
        let identities = derive_history_revision_identities(
            &edge.document_id,
            &source.source_sha256,
            &replayed.schema_version,
            &replayed,
            &edge.parent_revision,
            &event.base_state_id,
            &event.transition_kind,
        )
        .map_err(|error| RevisionMaterializerError::new(error.code, error.message))?;
        if identities.state_id != event.resulting_state_id
            || identities.service_revision_id != edge.child_revision
        {
            return Err(RevisionMaterializerError::new(
                "history_revision_identity_mismatch",
                "durable history edge differs from the canonical history-transition revision law",
            ));
        }

        Ok(replayed)
    }
}

pub fn encode_editor_revision_event_v1(
    event: &EditorRevisionEventV1,
) -> Result<Vec<u8>, RevisionMaterializerError> {
    validate_event_fields(event)?;
    let payload = canonical_json_bytes(event, "event_encode_failed", "editor revision event")?;
    encode_canonical_event(&payload)
        .map_err(|error| RevisionMaterializerError::new(error.code, error.message))
}

pub fn decode_editor_revision_event_v1(
    edge: &RevisionEdge,
) -> Result<EditorRevisionEventV1, RevisionMaterializerError> {
    let payload = decode_canonical_event(&edge.canonical_event)
        .map_err(|error| RevisionMaterializerError::new(error.code, error.message))?;
    let event: EditorRevisionEventV1 = serde_json::from_slice(&payload).map_err(|error| {
        RevisionMaterializerError::new(
            "event_decode_failed",
            format!("canonical RevisionStream event is not supported V1 JSON: {error}"),
        )
    })?;
    validate_event_fields(&event)?;
    Ok(event)
}

pub fn encode_editor_history_event_v1(
    event: &EditorHistoryEventV1,
) -> Result<Vec<u8>, RevisionMaterializerError> {
    validate_history_event_fields(event)?;
    let payload = canonical_json_bytes(event, "event_encode_failed", "editor history event")?;
    encode_canonical_event(&payload)
        .map_err(|error| RevisionMaterializerError::new(error.code, error.message))
}

pub fn decode_editor_history_event_v1(
    edge: &RevisionEdge,
) -> Result<EditorHistoryEventV1, RevisionMaterializerError> {
    let payload = decode_canonical_event(&edge.canonical_event)
        .map_err(|error| RevisionMaterializerError::new(error.code, error.message))?;
    let event: EditorHistoryEventV1 = serde_json::from_slice(&payload).map_err(|error| {
        RevisionMaterializerError::new(
            "event_decode_failed",
            format!("canonical RevisionStream event is not supported history-event JSON: {error}"),
        )
    })?;
    validate_history_event_fields(&event)?;
    Ok(event)
}

/// Project view used by Cloud revision identity/replay.
///
/// EditorProject v0.11 adds local project/document/history lineage. Those UUIDs
/// are intentionally excluded from Cloud semantic revision identity because
/// the Cloud Project/Document and RevisionStream authorities already own that
/// axis and exact source reopen must be deterministic across processes.
pub fn cloud_revision_project(project: &EditorProject) -> EditorProject {
    let mut projected = project.clone();
    projected.identity = None;
    projected.schema_version = cloud_revision_project_schema(project).to_owned();
    projected
}

fn cloud_replay_requires_local_identity(schema_version: &str) -> bool {
    [
        EDITOR_PROJECT_VERSION_V0_11,
        EDITOR_PROJECT_VERSION_V0_12,
        EDITOR_PROJECT_VERSION_V0_13,
        EDITOR_PROJECT_VERSION_V0_14,
        EDITOR_PROJECT_VERSION_V0_15,
        EDITOR_PROJECT_VERSION_V0_16,
        EDITOR_PROJECT_VERSION_V0_17,
        EDITOR_PROJECT_VERSION_V0_18,
        EDITOR_PROJECT_VERSION_V0_19,
        EDITOR_PROJECT_VERSION_V0_20,
        EDITOR_PROJECT_VERSION_V0_21,
        EDITOR_PROJECT_VERSION_V0_22,
        EDITOR_PROJECT_VERSION_V0_23,
        EDITOR_PROJECT_VERSION_V0_24,
        EDITOR_PROJECT_VERSION_V0_25,
        EDITOR_PROJECT_VERSION_V0_26,
        EDITOR_PROJECT_VERSION_V0_27,
        EDITOR_PROJECT_VERSION_V0_28,
        EDITOR_PROJECT_VERSION_V0_29,
        EDITOR_PROJECT_VERSION_V0_30,
        EDITOR_PROJECT_VERSION_V0_31,
    ]
    .contains(&schema_version)
}

fn cloud_revision_project_schema(project: &EditorProject) -> &'static str {
    let mut rank = if project.table_grids.is_empty() {
        2_u8
    } else {
        6_u8
    };

    for operation in &project.operations {
        let operation_rank = match operation {
            EditOperation::DuplicateAuthoredRectanglesPageV1 { .. } => 31,
            EditOperation::DuplicateAuthoredRectanglePageV1 { .. } => 30,
            EditOperation::DeleteAuthoredRectanglePageV1 { .. } => 29,
            EditOperation::InsertBlankPageAfterV1 { .. } => 28,
            EditOperation::DuplicateBlankPageV1 { .. } => 27,
            EditOperation::DeleteBlankAuthoredPageV1 { .. } => 26,
            EditOperation::AppendBlankPageV1 { .. } => 25,
            EditOperation::RegisterAuthoredPageIdentityV1 { .. } => 24,
            EditOperation::ReorderPagesV1 { .. } => 23,
            EditOperation::LinkTextFrameTail { .. } => 22,
            EditOperation::InsertTableRow { .. }
            | EditOperation::DeleteTableRow { .. }
            | EditOperation::InsertTableColumn { .. }
            | EditOperation::DeleteTableColumn { .. } => 21,
            EditOperation::SetTableTrackExtent { .. } => 20,
            EditOperation::SetImageCrop { .. } => 19,
            EditOperation::CreateTable { .. } => 18,
            EditOperation::CreateLine { .. } => 17,
            EditOperation::SetTextFormatPropertyScopedV1 { .. }
            | EditOperation::ClearTextFormatPropertyOverrideScopedV1 { .. } => 16,
            EditOperation::SetParagraphAlignmentOverride { .. }
            | EditOperation::ClearParagraphAlignmentOverride { .. } => 15,
            EditOperation::SetTextFormatProperty { .. }
            | EditOperation::ClearTextFormatPropertyOverride { .. } => 14,
            EditOperation::ReorderAuthoredStack { .. } => 13,
            EditOperation::DeleteNode { .. } => 12,
            EditOperation::CreateTextBox { .. } => 11,
            EditOperation::CreateShape { .. } => 10,
            EditOperation::ResizeNodes { .. } => 9,
            EditOperation::MoveNodes { .. } => 8,
            EditOperation::BreakTextFrameForwardLink { .. } => 7,
            EditOperation::ResizeNode { .. } => 5,
            EditOperation::MoveNode { .. } => 4,
            EditOperation::ReplaceImage { .. } => 3,
            EditOperation::ReplaceStoryRange { .. }
            | EditOperation::ReplaceStoryText { .. }
            | EditOperation::ReplaceTableCellText { .. } => 2,
        };
        rank = rank.max(operation_rank);
    }

    match rank {
        31 => EDITOR_PROJECT_VERSION_V0_31,
        30 => EDITOR_PROJECT_VERSION_V0_30,
        29 => EDITOR_PROJECT_VERSION_V0_29,
        28 => EDITOR_PROJECT_VERSION_V0_28,
        27 => EDITOR_PROJECT_VERSION_V0_27,
        26 => EDITOR_PROJECT_VERSION_V0_26,
        25 => EDITOR_PROJECT_VERSION_V0_25,
        24 => EDITOR_PROJECT_VERSION_V0_24,
        23 => EDITOR_PROJECT_VERSION_V0_23,
        22 => EDITOR_PROJECT_VERSION_V0_22,
        21 => EDITOR_PROJECT_VERSION_V0_21,
        20 => EDITOR_PROJECT_VERSION_V0_20,
        19 => EDITOR_PROJECT_VERSION_V0_19,
        18 => EDITOR_PROJECT_VERSION_V0_18,
        17 => EDITOR_PROJECT_VERSION_V0_17,
        16 => EDITOR_PROJECT_VERSION_V0_16,
        15 => EDITOR_PROJECT_VERSION_V0_15,
        14 => EDITOR_PROJECT_VERSION_V0_14,
        13 => EDITOR_PROJECT_VERSION_V0_13,
        12 => EDITOR_PROJECT_VERSION_V0_12,
        11 => EDITOR_PROJECT_VERSION_V0_11,
        10 => EDITOR_PROJECT_VERSION_V0_10,
        9 => EDITOR_PROJECT_VERSION_V0_9,
        8 => EDITOR_PROJECT_VERSION_V0_8,
        7 => EDITOR_PROJECT_VERSION_V0_7,
        6 => EDITOR_PROJECT_VERSION_V0_6,
        5 => EDITOR_PROJECT_VERSION_V0_5,
        4 => EDITOR_PROJECT_VERSION_V0_4,
        3 => EDITOR_PROJECT_VERSION_V0_3,
        _ => EDITOR_PROJECT_VERSION_V0_2,
    }
}

/// Raw lowercase SHA-256 of the existing Rar canonical JSON project law.
pub fn project_sha256(project: &EditorProject) -> Result<String, RevisionMaterializerError> {
    let bytes = canonical_json_bytes(project, "project_encode_failed", "EditorProject")?;
    Ok(sha256_hex(&bytes))
}

fn canonical_json_bytes<T: Serialize>(
    value: &T,
    code: &'static str,
    label: &'static str,
) -> Result<Vec<u8>, RevisionMaterializerError> {
    // serde_json::Value uses its canonical sorted-key map when the optional
    // preserve_order feature is not enabled. The workspace does not enable it.
    // Serializing via Value therefore matches the existing Rar/Python
    // sort_keys=True, separators=(",", ":"), ensure_ascii=False hash law.
    let canonical = serde_json::to_value(value).map_err(|error| {
        RevisionMaterializerError::new(
            code,
            format!("could not normalize {label} into canonical JSON: {error}"),
        )
    })?;
    serde_json::to_vec(&canonical).map_err(|error| {
        RevisionMaterializerError::new(
            code,
            format!("could not serialize canonical {label}: {error}"),
        )
    })
}

fn append_event_operation(
    mut project: EditorProject,
    operation: EditOperation,
) -> Result<EditorProject, RevisionMaterializerError> {
    require_project_source(&project, &project.source_hash.to_string())?;
    if !project.assets.is_empty() {
        return Err(RevisionMaterializerError::new(
            "editor_asset_replay_unsupported",
            "EditorProject asset metadata requires a separate immutable asset resolver",
        ));
    }
    project.operations.push(operation);
    Ok(cloud_revision_project(&project))
}

fn undo_event_operation(
    mut project: EditorProject,
    operation: &EditOperation,
) -> Result<EditorProject, RevisionMaterializerError> {
    require_project_source(&project, &project.source_hash.to_string())?;
    if !project.assets.is_empty() {
        return Err(RevisionMaterializerError::new(
            "editor_asset_replay_unsupported",
            "EditorProject asset metadata requires a separate immutable asset resolver",
        ));
    }
    let Some(last) = project.operations.last() else {
        return Err(RevisionMaterializerError::new(
            "history_undo_empty",
            "durable undo requires one canonical operation in current project history",
        ));
    };
    if last != operation {
        return Err(RevisionMaterializerError::new(
            "history_undo_operation_mismatch",
            "durable undo event does not match the latest canonical project operation",
        ));
    }
    project.operations.pop();
    Ok(cloud_revision_project(&project))
}

fn validate_history_event_fields(
    event: &EditorHistoryEventV1,
) -> Result<(), RevisionMaterializerError> {
    if event.schema_version != EDITOR_HISTORY_EVENT_SCHEMA_V1 {
        return Err(RevisionMaterializerError::new(
            "unsupported_event_schema",
            format!(
                "unsupported editor history event schema {:?}",
                event.schema_version
            ),
        ));
    }
    if event.transition_kind != "undo" {
        return Err(RevisionMaterializerError::new(
            "unsupported_history_transition",
            "materialization V2 currently admits durable undo only",
        ));
    }
    if !matches!(event.operation, EditOperation::MoveNode { .. }) {
        return Err(RevisionMaterializerError::new(
            "unsupported_history_operation",
            "durable history V2 currently admits MoveNode undo only",
        ));
    }
    require_sha256(&event.source_sha256, "event.source_sha256")?;
    require_sha256(&event.before_project_sha256, "event.before_project_sha256")?;
    require_sha256(&event.after_project_sha256, "event.after_project_sha256")?;
    require_prefixed_sha256(&event.base_state_id, "event.base_state_id")?;
    require_prefixed_sha256(&event.resulting_state_id, "event.resulting_state_id")?;
    if let Some(root) = &event.authoring_root_hash {
        require_sha256(root, "event.authoring_root_hash")?;
    }
    Ok(())
}

fn validate_event_fields(event: &EditorRevisionEventV1) -> Result<(), RevisionMaterializerError> {
    if event.schema_version != EDITOR_REVISION_EVENT_SCHEMA_V1 {
        return Err(RevisionMaterializerError::new(
            "unsupported_event_schema",
            format!(
                "unsupported editor revision event schema {:?}",
                event.schema_version
            ),
        ));
    }
    require_sha256(&event.source_sha256, "event.source_sha256")?;
    require_sha256(&event.before_project_sha256, "event.before_project_sha256")?;
    require_sha256(&event.after_project_sha256, "event.after_project_sha256")?;
    if let Some(root) = &event.authoring_root_hash {
        require_sha256(root, "event.authoring_root_hash")?;
    }
    Ok(())
}

fn validate_authorized_source(
    source: &AuthorizedDocumentSource,
    tenant_id: &str,
    document_id: &str,
) -> Result<(), RevisionMaterializerError> {
    if source.tenant_id != tenant_id {
        return Err(RevisionMaterializerError::new(
            "source_tenant_mismatch",
            "source authority returned a binding for a different tenant",
        ));
    }
    if source.document_id != document_id {
        return Err(RevisionMaterializerError::new(
            "source_document_mismatch",
            "source authority returned a binding for a different document",
        ));
    }
    require_identifier(&source.binding_id, "binding_id")?;
    require_identifier(&source.baseline_revision_id, "baseline_revision_id")?;
    require_sha256(&source.source_sha256, "source_sha256")?;
    if source.byte_len == 0 {
        return Err(RevisionMaterializerError::new(
            "source_length_invalid",
            "authorized immutable source length must be positive",
        ));
    }
    if source.baseline_cursor < 0 {
        return Err(RevisionMaterializerError::new(
            "invalid_baseline_cursor",
            "authorized baseline cursor must be non-negative",
        ));
    }
    Ok(())
}

fn require_project_source(
    project: &EditorProject,
    source_sha256: &str,
) -> Result<(), RevisionMaterializerError> {
    if project.source_hash.to_string() != source_sha256 {
        return Err(RevisionMaterializerError::new(
            "project_source_mismatch",
            "EditorProject source identity differs from the authorized immutable source",
        ));
    }
    Ok(())
}

fn require_identifier(value: &str, field: &'static str) -> Result<(), RevisionMaterializerError> {
    if value.is_empty()
        || value.len() > 160
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'-'))
    {
        return Err(RevisionMaterializerError::new(
            "invalid_identifier",
            format!("{field} is not a bounded opaque identifier"),
        ));
    }
    Ok(())
}

fn require_sha256(value: &str, field: &'static str) -> Result<(), RevisionMaterializerError> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        return Err(RevisionMaterializerError::new(
            "invalid_hash",
            format!("{field} must be 64 lowercase SHA-256 hex characters"),
        ));
    }
    Ok(())
}

fn require_prefixed_sha256(
    value: &str,
    field: &'static str,
) -> Result<(), RevisionMaterializerError> {
    let Some(raw) = value.strip_prefix("sha256:") else {
        return Err(RevisionMaterializerError::new(
            "invalid_hash",
            format!("{field} must use sha256: identity syntax"),
        ));
    };
    require_sha256(raw, field)
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;
        write!(&mut out, "{byte:02x}").expect("writing SHA-256 hex into String cannot fail");
    }
    out
}

#[cfg(test)]
mod replay_identity_tests {
    use super::*;

    #[test]
    fn durable_undo_rejects_empty_project_history() {
        let project: EditorProject = serde_json::from_value(serde_json::json!({
            "schema_version": "pub-editor-v0.2",
            "source_hash": "6a825ba26ba35d6e885acdc62e859591ed37cb0ff7480b554b9cb362b644dfcf",
            "operations": []
        }))
        .unwrap();
        let operation: EditOperation = serde_json::from_value(serde_json::json!({
            "kind": "move_node",
            "node_id": "00000000-0000-4000-8000-000000000001",
            "before": {"x": 0, "y": 0, "width": 100, "height": 100},
            "after": {"x": 10, "y": 20, "width": 100, "height": 100}
        }))
        .unwrap();

        assert_eq!(
            undo_event_operation(project, &operation).unwrap_err().code,
            "history_undo_empty"
        );
    }

    #[test]
    fn durable_undo_pops_only_the_exact_latest_canonical_operation() {
        let operation: EditOperation = serde_json::from_value(serde_json::json!({
            "kind": "move_node",
            "node_id": "00000000-0000-4000-8000-000000000001",
            "before": {"x": 0, "y": 0, "width": 100, "height": 100},
            "after": {"x": 10, "y": 20, "width": 100, "height": 100}
        }))
        .unwrap();
        let project: EditorProject = serde_json::from_value(serde_json::json!({
            "schema_version": "pub-editor-v0.4",
            "source_hash": "6a825ba26ba35d6e885acdc62e859591ed37cb0ff7480b554b9cb362b644dfcf",
            "operations": [operation.clone()]
        }))
        .unwrap();

        let undone = undo_event_operation(project, &operation).unwrap();
        assert!(undone.operations.is_empty());
        assert_eq!(undone.schema_version, EDITOR_PROJECT_VERSION_V0_2);
    }

    #[test]
    fn local_identity_rehydration_starts_at_v011() {
        assert!(!cloud_replay_requires_local_identity(
            EDITOR_PROJECT_VERSION_V0_2
        ));
        assert!(!cloud_replay_requires_local_identity(
            EDITOR_PROJECT_VERSION_V0_10
        ));
        assert!(cloud_replay_requires_local_identity(
            EDITOR_PROJECT_VERSION_V0_11
        ));
        assert!(cloud_replay_requires_local_identity(
            EDITOR_PROJECT_VERSION_V0_20
        ));
        assert!(cloud_replay_requires_local_identity(
            EDITOR_PROJECT_VERSION_V0_21
        ));
        assert!(cloud_replay_requires_local_identity(
            EDITOR_PROJECT_VERSION_V0_29
        ));
        assert!(cloud_replay_requires_local_identity(
            EDITOR_PROJECT_VERSION_V0_30
        ));
        assert!(cloud_replay_requires_local_identity(
            EDITOR_PROJECT_VERSION_V0_31
        ));
    }
}
