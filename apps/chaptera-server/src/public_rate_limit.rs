use std::{
    fmt,
    net::IpAddr,
    path::{Path, PathBuf},
    time::Duration,
};

use sha2::{Digest, Sha256};
use sqlx::{
    Connection, Row, SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
};

const SUBJECT_DOMAIN: &[u8] = b"chaptera.public-rate-limit.subject.v1\0";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublicRateClass {
    ReaderSessionCreate,
    ReaderSessionUpload,
    ReaderSessionOpen,
    PublicMetadata,
}

impl PublicRateClass {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ReaderSessionCreate => "reader_session_create",
            Self::ReaderSessionUpload => "reader_session_upload",
            Self::ReaderSessionOpen => "reader_session_open",
            Self::PublicMetadata => "public_metadata",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicRatePolicy {
    pub requests_per_window: u32,
    pub window: Duration,
    pub burst: u32,
}

impl PublicRatePolicy {
    fn validate(&self, name: &str) -> Result<(), PublicRateLimitError> {
        if self.requests_per_window == 0 || self.requests_per_window > 100_000 {
            return Err(PublicRateLimitError::new(
                "public_rate_policy_invalid",
                format!("{name}.requests_per_window must be 1..=100000"),
            ));
        }
        if self.window.is_zero() || self.window > Duration::from_secs(24 * 60 * 60) {
            return Err(PublicRateLimitError::new(
                "public_rate_policy_invalid",
                format!("{name}.window must be >0 and <=24 hours"),
            ));
        }
        if self.burst == 0 || self.burst > self.requests_per_window {
            return Err(PublicRateLimitError::new(
                "public_rate_policy_invalid",
                format!("{name}.burst must be 1..=requests_per_window"),
            ));
        }
        self.interval_us()?;
        Ok(())
    }

    fn interval_us(&self) -> Result<i64, PublicRateLimitError> {
        let window_us = i64::try_from(self.window.as_micros()).map_err(|_| {
            PublicRateLimitError::new(
                "public_rate_policy_invalid",
                "rate-limit window does not fit i64 microseconds",
            )
        })?;
        let requests = i64::from(self.requests_per_window);
        let interval = window_us
            .checked_add(requests - 1)
            .and_then(|value| value.checked_div(requests))
            .ok_or_else(|| {
                PublicRateLimitError::new(
                    "public_rate_policy_invalid",
                    "rate-limit interval calculation overflowed",
                )
            })?;
        if interval <= 0 {
            return Err(PublicRateLimitError::new(
                "public_rate_policy_invalid",
                "rate-limit interval must be positive",
            ));
        }
        Ok(interval)
    }

    fn burst_tolerance_us(&self) -> Result<i64, PublicRateLimitError> {
        self.interval_us()?
            .checked_mul(i64::from(self.burst.saturating_sub(1)))
            .ok_or_else(|| {
                PublicRateLimitError::new(
                    "public_rate_policy_invalid",
                    "rate-limit burst calculation overflowed",
                )
            })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicRateLimitConfig {
    pub reader_session_create: PublicRatePolicy,
    pub reader_session_upload: PublicRatePolicy,
    pub reader_session_open: PublicRatePolicy,
    pub public_metadata: PublicRatePolicy,
    pub retention: Duration,
    pub max_entries: i64,
}

impl PublicRateLimitConfig {
    pub fn validate(&self) -> Result<(), PublicRateLimitError> {
        self.reader_session_create
            .validate("reader_session_create")?;
        self.reader_session_upload
            .validate("reader_session_upload")?;
        self.reader_session_open.validate("reader_session_open")?;
        self.public_metadata.validate("public_metadata")?;
        let longest_window = [
            self.reader_session_create.window,
            self.reader_session_upload.window,
            self.reader_session_open.window,
            self.public_metadata.window,
        ]
        .into_iter()
        .max()
        .expect("fixed public rate-limit policy set");
        if self.retention < longest_window
            || self.retention > Duration::from_secs(30 * 24 * 60 * 60)
        {
            return Err(PublicRateLimitError::new(
                "public_rate_config_invalid",
                "retention must cover the longest policy window and be <=30 days",
            ));
        }
        if !(1..=10_000_000).contains(&self.max_entries) {
            return Err(PublicRateLimitError::new(
                "public_rate_config_invalid",
                "max_entries must be 1..=10000000",
            ));
        }
        Ok(())
    }

    fn policy(&self, class: PublicRateClass) -> &PublicRatePolicy {
        match class {
            PublicRateClass::ReaderSessionCreate => &self.reader_session_create,
            PublicRateClass::ReaderSessionUpload => &self.reader_session_upload,
            PublicRateClass::ReaderSessionOpen => &self.reader_session_open,
            PublicRateClass::PublicMetadata => &self.public_metadata,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PublicRateDecision {
    Allowed,
    Limited {
        code: &'static str,
        retry_at_ms: i64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicRateLimitError {
    pub code: &'static str,
    pub message: String,
}

impl PublicRateLimitError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl fmt::Display for PublicRateLimitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for PublicRateLimitError {}

#[derive(Clone)]
pub struct SqlitePublicRateLimitAuthority {
    path: PathBuf,
    pool: SqlitePool,
    config: PublicRateLimitConfig,
    subject_secret_digest: [u8; 32],
}

impl SqlitePublicRateLimitAuthority {
    pub async fn open(
        path: impl AsRef<Path>,
        max_connections: u32,
        busy_timeout: Duration,
        config: PublicRateLimitConfig,
        subject_secret: &[u8],
    ) -> Result<Self, PublicRateLimitError> {
        config.validate()?;
        if subject_secret.len() < 32 {
            return Err(PublicRateLimitError::new(
                "public_rate_secret_too_short",
                "public rate-limit subject secret must contain at least 32 bytes",
            ));
        }
        if !(1..=16).contains(&max_connections) {
            return Err(PublicRateLimitError::new(
                "public_rate_pool_size_invalid",
                "public rate-limit SQLite pool must use 1..=16 connections",
            ));
        }
        if busy_timeout.is_zero() || busy_timeout > Duration::from_secs(30) {
            return Err(PublicRateLimitError::new(
                "public_rate_busy_timeout_invalid",
                "public rate-limit SQLite busy timeout must be >0 and <=30 seconds",
            ));
        }
        let path = path.as_ref().to_path_buf();
        if !path.exists() {
            return Err(PublicRateLimitError::new(
                "public_rate_database_missing",
                "run chaptera migrate up before opening public rate limiting",
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

        let subject_secret_digest = Sha256::digest(subject_secret).into();
        let authority = Self {
            path,
            pool,
            config,
            subject_secret_digest,
        };
        authority.require_schema().await?;
        Ok(authority)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub async fn close(&self) {
        self.pool.close().await;
    }

    pub async fn admit(
        &self,
        client_ip: IpAddr,
        class: PublicRateClass,
        now_ms: i64,
    ) -> Result<PublicRateDecision, PublicRateLimitError> {
        if now_ms < 0 {
            return Err(PublicRateLimitError::new(
                "public_rate_time_invalid",
                "now_ms must be non-negative",
            ));
        }

        let policy = self.config.policy(class);
        let interval_us = policy.interval_us()?;
        let tolerance_us = policy.burst_tolerance_us()?;
        let now_us = now_ms.checked_mul(1000).ok_or_else(|| {
            PublicRateLimitError::new(
                "public_rate_time_overflow",
                "now_ms does not fit microsecond timeline",
            )
        })?;
        let retention_ms = duration_ms(self.config.retention, "retention")?;
        let cleanup_before_ms = now_ms.saturating_sub(retention_ms);
        let subject_key = self.subject_key(client_ip);
        let policy_class = class.as_str();

        let mut connection = self.pool.acquire().await.map_err(sqlite_error)?;
        let mut tx = (*connection)
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(sqlite_error)?;

        sqlx::query("DELETE FROM public_rate_limit_state WHERE last_seen_at_ms < ?")
            .bind(cleanup_before_ms)
            .execute(&mut *tx)
            .await
            .map_err(sqlite_error)?;

        let existing = sqlx::query(
            r#"
            SELECT theoretical_arrival_us, last_seen_at_ms
            FROM public_rate_limit_state
            WHERE subject_key = ? AND policy_class = ?
            "#,
        )
        .bind(subject_key.as_slice())
        .bind(policy_class)
        .fetch_optional(&mut *tx)
        .await
        .map_err(sqlite_error)?;

        let current_tat = existing
            .as_ref()
            .map(|row| row.try_get::<i64, _>("theoretical_arrival_us"))
            .transpose()
            .map_err(sqlite_error)?;

        if let Some(tat_us) = current_tat {
            let earliest_us = tat_us.saturating_sub(tolerance_us);
            if now_us < earliest_us {
                let retry_at_ms = ceil_div_positive(earliest_us, 1000)?;
                tx.commit().await.map_err(sqlite_error)?;
                return Ok(PublicRateDecision::Limited {
                    code: "public_rate_limited",
                    retry_at_ms,
                });
            }
        } else {
            let entries: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM public_rate_limit_state")
                .fetch_one(&mut *tx)
                .await
                .map_err(sqlite_error)?;
            if entries >= self.config.max_entries {
                let oldest: Option<i64> =
                    sqlx::query_scalar("SELECT MIN(last_seen_at_ms) FROM public_rate_limit_state")
                        .fetch_one(&mut *tx)
                        .await
                        .map_err(sqlite_error)?;
                let retry_at_ms = oldest
                    .unwrap_or(now_ms)
                    .saturating_add(retention_ms)
                    .max(now_ms.saturating_add(1));
                tx.commit().await.map_err(sqlite_error)?;
                return Ok(PublicRateDecision::Limited {
                    code: "public_rate_key_capacity",
                    retry_at_ms,
                });
            }
        }

        let next_tat = current_tat
            .unwrap_or(now_us)
            .max(now_us)
            .checked_add(interval_us)
            .ok_or_else(|| {
                PublicRateLimitError::new(
                    "public_rate_time_overflow",
                    "next theoretical arrival time overflowed",
                )
            })?;

        sqlx::query(
            r#"
            INSERT INTO public_rate_limit_state (
                subject_key, policy_class, theoretical_arrival_us, last_seen_at_ms
            ) VALUES (?, ?, ?, ?)
            ON CONFLICT(subject_key, policy_class) DO UPDATE SET
                theoretical_arrival_us = excluded.theoretical_arrival_us,
                last_seen_at_ms = excluded.last_seen_at_ms
            "#,
        )
        .bind(subject_key.as_slice())
        .bind(policy_class)
        .bind(next_tat)
        .bind(now_ms)
        .execute(&mut *tx)
        .await
        .map_err(sqlite_error)?;

        tx.commit().await.map_err(sqlite_error)?;
        Ok(PublicRateDecision::Allowed)
    }

    async fn require_schema(&self) -> Result<(), PublicRateLimitError> {
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='public_rate_limit_state'",
        )
        .fetch_one(&self.pool)
        .await
        .map_err(sqlite_error)?;
        if count != 1 {
            return Err(PublicRateLimitError::new(
                "public_rate_schema_missing",
                "run chaptera migrate up before opening public rate limiting",
            ));
        }
        Ok(())
    }

    fn subject_key(&self, client_ip: IpAddr) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(SUBJECT_DOMAIN);
        hasher.update(self.subject_secret_digest);
        match client_ip {
            IpAddr::V4(ip) => {
                hasher.update([4]);
                hasher.update(ip.octets());
            }
            IpAddr::V6(ip) => {
                hasher.update([6]);
                hasher.update(ip.octets());
            }
        }
        hasher.finalize().into()
    }
}

fn duration_ms(duration: Duration, name: &'static str) -> Result<i64, PublicRateLimitError> {
    i64::try_from(duration.as_millis()).map_err(|_| {
        PublicRateLimitError::new(
            "public_rate_duration_overflow",
            format!("{name} does not fit i64 milliseconds"),
        )
    })
}

fn ceil_div_positive(value: i64, divisor: i64) -> Result<i64, PublicRateLimitError> {
    if value < 0 || divisor <= 0 {
        return Err(PublicRateLimitError::new(
            "public_rate_math_invalid",
            "ceil division expects non-negative value and positive divisor",
        ));
    }
    value
        .checked_add(divisor - 1)
        .and_then(|adjusted| adjusted.checked_div(divisor))
        .ok_or_else(|| {
            PublicRateLimitError::new(
                "public_rate_time_overflow",
                "retry time calculation overflowed",
            )
        })
}

fn sqlite_error(error: impl fmt::Display) -> PublicRateLimitError {
    let message = error.to_string();
    let bounded = if message.len() <= 512 {
        message
    } else {
        format!("{}...", &message[..512])
    };
    PublicRateLimitError::new("public_rate_sqlite_error", bounded)
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        net::{IpAddr, Ipv4Addr, Ipv6Addr},
        sync::atomic::{AtomicU64, Ordering},
    };

    use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};

    use super::*;
    use crate::schema_migration::SqliteMigrationRuntime;

    static NEXT_PATH: AtomicU64 = AtomicU64::new(1);

    fn temp_path(label: &str) -> PathBuf {
        let n = NEXT_PATH.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "chaptera-public-rate-{label}-{}-{n}.sqlite",
            std::process::id()
        ))
    }

    fn policy(requests_per_window: u32, window_ms: u64, burst: u32) -> PublicRatePolicy {
        PublicRatePolicy {
            requests_per_window,
            window: Duration::from_millis(window_ms),
            burst,
        }
    }

    fn config(max_entries: i64) -> PublicRateLimitConfig {
        PublicRateLimitConfig {
            reader_session_create: policy(2, 1000, 2),
            reader_session_upload: policy(1, 1000, 1),
            reader_session_open: policy(10, 1000, 3),
            public_metadata: policy(20, 1000, 5),
            retention: Duration::from_secs(2),
            max_entries,
        }
    }

    async fn migrated_path(label: &str) -> PathBuf {
        let path = temp_path(label);
        SqliteMigrationRuntime::new(&path, Duration::from_secs(5))
            .unwrap()
            .migrate_up()
            .await
            .unwrap();
        path
    }

    #[tokio::test]
    async fn gcra_enforces_burst_and_sustained_rate() {
        let path = migrated_path("gcra").await;
        let authority = SqlitePublicRateLimitAuthority::open(
            &path,
            2,
            Duration::from_secs(5),
            config(100),
            b"0123456789abcdef0123456789abcdef",
        )
        .await
        .unwrap();
        let ip = IpAddr::V4(Ipv4Addr::new(203, 0, 113, 10));

        assert_eq!(
            authority
                .admit(ip, PublicRateClass::ReaderSessionCreate, 0)
                .await
                .unwrap(),
            PublicRateDecision::Allowed
        );
        assert_eq!(
            authority
                .admit(ip, PublicRateClass::ReaderSessionCreate, 0)
                .await
                .unwrap(),
            PublicRateDecision::Allowed
        );
        assert_eq!(
            authority
                .admit(ip, PublicRateClass::ReaderSessionCreate, 0)
                .await
                .unwrap(),
            PublicRateDecision::Limited {
                code: "public_rate_limited",
                retry_at_ms: 500,
            }
        );
        assert_eq!(
            authority
                .admit(ip, PublicRateClass::ReaderSessionCreate, 500)
                .await
                .unwrap(),
            PublicRateDecision::Allowed
        );

        authority.close().await;
        let _ = fs::remove_file(path);
    }

    #[tokio::test]
    async fn state_survives_restart_with_same_secret() {
        let path = migrated_path("restart").await;
        let ip = IpAddr::V6("2001:db8::5".parse().unwrap());
        let secret = b"restart-secret-0123456789abcdef00";

        let first = SqlitePublicRateLimitAuthority::open(
            &path,
            1,
            Duration::from_secs(5),
            config(100),
            secret,
        )
        .await
        .unwrap();
        assert_eq!(
            first
                .admit(ip, PublicRateClass::ReaderSessionUpload, 100)
                .await
                .unwrap(),
            PublicRateDecision::Allowed
        );
        first.close().await;

        let reopened = SqlitePublicRateLimitAuthority::open(
            &path,
            1,
            Duration::from_secs(5),
            config(100),
            secret,
        )
        .await
        .unwrap();
        assert_eq!(
            reopened
                .admit(ip, PublicRateClass::ReaderSessionUpload, 100)
                .await
                .unwrap(),
            PublicRateDecision::Limited {
                code: "public_rate_limited",
                retry_at_ms: 1100,
            }
        );

        reopened.close().await;
        let _ = fs::remove_file(path);
    }

    #[tokio::test]
    async fn database_never_stores_raw_client_ip() {
        let path = migrated_path("privacy").await;
        let ip = IpAddr::V4(Ipv4Addr::new(198, 51, 100, 77));
        let authority = SqlitePublicRateLimitAuthority::open(
            &path,
            1,
            Duration::from_secs(5),
            config(100),
            b"privacy-secret-0123456789abcdef00",
        )
        .await
        .unwrap();
        assert_eq!(
            authority
                .admit(ip, PublicRateClass::ReaderSessionOpen, 10)
                .await
                .unwrap(),
            PublicRateDecision::Allowed
        );
        authority.close().await;

        let options = SqliteConnectOptions::new().filename(&path);
        let mut connection = SqliteConnection::connect_with(&options).await.unwrap();
        let row: (Vec<u8>, String) =
            sqlx::query_as("SELECT subject_key, policy_class FROM public_rate_limit_state")
                .fetch_one(&mut connection)
                .await
                .unwrap();
        assert_eq!(row.0.len(), 32);
        assert_eq!(row.1, "reader_session_open");
        assert_ne!(row.0, ip.to_string().as_bytes());

        connection.close().await.unwrap();
        let bytes = fs::read(&path).unwrap();
        assert!(
            !String::from_utf8_lossy(&bytes).contains(&ip.to_string()),
            "SQLite file must not contain the textual raw client IP"
        );
        let _ = fs::remove_file(path);
    }

    #[tokio::test]
    async fn key_cardinality_is_bounded_and_ttl_cleanup_releases_capacity() {
        let path = migrated_path("capacity").await;
        let authority = SqlitePublicRateLimitAuthority::open(
            &path,
            1,
            Duration::from_secs(5),
            config(2),
            b"capacity-secret-0123456789abcdef",
        )
        .await
        .unwrap();

        for last_octet in [1, 2] {
            assert_eq!(
                authority
                    .admit(
                        IpAddr::V4(Ipv4Addr::new(192, 0, 2, last_octet)),
                        PublicRateClass::PublicMetadata,
                        0,
                    )
                    .await
                    .unwrap(),
                PublicRateDecision::Allowed
            );
        }

        assert!(matches!(
            authority
                .admit(
                    IpAddr::V4(Ipv4Addr::new(192, 0, 2, 3)),
                    PublicRateClass::PublicMetadata,
                    0,
                )
                .await
                .unwrap(),
            PublicRateDecision::Limited {
                code: "public_rate_key_capacity",
                ..
            }
        ));

        assert_eq!(
            authority
                .admit(
                    IpAddr::V4(Ipv4Addr::new(192, 0, 2, 3)),
                    PublicRateClass::PublicMetadata,
                    2001,
                )
                .await
                .unwrap(),
            PublicRateDecision::Allowed
        );

        authority.close().await;
        let _ = fs::remove_file(path);
    }

    #[tokio::test]
    async fn policy_classes_have_independent_state() {
        let path = migrated_path("classes").await;
        let authority = SqlitePublicRateLimitAuthority::open(
            &path,
            1,
            Duration::from_secs(5),
            config(100),
            b"class-secret-0123456789abcdef0123",
        )
        .await
        .unwrap();
        let ip = IpAddr::V4(Ipv4Addr::new(203, 0, 113, 9));

        assert_eq!(
            authority
                .admit(ip, PublicRateClass::ReaderSessionUpload, 0)
                .await
                .unwrap(),
            PublicRateDecision::Allowed
        );
        assert!(matches!(
            authority
                .admit(ip, PublicRateClass::ReaderSessionUpload, 0)
                .await
                .unwrap(),
            PublicRateDecision::Limited { .. }
        ));
        assert_eq!(
            authority
                .admit(ip, PublicRateClass::ReaderSessionOpen, 0)
                .await
                .unwrap(),
            PublicRateDecision::Allowed
        );

        authority.close().await;
        let _ = fs::remove_file(path);
    }
}
