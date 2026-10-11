//! Authenticated project catalog for a selected workspace.
//!
//! SourceIngress owns creation, SqliteProjectPersistence owns lifecycle state,
//! and WorkspaceContextResolver owns tenant admission. This is a read-only
//! service projection, not a new project database or browser authority.

use std::time::{SystemTime, UNIX_EPOCH};

use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use axum_extra::extract::cookie::CookieJar;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{
    auth_http::{AuthHttpError, AuthHttpState},
    authz_runtime::{AuthzError, CAP_MEMBER_MANAGE, SqliteAuthzAuthority},
    project_persistence_sqlite::{
        ProjectCatalogEntry, ProjectRenameReceipt, RenameProjectRequest, SqliteProjectPersistence,
    },
    source_ingress::IngressError,
    workspace_context::{SqliteWorkspaceContextResolver, WorkspaceContextError},
};

#[derive(Clone)]
pub struct WorkspaceProjectsHttpState {
    auth: AuthHttpState,
    workspace: SqliteWorkspaceContextResolver,
    projects: SqliteProjectPersistence,
    authz: SqliteAuthzAuthority,
}

impl WorkspaceProjectsHttpState {
    pub fn new(
        auth: AuthHttpState,
        workspace: SqliteWorkspaceContextResolver,
        projects: SqliteProjectPersistence,
        authz: SqliteAuthzAuthority,
    ) -> Self {
        Self {
            auth,
            workspace,
            projects,
            authz,
        }
    }
}

pub fn router(state: WorkspaceProjectsHttpState) -> Router {
    Router::new()
        .route("/v1/workspaces/{workspace_id}/projects", get(list))
        .route("/v1/projects/{project_id}/rename", post(rename))
        .with_state(state)
}

#[derive(Serialize)]
struct ProjectsResponse {
    projects: Vec<ProjectCatalogEntry>,
}

#[derive(Deserialize)]
struct RenameRequestBody {
    protocol_version: String,
    expected_lifecycle_generation: u64,
    expected_metadata_version: u64,
    name: String,
    client_request_id: String,
}

#[derive(Serialize)]
struct RenameResponse {
    protocol_version: &'static str,
    receipt: ProjectRenameReceipt,
}

async fn list(
    State(state): State<WorkspaceProjectsHttpState>,
    Path(workspace_id): Path<String>,
    headers: HeaderMap,
    jar: CookieJar,
) -> Result<Json<ProjectsResponse>, CatalogHttpError> {
    let principal = state.auth.authenticate_read_request(&headers, &jar).await?;
    let workspace = state
        .workspace
        .resolve(&principal.principal_id, &workspace_id)
        .await?;
    let now = i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| CatalogHttpError::Clock)?
            .as_millis(),
    )
    .map_err(|_| CatalogHttpError::Clock)?;
    let projects = state
        .projects
        .list_visible_active_projects(
            &principal.principal_id,
            &workspace.tenant_id,
            &workspace.workspace_id,
            now,
        )
        .await?;
    Ok(Json(ProjectsResponse { projects }))
}

async fn rename(
    State(state): State<WorkspaceProjectsHttpState>,
    Path(project_id): Path<String>,
    headers: HeaderMap,
    jar: CookieJar,
    Json(body): Json<RenameRequestBody>,
) -> Result<Json<RenameResponse>, CatalogHttpError> {
    if body.protocol_version != "chaptera.project-rename.v1" {
        return Err(CatalogHttpError::Persistence(IngressError::new(
            "project_rename_protocol_invalid",
            "project rename protocol version is unsupported",
        )));
    }

    let principal = state
        .auth
        .authenticate_mutation_request(&headers, &jar)
        .await?;
    let identity = state
        .projects
        .project_lifecycle_identity(&project_id)
        .await?;
    let workspace = state
        .workspace
        .resolve(&principal.principal_id, &identity.workspace_id)
        .await?;
    if workspace.tenant_id != identity.tenant_id {
        return Err(CatalogHttpError::Persistence(IngressError::new(
            "project_workspace_tenant_mismatch",
            "project identity does not match resolved workspace tenant",
        )));
    }

    let now = i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| CatalogHttpError::Clock)?
            .as_millis(),
    )
    .map_err(|_| CatalogHttpError::Clock)?;

    state
        .authz
        .authorize(
            &identity.tenant_id,
            &identity.document_id,
            &principal.principal_id,
            CAP_MEMBER_MANAGE,
            &body.client_request_id,
            now,
        )
        .await?;

    let receipt = state
        .projects
        .rename_project(RenameProjectRequest {
            tenant_id: identity.tenant_id,
            project_id: identity.project_id,
            expected_lifecycle_generation: body.expected_lifecycle_generation,
            expected_metadata_version: body.expected_metadata_version,
            name: body.name,
            client_request_id: body.client_request_id,
            now_ms: now,
        })
        .await?;

    Ok(Json(RenameResponse {
        protocol_version: "chaptera.project-rename-receipt.v1",
        receipt,
    }))
}

enum CatalogHttpError {
    Auth(AuthHttpError),
    Workspace(WorkspaceContextError),
    Authz(AuthzError),
    Persistence(IngressError),
    Clock,
}

impl From<AuthHttpError> for CatalogHttpError {
    fn from(value: AuthHttpError) -> Self {
        Self::Auth(value)
    }
}

impl From<WorkspaceContextError> for CatalogHttpError {
    fn from(value: WorkspaceContextError) -> Self {
        Self::Workspace(value)
    }
}

impl From<AuthzError> for CatalogHttpError {
    fn from(value: AuthzError) -> Self {
        Self::Authz(value)
    }
}

impl From<IngressError> for CatalogHttpError {
    fn from(value: IngressError) -> Self {
        Self::Persistence(value)
    }
}

impl IntoResponse for CatalogHttpError {
    fn into_response(self) -> Response {
        match self {
            Self::Auth(error) => error.into_response(),
            Self::Workspace(error) => {
                let code = error.code;
                let status = match code {
                    "workspace_membership_denied"
                    | "workspace_principal_inactive"
                    | "personal_workspace_unavailable" => StatusCode::FORBIDDEN,
                    "workspace_context_identity_invalid" => StatusCode::BAD_REQUEST,
                    _ => StatusCode::INTERNAL_SERVER_ERROR,
                };
                (status, Json(json!({ "error": code }))).into_response()
            }
            Self::Authz(error) => {
                let code = error.code;
                let status = match code {
                    "grant_missing" | "grant_expired" | "capability_denied" => {
                        StatusCode::FORBIDDEN
                    }
                    "invalid_identity" | "unknown_capability" | "invalid_now" => {
                        StatusCode::BAD_REQUEST
                    }
                    _ => StatusCode::INTERNAL_SERVER_ERROR,
                };
                (status, Json(json!({ "error": code }))).into_response()
            }
            Self::Persistence(error) => {
                let code = error.code;
                let status = match code {
                    "project_catalog_pagination_required"
                    | "stale_project_lifecycle_generation"
                    | "stale_project_metadata_version"
                    | "project_not_active"
                    | "idempotency_conflict" => StatusCode::CONFLICT,
                    "project_not_found" => StatusCode::NOT_FOUND,
                    "invalid_identifier"
                    | "invalid_project_name"
                    | "invalid_now"
                    | "project_version_out_of_range"
                    | "project_rename_protocol_invalid" => StatusCode::BAD_REQUEST,
                    _ => StatusCode::INTERNAL_SERVER_ERROR,
                };
                (status, Json(json!({ "error": code }))).into_response()
            }
            Self::Clock => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "project_catalog_clock_invalid" })),
            )
                .into_response(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        sync::atomic::{AtomicU64, Ordering},
        time::Duration,
    };

    use axum::{
        body::{Body, to_bytes},
        http::{
            Request,
            header::{CONTENT_TYPE, COOKIE, HOST, ORIGIN},
        },
    };
    use sqlx::SqlitePool;
    use tower::ServiceExt;

    use crate::{
        auth_http::{CSRF_HEADER, SESSION_COOKIE},
        authn::SqliteAuthnStore,
        authn_session::{SessionPolicy, issue_verified_login_session},
        authz_runtime::{DocumentRole, SqliteAuthzAuthority},
        oidc_authn::OidcVerifiedIdentity,
        schema_migration::SqliteMigrationRuntime,
    };

    use super::*;

    static NEXT: AtomicU64 = AtomicU64::new(1);

    fn request(workspace_id: &str, session: Option<&str>) -> Request<Body> {
        let mut builder = Request::builder()
            .uri(format!("/v1/workspaces/{workspace_id}/projects"))
            .header(HOST, "cloud.example.test");
        if let Some(token) = session {
            builder = builder.header(COOKIE, format!("{SESSION_COOKIE}={token}"));
        }
        builder.body(Body::empty()).unwrap()
    }

    async fn parsed(response: Response) -> serde_json::Value {
        let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    fn rename_request(
        project_id: &str,
        session: &str,
        csrf: &str,
        expected_metadata_version: u64,
        name: &str,
        request_id: &str,
    ) -> Request<Body> {
        Request::builder()
            .method("POST")
            .uri(format!("/v1/projects/{project_id}/rename"))
            .header(HOST, "cloud.example.test")
            .header(ORIGIN, "https://cloud.example.test")
            .header(COOKIE, format!("{SESSION_COOKIE}={session}"))
            .header(CSRF_HEADER, csrf)
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(
                serde_json::to_vec(&json!({
                    "protocol_version": "chaptera.project-rename.v1",
                    "expected_lifecycle_generation": 0,
                    "expected_metadata_version": expected_metadata_version,
                    "name": name,
                    "client_request_id": request_id,
                }))
                .unwrap(),
            ))
            .unwrap()
    }

    #[tokio::test]
    async fn catalog_is_session_member_and_document_grant_scoped() {
        let unique = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "chaptera-project-catalog-{}-{unique}.sqlite",
            std::process::id()
        ));
        SqliteMigrationRuntime::new(&path, Duration::from_secs(2))
            .unwrap()
            .migrate_up()
            .await
            .unwrap();
        let authn = SqliteAuthnStore::open(&path, 3, Duration::from_secs(2))
            .await
            .unwrap();
        let policy =
            SessionPolicy::new(Duration::from_secs(600), Duration::from_secs(3600)).unwrap();
        let now = i64::try_from(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis(),
        )
        .unwrap();
        let alice = issue_verified_login_session(
            &authn,
            OidcVerifiedIdentity {
                issuer: "https://issuer.example.test".into(),
                subject: "catalog-alice".into(),
                email_snapshot: None,
                return_path: "/".into(),
            },
            now,
            policy,
        )
        .await
        .unwrap();
        let bob = issue_verified_login_session(
            &authn,
            OidcVerifiedIdentity {
                issuer: "https://issuer.example.test".into(),
                subject: "catalog-bob".into(),
                email_snapshot: None,
                return_path: "/".into(),
            },
            now,
            policy,
        )
        .await
        .unwrap();
        let workspace = SqliteWorkspaceContextResolver::open(&path, 3, Duration::from_secs(2))
            .await
            .unwrap();
        let context = workspace
            .ensure_personal(&alice.principal_id, now)
            .await
            .unwrap();
        let other = workspace
            .ensure_personal(&bob.principal_id, now)
            .await
            .unwrap();
        assert_ne!(context.tenant_id, other.tenant_id);

        let pool = SqlitePool::connect(&format!("sqlite://{}", path.display()))
            .await
            .unwrap();
        let upload_id = b"upload:catalog:alice";
        let project_id = b"project:catalog:alice";
        let document_id = b"document:catalog:alice";
        let sha = b"a".repeat(64);
        let genesis_revision = format!("sha256:{}", "1".repeat(64));
        let edited_revision = format!("sha256:{}", "2".repeat(64));
        sqlx::query(
            r#"
            INSERT INTO uploads (
                upload_id, tenant_id, principal_id, purpose, expected_byte_len,
                physical_upload_ref, state, upload_generation, object_version,
                object_etag, observed_byte_len, canonical_sha256, durable_binding_id,
                created_at_ms, expires_at_ms, completed_at_ms,
                idempotency_key, request_hash
            ) VALUES (?, ?, ?, 'pub_source', 1, 'fixture/ref',
                      'CONSUMED', 2, 'v1', 'etag', 1, ?, 'binding:catalog',
                      ?, ?, ?, 'catalog-idempotency', ?)
            "#,
        )
        .bind(upload_id.as_slice())
        .bind(context.tenant_id.as_bytes())
        .bind(alice.principal_id.as_bytes())
        .bind(sha.as_slice())
        .bind(now)
        .bind(now + 86_400_000)
        .bind(now)
        .bind(b"b".repeat(64))
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            r#"
            INSERT INTO projects (
                project_id, tenant_id, workspace_id, name, lifecycle_state,
                lifecycle_generation, metadata_version, deleted, created_at_ms
            ) VALUES (?, ?, ?, 'Catalog Test.pub', 'active', 0, 0, 0, ?)
            "#,
        )
        .bind(project_id.as_slice())
        .bind(context.tenant_id.as_bytes())
        .bind(context.workspace_id.as_bytes())
        .bind(now)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            r#"
            INSERT INTO documents (
                document_id, tenant_id, project_id, source_upload_id,
                durable_binding_id, source_sha256, genesis_revision_id, created_at_ms
            ) VALUES (?, ?, ?, ?, 'binding:catalog', ?, ?, ?)
            "#,
        )
        .bind(document_id.as_slice())
        .bind(context.tenant_id.as_bytes())
        .bind(project_id.as_slice())
        .bind(upload_id.as_slice())
        .bind(sha.as_slice())
        .bind(genesis_revision.as_bytes())
        .bind(now)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO authz_documents (tenant_id, document_id, authz_version) VALUES (?, ?, 1)",
        )
        .bind(context.tenant_id.as_bytes())
        .bind(document_id.as_slice())
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            r#"
            INSERT INTO authz_principal_grants (
                tenant_id, document_id, principal_id, role, expires_at_ms, updated_at_ms
            ) VALUES (?, ?, ?, 'owner', NULL, ?)
            "#,
        )
        .bind(context.tenant_id.as_bytes())
        .bind(document_id.as_slice())
        .bind(alice.principal_id.as_bytes())
        .bind(now)
        .execute(&pool)
        .await
        .unwrap();

        let projects = SqliteProjectPersistence::open(&path, 3, Duration::from_secs(2))
            .await
            .unwrap();
        let authz = SqliteAuthzAuthority::open(&path, 3, Duration::from_secs(2))
            .await
            .unwrap();
        let auth =
            AuthHttpState::api_test(authn.clone(), policy, "https://cloud.example.test").unwrap();
        let app = router(WorkspaceProjectsHttpState::new(
            auth,
            workspace.clone(),
            projects.clone(),
            authz.clone(),
        ));
        let anonymous = app
            .clone()
            .oneshot(request(&context.workspace_id, None))
            .await
            .unwrap();
        assert_eq!(anonymous.status(), StatusCode::UNAUTHORIZED);
        let cross = app
            .clone()
            .oneshot(request(&context.workspace_id, Some(&bob.session_token)))
            .await
            .unwrap();
        assert_eq!(cross.status(), StatusCode::FORBIDDEN);
        let visible = app
            .clone()
            .oneshot(request(&context.workspace_id, Some(&alice.session_token)))
            .await
            .unwrap();
        assert_eq!(visible.status(), StatusCode::OK);
        let visible = parsed(visible).await;
        assert_eq!(
            visible["projects"][0]["project_id"],
            "project:catalog:alice"
        );
        assert_eq!(
            visible["projects"][0]["document_id"],
            "document:catalog:alice"
        );
        assert_eq!(visible["projects"][0]["name"], "Catalog Test.pub");
        assert_eq!(visible["projects"][0]["lifecycle_state"], "active");
        assert_eq!(visible["projects"][0]["lifecycle_generation"], 0);
        assert_eq!(visible["projects"][0]["metadata_version"], 0);
        assert_eq!(visible["projects"][0]["workspace_id"], context.workspace_id);
        assert_eq!(
            visible["projects"][0]["current_revision_id"],
            genesis_revision
        );
        assert!(visible.get("tenant_id").is_none());
        assert!(visible["projects"][0].get("principal_id").is_none());

        let renamed = app
            .clone()
            .oneshot(rename_request(
                "project:catalog:alice",
                &alice.session_token,
                &alice.csrf_token,
                0,
                "Renamed Catalog.pub",
                "rename-http-0001",
            ))
            .await
            .unwrap();
        assert_eq!(renamed.status(), StatusCode::OK);
        let renamed = parsed(renamed).await;
        assert_eq!(
            renamed["protocol_version"],
            "chaptera.project-rename-receipt.v1"
        );
        assert_eq!(renamed["receipt"]["name"], "Renamed Catalog.pub");
        assert_eq!(renamed["receipt"]["metadata_version"], 1);
        assert_eq!(renamed["receipt"]["replayed"], false);

        let replay = app
            .clone()
            .oneshot(rename_request(
                "project:catalog:alice",
                &alice.session_token,
                &alice.csrf_token,
                0,
                "Renamed Catalog.pub",
                "rename-http-0001",
            ))
            .await
            .unwrap();
        assert_eq!(replay.status(), StatusCode::OK);
        assert_eq!(parsed(replay).await["receipt"]["replayed"], true);

        let stale = app
            .clone()
            .oneshot(rename_request(
                "project:catalog:alice",
                &alice.session_token,
                &alice.csrf_token,
                0,
                "Stale rename",
                "rename-http-0002",
            ))
            .await
            .unwrap();
        assert_eq!(stale.status(), StatusCode::CONFLICT);
        assert_eq!(
            parsed(stale).await["error"],
            "stale_project_metadata_version"
        );

        sqlx::query(
            r#"
            INSERT INTO workspace_memberships (
                workspace_id, principal_id, role, membership_state,
                membership_version, created_at_ms, revoked_at_ms
            ) VALUES (?, ?, 'member', 'active', 0, ?, NULL)
            "#,
        )
        .bind(context.workspace_id.as_bytes())
        .bind(bob.principal_id.as_bytes())
        .bind(now)
        .execute(&pool)
        .await
        .unwrap();
        authz
            .set_role(
                &context.tenant_id,
                "document:catalog:alice",
                &bob.principal_id,
                DocumentRole::Viewer,
                None,
                "grant-catalog-viewer",
                now + 1,
            )
            .await
            .unwrap();

        let viewer_rename = app
            .clone()
            .oneshot(rename_request(
                "project:catalog:alice",
                &bob.session_token,
                &bob.csrf_token,
                1,
                "Viewer rename must fail",
                "rename-http-viewer",
            ))
            .await
            .unwrap();
        assert_eq!(viewer_rename.status(), StatusCode::FORBIDDEN);
        assert_eq!(parsed(viewer_rename).await["error"], "capability_denied");

        sqlx::query(
            r#"
            INSERT INTO revision_edges (
                document_id, parent_revision, parent_cursor, operation_id,
                request_hash, canonical_event, child_revision, child_cursor,
                resulting_state_hash, authoring_root_hash,
                semantic_schema_version, committed_at_ms
            ) VALUES (?, ?, 0, 'catalog-edit-1', ?, '{}', ?, 1, ?, NULL, 1, ?)
            "#,
        )
        .bind(document_id.as_slice())
        .bind(genesis_revision.as_bytes())
        .bind(b"c".repeat(64))
        .bind(edited_revision.as_bytes())
        .bind(b"d".repeat(64))
        .bind(now + 1)
        .execute(&pool)
        .await
        .unwrap();
        let edited = app
            .clone()
            .oneshot(request(&context.workspace_id, Some(&alice.session_token)))
            .await
            .unwrap();
        assert_eq!(edited.status(), StatusCode::OK);
        assert_eq!(
            parsed(edited).await["projects"][0]["current_revision_id"],
            edited_revision
        );

        sqlx::query("UPDATE authz_principal_grants SET expires_at_ms=? WHERE document_id=?")
            .bind(now - 1)
            .bind(document_id.as_slice())
            .execute(&pool)
            .await
            .unwrap();
        let expired = app
            .clone()
            .oneshot(request(&context.workspace_id, Some(&alice.session_token)))
            .await
            .unwrap();
        assert_eq!(expired.status(), StatusCode::OK);
        assert!(
            parsed(expired).await["projects"]
                .as_array()
                .unwrap()
                .is_empty()
        );

        sqlx::query("UPDATE authz_principal_grants SET expires_at_ms=NULL WHERE document_id=?")
            .bind(document_id.as_slice())
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("UPDATE projects SET lifecycle_state='trashed' WHERE project_id=?")
            .bind(project_id.as_slice())
            .execute(&pool)
            .await
            .unwrap();
        let trashed = app
            .clone()
            .oneshot(request(&context.workspace_id, Some(&alice.session_token)))
            .await
            .unwrap();
        assert!(
            parsed(trashed).await["projects"]
                .as_array()
                .unwrap()
                .is_empty()
        );

        sqlx::query("UPDATE projects SET lifecycle_state='active' WHERE project_id=?")
            .bind(project_id.as_slice())
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            r#"
            UPDATE workspace_memberships
            SET membership_state='revoked', membership_version=membership_version+1,
                revoked_at_ms=?
            WHERE workspace_id=? AND principal_id=?
            "#,
        )
        .bind(now + 1)
        .bind(context.workspace_id.as_bytes())
        .bind(alice.principal_id.as_bytes())
        .execute(&pool)
        .await
        .unwrap();
        let revoked = app
            .oneshot(request(&context.workspace_id, Some(&alice.session_token)))
            .await
            .unwrap();
        assert_eq!(revoked.status(), StatusCode::FORBIDDEN);

        pool.close().await;
        authz.close().await;
        projects.close().await;
        workspace.close().await;
        authn.close().await;
        for suffix in ["", "-wal", "-shm"] {
            let _ = fs::remove_file(format!("{}{suffix}", path.display()));
        }
    }
}
