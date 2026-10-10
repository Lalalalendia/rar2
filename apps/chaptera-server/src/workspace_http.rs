use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode},
    routing::{get, post},
};
use axum_extra::extract::cookie::CookieJar;
use serde::Serialize;
use serde_json::json;

use crate::{
    auth_http::{AuthHttpError, AuthHttpState},
    workspace_context::{SqliteWorkspaceContextResolver, WorkspaceContextError, WorkspaceSummary},
};

#[derive(Clone)]
pub struct WorkspaceHttpState {
    auth: AuthHttpState,
    workspace: SqliteWorkspaceContextResolver,
}

impl WorkspaceHttpState {
    pub fn new(auth: AuthHttpState, workspace: SqliteWorkspaceContextResolver) -> Self {
        Self { auth, workspace }
    }
}

pub fn router(state: WorkspaceHttpState) -> Router {
    Router::new()
        .route("/v1/workspaces", get(list))
        .route("/v1/workspaces/personal", post(ensure_personal))
        .with_state(state)
}

#[derive(Serialize)]
struct WorkspaceListResponse {
    workspaces: Vec<WorkspaceSummary>,
}

#[derive(Serialize)]
struct PersonalWorkspaceResponse {
    workspace_id: String,
    role: &'static str,
}

async fn list(
    State(state): State<WorkspaceHttpState>,
    headers: HeaderMap,
    jar: CookieJar,
) -> Result<Json<WorkspaceListResponse>, WorkspaceHttpError> {
    let principal = state.auth.authenticate_read_request(&headers, &jar).await?;
    let workspaces = state.workspace.list_active(&principal.principal_id).await?;
    Ok(Json(WorkspaceListResponse { workspaces }))
}

async fn ensure_personal(
    State(state): State<WorkspaceHttpState>,
    headers: HeaderMap,
    jar: CookieJar,
) -> Result<Json<PersonalWorkspaceResponse>, WorkspaceHttpError> {
    let principal = state
        .auth
        .authenticate_mutation_request(&headers, &jar)
        .await?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| WorkspaceHttpError::Clock)?
        .as_millis();
    let now = i64::try_from(now).map_err(|_| WorkspaceHttpError::Clock)?;
    let workspace = state
        .workspace
        .ensure_personal(&principal.principal_id, now)
        .await?;
    Ok(Json(PersonalWorkspaceResponse {
        workspace_id: workspace.workspace_id,
        role: "owner",
    }))
}

enum WorkspaceHttpError {
    Auth(AuthHttpError),
    Workspace(WorkspaceContextError),
    Clock,
}

impl From<AuthHttpError> for WorkspaceHttpError {
    fn from(value: AuthHttpError) -> Self {
        Self::Auth(value)
    }
}

impl From<WorkspaceContextError> for WorkspaceHttpError {
    fn from(value: WorkspaceContextError) -> Self {
        Self::Workspace(value)
    }
}

impl axum::response::IntoResponse for WorkspaceHttpError {
    fn into_response(self) -> axum::response::Response {
        use axum::response::IntoResponse;
        match self {
            Self::Auth(error) => error.into_response(),
            Self::Workspace(error) => {
                let status = match error.code {
                    "workspace_principal_inactive" | "personal_workspace_unavailable" => {
                        StatusCode::FORBIDDEN
                    }
                    "workspace_list_limit_exceeded" => StatusCode::CONFLICT,
                    _ => StatusCode::INTERNAL_SERVER_ERROR,
                };
                (status, Json(json!({ "error": error.code }))).into_response()
            }
            Self::Clock => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "workspace_clock_invalid" })),
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
        time::{Duration, SystemTime, UNIX_EPOCH},
    };

    use axum::{
        body::{Body, to_bytes},
        http::{
            Request,
            header::{COOKIE, HOST, ORIGIN},
        },
    };
    use tower::ServiceExt;

    use crate::{
        auth_http::{CSRF_HEADER, SESSION_COOKIE},
        authn::SqliteAuthnStore,
        authn_session::{SessionPolicy, issue_verified_login_session},
        oidc_authn::OidcVerifiedIdentity,
        schema_migration::SqliteMigrationRuntime,
    };

    use super::*;

    static NEXT: AtomicU64 = AtomicU64::new(1);

    fn request(
        method: &str,
        uri: &str,
        cookie: Option<&str>,
        csrf: Option<&str>,
        asserted_principal: Option<&str>,
    ) -> Request<Body> {
        let mut builder = Request::builder()
            .method(method)
            .uri(uri)
            .header(HOST, "cloud.example.test");
        if method != "GET" {
            builder = builder.header(ORIGIN, "https://cloud.example.test");
        }
        if let Some(cookie) = cookie {
            builder = builder.header(COOKIE, format!("{SESSION_COOKIE}={cookie}"));
        }
        if let Some(csrf) = csrf {
            builder = builder.header(CSRF_HEADER, csrf);
        }
        if let Some(asserted_principal) = asserted_principal {
            builder = builder.header("x-chaptera-principal-id", asserted_principal);
        }
        builder.body(Body::empty()).unwrap()
    }

    async fn response_json(response: axum::response::Response) -> serde_json::Value {
        let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn personal_http_entry_is_authenticated_csrf_checked_and_tenant_isolated() {
        let sequence = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "chaptera-workspace-http-{}-{sequence}.sqlite",
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
        let a = issue_verified_login_session(
            &authn,
            OidcVerifiedIdentity {
                issuer: "https://issuer.example.test".to_owned(),
                subject: "workspace-http-a".to_owned(),
                email_snapshot: None,
                return_path: "/".to_owned(),
            },
            now,
            policy,
        )
        .await
        .unwrap();
        let b = issue_verified_login_session(
            &authn,
            OidcVerifiedIdentity {
                issuer: "https://issuer.example.test".to_owned(),
                subject: "workspace-http-b".to_owned(),
                email_snapshot: None,
                return_path: "/".to_owned(),
            },
            now,
            policy,
        )
        .await
        .unwrap();

        let auth =
            AuthHttpState::api_test(authn.clone(), policy, "https://cloud.example.test").unwrap();
        let workspace = SqliteWorkspaceContextResolver::open(&path, 3, Duration::from_secs(2))
            .await
            .unwrap();
        let app = router(WorkspaceHttpState::new(auth, workspace.clone()));

        let anonymous = app
            .clone()
            .oneshot(request("GET", "/v1/workspaces", None, None, None))
            .await
            .unwrap();
        assert_eq!(anonymous.status(), StatusCode::UNAUTHORIZED);

        let missing_csrf = app
            .clone()
            .oneshot(request(
                "POST",
                "/v1/workspaces/personal",
                Some(&a.session_token),
                None,
                None,
            ))
            .await
            .unwrap();
        assert_eq!(missing_csrf.status(), StatusCode::FORBIDDEN);

        let first = app
            .clone()
            .oneshot(request(
                "POST",
                "/v1/workspaces/personal",
                Some(&a.session_token),
                Some(&a.csrf_token),
                Some(&b.principal_id),
            ))
            .await
            .unwrap();
        assert_eq!(first.status(), StatusCode::OK);
        let first = response_json(first).await;
        let a_workspace = first["workspace_id"].as_str().unwrap().to_owned();
        assert_eq!(first["role"], "owner");

        let repeated = app
            .clone()
            .oneshot(request(
                "POST",
                "/v1/workspaces/personal",
                Some(&a.session_token),
                Some(&a.csrf_token),
                None,
            ))
            .await
            .unwrap();
        assert_eq!(repeated.status(), StatusCode::OK);
        assert_eq!(response_json(repeated).await["workspace_id"], a_workspace);

        let second = app
            .clone()
            .oneshot(request(
                "POST",
                "/v1/workspaces/personal",
                Some(&b.session_token),
                Some(&b.csrf_token),
                None,
            ))
            .await
            .unwrap();
        assert_eq!(second.status(), StatusCode::OK);
        let second = response_json(second).await;
        let b_workspace = second["workspace_id"].as_str().unwrap();
        assert_ne!(a_workspace, b_workspace);
        assert_eq!(
            workspace
                .resolve(&a.principal_id, b_workspace)
                .await
                .unwrap_err()
                .code,
            "workspace_membership_denied"
        );

        let listed = app
            .clone()
            .oneshot(request(
                "GET",
                "/v1/workspaces",
                Some(&a.session_token),
                None,
                Some(&b.principal_id),
            ))
            .await
            .unwrap();
        assert_eq!(listed.status(), StatusCode::OK);
        let listed = response_json(listed).await;
        let workspaces = listed["workspaces"].as_array().unwrap();
        assert_eq!(workspaces.len(), 1);
        assert_eq!(workspaces[0]["workspace_id"], a_workspace);
        assert_eq!(workspaces[0]["role"], "owner");
        assert!(listed.get("tenant_id").is_none());

        drop(app);
        workspace.close().await;
        authn.close().await;
        for suffix in ["", "-wal", "-shm"] {
            let _ = fs::remove_file(format!("{}{suffix}", path.display()));
        }
    }
}
