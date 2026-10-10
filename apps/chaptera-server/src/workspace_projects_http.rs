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
    routing::get,
};
use axum_extra::extract::cookie::CookieJar;
use serde::Serialize;
use serde_json::json;

use crate::{
    auth_http::{AuthHttpError, AuthHttpState},
    project_persistence_sqlite::{ProjectCatalogEntry, SqliteProjectPersistence},
    source_ingress::IngressError,
    workspace_context::{SqliteWorkspaceContextResolver, WorkspaceContextError},
};

#[derive(Clone)]
pub struct WorkspaceProjectsHttpState {
    auth: AuthHttpState,
    workspace: SqliteWorkspaceContextResolver,
    projects: SqliteProjectPersistence,
}

impl WorkspaceProjectsHttpState {
    pub fn new(
        auth: AuthHttpState,
        workspace: SqliteWorkspaceContextResolver,
        projects: SqliteProjectPersistence,
    ) -> Self {
        Self {
            auth,
            workspace,
            projects,
        }
    }
}

pub fn router(state: WorkspaceProjectsHttpState) -> Router {
    Router::new()
        .route("/v1/workspaces/{workspace_id}/projects", get(list))
        .with_state(state)
}

#[derive(Serialize)]
struct ProjectsResponse {
    projects: Vec<ProjectCatalogEntry>,
}

async fn list(
    State(state): State<WorkspaceProjectsHttpState>,
    Path(workspace_id): Path<String>,
    headers: HeaderMap,
    jar: CookieJar,
) -> Result<Json<ProjectsResponse>, CatalogHttpError> {
    let principal = state
        .auth
        .authenticate_read_request(&headers, &jar)
        .await?;
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

enum CatalogHttpError {
    Auth(AuthHttpError),
    Workspace(WorkspaceContextError),
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
            Self::Persistence(error) => {
                let code = error.code;
                let status = match code {
                    "project_catalog_pagination_required" => StatusCode::CONFLICT,
                    "invalid_identifier" => StatusCode::BAD_REQUEST,
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
