use std::{
    fmt,
    path::{Path, PathBuf},
    time::Duration,
};

use serde::Serialize;
use sha2::{Digest, Sha256};
use sqlx::{
    Row, SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceContext {
    pub principal_id: String,
    pub workspace_id: String,
    pub tenant_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WorkspaceSummary {
    pub workspace_id: String,
    pub role: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceContextError {
    pub code: &'static str,
    pub message: String,
}

impl WorkspaceContextError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl fmt::Display for WorkspaceContextError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for WorkspaceContextError {}

#[derive(Clone)]
pub struct SqliteWorkspaceContextResolver {
    path: PathBuf,
    pool: SqlitePool,
}

impl SqliteWorkspaceContextResolver {
    pub async fn open(
        path: impl AsRef<Path>,
        max_connections: u32,
        busy_timeout: Duration,
    ) -> Result<Self, WorkspaceContextError> {
        if !(1..=16).contains(&max_connections) {
            return Err(WorkspaceContextError::new(
                "workspace_context_pool_size_invalid",
                "workspace context SQLite pool must use 1..=16 connections",
            ));
        }
        if busy_timeout.is_zero() || busy_timeout > Duration::from_secs(30) {
            return Err(WorkspaceContextError::new(
                "workspace_context_busy_timeout_invalid",
                "workspace context SQLite busy timeout must be >0 and <=30 seconds",
            ));
        }

        let path = path.as_ref().to_path_buf();
        if !path.exists() {
            return Err(WorkspaceContextError::new(
                "workspace_context_database_missing",
                "run chaptera migrate up before opening workspace context",
            ));
        }

        let options = SqliteConnectOptions::new()
            .filename(&path)
            .create_if_missing(false)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Full)
            .foreign_keys(true)
            .busy_timeout(busy_timeout);
        let pool = SqlitePoolOptions::new()
            .max_connections(max_connections)
            .min_connections(1)
            .connect_with(options)
            .await
            .map_err(sqlite_error)?;

        let resolver = Self { path, pool };
        resolver.require_schema().await?;
        Ok(resolver)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub async fn resolve(
        &self,
        principal_id: &str,
        workspace_id: &str,
    ) -> Result<WorkspaceContext, WorkspaceContextError> {
        require_ident(principal_id, "principal_id")?;
        require_ident(workspace_id, "workspace_id")?;

        let rows = sqlx::query(
            r#"
            SELECT
                wm.principal_id,
                wm.workspace_id,
                w.tenant_id
            FROM workspace_memberships AS wm
            JOIN workspaces AS w
              ON w.workspace_id = wm.workspace_id
            JOIN principals AS p
              ON p.principal_id = wm.principal_id
            WHERE wm.principal_id = ?
              AND wm.workspace_id = ?
              AND wm.membership_state = 'active'
              AND wm.revoked_at_ms IS NULL
              AND w.lifecycle_state = 'active'
              AND p.disabled_at_ms IS NULL
            LIMIT 2
            "#,
        )
        .bind(principal_id.as_bytes())
        .bind(workspace_id.as_bytes())
        .fetch_all(&self.pool)
        .await
        .map_err(sqlite_error)?;

        match rows.as_slice() {
            [] => Err(WorkspaceContextError::new(
                "workspace_membership_denied",
                "authenticated principal has no active membership in the selected workspace",
            )),
            [row] => Ok(WorkspaceContext {
                principal_id: blob_text(row, "principal_id")?,
                workspace_id: blob_text(row, "workspace_id")?,
                tenant_id: blob_text(row, "tenant_id")?,
            }),
            _ => Err(WorkspaceContextError::new(
                "workspace_membership_ambiguous",
                "workspace membership resolved to multiple authority rows",
            )),
        }
    }

    /// Only active memberships are visible, and tenant identity is intentionally
    /// never returned to the browser as a trusted input.
    pub async fn list_active(
        &self,
        principal_id: &str,
    ) -> Result<Vec<WorkspaceSummary>, WorkspaceContextError> {
        require_ident(principal_id, "principal_id")?;
        let rows = sqlx::query(
            r#"
            SELECT wm.workspace_id, wm.role
            FROM workspace_memberships AS wm
            JOIN workspaces AS w ON w.workspace_id = wm.workspace_id
            JOIN principals AS p ON p.principal_id = wm.principal_id
            WHERE wm.principal_id = ?
              AND wm.membership_state = 'active'
              AND wm.revoked_at_ms IS NULL
              AND w.lifecycle_state = 'active'
              AND p.disabled_at_ms IS NULL
            ORDER BY wm.workspace_id
            LIMIT 257
            "#,
        )
        .bind(principal_id.as_bytes())
        .fetch_all(&self.pool)
        .await
        .map_err(sqlite_error)?;

        if rows.len() > 256 {
            return Err(WorkspaceContextError::new(
                "workspace_list_limit_exceeded",
                "workspace listing requires pagination",
            ));
        }
        rows.iter()
            .map(|row| {
                let role: String = row.try_get("role").map_err(sqlite_error)?;
                if !matches!(role.as_str(), "owner" | "member") {
                    return Err(WorkspaceContextError::new(
                        "workspace_context_row_corrupt",
                        "workspace role is invalid",
                    ));
                }
                Ok(WorkspaceSummary {
                    workspace_id: blob_text(row, "workspace_id")?,
                    role,
                })
            })
            .collect()
    }

    /// Idempotent, authenticated personal workspace admission. The caller must
    /// already have a real AuthN principal; no tenant or owner is client-supplied.
    /// Revoked/deleted workspaces are NOT silently reactivated on repeat calls.
    pub async fn ensure_personal(
        &self,
        principal_id: &str,
        now_ms: i64,
    ) -> Result<WorkspaceContext, WorkspaceContextError> {
        require_ident(principal_id, "principal_id")?;
        if now_ms < 0 {
            return Err(WorkspaceContextError::new(
                "workspace_clock_invalid",
                "timestamp must be nonnegative",
            ));
        }
        let workspace_id = personal_identity("workspace", principal_id);
        let tenant_id = personal_identity("tenant", principal_id);
        let mut conn = self.pool.acquire().await.map_err(sqlite_error)?;
        sqlx::query("BEGIN IMMEDIATE")
            .execute(&mut *conn)
            .await
            .map_err(sqlite_error)?;

        let outcome = async {
            let active_principal: Option<i64> = sqlx::query_scalar(
                "SELECT 1 FROM principals WHERE principal_id = ? AND disabled_at_ms IS NULL",
            )
            .bind(principal_id.as_bytes())
            .fetch_optional(&mut *conn)
            .await
            .map_err(sqlite_error)?;
            if active_principal.is_none() {
                return Err(WorkspaceContextError::new(
                    "workspace_principal_inactive",
                    "personal workspace requires an active authenticated principal",
                ));
            }

            sqlx::query(
                r#"
                INSERT INTO workspaces (
                    workspace_id, tenant_id, lifecycle_state,
                    lifecycle_generation, metadata_version, created_at_ms
                ) VALUES (?, ?, 'active', 0, 0, ?)
                ON CONFLICT(workspace_id) DO NOTHING
                "#,
            )
            .bind(workspace_id.as_bytes())
            .bind(tenant_id.as_bytes())
            .bind(now_ms)
            .execute(&mut *conn)
            .await
            .map_err(sqlite_error)?;

            // The id is deterministic, but we still prove a collision cannot
            // bind it to another tenant or revive a previously deleted scope.
            let stored = sqlx::query(
                "SELECT tenant_id, lifecycle_state FROM workspaces WHERE workspace_id = ?",
            )
            .bind(workspace_id.as_bytes())
            .fetch_one(&mut *conn)
            .await
            .map_err(sqlite_error)?;
            let stored_tenant: Vec<u8> = stored.try_get("tenant_id").map_err(sqlite_error)?;
            let lifecycle: String = stored.try_get("lifecycle_state").map_err(sqlite_error)?;
            if stored_tenant != tenant_id.as_bytes() || lifecycle != "active" {
                return Err(WorkspaceContextError::new(
                    "personal_workspace_unavailable",
                    "personal workspace is not active under the expected tenant",
                ));
            }

            sqlx::query(
                r#"
                INSERT INTO workspace_memberships (
                    workspace_id, principal_id, role, membership_state,
                    membership_version, created_at_ms, revoked_at_ms
                ) VALUES (?, ?, 'owner', 'active', 0, ?, NULL)
                ON CONFLICT(workspace_id, principal_id) DO NOTHING
                "#,
            )
            .bind(workspace_id.as_bytes())
            .bind(principal_id.as_bytes())
            .bind(now_ms)
            .execute(&mut *conn)
            .await
            .map_err(sqlite_error)?;

            let valid: Option<i64> = sqlx::query_scalar(
                r#"
                SELECT 1 FROM workspace_memberships
                WHERE workspace_id = ? AND principal_id = ?
                  AND role = 'owner' AND membership_state = 'active'
                  AND revoked_at_ms IS NULL
                "#,
            )
            .bind(workspace_id.as_bytes())
            .bind(principal_id.as_bytes())
            .fetch_optional(&mut *conn)
            .await
            .map_err(sqlite_error)?;
            if valid.is_none() {
                return Err(WorkspaceContextError::new(
                    "personal_workspace_unavailable",
                    "personal workspace owner membership is inactive",
                ));
            }

            Ok(WorkspaceContext {
                principal_id: principal_id.to_owned(),
                workspace_id,
                tenant_id,
            })
        }
        .await;

        match outcome {
            Ok(context) => {
                sqlx::query("COMMIT")
                    .execute(&mut *conn)
                    .await
                    .map_err(sqlite_error)?;
                Ok(context)
            }
            Err(error) => {
                let _ = sqlx::query("ROLLBACK").execute(&mut *conn).await;
                Err(error)
            }
        }
    }

    pub async fn close(&self) {
        self.pool.close().await;
    }

    async fn require_schema(&self) -> Result<(), WorkspaceContextError> {
        for table in ["principals", "workspaces", "workspace_memberships"] {
            let count: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?",
            )
            .bind(table)
            .fetch_one(&self.pool)
            .await
            .map_err(sqlite_error)?;
            if count != 1 {
                return Err(WorkspaceContextError::new(
                    "workspace_context_schema_missing",
                    format!("{table} table is absent; run chaptera migrate up"),
                ));
            }
        }
        Ok(())
    }
}

fn personal_identity(kind: &str, principal_id: &str) -> String {
    let hash = Sha256::digest(format!("chaptera.personal.{kind}.v1:{principal_id}").as_bytes());
    format!("{kind}:personal:{hash:x}")
}

fn require_ident(value: &str, label: &str) -> Result<(), WorkspaceContextError> {
    if value.is_empty()
        || value.len() > 256
        || value
            .chars()
            .any(|ch| ch.is_control() || ch.is_whitespace())
    {
        return Err(WorkspaceContextError::new(
            "workspace_context_identity_invalid",
            format!("{label} must be a bounded non-whitespace identity"),
        ));
    }
    Ok(())
}

fn blob_text(row: &sqlx::sqlite::SqliteRow, column: &str) -> Result<String, WorkspaceContextError> {
    let bytes: Vec<u8> = row.try_get(column).map_err(sqlite_error)?;
    String::from_utf8(bytes).map_err(|_| {
        WorkspaceContextError::new(
            "workspace_context_row_corrupt",
            format!("{column} is not valid UTF-8"),
        )
    })
}

fn sqlite_error(error: impl fmt::Display) -> WorkspaceContextError {
    WorkspaceContextError::new(
        "workspace_context_sqlite_error",
        error.to_string().chars().take(512).collect::<String>(),
    )
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };

    use crate::schema_migration::SqliteMigrationRuntime;

    use super::*;

    static NEXT: AtomicU64 = AtomicU64::new(1);

    fn temp_db(label: &str) -> PathBuf {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "chaptera-workspace-context-{label}-{}-{n}.sqlite",
            std::process::id()
        ))
    }

    fn cleanup(path: &Path) {
        for suffix in ["", "-wal", "-shm"] {
            let _ = fs::remove_file(format!("{}{}", path.display(), suffix));
        }
    }

    async fn setup(label: &str) -> (PathBuf, SqliteWorkspaceContextResolver) {
        let path = temp_db(label);
        SqliteMigrationRuntime::new(&path, Duration::from_secs(2))
            .unwrap()
            .migrate_up()
            .await
            .unwrap();

        sqlx::query(
            "INSERT INTO principals (principal_id, created_at_ms, disabled_at_ms) VALUES (?, 1, NULL)",
        )
        .bind(b"principal-a".as_slice())
        .execute(
            &SqlitePoolOptions::new()
                .max_connections(1)
                .connect_with(
                    SqliteConnectOptions::new()
                        .filename(&path)
                        .create_if_missing(false)
                        .foreign_keys(true),
                )
                .await
                .unwrap(),
        )
        .await
        .unwrap();

        let resolver = SqliteWorkspaceContextResolver::open(&path, 2, Duration::from_secs(2))
            .await
            .unwrap();
        (path, resolver)
    }

    async fn insert_workspace(
        resolver: &SqliteWorkspaceContextResolver,
        workspace_id: &str,
        tenant_id: &str,
        principal_id: &str,
    ) {
        sqlx::query(
            r#"
            INSERT INTO workspaces (
                workspace_id, tenant_id, lifecycle_state,
                lifecycle_generation, metadata_version, created_at_ms
            ) VALUES (?, ?, 'active', 0, 0, 10)
            "#,
        )
        .bind(workspace_id.as_bytes())
        .bind(tenant_id.as_bytes())
        .execute(&resolver.pool)
        .await
        .unwrap();

        sqlx::query(
            r#"
            INSERT INTO workspace_memberships (
                workspace_id, principal_id, role, membership_state,
                membership_version, created_at_ms, revoked_at_ms
            ) VALUES (?, ?, 'owner', 'active', 0, 11, NULL)
            "#,
        )
        .bind(workspace_id.as_bytes())
        .bind(principal_id.as_bytes())
        .execute(&resolver.pool)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn personal_workspace_provision_is_idempotent_and_survives_restart() {
        let (path, resolver) = setup("personal-idempotent").await;
        let first = resolver.ensure_personal("principal-a", 100).await.unwrap();
        let again = resolver.ensure_personal("principal-a", 200).await.unwrap();
        assert_eq!(first, again);
        assert_ne!(first.workspace_id, first.tenant_id);
        assert_eq!(
            resolver
                .resolve("principal-a", &first.workspace_id)
                .await
                .unwrap(),
            first
        );
        assert_eq!(
            resolver.list_active("principal-a").await.unwrap(),
            vec![WorkspaceSummary {
                workspace_id: first.workspace_id.clone(),
                role: "owner".to_owned(),
            }]
        );

        resolver.close().await;
        let reopened = SqliteWorkspaceContextResolver::open(&path, 2, Duration::from_secs(2))
            .await
            .unwrap();
        assert_eq!(
            reopened.ensure_personal("principal-a", 300).await.unwrap(),
            first
        );
        reopened.close().await;
        cleanup(&path);
    }

    #[tokio::test]
    async fn personal_workspaces_are_isolated_by_authenticated_principal() {
        let (path, resolver) = setup("personal-isolation").await;
        sqlx::query(
            "INSERT INTO principals (principal_id, created_at_ms, disabled_at_ms) VALUES (?, 1, NULL)",
        )
        .bind(b"principal-b".as_slice())
        .execute(&resolver.pool)
        .await
        .unwrap();

        let a = resolver.ensure_personal("principal-a", 100).await.unwrap();
        let b = resolver.ensure_personal("principal-b", 100).await.unwrap();
        assert_ne!(a.workspace_id, b.workspace_id);
        assert_ne!(a.tenant_id, b.tenant_id);
        assert_eq!(
            resolver
                .resolve("principal-a", &b.workspace_id)
                .await
                .unwrap_err()
                .code,
            "workspace_membership_denied"
        );
        assert_eq!(resolver.list_active("principal-a").await.unwrap().len(), 1);
        assert_eq!(resolver.list_active("principal-b").await.unwrap().len(), 1);
        resolver.close().await;
        cleanup(&path);
    }

    #[tokio::test]
    async fn personal_provision_never_revives_revoked_or_deleted_workspace() {
        let (path, resolver) = setup("personal-revoke").await;
        let context = resolver.ensure_personal("principal-a", 100).await.unwrap();
        sqlx::query(
            "UPDATE workspace_memberships SET membership_state='revoked', membership_version=1, revoked_at_ms=200 WHERE workspace_id=? AND principal_id=?",
        )
        .bind(context.workspace_id.as_bytes())
        .bind(b"principal-a".as_slice())
        .execute(&resolver.pool)
        .await
        .unwrap();
        assert_eq!(
            resolver
                .ensure_personal("principal-a", 300)
                .await
                .unwrap_err()
                .code,
            "personal_workspace_unavailable"
        );
        assert!(
            resolver
                .list_active("principal-a")
                .await
                .unwrap()
                .is_empty()
        );

        sqlx::query("UPDATE workspace_memberships SET membership_state='active', revoked_at_ms=NULL WHERE workspace_id=?")
            .bind(context.workspace_id.as_bytes())
            .execute(&resolver.pool)
            .await
            .unwrap();
        sqlx::query("UPDATE workspaces SET lifecycle_state='deleted' WHERE workspace_id=?")
            .bind(context.workspace_id.as_bytes())
            .execute(&resolver.pool)
            .await
            .unwrap();
        assert_eq!(
            resolver
                .ensure_personal("principal-a", 400)
                .await
                .unwrap_err()
                .code,
            "personal_workspace_unavailable"
        );
        assert!(
            resolver
                .list_active("principal-a")
                .await
                .unwrap()
                .is_empty()
        );
        resolver.close().await;
        cleanup(&path);
    }

    #[tokio::test]
    async fn active_membership_resolves_authoritative_tenant() {
        let (path, resolver) = setup("active").await;
        insert_workspace(&resolver, "workspace-a", "tenant-a", "principal-a").await;

        let context = resolver
            .resolve("principal-a", "workspace-a")
            .await
            .unwrap();
        assert_eq!(
            context,
            WorkspaceContext {
                principal_id: "principal-a".into(),
                workspace_id: "workspace-a".into(),
                tenant_id: "tenant-a".into(),
            }
        );

        resolver.close().await;
        cleanup(&path);
    }

    #[tokio::test]
    async fn client_cannot_substitute_tenant_because_resolver_has_no_tenant_input() {
        let (path, resolver) = setup("tenant").await;
        insert_workspace(
            &resolver,
            "workspace-a",
            "tenant-authoritative",
            "principal-a",
        )
        .await;

        let context = resolver
            .resolve("principal-a", "workspace-a")
            .await
            .unwrap();
        assert_eq!(context.tenant_id, "tenant-authoritative");

        resolver.close().await;
        cleanup(&path);
    }

    #[tokio::test]
    async fn missing_or_revoked_membership_fails_closed() {
        let (path, resolver) = setup("revoke").await;
        insert_workspace(&resolver, "workspace-a", "tenant-a", "principal-a").await;

        assert_eq!(
            resolver
                .resolve("principal-a", "missing-workspace")
                .await
                .unwrap_err()
                .code,
            "workspace_membership_denied"
        );

        sqlx::query(
            "UPDATE workspace_memberships SET membership_state='revoked', membership_version=1, revoked_at_ms=20 WHERE workspace_id=? AND principal_id=?",
        )
        .bind(b"workspace-a".as_slice())
        .bind(b"principal-a".as_slice())
        .execute(&resolver.pool)
        .await
        .unwrap();

        assert_eq!(
            resolver
                .resolve("principal-a", "workspace-a")
                .await
                .unwrap_err()
                .code,
            "workspace_membership_denied"
        );

        resolver.close().await;
        cleanup(&path);
    }

    #[tokio::test]
    async fn disabled_principal_and_deleted_workspace_fail_closed() {
        let (path, resolver) = setup("disabled").await;
        insert_workspace(&resolver, "workspace-a", "tenant-a", "principal-a").await;

        sqlx::query("UPDATE principals SET disabled_at_ms=30 WHERE principal_id=?")
            .bind(b"principal-a".as_slice())
            .execute(&resolver.pool)
            .await
            .unwrap();
        assert_eq!(
            resolver
                .resolve("principal-a", "workspace-a")
                .await
                .unwrap_err()
                .code,
            "workspace_membership_denied"
        );

        sqlx::query("UPDATE principals SET disabled_at_ms=NULL WHERE principal_id=?")
            .bind(b"principal-a".as_slice())
            .execute(&resolver.pool)
            .await
            .unwrap();
        sqlx::query("UPDATE workspaces SET lifecycle_state='deleted', lifecycle_generation=1 WHERE workspace_id=?")
            .bind(b"workspace-a".as_slice())
            .execute(&resolver.pool)
            .await
            .unwrap();
        assert_eq!(
            resolver
                .resolve("principal-a", "workspace-a")
                .await
                .unwrap_err()
                .code,
            "workspace_membership_denied"
        );

        resolver.close().await;
        cleanup(&path);
    }

    #[tokio::test]
    async fn restart_reopens_same_workspace_tenant_binding() {
        let (path, resolver) = setup("restart").await;
        insert_workspace(&resolver, "workspace-a", "tenant-a", "principal-a").await;
        resolver.close().await;

        let reopened = SqliteWorkspaceContextResolver::open(&path, 2, Duration::from_secs(2))
            .await
            .unwrap();
        assert_eq!(
            reopened
                .resolve("principal-a", "workspace-a")
                .await
                .unwrap()
                .tenant_id,
            "tenant-a"
        );

        reopened.close().await;
        cleanup(&path);
    }
}
