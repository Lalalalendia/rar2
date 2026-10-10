use std::{
    str::FromStr,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use axum::{
    Json, Router,
    extract::{Path, Request, State},
    http::{HeaderMap, HeaderValue, StatusCode, header::CACHE_CONTROL},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use axum_extra::extract::cookie::CookieJar;
use chaptera_cdm_model::{
    AUTHORING_REVISION_SCHEMA_V1, AuthoringRevisionIdV1, canonical_revision_json_v1,
    derive_authoring_revision_id_v1,
};
use chaptera_scene_instance::{
    GeometrySyncPolicyV1, direct_page_local_instance_v1, geometry_sync_policy_v1,
};
use pub_editor::{
    EditOperation, EditorProject, LengthEmu, NodeId, Sha256Digest, open_mature_0x2c_editor,
};
use pub_reader::PubResolvedGraph;
use pub_viewer::{open_pub_bundle, viewer_geometry_environment_v0_1};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::{
    auth_http::{AuthHttpError, AuthHttpState},
    authz_runtime::{
        AuthzError, CAP_VIEW, SqliteAuthorizedRevisionCommitter, SqliteAuthzAuthority,
    },
    product_replay_worker::{
        IsolatedMoveNodeIntentV1, IsolatedProductReplayProducer, IsolatedReaderSceneIntentV1,
        ProductReplayWorkerError,
    },
    reader_scene_v1::{ReaderSceneV1, from_viewer_geometry},
    revision_materializer::{
        BlobStoreExactSourceLoader, EDITOR_REVISION_EVENT_SCHEMA_V1,
        EDITOR_REVISION_EVENT_SEMANTIC_SCHEMA_VERSION, EditorRevisionEventV1,
        ExactRevisionMaterializer, ExactSourceLoader, PubEditorReplayEngine,
        RevisionMaterializerError, decode_editor_revision_event_v1,
        encode_editor_revision_event_v1, project_sha256,
    },
    source_authority::{SourceAuthorityError, SqliteDocumentSourceAuthority},
    source_baseline::{
        SourceBaselineError, SourceBaselineProducerConfig, derive_commit_revision_identities,
    },
    sqlite_store::{RevisionEdge, RevisionIdentityBinding, SqliteRevisionStore},
};

pub const COMMIT_REQUEST_V1: &str = "chaptera.commit-request.v1";
pub const COMMIT_ACCEPTED_V1: &str = "chaptera.commit-accepted.v1";
pub const CURRENT_DOCUMENT_V1: &str = "chaptera.current-document.v1";

#[derive(Clone)]
pub struct ProductApiHttpState {
    auth: AuthHttpState,
    source: SqliteDocumentSourceAuthority,
    authz: SqliteAuthzAuthority,
    revisions: SqliteRevisionStore,
    committer: SqliteAuthorizedRevisionCommitter,
    materializer: Arc<ExactRevisionMaterializer>,
    isolated_replay: Option<IsolatedProductReplayProducer>,
}

impl ProductApiHttpState {
    pub fn new(
        auth: AuthHttpState,
        source: SqliteDocumentSourceAuthority,
        authz: SqliteAuthzAuthority,
        revisions: SqliteRevisionStore,
        source_loader: BlobStoreExactSourceLoader,
    ) -> Result<Self, AuthzError> {
        Self::with_source_loader(auth, source, authz, revisions, Arc::new(source_loader))
    }

    fn with_source_loader(
        auth: AuthHttpState,
        source: SqliteDocumentSourceAuthority,
        authz: SqliteAuthzAuthority,
        revisions: SqliteRevisionStore,
        source_loader: Arc<dyn ExactSourceLoader>,
    ) -> Result<Self, AuthzError> {
        let committer = SqliteAuthorizedRevisionCommitter::new(authz.clone(), revisions.clone())?;
        let materializer = Arc::new(ExactRevisionMaterializer::new(
            Arc::new(source.clone()),
            source_loader,
            revisions.clone(),
            Arc::new(PubEditorReplayEngine),
        ));
        Ok(Self {
            auth,
            source,
            authz,
            revisions,
            committer,
            materializer,
            isolated_replay: None,
        })
    }

    /// Production must configure this at startup; tests using an injected
    /// in-process source loader retain their existing isolated test scope.
    pub fn with_isolated_replay(
        mut self,
        config: SourceBaselineProducerConfig,
    ) -> Result<Self, ProductReplayWorkerError> {
        let worker = IsolatedProductReplayProducer::new(config)?;
        let materializer =
            Arc::get_mut(&mut self.materializer).ok_or(ProductReplayWorkerError {
                code: "product_replay_config_invalid",
                message: "production materializer is shared before sandbox wiring",
            })?;
        // Fail closed: no host PUB parsing is permitted while canonical
        // baselines or historical revisions are being materialized.
        materializer.set_isolated_replay(worker.clone());
        self.isolated_replay = Some(worker);
        Ok(self)
    }
}

pub fn router(state: ProductApiHttpState) -> Router {
    Router::new()
        .route("/v1/documents/{document_id}/current", get(current_document))
        .route(
            "/v1/reader/documents/{document_id}/scene",
            get(reader_scene),
        )
        .route("/v1/documents/{document_id}/commit", post(commit_move_node))
        .with_state(state)
        .layer(middleware::from_fn(private_document_response))
}

async fn private_document_response(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

#[derive(Debug, Serialize)]
struct CurrentDocumentResponse {
    protocol_version: &'static str,
    document_id: String,
    source_hash: String,
    revision_id: String,
    revision_cursor: i64,
    canonical_revision_schema_version: String,
    canonical_authoring_revision_id: String,
    project: pub_editor::EditorProject,
    authoring_graph: PubResolvedGraph,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CommitRequestV1 {
    protocol_version: String,
    document_id: String,
    source_hash: String,
    base_revision_id: String,
    client_operation_id: String,
    command: MoveNodeToV1,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MoveNodeToV1 {
    kind: String,
    node_id: String,
    x_emu: i64,
    y_emu: i64,
}

#[derive(Debug, Serialize)]
struct CommitAcceptedResponse {
    protocol_version: &'static str,
    document_id: String,
    source_hash: String,
    base_revision_id: String,
    revision_id: String,
    state_id: String,
    client_operation_id: String,
    canonical_operation: EditOperation,
    project_schema_version: String,
    canonical_revision_schema_version: String,
    canonical_authoring_revision_id: String,
    replayed: bool,
    scene_refresh: &'static str,
}

#[derive(Debug, Clone)]
struct CurrentHead {
    revision_id: String,
    cursor: i64,
}

async fn current_document(
    State(state): State<ProductApiHttpState>,
    Path(document_id): Path<String>,
    headers: HeaderMap,
    jar: CookieJar,
) -> Result<Json<CurrentDocumentResponse>, ProductApiError> {
    let principal = state
        .auth
        .authenticate_read_request(&headers, &jar)
        .await
        .map_err(ProductApiError::Auth)?;

    let source = state
        .source
        .resolve_by_document_id(&document_id)
        .await
        .map_err(ProductApiError::Source)?;

    state
        .authz
        .authorize(
            &source.tenant_id,
            &document_id,
            &principal.principal_id,
            CAP_VIEW,
            "product-open",
            now_ms()?,
        )
        .await
        .map_err(ProductApiError::Authz)?;

    let head = current_head(&state.revisions, &source).await?;
    let materialized = state
        .materializer
        .materialize_state(&source.tenant_id, &document_id, &head.revision_id)
        .await
        .map_err(ProductApiError::Materializer)?;

    let authoring_graph = if let Some(worker) = &state.isolated_replay {
        worker
            .project_authoring_graph(
                &document_id,
                &source.source_sha256,
                &materialized.source_bytes,
                &materialized.receipt.project,
                &materialized.receipt.project_sha256,
            )
            .await
            .map_err(|error| ProductApiError::internal(error.code, error.message))?
    } else {
        // Test-only legacy producer when the HTTP state is manually injected.
        // Configured production routes always use the isolated replay worker.
        let source_hash = Sha256Digest::from_str(&source.source_sha256).map_err(|_| {
            ProductApiError::internal(
                "source_hash_invalid",
                "durable source authority contains an invalid SHA-256 identity",
            )
        })?;
        let mut session = open_mature_0x2c_editor(&materialized.source_bytes, source_hash)
            .map_err(|error| {
                ProductApiError::internal(
                    "editor_source_unsupported",
                    format!("canonical editor could not open durable source: {error}"),
                )
            })?;
        session
            .apply_project(&materialized.receipt.project)
            .map_err(|error| {
                ProductApiError::internal(
                    "editor_replay_failed",
                    format!("canonical editor could not replay exact current revision: {error}"),
                )
            })?;
        session.graph().clone()
    };
    let receipt = materialized.receipt;

    Ok(Json(CurrentDocumentResponse {
        protocol_version: CURRENT_DOCUMENT_V1,
        document_id,
        source_hash: source.source_sha256,
        revision_id: head.revision_id,
        revision_cursor: head.cursor,
        canonical_revision_schema_version: receipt.canonical_revision_schema_version,
        canonical_authoring_revision_id: receipt.canonical_authoring_revision_id,
        project: receipt.project,
        authoring_graph,
    }))
}

async fn reader_scene(
    State(state): State<ProductApiHttpState>,
    Path(document_id): Path<String>,
    headers: HeaderMap,
    jar: CookieJar,
) -> Result<Json<serde_json::Value>, ProductApiError> {
    let principal = state
        .auth
        .authenticate_read_request(&headers, &jar)
        .await
        .map_err(ProductApiError::Auth)?;

    let source = state
        .source
        .resolve_by_document_id(&document_id)
        .await
        .map_err(ProductApiError::Source)?;

    state
        .authz
        .authorize(
            &source.tenant_id,
            &document_id,
            &principal.principal_id,
            CAP_VIEW,
            "reader-scene",
            now_ms()?,
        )
        .await
        .map_err(ProductApiError::Authz)?;

    let head = current_head(&state.revisions, &source).await?;
    let materialized = state
        .materializer
        .materialize_state(&source.tenant_id, &document_id, &head.revision_id)
        .await
        .map_err(ProductApiError::Materializer)?;

    if let Some(worker) = &state.isolated_replay {
        // The authorized source/revision is selected by the server, not by
        // a browser-supplied worker path or revision. No in-process fallback.
        let scene = worker
            .project_reader_scene(
                &document_id,
                &source.source_sha256,
                &materialized.source_bytes,
                &materialized.receipt.project,
                &materialized.receipt.project_sha256,
                &IsolatedReaderSceneIntentV1 {
                    revision_id: head.revision_id.clone(),
                    baseline_revision_id: source.baseline_revision_id.clone(),
                },
            )
            .await
            .map_err(|error| ProductApiError::internal(error.code, error.message))?;
        return Ok(Json(scene));
    }

    // Injected test-only Product API state uses the canonical projection
    // without launching a Linux worker. Production always configures one.
    let scene = project_reader_scene_from_exact_source(
        document_id,
        source.source_sha256,
        head.revision_id,
        &source.baseline_revision_id,
        &materialized.source_bytes,
        &materialized.receipt.project,
    )?;
    Ok(Json(serde_json::to_value(scene).map_err(|_| {
        ProductApiError::internal(
            "reader_scene_projection_failed",
            "Reader scene could not be serialized",
        )
    })?))
}

fn project_reader_scene_from_exact_source(
    document_id: String,
    source_sha256: String,
    revision_id: String,
    baseline_revision_id: &str,
    source_bytes: &[u8],
    project: &EditorProject,
) -> Result<ReaderSceneV1, ProductApiError> {
    let mut bundle =
        open_pub_bundle(source_bytes, viewer_geometry_environment_v0_1()).map_err(|_| {
            ProductApiError::unprocessable(
                "reader_scene_open_failed",
                "source-neutral Viewer could not open this PUB source",
            )
        })?;

    if revision_id != baseline_revision_id {
        let source_hash = Sha256Digest::from_str(&source_sha256).map_err(|_| {
            ProductApiError::internal(
                "source_hash_invalid",
                "durable source authority contains an invalid SHA-256 identity",
            )
        })?;
        let mut session = open_mature_0x2c_editor(source_bytes, source_hash).map_err(|error| {
            ProductApiError::internal(
                "reader_scene_editor_source_unsupported",
                format!("canonical editor could not open durable source for scene replay: {error}"),
            )
        })?;
        session.apply_project(project).map_err(|error| {
            ProductApiError::internal(
                "reader_scene_editor_replay_failed",
                format!("canonical editor could not replay exact scene revision: {error}"),
            )
        })?;

        let mut moved_node_ids = Vec::new();
        for operation in &project.operations {
            match operation {
                EditOperation::MoveNode { node_id, .. } => moved_node_ids.push(*node_id),
                _ => {
                    return Err(ProductApiError::conflict(
                        "reader_scene_revision_operation_unsupported",
                        "rich Reader scene replay is currently admitted only for MoveNode revisions",
                    ));
                }
            }
        }

        for scene_node in &mut bundle.geometry.scene.nodes {
            let Some(authored_node) = session.graph().nodes.get(&scene_node.origin) else {
                continue;
            };
            if authored_node.header.parent_id != scene_node.parent_origin {
                continue;
            }
            let Ok(instance) = direct_page_local_instance_v1(
                &scene_node.origin.as_canonical().to_string(),
                &scene_node.parent_origin.to_string(),
            ) else {
                continue;
            };
            if geometry_sync_policy_v1(&instance)
                != GeometrySyncPolicyV1::ApplyAuthoredOriginGeometry
            {
                continue;
            }
            let bounds = authored_node.header.bounds;
            if session
                .can_move_node_to(scene_node.origin, bounds.x, bounds.y)
                .is_ok()
            {
                scene_node.bounds = bounds;
            }
        }

        bundle
            .geometry
            .refresh_text_projection_from_resolved(session.graph())
            .map_err(|error| {
                ProductApiError::internal(
                    "reader_scene_text_refresh_failed",
                    format!("current revision Viewer text refresh failed: {error}"),
                )
            })?;

        for moved_node_id in moved_node_ids {
            let Some(authored_node) = session.graph().nodes.get(&moved_node_id) else {
                return Err(ProductApiError::internal(
                    "reader_scene_move_node_missing",
                    "current canonical graph lost a durable MoveNode target",
                ));
            };
            let represented = bundle.geometry.scene.nodes.iter().any(|scene_node| {
                scene_node.origin == moved_node_id
                    && scene_node.parent_origin == authored_node.header.parent_id
                    && scene_node.bounds == authored_node.header.bounds
            });
            if !represented {
                return Err(ProductApiError::conflict(
                    "reader_scene_move_projection_unavailable",
                    "current MoveNode revision cannot be represented by the admitted rich Viewer scene",
                ));
            }
        }
    }

    let scene = from_viewer_geometry(
        document_id,
        source_sha256,
        revision_id,
        &bundle.geometry,
        &bundle.source_page_paint_orders,
    )
    .map_err(|error| {
        ProductApiError::internal(
            "reader_scene_projection_failed",
            format!("source-neutral Reader scene projection failed: {error}"),
        )
    })?;

    Ok(scene)
}

/// Called only inside the already confined product worker after post-read
/// filesystem seccomp is installed. This retains exactly the same Viewer
/// scene projection as the injected Product HTTP tests.
pub(crate) fn render_reader_scene_in_isolated_worker(
    document_id: &str,
    source_sha256: &str,
    revision_id: &str,
    baseline_revision_id: &str,
    source_bytes: &[u8],
    project: &EditorProject,
) -> Result<serde_json::Value, &'static str> {
    let scene = project_reader_scene_from_exact_source(
        document_id.to_owned(),
        source_sha256.to_owned(),
        revision_id.to_owned(),
        baseline_revision_id,
        source_bytes,
        project,
    )
    .map_err(|_| "reader_scene_projection_failed")?;
    serde_json::to_value(scene).map_err(|_| "reader_scene_projection_failed")
}

async fn commit_move_node(
    State(state): State<ProductApiHttpState>,
    Path(document_id): Path<String>,
    headers: HeaderMap,
    jar: CookieJar,
    Json(request): Json<CommitRequestV1>,
) -> Result<Json<CommitAcceptedResponse>, ProductApiError> {
    validate_request(&request, &document_id)?;

    let principal = state
        .auth
        .authenticate_mutation_request(&headers, &jar)
        .await
        .map_err(ProductApiError::Auth)?;
    let source = state
        .source
        .resolve_by_document_id(&document_id)
        .await
        .map_err(ProductApiError::Source)?;
    let request_hash = request_hash(&request)?;
    let now = now_ms()?;

    if let Some(existing) = state
        .committer
        .reconcile_geometry_revision(
            &source.tenant_id,
            &document_id,
            &principal.principal_id,
            &request.client_operation_id,
            &request_hash,
            now,
        )
        .await
        .map_err(ProductApiError::Authz)?
    {
        return accepted_from_receipt(&state, &source, existing, true).await;
    }

    // Check current geometry authorization before source identity comparison.
    if request.source_hash != source.source_sha256 {
        return Err(ProductApiError::bad_request(
            "source_hash_mismatch",
            "commit request source_hash differs from durable source authority",
        ));
    }

    let head = current_head(&state.revisions, &source).await?;
    if request.base_revision_id != head.revision_id {
        return Err(ProductApiError::conflict(
            "stale_revision",
            "base_revision_id is no longer the current RevisionStream head",
        ));
    }

    let materialized = state
        .materializer
        .materialize_state(&source.tenant_id, &document_id, &head.revision_id)
        .await
        .map_err(ProductApiError::Materializer)?;

    let (operation, resulting_project) = if let Some(worker) = &state.isolated_replay {
        // Production executes canonical PUB replay and mutation inside the
        // sandbox. AuthN/AuthZ, exact revision head and durable SQLite commit
        // remain authoritative in this main server process.
        worker
            .move_node_to(
                &document_id,
                &source.source_sha256,
                &materialized.source_bytes,
                &materialized.receipt.project,
                &materialized.receipt.project_sha256,
                &IsolatedMoveNodeIntentV1 {
                    node_id: request.command.node_id.clone(),
                    x_emu: request.command.x_emu,
                    y_emu: request.command.y_emu,
                },
            )
            .await
            .map_err(|error| match error.code {
                "product_move_node_invalid" | "product_move_node_rejected" => {
                    ProductApiError::bad_request(error.code, error.message)
                }
                _ => ProductApiError::internal(error.code, error.message),
            })?
    } else {
        // The in-process path remains only for manually injected unit tests.
        let source_hash = Sha256Digest::from_str(&source.source_sha256).map_err(|_| {
            ProductApiError::internal(
                "source_hash_invalid",
                "durable source authority contains an invalid SHA-256 identity",
            )
        })?;
        let mut session = open_mature_0x2c_editor(&materialized.source_bytes, source_hash)
            .map_err(|error| {
                ProductApiError::internal(
                    "editor_source_unsupported",
                    format!("canonical editor could not open durable source: {error}"),
                )
            })?;
        session
            .apply_project(&materialized.receipt.project)
            .map_err(|error| {
                ProductApiError::internal(
                    "editor_replay_failed",
                    format!("canonical editor could not replay exact base revision: {error}"),
                )
            })?;

        let node_id: NodeId =
            serde_json::from_value(serde_json::Value::String(request.command.node_id.clone()))
                .map_err(|_| {
                    ProductApiError::bad_request("node_id_invalid", "node_id is not canonical")
                })?;

        let operation = session
            .move_node_to(
                node_id,
                LengthEmu::new(request.command.x_emu),
                LengthEmu::new(request.command.y_emu),
            )
            .map_err(|error| {
                ProductApiError::bad_request(
                    "move_node_rejected",
                    format!("canonical MoveNode rejected the intent: {error}"),
                )
            })?;
        if !matches!(operation, EditOperation::MoveNode { .. }) {
            return Err(ProductApiError::internal(
                "move_node_operation_invalid",
                "canonical editor returned a non-MoveNode operation",
            ));
        }

        let resulting_project =
            crate::revision_materializer::cloud_revision_project(&session.project());
        (operation, resulting_project)
    };

    let before_project_sha256 = materialized.receipt.project_sha256.clone();
    let after_project_sha256 =
        project_sha256(&resulting_project).map_err(ProductApiError::Materializer)?;

    let identities = derive_commit_revision_identities(
        &document_id,
        &source.source_sha256,
        &resulting_project.schema_version,
        &resulting_project,
        &head.revision_id,
        &operation,
    )
    .map_err(ProductApiError::Baseline)?;

    let parent_canonical: AuthoringRevisionIdV1 = materialized
        .receipt
        .canonical_authoring_revision_id
        .parse()
        .map_err(|_| {
            ProductApiError::internal(
                "canonical_parent_revision_invalid",
                "materialized parent canonical revision identity is invalid",
            )
        })?;
    let canonical_child =
        derive_authoring_revision_id_v1(&resulting_project, Some(parent_canonical))
            .map_err(|_| {
                ProductApiError::internal(
                    "canonical_child_revision_failed",
                    "canonical child AuthoringRevisionId derivation failed",
                )
            })?
            .to_string();

    let event = EditorRevisionEventV1 {
        schema_version: EDITOR_REVISION_EVENT_SCHEMA_V1.to_owned(),
        source_sha256: source.source_sha256.clone(),
        before_project_sha256,
        after_project_sha256: after_project_sha256.clone(),
        authoring_root_hash: None,
        operation: operation.clone(),
    };
    let canonical_event =
        encode_editor_revision_event_v1(&event).map_err(ProductApiError::Materializer)?;

    let child_cursor = head.cursor.checked_add(1).ok_or_else(|| {
        ProductApiError::internal("revision_cursor_overflow", "revision cursor overflow")
    })?;

    let edge = RevisionEdge {
        document_id: document_id.clone(),
        parent_revision: head.revision_id.clone(),
        parent_cursor: head.cursor,
        operation_id: request.client_operation_id.clone(),
        request_hash,
        canonical_event,
        child_revision: identities.service_revision_id.clone(),
        child_cursor,
        resulting_state_hash: after_project_sha256,
        authoring_root_hash: None,
        semantic_schema_version: EDITOR_REVISION_EVENT_SEMANTIC_SCHEMA_VERSION,
        committed_at_ms: now,
    };
    let binding = RevisionIdentityBinding {
        document_id: document_id.clone(),
        service_revision_id: identities.service_revision_id.clone(),
        canonical_schema_version: AUTHORING_REVISION_SCHEMA_V1.to_owned(),
        canonical_revision_id: canonical_child,
        bound_at_ms: now,
    };

    let committed = state
        .committer
        .commit_geometry_revision(
            &source.tenant_id,
            &principal.principal_id,
            edge,
            binding,
            now,
        )
        .await
        .map_err(ProductApiError::Authz)?;

    accepted_from_receipt(&state, &source, committed, false).await
}

async fn current_head(
    revisions: &SqliteRevisionStore,
    source: &crate::revision_materializer::AuthorizedDocumentSource,
) -> Result<CurrentHead, ProductApiError> {
    let edges = revisions
        .load_document_edges(&source.document_id)
        .await
        .map_err(ProductApiError::Store)?;
    if let Some(last) = edges.last() {
        Ok(CurrentHead {
            revision_id: last.child_revision.clone(),
            cursor: last.child_cursor,
        })
    } else {
        Ok(CurrentHead {
            revision_id: source.baseline_revision_id.clone(),
            cursor: source.baseline_cursor,
        })
    }
}

async fn accepted_from_receipt(
    state: &ProductApiHttpState,
    source: &crate::revision_materializer::AuthorizedDocumentSource,
    receipt: crate::authz_runtime::AuthorizedRevisionCommitReceipt,
    replayed_hint: bool,
) -> Result<Json<CommitAcceptedResponse>, ProductApiError> {
    let event =
        decode_editor_revision_event_v1(&receipt.edge).map_err(ProductApiError::Materializer)?;
    let child = state
        .materializer
        .materialize(
            &source.tenant_id,
            &receipt.edge.document_id,
            &receipt.edge.child_revision,
        )
        .await
        .map_err(ProductApiError::Materializer)?;

    let identities = derive_commit_revision_identities(
        &receipt.edge.document_id,
        &event.source_sha256,
        &child.project.schema_version,
        &child.project,
        &receipt.edge.parent_revision,
        &event.operation,
    )
    .map_err(ProductApiError::Baseline)?;
    if identities.service_revision_id != receipt.edge.child_revision {
        return Err(ProductApiError::internal(
            "accepted_revision_identity_mismatch",
            "durable accepted edge differs from the existing V1 service revision law",
        ));
    }

    Ok(Json(CommitAcceptedResponse {
        protocol_version: COMMIT_ACCEPTED_V1,
        document_id: receipt.edge.document_id.clone(),
        source_hash: event.source_sha256,
        base_revision_id: receipt.edge.parent_revision,
        revision_id: receipt.edge.child_revision,
        state_id: identities.state_id,
        client_operation_id: receipt.edge.operation_id,
        canonical_operation: event.operation,
        project_schema_version: child.project.schema_version,
        canonical_revision_schema_version: receipt.binding.canonical_schema_version,
        canonical_authoring_revision_id: receipt.binding.canonical_revision_id,
        replayed: replayed_hint || receipt.replayed,
        scene_refresh: "full_snapshot",
    }))
}

fn validate_request(
    request: &CommitRequestV1,
    path_document_id: &str,
) -> Result<(), ProductApiError> {
    if request.protocol_version != COMMIT_REQUEST_V1 {
        return Err(ProductApiError::bad_request(
            "protocol_version_invalid",
            "chaptera.commit-request.v1 is required",
        ));
    }
    if request.document_id != path_document_id {
        return Err(ProductApiError::bad_request(
            "document_id_mismatch",
            "path document_id differs from request document_id",
        ));
    }
    if request.command.kind != "move_node_to" {
        return Err(ProductApiError::bad_request(
            "command_unsupported",
            "only move_node_to is admitted by this product slice",
        ));
    }
    require_ident(&request.document_id, "document_id")?;
    require_ident(&request.client_operation_id, "client_operation_id")?;
    require_hash(&request.source_hash, "source_hash")?;
    require_revision_id(&request.base_revision_id)?;
    Ok(())
}

fn request_hash(request: &CommitRequestV1) -> Result<String, ProductApiError> {
    let bytes = canonical_revision_json_v1(request).map_err(|_| {
        ProductApiError::internal(
            "request_hash_failed",
            "commit request canonicalization failed",
        )
    })?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

fn require_ident(value: &str, label: &'static str) -> Result<(), ProductApiError> {
    if value.is_empty()
        || value.len() > 192
        || !value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'@' | b'/' | b'-')
        })
    {
        return Err(ProductApiError::bad_request(
            "invalid_identity",
            format!("invalid {label}"),
        ));
    }
    Ok(())
}

fn require_hash(value: &str, label: &'static str) -> Result<(), ProductApiError> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        return Err(ProductApiError::bad_request(
            "invalid_hash",
            format!("{label} must be 64 lowercase SHA-256 hex characters"),
        ));
    }
    Ok(())
}

fn require_revision_id(value: &str) -> Result<(), ProductApiError> {
    let Some(raw) = value.strip_prefix("sha256:") else {
        return Err(ProductApiError::bad_request(
            "invalid_revision_id",
            "base_revision_id must use sha256: identity",
        ));
    };
    require_hash(raw, "base_revision_id")
}

fn now_ms() -> Result<i64, ProductApiError> {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| ProductApiError::internal("clock_invalid", "system clock is before epoch"))?
        .as_millis();
    i64::try_from(millis).map_err(|_| {
        ProductApiError::internal("clock_out_of_range", "system clock is out of range")
    })
}

#[derive(Debug)]
enum ProductApiError {
    Auth(AuthHttpError),
    Authz(AuthzError),
    Source(SourceAuthorityError),
    Materializer(RevisionMaterializerError),
    Store(crate::sqlite_store::SqliteStoreError),
    Baseline(SourceBaselineError),
    Http {
        status: StatusCode,
        code: &'static str,
        message: String,
    },
}

impl ProductApiError {
    fn bad_request(code: &'static str, message: impl Into<String>) -> Self {
        Self::Http {
            status: StatusCode::BAD_REQUEST,
            code,
            message: message.into(),
        }
    }

    fn conflict(code: &'static str, message: impl Into<String>) -> Self {
        Self::Http {
            status: StatusCode::CONFLICT,
            code,
            message: message.into(),
        }
    }

    fn unprocessable(code: &'static str, message: impl Into<String>) -> Self {
        Self::Http {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            code,
            message: message.into(),
        }
    }

    fn internal(code: &'static str, message: impl Into<String>) -> Self {
        Self::Http {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            code,
            message: message.into(),
        }
    }
}

fn public_product_error_message(status: StatusCode, original: &str) -> &str {
    if status.is_server_error() {
        "internal server error"
    } else {
        original
    }
}

impl IntoResponse for ProductApiError {
    fn into_response(self) -> Response {
        match self {
            Self::Auth(error) => error.into_response(),
            Self::Authz(error) => {
                let status = match error.code {
                    "stale_revision" | "idempotency_conflict" => StatusCode::CONFLICT,
                    "grant_missing" | "grant_expired" | "capability_denied" => {
                        StatusCode::FORBIDDEN
                    }
                    _ => StatusCode::INTERNAL_SERVER_ERROR,
                };
                let public_message = public_product_error_message(status, &error.message);
                (
                    status,
                    Json(json!({"error": {"code": error.code, "message": public_message}})),
                )
                    .into_response()
            }
            Self::Source(error) => {
                let status = if error.code == "document_source_not_found" {
                    StatusCode::NOT_FOUND
                } else {
                    StatusCode::INTERNAL_SERVER_ERROR
                };
                let public_message = public_product_error_message(status, &error.message);
                (
                    status,
                    Json(json!({"error": {"code": error.code, "message": public_message}})),
                )
                    .into_response()
            }
            Self::Materializer(error) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": {"code": error.code, "message": "internal server error"}})),
            )
                .into_response(),
            Self::Store(error) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": {"code": error.code, "message": "internal server error"}})),
            )
                .into_response(),
            Self::Baseline(error) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": {"code": error.code, "message": "internal server error"}})),
            )
                .into_response(),
            Self::Http {
                status,
                code,
                message,
            } => {
                let public_message = public_product_error_message(status, &message);
                (
                    status,
                    Json(json!({"error": {"code": code, "message": public_message}})),
                )
                    .into_response()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, sync::Arc, time::Duration};

    use axum::{
        body::{Body, to_bytes},
        http::{
            Request,
            header::{CONTENT_TYPE, COOKIE, HOST, ORIGIN},
        },
    };
    use chaptera_cdm_model::AUTHORING_REVISION_SCHEMA_V1;
    use sqlx::SqlitePool;
    use tower::ServiceExt;

    use crate::{
        auth_http::{CSRF_HEADER, SESSION_COOKIE},
        authn::SqliteAuthnStore,
        authn_session::{SessionPolicy, issue_verified_login_session},
        authz_runtime::DocumentRole,
        oidc_authn::OidcVerifiedIdentity,
        revision_materializer::{AuthorizedDocumentSource, EditorReplayEngine},
        schema_migration::SqliteMigrationRuntime,
        source_baseline::derive_import_baseline_identities,
    };

    use super::*;

    #[derive(Clone)]
    struct FixtureSourceLoader {
        bytes: Arc<Vec<u8>>,
        source_sha256: String,
    }

    #[async_trait::async_trait]
    impl ExactSourceLoader for FixtureSourceLoader {
        async fn load_exact_source(
            &self,
            source: &AuthorizedDocumentSource,
        ) -> Result<Vec<u8>, RevisionMaterializerError> {
            if source.source_sha256 != self.source_sha256
                || source.byte_len != self.bytes.len() as u64
            {
                return Err(RevisionMaterializerError::new(
                    "fixture_source_identity_mismatch",
                    "fixture source differs from durable source authority",
                ));
            }
            Ok(self.bytes.as_ref().clone())
        }
    }

    fn sample3_pub() -> Vec<u8> {
        decode_base64(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../vendor/producer-a/crates/pub-quill/tests/fixtures/Sample3.pub.b64"
        )))
    }

    fn decode_base64(text: &str) -> Vec<u8> {
        let cleaned = text
            .bytes()
            .filter(|byte| !byte.is_ascii_whitespace())
            .collect::<Vec<_>>();
        assert_eq!(cleaned.len() % 4, 0, "base64 fixture length");

        let mut output = Vec::with_capacity(cleaned.len() / 4 * 3);
        for quartet in cleaned.chunks_exact(4) {
            let a = base64_value(quartet[0]);
            let b = base64_value(quartet[1]);
            let c = if quartet[2] == b'=' {
                0
            } else {
                base64_value(quartet[2])
            };
            let d = if quartet[3] == b'=' {
                0
            } else {
                base64_value(quartet[3])
            };
            output.push((a << 2) | (b >> 4));
            if quartet[2] != b'=' {
                output.push((b << 4) | (c >> 2));
            }
            if quartet[3] != b'=' {
                output.push((c << 6) | d);
            }
        }
        output
    }

    fn base64_value(byte: u8) -> u8 {
        match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            other => panic!("invalid base64 byte {other:#x}"),
        }
    }

    fn fixture_hash(bytes: &[u8]) -> String {
        format!("{:x}", Sha256::digest(bytes))
    }

    async fn seed_document(
        pool: &SqlitePool,
        tenant_id: &str,
        document_id: &str,
        source_sha256: &str,
        source_len: usize,
        genesis_revision_id: &str,
        canonical_revision_id: &str,
    ) {
        sqlx::query(
            r#"
            INSERT INTO uploads (
                upload_id, tenant_id, principal_id, purpose, expected_byte_len,
                physical_upload_ref, state, upload_generation,
                object_version, object_etag, observed_byte_len,
                canonical_sha256, durable_binding_id,
                created_at_ms, expires_at_ms, completed_at_ms,
                idempotency_key, request_hash
            ) VALUES (?, ?, ?, 'pub_source', ?, 'fixture/ref', 'CONSUMED', 2,
                      'v1', 'etag', ?, ?, 'binding-sample3',
                      1, 9999999999999, 2, 'upload-idem', ?)
            "#,
        )
        .bind(b"upload-sample3".as_slice())
        .bind(tenant_id.as_bytes())
        .bind(b"principal-seed".as_slice())
        .bind(i64::try_from(source_len).unwrap())
        .bind(i64::try_from(source_len).unwrap())
        .bind(source_sha256.as_bytes())
        .bind(b"a".repeat(64))
        .execute(pool)
        .await
        .unwrap();

        sqlx::query(
            r#"
            INSERT INTO upload_consumptions (
                upload_id, tenant_id, idempotency_key, request_hash,
                project_id, document_id, genesis_revision_id, committed_at_ms
            ) VALUES (?, ?, 'consume-idem', ?, 'project-sample3', ?, ?, 3)
            "#,
        )
        .bind(b"upload-sample3".as_slice())
        .bind(tenant_id.as_bytes())
        .bind(b"b".repeat(64))
        .bind(document_id.as_bytes())
        .bind(genesis_revision_id.as_bytes())
        .execute(pool)
        .await
        .unwrap();

        sqlx::query(
            r#"
            INSERT INTO revision_identity_bindings (
                document_id, service_revision_id, canonical_schema_version,
                canonical_revision_id, bound_at_ms
            ) VALUES (?, ?, ?, ?, 3)
            "#,
        )
        .bind(document_id.as_bytes())
        .bind(genesis_revision_id.as_bytes())
        .bind(AUTHORING_REVISION_SCHEMA_V1)
        .bind(canonical_revision_id)
        .execute(pool)
        .await
        .unwrap();
    }

    fn authenticated_request(
        method: &str,
        uri: &str,
        session_token: &str,
        csrf: Option<&str>,
        body: Option<serde_json::Value>,
    ) -> Request<Body> {
        let mut builder = Request::builder()
            .method(method)
            .uri(uri)
            .header(HOST, "cloud.example.test")
            .header(COOKIE, format!("{SESSION_COOKIE}={session_token}"));
        if method != "GET" {
            builder = builder.header(ORIGIN, "https://cloud.example.test");
        }
        if let Some(csrf) = csrf {
            builder = builder.header(CSRF_HEADER, csrf);
        }
        if body.is_some() {
            builder = builder.header(CONTENT_TYPE, "application/json");
        }
        builder
            .body(match body {
                Some(value) => Body::from(serde_json::to_vec(&value).unwrap()),
                None => Body::empty(),
            })
            .unwrap()
    }

    async fn json_body(response: Response) -> serde_json::Value {
        let bytes = to_bytes(response.into_body(), 2 * 1024 * 1024)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[test]
    fn request_shape_rejects_tenant_and_authoritative_before_fields() {
        let value = json!({
            "protocol_version": COMMIT_REQUEST_V1,
            "document_id": "document-1",
            "source_hash": "a".repeat(64),
            "base_revision_id": format!("sha256:{}", "b".repeat(64)),
            "client_operation_id": "operation-1",
            "tenant_id": "tenant-a",
            "command": {
                "kind": "move_node_to",
                "node_id": "00112233-4455-6677-8899-aabbccddeeff",
                "x_emu": 1,
                "y_emu": 2,
                "before": {"x": 0, "y": 0}
            }
        });
        assert!(serde_json::from_value::<CommitRequestV1>(value).is_err());
    }

    #[test]
    fn canonical_request_hash_is_stable() {
        let request = CommitRequestV1 {
            protocol_version: COMMIT_REQUEST_V1.to_owned(),
            document_id: "document-1".to_owned(),
            source_hash: "a".repeat(64),
            base_revision_id: format!("sha256:{}", "b".repeat(64)),
            client_operation_id: "operation-1".to_owned(),
            command: MoveNodeToV1 {
                kind: "move_node_to".to_owned(),
                node_id: "00112233-4455-6677-8899-aabbccddeeff".to_owned(),
                x_emu: 10,
                y_emu: 20,
            },
        };
        assert_eq!(
            request_hash(&request).unwrap(),
            request_hash(&request).unwrap()
        );
    }

    #[tokio::test]
    async fn internal_failures_never_echo_parser_or_storage_diagnostics() {
        let canary = "private-pub-text-or-provider-secret";
        let errors = [
            ProductApiError::internal("editor_open_failed", canary),
            ProductApiError::Source(SourceAuthorityError {
                code: "sqlite_source_error",
                message: canary.to_owned(),
            }),
            ProductApiError::Authz(AuthzError {
                code: "sqlite_authz_error",
                message: canary.to_owned(),
            }),
            ProductApiError::Materializer(RevisionMaterializerError::new("replay_failed", canary)),
        ];
        for error in errors {
            let response = error.into_response();
            assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
            let body = json_body(response).await;
            assert_eq!(body["error"]["message"], "internal server error");
            assert!(!body.to_string().contains(canary));
        }

        let denied = ProductApiError::Authz(AuthzError {
            code: "grant_missing",
            message: "document permission missing".into(),
        })
        .into_response();
        assert_eq!(denied.status(), StatusCode::FORBIDDEN);
        let body = json_body(denied).await;
        assert_eq!(body["error"]["code"], "grant_missing");
        assert_eq!(body["error"]["message"], "document permission missing");
    }

    #[tokio::test]
    async fn http_open_move_retry_stale_restart_and_auth_csrf_are_authoritative() {
        let path = std::env::temp_dir().join(format!(
            "chaptera-product-api-http-{}-{}.sqlite",
            std::process::id(),
            now_ms().unwrap()
        ));
        SqliteMigrationRuntime::new(&path, Duration::from_secs(2))
            .unwrap()
            .migrate_up()
            .await
            .unwrap();

        let source_bytes = sample3_pub();
        let source_sha256 = fixture_hash(&source_bytes);
        let document_id = "document-sample3";
        let tenant_id = "tenant-sample3";
        let replay = PubEditorReplayEngine;
        let baseline_project = replay
            .baseline_project(&source_bytes, &source_sha256)
            .unwrap();
        let baseline = derive_import_baseline_identities(
            document_id,
            &source_sha256,
            &baseline_project.schema_version,
            &baseline_project,
        )
        .unwrap();

        let pool = SqlitePool::connect(&format!("sqlite://{}", path.display()))
            .await
            .unwrap();
        seed_document(
            &pool,
            tenant_id,
            document_id,
            &source_sha256,
            source_bytes.len(),
            &baseline.service_revision_id,
            &baseline.canonical_authoring_revision_id,
        )
        .await;

        let policy =
            SessionPolicy::new(Duration::from_secs(600), Duration::from_secs(3600)).unwrap();
        let authn = SqliteAuthnStore::open(&path, 4, Duration::from_secs(2))
            .await
            .unwrap();
        let issued = issue_verified_login_session(
            &authn,
            OidcVerifiedIdentity {
                issuer: "https://issuer.example.test".to_owned(),
                subject: "subject-product-api".to_owned(),
                email_snapshot: Some("editor@example.test".to_owned()),
                return_path: "/".to_owned(),
            },
            now_ms().unwrap(),
            policy,
        )
        .await
        .unwrap();
        let auth =
            AuthHttpState::api_test(authn.clone(), policy, "https://cloud.example.test").unwrap();
        let authz = SqliteAuthzAuthority::open(&path, 4, Duration::from_secs(2))
            .await
            .unwrap();
        authz
            .set_role(
                tenant_id,
                document_id,
                &issued.principal_id,
                DocumentRole::Editor,
                None,
                "grant-product-api",
                now_ms().unwrap(),
            )
            .await
            .unwrap();

        let revisions = SqliteRevisionStore::open(&path, 4, Duration::from_secs(2))
            .await
            .unwrap();
        let source = SqliteDocumentSourceAuthority::open(&path, 4, Duration::from_secs(2))
            .await
            .unwrap();
        let source_loader = FixtureSourceLoader {
            bytes: Arc::new(source_bytes.clone()),
            source_sha256: source_sha256.clone(),
        };
        let app = router(
            ProductApiHttpState::with_source_loader(
                auth,
                source.clone(),
                authz.clone(),
                revisions.clone(),
                Arc::new(source_loader),
            )
            .unwrap(),
        );

        let unauthorized = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/v1/documents/{document_id}/current"))
                    .header(HOST, "cloud.example.test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

        let opened = app
            .clone()
            .oneshot(authenticated_request(
                "GET",
                &format!("/v1/documents/{document_id}/current"),
                &issued.session_token,
                None,
                None,
            ))
            .await
            .unwrap();
        assert_eq!(opened.status(), StatusCode::OK);
        assert_eq!(opened.headers()[CACHE_CONTROL], "no-store");
        let opened = json_body(opened).await;
        assert_eq!(opened["revision_id"], baseline.service_revision_id);

        let reader_scene = app
            .clone()
            .oneshot(authenticated_request(
                "GET",
                &format!("/v1/reader/documents/{document_id}/scene"),
                &issued.session_token,
                None,
                None,
            ))
            .await
            .unwrap();
        assert_eq!(reader_scene.status(), StatusCode::OK);
        assert_eq!(reader_scene.headers()[CACHE_CONTROL], "no-store");
        let reader_scene = json_body(reader_scene).await;
        assert_eq!(
            reader_scene["protocol_version"],
            crate::reader_scene_v1::READER_SCENE_V1
        );
        assert_eq!(reader_scene["source_hash"], source_sha256);
        assert_eq!(reader_scene["revision_id"], baseline.service_revision_id);
        assert_eq!(reader_scene["scene_authority"], "server_viewer_projection");
        let reader_pages = reader_scene["pages"]
            .as_array()
            .expect("Reader pages array");
        let reader_nodes = reader_scene["nodes"]
            .as_array()
            .expect("Reader nodes array");
        assert!(!reader_pages.is_empty());
        assert!(!reader_nodes.is_empty());
        assert!(reader_nodes.iter().all(|node| {
            reader_pages
                .iter()
                .any(|page| page["page_id"] == node["page_id"])
        }));
        assert!(reader_scene.get("geometry").is_none());
        assert!(reader_scene.get("project").is_none());
        assert!(reader_scene.get("authoring_graph").is_none());

        let source_digest = Sha256Digest::from_str(&source_sha256).unwrap();
        let session = open_mature_0x2c_editor(&source_bytes, source_digest).unwrap();
        let (node_id, before_x_emu, before_y_emu, x_emu, y_emu) = session
            .graph()
            .nodes
            .iter()
            .find_map(|(node_id, node)| {
                let node_id_json = serde_json::to_value(node_id).ok()?;
                if !reader_nodes
                    .iter()
                    .any(|scene_node| scene_node["node_id"] == node_id_json)
                {
                    return None;
                }
                let before_x = node.header.bounds.x.get();
                let before_y = node.header.bounds.y.get();
                let x = before_x.checked_add(9_525)?;
                let y = before_y.checked_add(9_525)?;
                session
                    .can_move_node_to(*node_id, LengthEmu::new(x), LengthEmu::new(y))
                    .ok()
                    .map(|_| (*node_id, before_x, before_y, x, y))
            })
            .expect("Sample3 must contain one movable canonical node in the rich Reader scene");
        let node_id = serde_json::to_value(node_id)
            .unwrap()
            .as_str()
            .unwrap()
            .to_owned();
        let opened_node = &opened["authoring_graph"]["nodes"][node_id.as_str()];
        assert_eq!(opened_node["header"]["bounds"]["x"], before_x_emu);
        assert_eq!(opened_node["header"]["bounds"]["y"], before_y_emu);

        let body = json!({
            "protocol_version": COMMIT_REQUEST_V1,
            "document_id": document_id,
            "source_hash": source_sha256,
            "base_revision_id": baseline.service_revision_id,
            "client_operation_id": "move-sample3-1",
            "command": {
                "kind": "move_node_to",
                "node_id": node_id,
                "x_emu": x_emu,
                "y_emu": y_emu
            }
        });

        let missing_csrf = app
            .clone()
            .oneshot(authenticated_request(
                "POST",
                &format!("/v1/documents/{document_id}/commit"),
                &issued.session_token,
                None,
                Some(body.clone()),
            ))
            .await
            .unwrap();
        assert_eq!(missing_csrf.status(), StatusCode::FORBIDDEN);

        let accepted = app
            .clone()
            .oneshot(authenticated_request(
                "POST",
                &format!("/v1/documents/{document_id}/commit"),
                &issued.session_token,
                Some(&issued.csrf_token),
                Some(body.clone()),
            ))
            .await
            .unwrap();
        assert_eq!(accepted.status(), StatusCode::OK);
        let accepted = json_body(accepted).await;
        let child_revision = accepted["revision_id"].as_str().unwrap().to_owned();
        assert_ne!(child_revision, baseline.service_revision_id);
        assert_eq!(accepted["replayed"], false);

        let reader_after_edit = app
            .clone()
            .oneshot(authenticated_request(
                "GET",
                &format!("/v1/reader/documents/{document_id}/scene"),
                &issued.session_token,
                None,
                None,
            ))
            .await
            .unwrap();
        assert_eq!(reader_after_edit.status(), StatusCode::OK);
        let reader_after_edit = json_body(reader_after_edit).await;
        assert_eq!(reader_after_edit["revision_id"], child_revision);
        assert_eq!(reader_after_edit["source_hash"], source_sha256);
        assert_eq!(
            reader_after_edit["scene_authority"],
            "server_viewer_projection"
        );
        assert!(reader_after_edit.get("project").is_none());
        assert!(reader_after_edit.get("authoring_graph").is_none());

        let baseline_node = reader_scene["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|scene_node| scene_node["node_id"] == node_id)
            .expect("baseline Reader scene must contain moved node");
        let edited_node = reader_after_edit["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|scene_node| scene_node["node_id"] == node_id)
            .expect("edited Reader scene must contain moved node");
        assert_eq!(baseline_node["bounds"]["x"], before_x_emu);
        assert_eq!(baseline_node["bounds"]["y"], before_y_emu);
        assert_eq!(edited_node["bounds"]["x"], x_emu);
        assert_eq!(edited_node["bounds"]["y"], y_emu);
        assert_eq!(
            edited_node["bounds"]["width"],
            baseline_node["bounds"]["width"]
        );
        assert_eq!(
            edited_node["bounds"]["height"],
            baseline_node["bounds"]["height"]
        );
        assert_eq!(
            reader_after_edit["nodes"].as_array().unwrap().len(),
            reader_scene["nodes"].as_array().unwrap().len()
        );

        let retry = app
            .clone()
            .oneshot(authenticated_request(
                "POST",
                &format!("/v1/documents/{document_id}/commit"),
                &issued.session_token,
                Some(&issued.csrf_token),
                Some(body.clone()),
            ))
            .await
            .unwrap();
        assert_eq!(retry.status(), StatusCode::OK);
        let retry = json_body(retry).await;
        assert_eq!(retry["revision_id"], child_revision);
        assert_eq!(retry["replayed"], true);

        let mut stale_body = body.clone();
        stale_body["client_operation_id"] = json!("move-sample3-stale");
        let stale = app
            .clone()
            .oneshot(authenticated_request(
                "POST",
                &format!("/v1/documents/{document_id}/commit"),
                &issued.session_token,
                Some(&issued.csrf_token),
                Some(stale_body),
            ))
            .await
            .unwrap();
        assert_eq!(stale.status(), StatusCode::CONFLICT);
        let stale = json_body(stale).await;
        assert_eq!(stale["error"]["code"], "stale_revision");

        let reopened = app
            .oneshot(authenticated_request(
                "GET",
                &format!("/v1/documents/{document_id}/current"),
                &issued.session_token,
                None,
                None,
            ))
            .await
            .unwrap();
        assert_eq!(reopened.status(), StatusCode::OK);
        let reopened = json_body(reopened).await;
        assert_eq!(reopened["revision_id"], child_revision);
        let reopened_node = &reopened["authoring_graph"]["nodes"][node_id.as_str()];
        assert_eq!(reopened_node["header"]["bounds"]["x"], x_emu);
        assert_eq!(reopened_node["header"]["bounds"]["y"], y_emu);

        // A second HTTP request through the same router is NOT a durability
        // proof. Close all original stores, reopen new SQLite pools and rebuild
        // the Product API, including its source authority and replay engine.
        pool.close().await;
        authn.close().await;
        authz.close().await;
        source.close().await;
        revisions.close().await;

        let restarted_authn = SqliteAuthnStore::open(&path, 4, Duration::from_secs(2))
            .await
            .unwrap();
        let restarted_authz = SqliteAuthzAuthority::open(&path, 4, Duration::from_secs(2))
            .await
            .unwrap();
        let restarted_source =
            SqliteDocumentSourceAuthority::open(&path, 4, Duration::from_secs(2))
                .await
                .unwrap();
        let restarted_revisions = SqliteRevisionStore::open(&path, 4, Duration::from_secs(2))
            .await
            .unwrap();
        let restarted_auth = AuthHttpState::api_test(
            restarted_authn.clone(),
            policy,
            "https://cloud.example.test",
        )
        .unwrap();
        let restarted_app = router(
            ProductApiHttpState::with_source_loader(
                restarted_auth,
                restarted_source.clone(),
                restarted_authz.clone(),
                restarted_revisions.clone(),
                Arc::new(FixtureSourceLoader {
                    bytes: Arc::new(source_bytes.clone()),
                    source_sha256: source_sha256.clone(),
                }),
            )
            .unwrap(),
        );

        // The original OIDC-issued cookie must still work after the original
        // AuthN and revision connections are gone: no in-memory project state
        // or authorization grants are re-seeded for this reopened application.
        let durable = restarted_app
            .clone()
            .oneshot(authenticated_request(
                "GET",
                &format!("/v1/documents/{document_id}/current"),
                &issued.session_token,
                None,
                None,
            ))
            .await
            .unwrap();
        assert_eq!(durable.status(), StatusCode::OK);
        let durable = json_body(durable).await;
        assert_eq!(durable["revision_id"], child_revision);
        assert_eq!(
            durable["canonical_authoring_revision_id"],
            reopened["canonical_authoring_revision_id"]
        );
        assert_eq!(
            durable["authoring_graph"]["nodes"][node_id.as_str()]["header"]["bounds"]["x"],
            x_emu
        );
        assert_eq!(
            durable["authoring_graph"]["nodes"][node_id.as_str()]["header"]["bounds"]["y"],
            y_emu
        );

        let scene_after_restart = restarted_app
            .clone()
            .oneshot(authenticated_request(
                "GET",
                &format!("/v1/reader/documents/{document_id}/scene"),
                &issued.session_token,
                None,
                None,
            ))
            .await
            .unwrap();
        assert_eq!(scene_after_restart.status(), StatusCode::OK);
        let scene_after_restart = json_body(scene_after_restart).await;
        assert_eq!(scene_after_restart["revision_id"], child_revision);
        let moved_node = scene_after_restart["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|node| node["node_id"] == node_id)
            .expect("Reader must recover edited node from persisted revision");
        assert_eq!(moved_node["bounds"]["x"], x_emu);
        assert_eq!(moved_node["bounds"]["y"], y_emu);

        // Lost commit ACKs must remain idempotent across the connection
        // restart, not append another revision on replay.
        let replay_after_restart = restarted_app
            .clone()
            .oneshot(authenticated_request(
                "POST",
                &format!("/v1/documents/{document_id}/commit"),
                &issued.session_token,
                Some(&issued.csrf_token),
                Some(body.clone()),
            ))
            .await
            .unwrap();
        assert_eq!(replay_after_restart.status(), StatusCode::OK);
        let replay_after_restart = json_body(replay_after_restart).await;
        assert_eq!(replay_after_restart["replayed"], true);
        assert_eq!(replay_after_restart["revision_id"], child_revision);

        // An independently authenticated principal does not gain access to
        // the recovered document simply by knowing the DocumentId.
        let other = issue_verified_login_session(
            &restarted_authn,
            OidcVerifiedIdentity {
                issuer: "https://issuer.example.test".to_owned(),
                subject: "subject-product-api-unrelated".to_owned(),
                email_snapshot: None,
                return_path: "/".to_owned(),
            },
            now_ms().unwrap(),
            policy,
        )
        .await
        .unwrap();
        let forbidden = restarted_app
            .clone()
            .oneshot(authenticated_request(
                "GET",
                &format!("/v1/documents/{document_id}/current"),
                &other.session_token,
                None,
                None,
            ))
            .await
            .unwrap();
        assert_eq!(forbidden.status(), StatusCode::FORBIDDEN);
        assert_eq!(forbidden.headers()[CACHE_CONTROL], "no-store");
        assert_eq!(json_body(forbidden).await["error"]["code"], "grant_missing");

        let mut denial_receipts = Vec::new();
        for (index, source_hash) in [source_sha256.clone(), "0".repeat(64)]
            .into_iter()
            .enumerate()
        {
            let mut attempt = body.clone();
            attempt["client_operation_id"] = json!(format!("rejected-source-check-{index}"));
            attempt["source_hash"] = json!(source_hash);
            let denied = restarted_app
                .clone()
                .oneshot(authenticated_request(
                    "POST",
                    &format!("/v1/documents/{document_id}/commit"),
                    &other.session_token,
                    Some(&other.csrf_token),
                    Some(attempt),
                ))
                .await
                .unwrap();
            assert_eq!(denied.status(), StatusCode::FORBIDDEN);
            assert_eq!(denied.headers()[CACHE_CONTROL], "no-store");
            denial_receipts.push(json_body(denied).await);
        }
        assert_eq!(denial_receipts[0], denial_receipts[1]);
        assert_eq!(denial_receipts[0]["error"]["code"], "grant_missing");

        // A real Viewer grant allows reading the same reopened document but
        // never authorizes a MoveNode mutation. This tests the canonical
        // capability_denied error, not a mocked policy decision.
        restarted_authz
            .set_role(
                tenant_id,
                document_id,
                &other.principal_id,
                DocumentRole::Viewer,
                None,
                "grant-restarted-viewer",
                now_ms().unwrap(),
            )
            .await
            .unwrap();
        let viewer_read = restarted_app
            .clone()
            .oneshot(authenticated_request(
                "GET",
                &format!("/v1/documents/{document_id}/current"),
                &other.session_token,
                None,
                None,
            ))
            .await
            .unwrap();
        assert_eq!(viewer_read.status(), StatusCode::OK);

        let mut viewer_body = body;
        viewer_body["client_operation_id"] = json!("viewer-move-denied");
        viewer_body["base_revision_id"] = json!(child_revision);
        let viewer_commit = restarted_app
            .clone()
            .oneshot(authenticated_request(
                "POST",
                &format!("/v1/documents/{document_id}/commit"),
                &other.session_token,
                Some(&other.csrf_token),
                Some(viewer_body),
            ))
            .await
            .unwrap();
        assert_eq!(viewer_commit.status(), StatusCode::FORBIDDEN);
        assert_eq!(
            json_body(viewer_commit).await["error"]["code"],
            "capability_denied"
        );

        // Expiring the existing Viewer grant must deny GET with 403 rather
        // than turning a normal authorization decision into HTTP 500.
        let current_time = now_ms().unwrap();
        restarted_authz
            .set_role(
                tenant_id,
                document_id,
                &other.principal_id,
                DocumentRole::Viewer,
                Some(current_time - 1),
                "grant-restarted-expired",
                current_time,
            )
            .await
            .unwrap();
        let expired = restarted_app
            .oneshot(authenticated_request(
                "GET",
                &format!("/v1/documents/{document_id}/current"),
                &other.session_token,
                None,
                None,
            ))
            .await
            .unwrap();
        assert_eq!(expired.status(), StatusCode::FORBIDDEN);
        assert_eq!(json_body(expired).await["error"]["code"], "grant_expired");

        restarted_authn.close().await;
        restarted_authz.close().await;
        restarted_source.close().await;
        restarted_revisions.close().await;
        for suffix in ["", "-wal", "-shm"] {
            let _ = fs::remove_file(format!("{}{}", path.display(), suffix));
        }
    }
}
