use std::{
    fmt,
    path::{Path as FsPath, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use axum::{
    Json, Router,
    body::Body,
    extract::{Extension, Path, State},
    http::{
        HeaderMap, HeaderValue, StatusCode,
        header::{CACHE_CONTROL, CONTENT_LENGTH, RETRY_AFTER},
    },
    response::{IntoResponse, Response},
    routing::{get, post, put},
};
use futures_util::StreamExt;
use rand::{RngCore, rngs::OsRng};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::{
    Row, SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
};
use tokio::io::AsyncWriteExt;

use crate::{
    blob_store::{BlobStoreError, BlobStoreService},
    edge::ClientIp,
    public_rate_limit::{
        PublicRateClass, PublicRateDecision, PublicRateLimitError, SqlitePublicRateLimitAuthority,
    },
    guest_reader_worker::{GuestSceneWorkerError, IsolatedGuestSceneProducer},
    source_ingress_async::{AsyncSourceSecurityScanner, SourceSecurityScanOutcome},
    source_ingress_security::ProductionSourceSecurityScanner,
    upload_admission::{
        ReserveUploadOutcome, SqliteUploadAdmissionAuthority, UploadAdmissionError,
        UploadAdmissionRequest,
    },
};

pub const GUEST_SERVICE_TENANT_ID: &str = "tenant:cloud-reader-guest-service";
pub const GUEST_SERVICE_PRINCIPAL_ID: &str = "principal:cloud-reader-guest-service";
pub const GUEST_TOKEN_HEADER: &str = "x-chaptera-reader-session";
const STREAM_BUFFER_BYTES: usize = 64 * 1024;
const CLEANUP_BATCH: i64 = 16;
const MAX_SCENE_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, Copy)]
pub struct GuestReaderHttpConfig {
    pub session_ttl: Duration,
    pub max_file_bytes: u64,
}

impl GuestReaderHttpConfig {
    pub fn validate(
        &self,
        admission: &SqliteUploadAdmissionAuthority,
    ) -> Result<(), GuestReaderError> {
        if self.session_ttl.is_zero() || self.session_ttl > admission.lease_duration() {
            return Err(GuestReaderError::internal("guest_session_ttl_invalid"));
        }
        let admission_max = u64::try_from(admission.max_single_upload_bytes())
            .map_err(|_| GuestReaderError::internal("guest_admission_max_invalid"))?;
        if self.max_file_bytes == 0 || self.max_file_bytes > admission_max {
            return Err(GuestReaderError::internal("guest_max_file_bytes_invalid"));
        }
        Ok(())
    }
}

#[derive(Clone)]
pub struct GuestReaderHttpState {
    rate: SqlitePublicRateLimitAuthority,
    admission: SqliteUploadAdmissionAuthority,
    sessions: SqliteGuestReaderSessionStore,
    blob_store: BlobStoreService,
    scanner: Arc<ProductionSourceSecurityScanner>,
    scene_worker: IsolatedGuestSceneProducer,
    config: GuestReaderHttpConfig,
}

impl GuestReaderHttpState {
    pub fn new(
        rate: SqlitePublicRateLimitAuthority,
        admission: SqliteUploadAdmissionAuthority,
        sessions: SqliteGuestReaderSessionStore,
        blob_store: BlobStoreService,
        scanner: ProductionSourceSecurityScanner,
        scene_worker: IsolatedGuestSceneProducer,
        config: GuestReaderHttpConfig,
    ) -> Result<Self, GuestReaderError> {
        config.validate(&admission)?;
        Ok(Self {
            rate,
            admission,
            sessions,
            blob_store,
            scanner: Arc::new(scanner),
            scene_worker,
            config,
        })
    }
}

pub fn router(state: GuestReaderHttpState) -> Router {
    Router::new()
        .route("/v1/reader/guest-sessions", post(issue_session))
        .route(
            "/v1/reader/guest-sessions/{session_id}/content",
            put(put_content),
        )
        .route(
            "/v1/reader/guest-sessions/{session_id}/open",
            post(open_session),
        )
        .route(
            "/v1/reader/guest-sessions/{session_id}/scene",
            get(get_scene),
        )
        .with_state(state)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct IssueGuestSessionBody {
    expected_byte_len: u64,
}

#[derive(Debug, Serialize)]
struct IssueGuestSessionResponse {
    protocol_version: &'static str,
    session_id: String,
    access_token: String,
    expires_at_ms: i64,
    max_file_bytes: u64,
    upload_path: String,
    open_path: String,
    scene_path: String,
}

#[derive(Debug, Serialize)]
struct GuestUploadResponse {
    protocol_version: &'static str,
    session_id: String,
    state: &'static str,
    expires_at_ms: i64,
}

#[derive(Debug, Serialize)]
struct GuestOpenResponse {
    protocol_version: &'static str,
    session_id: String,
    classification: String,
    expires_at_ms: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    terminal_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    scene: Option<Value>,
}

#[derive(Debug, Serialize)]
struct GuestSceneResponse {
    protocol_version: &'static str,
    session_id: String,
    classification: String,
    expires_at_ms: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    terminal_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    scene: Option<Value>,
}

const GUEST_PROTOCOL_V1: &str = "chaptera.reader-guest-session.v1";

struct GuestJson<T>(T);

impl<T: Serialize> IntoResponse for GuestJson<T> {
    fn into_response(self) -> Response {
        let mut response = Json(self.0).into_response();
        response
            .headers_mut()
            .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
        response
    }
}

async fn issue_session(
    State(state): State<GuestReaderHttpState>,
    Extension(client_ip): Extension<ClientIp>,
    Json(body): Json<IssueGuestSessionBody>,
) -> Result<GuestJson<IssueGuestSessionResponse>, GuestReaderError> {
    let now_ms = now_ms()?;
    state.cleanup_expired(now_ms).await?;
    state
        .admit_network(client_ip.0, PublicRateClass::ReaderSessionCreate, now_ms)
        .await?;

    if body.expected_byte_len == 0 || body.expected_byte_len > state.config.max_file_bytes {
        return Err(GuestReaderError::payload_too_large("guest_upload_too_large"));
    }

    let session_id = random_id("guest")?;
    let upload_id = format!("guest-upload:{}", opaque_suffix(&session_id));
    let reservation_id = format!("guest-reservation:{}", opaque_suffix(&session_id));
    let access_token = random_token()?;
    let access_token_hash = token_hash(access_token.as_bytes());
    let expires_at_ms = add_duration(now_ms, state.config.session_ttl)?;
    let request_hash = admission_request_hash(&session_id, body.expected_byte_len);
    let expected_bytes = i64::try_from(body.expected_byte_len)
        .map_err(|_| GuestReaderError::payload_too_large("guest_upload_too_large"))?;

    let admission_request = UploadAdmissionRequest {
        reservation_id: reservation_id.clone(),
        tenant_id: GUEST_SERVICE_TENANT_ID.to_owned(),
        principal_id: GUEST_SERVICE_PRINCIPAL_ID.to_owned(),
        expected_bytes,
        request_hash,
    };
    match state.admission.reserve(admission_request.clone(), now_ms).await {
        Ok(ReserveUploadOutcome::Reserved(_)) => {}
        Ok(ReserveUploadOutcome::Existing(_)) => {
            return Err(GuestReaderError::internal("guest_reservation_collision"));
        }
        Err(error) => return Err(map_admission_error(error)),
    }

    let session = GuestReaderSession {
        session_id: session_id.clone(),
        access_token_hash: access_token_hash.to_vec(),
        upload_id,
        reservation_id,
        expected_byte_len: body.expected_byte_len,
        observed_byte_len: None,
        storage_generation: None,
        object_etag: None,
        source_sha256: None,
        state: GuestSessionState::Issued,
        classification: None,
        scene_json: None,
        terminal_code: None,
        created_at_ms: now_ms,
        updated_at_ms: now_ms,
        expires_at_ms,
        quarantine_deleted_at_ms: None,
    };

    if let Err(error) = state.sessions.insert(&session).await {
        let _ = state.admission.release_exact(admission_request, now_ms).await;
        return Err(error);
    }

    Ok(GuestJson(IssueGuestSessionResponse {
        protocol_version: GUEST_PROTOCOL_V1,
        session_id: session_id.clone(),
        access_token,
        expires_at_ms,
        max_file_bytes: state.config.max_file_bytes,
        upload_path: format!("/v1/reader/guest-sessions/{session_id}/content"),
        open_path: format!("/v1/reader/guest-sessions/{session_id}/open"),
        scene_path: format!("/v1/reader/guest-sessions/{session_id}/scene"),
    }))
}

async fn put_content(
    State(state): State<GuestReaderHttpState>,
    Extension(client_ip): Extension<ClientIp>,
    Path(session_id): Path<String>,
    headers: HeaderMap,
    body: Body,
) -> Result<GuestJson<GuestUploadResponse>, GuestReaderError> {
    let now_ms = now_ms()?;
    state.cleanup_expired(now_ms).await?;
    state
        .admit_network(client_ip.0, PublicRateClass::ReaderSessionUpload, now_ms)
        .await?;

    let session = state.authorized_session(&session_id, &headers, now_ms).await?;
    if session.state != GuestSessionState::Issued {
        return Err(GuestReaderError::conflict("guest_upload_state_conflict"));
    }

    let declared = content_length(&headers)?;
    if declared != session.expected_byte_len {
        return Err(GuestReaderError::bad_request("guest_upload_length_mismatch"));
    }

    let (mut writer, mut reader) = tokio::io::duplex(STREAM_BUFFER_BYTES);
    let mut stream = body.into_data_stream();
    let pump = async move {
        while let Some(chunk) = stream.next().await {
            let chunk =
                chunk.map_err(|_| GuestReaderError::bad_request("guest_upload_body_read_failed"))?;
            writer
                .write_all(&chunk)
                .await
                .map_err(|_| GuestReaderError::bad_request("guest_upload_body_write_failed"))?;
        }
        writer
            .shutdown()
            .await
            .map_err(|_| GuestReaderError::bad_request("guest_upload_body_write_failed"))
    };

    let create = state.blob_store.create_quarantine_streamed(
        GUEST_SERVICE_TENANT_ID,
        &session.upload_id,
        session.expected_byte_len,
        &mut reader,
    );
    let (create_result, pump_result) = tokio::join!(create, pump);
    let metadata = create_result.map_err(map_blob_error)?;
    pump_result?;

    let stored = state
        .sessions
        .mark_stored(
            &session.session_id,
            metadata.byte_len,
            &metadata.storage_generation,
            &metadata.etag,
            now_ms,
        )
        .await?;

    Ok(GuestJson(GuestUploadResponse {
        protocol_version: GUEST_PROTOCOL_V1,
        session_id: stored.session_id,
        state: stored.state.as_str(),
        expires_at_ms: stored.expires_at_ms,
    }))
}

async fn open_session(
    State(state): State<GuestReaderHttpState>,
    Extension(client_ip): Extension<ClientIp>,
    Path(session_id): Path<String>,
    headers: HeaderMap,
) -> Result<GuestJson<GuestOpenResponse>, GuestReaderError> {
    let now_ms = now_ms()?;
    state.cleanup_expired(now_ms).await?;
    state
        .admit_network(client_ip.0, PublicRateClass::ReaderSessionOpen, now_ms)
        .await?;

    let session = state.authorized_session(&session_id, &headers, now_ms).await?;
    match session.state {
        GuestSessionState::Opened => return open_response_from_stored(&session),
        GuestSessionState::Rejected => return open_response_from_stored(&session),
        GuestSessionState::Opening => {
            return Err(GuestReaderError::conflict("guest_open_in_progress"));
        }
        GuestSessionState::Issued => {
            return Err(GuestReaderError::conflict("guest_upload_not_stored"));
        }
        GuestSessionState::Expired => {
            return Err(GuestReaderError::gone("guest_session_expired"));
        }
        GuestSessionState::Stored => {}
    }

    let opening = state.sessions.claim_open(&session.session_id, now_ms).await?;
    let generation = opening
        .storage_generation
        .as_deref()
        .ok_or_else(|| GuestReaderError::internal("guest_storage_identity_missing"))?;
    let etag = opening
        .object_etag
        .as_deref()
        .ok_or_else(|| GuestReaderError::internal("guest_storage_identity_missing"))?;
    let observed = opening
        .observed_byte_len
        .ok_or_else(|| GuestReaderError::internal("guest_storage_identity_missing"))?;

    let first = match state
        .blob_store
        .open_quarantine_exact(
            GUEST_SERVICE_TENANT_ID,
            &opening.upload_id,
            generation,
            etag,
            observed,
        )
        .await
    {
        Ok(input) => input,
        Err(error) => {
            state.sessions.reset_open(&opening.session_id, now_ms).await?;
            return Err(map_blob_error(error));
        }
    };
    let mut scan_input = first;
    let scan_outcome = match state.scanner.scan(&mut *scan_input).await {
        Ok(outcome) => outcome,
        Err(error) => {
            state.sessions.reset_open(&opening.session_id, now_ms).await?;
            return Err(map_scan_error(error));
        }
    };
    match scan_outcome {
        SourceSecurityScanOutcome::Accepted(_) => {}
        SourceSecurityScanOutcome::Rejected { code } => {
            let rejected = state
                .sessions
                .finish_rejected(&opening.session_id, code, now_ms)
                .await?;
            state.release_admission(&rejected, now_ms).await?;
            state.delete_quarantine(&rejected, now_ms).await?;
            return Ok(GuestJson(GuestOpenResponse {
                protocol_version: GUEST_PROTOCOL_V1,
                session_id: rejected.session_id,
                classification: "rejected".to_owned(),
                expires_at_ms: rejected.expires_at_ms,
                source_sha256: None,
                terminal_code: rejected.terminal_code,
                scene: None,
            }));
        }
    }

    let receipt = match state
        .scene_worker
        .produce_from_quarantine(
            &state.blob_store,
            GUEST_SERVICE_TENANT_ID,
            &opening.upload_id,
            generation,
            etag,
            observed,
            &opening.session_id,
        )
        .await
    {
        Ok(receipt) => receipt,
        Err(error) => {
            state.sessions.reset_open(&opening.session_id, now_ms).await?;
            return Err(map_scene_worker_error(error));
        }
    };

    let mut classification = receipt.classification;
    let mut terminal_code = receipt.terminal_code;
    let mut scene = receipt.scene;
    let mut scene_json = scene
        .as_ref()
        .map(serde_json::to_vec)
        .transpose()
        .map_err(|_| GuestReaderError::internal("guest_reader_scene_serialize_failed"))?;
    if scene_json
        .as_ref()
        .is_some_and(|encoded| encoded.len() > MAX_SCENE_BYTES)
    {
        classification = "unsupported".to_owned();
        terminal_code = Some("reader_scene_too_large".to_owned());
        scene = None;
        scene_json = None;
    }

    let opened = state
        .sessions
        .finish_opened(
            &opening.session_id,
            &classification,
            &receipt.source_sha256,
            scene_json.as_deref(),
            terminal_code.as_deref(),
            now_ms,
        )
        .await?;
    state.release_admission(&opened, now_ms).await?;

    Ok(GuestJson(GuestOpenResponse {
        protocol_version: GUEST_PROTOCOL_V1,
        session_id: opened.session_id,
        classification,
        expires_at_ms: opened.expires_at_ms,
        source_sha256: Some(receipt.source_sha256),
        terminal_code,
        scene,
    }))
}

async fn get_scene(
    State(state): State<GuestReaderHttpState>,
    Extension(client_ip): Extension<ClientIp>,
    Path(session_id): Path<String>,
    headers: HeaderMap,
) -> Result<GuestJson<GuestSceneResponse>, GuestReaderError> {
    let now_ms = now_ms()?;
    state.cleanup_expired(now_ms).await?;
    state
        .admit_network(client_ip.0, PublicRateClass::ReaderSessionOpen, now_ms)
        .await?;

    let session = state.authorized_session(&session_id, &headers, now_ms).await?;
    if !matches!(
        session.state,
        GuestSessionState::Opened | GuestSessionState::Rejected
    ) {
        return Err(GuestReaderError::conflict("guest_scene_not_ready"));
    }
    let scene = session
        .scene_json
        .as_deref()
        .map(serde_json::from_slice::<Value>)
        .transpose()
        .map_err(|_| GuestReaderError::internal("guest_scene_corrupt"))?;

    Ok(GuestJson(GuestSceneResponse {
        protocol_version: GUEST_PROTOCOL_V1,
        session_id: session.session_id,
        classification: session
            .classification
            .unwrap_or_else(|| "rejected".to_owned()),
        expires_at_ms: session.expires_at_ms,
        source_sha256: session.source_sha256,
        terminal_code: session.terminal_code,
        scene,
    }))
}

impl GuestReaderHttpState {
    async fn admit_network(
        &self,
        client_ip: std::net::IpAddr,
        class: PublicRateClass,
        now_ms: i64,
    ) -> Result<(), GuestReaderError> {
        match self.rate.admit(client_ip, class, now_ms).await {
            Ok(PublicRateDecision::Allowed) => Ok(()),
            Ok(PublicRateDecision::Limited { code, retry_at_ms }) => {
                Err(GuestReaderError::rate_limited(code, retry_at_ms))
            }
            Err(error) => Err(map_rate_error(error)),
        }
    }

    async fn authorized_session(
        &self,
        session_id: &str,
        headers: &HeaderMap,
        now_ms: i64,
    ) -> Result<GuestReaderSession, GuestReaderError> {
        let token = headers
            .get(GUEST_TOKEN_HEADER)
            .and_then(|value| value.to_str().ok())
            .filter(|value| value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit()))
            .ok_or_else(|| GuestReaderError::unauthorized("guest_session_token_required"))?;
        let session = self
            .sessions
            .get(session_id)
            .await?
            .ok_or_else(|| GuestReaderError::not_found("guest_session_not_found"))?;
        if !constant_time_eq(&session.access_token_hash, &token_hash(token.as_bytes())) {
            return Err(GuestReaderError::not_found("guest_session_not_found"));
        }
        if now_ms >= session.expires_at_ms || session.state == GuestSessionState::Expired {
            self.cleanup_one(session.clone(), now_ms).await?;
            return Err(GuestReaderError::gone("guest_session_expired"));
        }
        Ok(session)
    }

    async fn release_admission(
        &self,
        session: &GuestReaderSession,
        now_ms: i64,
    ) -> Result<(), GuestReaderError> {
        let request = admission_request(session)?;
        match self.admission.release_exact(request, now_ms).await {
            Ok(_) => Ok(()),
            Err(error)
                if matches!(
                    error.code,
                    "upload_admission_not_found" | "upload_admission_already_released"
                ) =>
            {
                Ok(())
            }
            Err(error) => Err(map_admission_error(error)),
        }
    }

    async fn delete_quarantine(
        &self,
        session: &GuestReaderSession,
        now_ms: i64,
    ) -> Result<(), GuestReaderError> {
        if session.quarantine_deleted_at_ms.is_some() {
            return Ok(());
        }
        let Some(generation) = session.storage_generation.as_deref() else {
            self.sessions
                .mark_quarantine_deleted(&session.session_id, now_ms)
                .await?;
            return Ok(());
        };
        let etag = session
            .object_etag
            .as_deref()
            .ok_or_else(|| GuestReaderError::internal("guest_storage_identity_missing"))?;
        let observed = session
            .observed_byte_len
            .ok_or_else(|| GuestReaderError::internal("guest_storage_identity_missing"))?;
        self.blob_store
            .delete_quarantine_exact(
                GUEST_SERVICE_TENANT_ID,
                &session.upload_id,
                generation,
                etag,
                observed,
            )
            .await
            .map_err(map_blob_error)?;
        self.sessions
            .mark_quarantine_deleted(&session.session_id, now_ms)
            .await?;
        Ok(())
    }

    async fn cleanup_one(
        &self,
        session: GuestReaderSession,
        now_ms: i64,
    ) -> Result<(), GuestReaderError> {
        let _ = self.release_admission(&session, now_ms).await;
        self.delete_quarantine(&session, now_ms).await?;
        self.sessions.mark_expired(&session.session_id, now_ms).await?;
        Ok(())
    }

    async fn cleanup_expired(&self, now_ms: i64) -> Result<(), GuestReaderError> {
        let expired = self.sessions.expired_pending(now_ms, CLEANUP_BATCH).await?;
        for session in expired {
            self.cleanup_one(session, now_ms).await?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GuestSessionState {
    Issued,
    Stored,
    Opening,
    Opened,
    Rejected,
    Expired,
}

impl GuestSessionState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Issued => "issued",
            Self::Stored => "stored",
            Self::Opening => "opening",
            Self::Opened => "opened",
            Self::Rejected => "rejected",
            Self::Expired => "expired",
        }
    }

    fn parse(value: &str) -> Result<Self, GuestReaderError> {
        match value {
            "issued" => Ok(Self::Issued),
            "stored" => Ok(Self::Stored),
            "opening" => Ok(Self::Opening),
            "opened" => Ok(Self::Opened),
            "rejected" => Ok(Self::Rejected),
            "expired" => Ok(Self::Expired),
            _ => Err(GuestReaderError::internal("guest_session_state_invalid")),
        }
    }
}

#[derive(Debug, Clone)]
struct GuestReaderSession {
    session_id: String,
    access_token_hash: Vec<u8>,
    upload_id: String,
    reservation_id: String,
    expected_byte_len: u64,
    observed_byte_len: Option<u64>,
    storage_generation: Option<String>,
    object_etag: Option<String>,
    source_sha256: Option<String>,
    state: GuestSessionState,
    classification: Option<String>,
    scene_json: Option<Vec<u8>>,
    terminal_code: Option<String>,
    created_at_ms: i64,
    updated_at_ms: i64,
    expires_at_ms: i64,
    quarantine_deleted_at_ms: Option<i64>,
}

#[derive(Clone)]
pub struct SqliteGuestReaderSessionStore {
    path: PathBuf,
    pool: SqlitePool,
}

impl SqliteGuestReaderSessionStore {
    pub async fn open(
        path: impl AsRef<FsPath>,
        max_connections: u32,
        busy_timeout: Duration,
    ) -> Result<Self, GuestReaderError> {
        if !(1..=16).contains(&max_connections) {
            return Err(GuestReaderError::internal("guest_session_pool_invalid"));
        }
        if busy_timeout.is_zero() || busy_timeout > Duration::from_secs(30) {
            return Err(GuestReaderError::internal("guest_session_busy_timeout_invalid"));
        }
        let path = path.as_ref().to_path_buf();
        if !path.exists() {
            return Err(GuestReaderError::internal("guest_session_database_missing"));
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
        let store = Self { path, pool };
        store.require_schema().await?;
        Ok(store)
    }

    pub fn path(&self) -> &FsPath {
        &self.path
    }

    pub async fn close(&self) {
        self.pool.close().await;
    }

    async fn require_schema(&self) -> Result<(), GuestReaderError> {
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='reader_guest_sessions'",
        )
        .fetch_one(&self.pool)
        .await
        .map_err(sqlite_error)?;
        if count != 1 {
            return Err(GuestReaderError::internal("guest_session_schema_missing"));
        }
        Ok(())
    }

    async fn insert(&self, session: &GuestReaderSession) -> Result<(), GuestReaderError> {
        sqlx::query(
            r#"
            INSERT INTO reader_guest_sessions (
                session_id, access_token_hash, upload_id, reservation_id,
                expected_byte_len, observed_byte_len, storage_generation,
                object_etag, source_sha256, state, classification, scene_json,
                terminal_code, created_at_ms, updated_at_ms, expires_at_ms,
                quarantine_deleted_at_ms
            ) VALUES (?, ?, ?, ?, ?, NULL, NULL, NULL, NULL, ?, NULL, NULL, NULL, ?, ?, ?, NULL)
            "#,
        )
        .bind(session.session_id.as_bytes())
        .bind(&session.access_token_hash)
        .bind(session.upload_id.as_bytes())
        .bind(session.reservation_id.as_bytes())
        .bind(i64::try_from(session.expected_byte_len).map_err(|_| {
            GuestReaderError::payload_too_large("guest_upload_too_large")
        })?)
        .bind(session.state.as_str())
        .bind(session.created_at_ms)
        .bind(session.updated_at_ms)
        .bind(session.expires_at_ms)
        .execute(&self.pool)
        .await
        .map_err(sqlite_error)?;
        Ok(())
    }

    async fn get(&self, session_id: &str) -> Result<Option<GuestReaderSession>, GuestReaderError> {
        let row = sqlx::query(
            r#"
            SELECT session_id, access_token_hash, upload_id, reservation_id,
                   expected_byte_len, observed_byte_len, storage_generation,
                   object_etag, source_sha256, state, classification, scene_json,
                   terminal_code, created_at_ms, updated_at_ms, expires_at_ms,
                   quarantine_deleted_at_ms
            FROM reader_guest_sessions
            WHERE session_id = ?
            "#,
        )
        .bind(session_id.as_bytes())
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlite_error)?;
        row.as_ref().map(session_from_row).transpose()
    }

    async fn mark_stored(
        &self,
        session_id: &str,
        observed_byte_len: u64,
        generation: &str,
        etag: &str,
        now_ms: i64,
    ) -> Result<GuestReaderSession, GuestReaderError> {
        let observed = i64::try_from(observed_byte_len)
            .map_err(|_| GuestReaderError::internal("guest_observed_length_invalid"))?;
        let result = sqlx::query(
            r#"
            UPDATE reader_guest_sessions
            SET state='stored', observed_byte_len=?, storage_generation=?,
                object_etag=?, updated_at_ms=?
            WHERE session_id=? AND state='issued' AND expires_at_ms>?
            "#,
        )
        .bind(observed)
        .bind(generation.as_bytes())
        .bind(etag.as_bytes())
        .bind(now_ms)
        .bind(session_id.as_bytes())
        .bind(now_ms)
        .execute(&self.pool)
        .await
        .map_err(sqlite_error)?;
        if result.rows_affected() != 1 {
            return Err(GuestReaderError::conflict("guest_upload_state_conflict"));
        }
        self.get(session_id)
            .await?
            .ok_or_else(|| GuestReaderError::internal("guest_session_disappeared"))
    }

    async fn claim_open(
        &self,
        session_id: &str,
        now_ms: i64,
    ) -> Result<GuestReaderSession, GuestReaderError> {
        let result = sqlx::query(
            r#"
            UPDATE reader_guest_sessions
            SET state='opening', updated_at_ms=?
            WHERE session_id=? AND state='stored' AND expires_at_ms>?
            "#,
        )
        .bind(now_ms)
        .bind(session_id.as_bytes())
        .bind(now_ms)
        .execute(&self.pool)
        .await
        .map_err(sqlite_error)?;
        if result.rows_affected() != 1 {
            return Err(GuestReaderError::conflict("guest_open_state_conflict"));
        }
        self.get(session_id)
            .await?
            .ok_or_else(|| GuestReaderError::internal("guest_session_disappeared"))
    }

    async fn reset_open(
        &self,
        session_id: &str,
        now_ms: i64,
    ) -> Result<(), GuestReaderError> {
        let result = sqlx::query(
            r#"
            UPDATE reader_guest_sessions
            SET state='stored', updated_at_ms=?
            WHERE session_id=? AND state='opening' AND expires_at_ms>?
            "#,
        )
        .bind(now_ms)
        .bind(session_id.as_bytes())
        .bind(now_ms)
        .execute(&self.pool)
        .await
        .map_err(sqlite_error)?;
        if result.rows_affected() != 1 {
            return Err(GuestReaderError::conflict("guest_open_reset_conflict"));
        }
        Ok(())
    }

    async fn finish_opened(
        &self,
        session_id: &str,
        classification: &str,
        source_sha256: &str,
        scene_json: Option<&[u8]>,
        terminal_code: Option<&str>,
        now_ms: i64,
    ) -> Result<GuestReaderSession, GuestReaderError> {
        let result = sqlx::query(
            r#"
            UPDATE reader_guest_sessions
            SET state='opened', classification=?, source_sha256=?, scene_json=?,
                terminal_code=?, updated_at_ms=?
            WHERE session_id=? AND state='opening'
            "#,
        )
        .bind(classification)
        .bind(source_sha256.as_bytes())
        .bind(scene_json)
        .bind(terminal_code)
        .bind(now_ms)
        .bind(session_id.as_bytes())
        .execute(&self.pool)
        .await
        .map_err(sqlite_error)?;
        if result.rows_affected() != 1 {
            return Err(GuestReaderError::conflict("guest_open_state_conflict"));
        }
        self.get(session_id)
            .await?
            .ok_or_else(|| GuestReaderError::internal("guest_session_disappeared"))
    }

    async fn finish_rejected(
        &self,
        session_id: &str,
        code: &'static str,
        now_ms: i64,
    ) -> Result<GuestReaderSession, GuestReaderError> {
        let result = sqlx::query(
            r#"
            UPDATE reader_guest_sessions
            SET state='rejected', classification='rejected', terminal_code=?,
                updated_at_ms=?
            WHERE session_id=? AND state='opening'
            "#,
        )
        .bind(code)
        .bind(now_ms)
        .bind(session_id.as_bytes())
        .execute(&self.pool)
        .await
        .map_err(sqlite_error)?;
        if result.rows_affected() != 1 {
            return Err(GuestReaderError::conflict("guest_open_state_conflict"));
        }
        self.get(session_id)
            .await?
            .ok_or_else(|| GuestReaderError::internal("guest_session_disappeared"))
    }

    async fn mark_quarantine_deleted(
        &self,
        session_id: &str,
        now_ms: i64,
    ) -> Result<(), GuestReaderError> {
        sqlx::query(
            r#"
            UPDATE reader_guest_sessions
            SET quarantine_deleted_at_ms=COALESCE(quarantine_deleted_at_ms, ?),
                updated_at_ms=MAX(updated_at_ms, ?)
            WHERE session_id=?
            "#,
        )
        .bind(now_ms)
        .bind(now_ms)
        .bind(session_id.as_bytes())
        .execute(&self.pool)
        .await
        .map_err(sqlite_error)?;
        Ok(())
    }

    async fn mark_expired(
        &self,
        session_id: &str,
        now_ms: i64,
    ) -> Result<(), GuestReaderError> {
        sqlx::query(
            r#"
            UPDATE reader_guest_sessions
            SET state='expired', terminal_code=COALESCE(terminal_code, 'guest_session_expired'),
                updated_at_ms=MAX(updated_at_ms, ?)
            WHERE session_id=? AND expires_at_ms<=?
            "#,
        )
        .bind(now_ms)
        .bind(session_id.as_bytes())
        .bind(now_ms)
        .execute(&self.pool)
        .await
        .map_err(sqlite_error)?;
        Ok(())
    }

    async fn expired_pending(
        &self,
        now_ms: i64,
        limit: i64,
    ) -> Result<Vec<GuestReaderSession>, GuestReaderError> {
        let rows = sqlx::query(
            r#"
            SELECT session_id, access_token_hash, upload_id, reservation_id,
                   expected_byte_len, observed_byte_len, storage_generation,
                   object_etag, source_sha256, state, classification, scene_json,
                   terminal_code, created_at_ms, updated_at_ms, expires_at_ms,
                   quarantine_deleted_at_ms
            FROM reader_guest_sessions
            WHERE expires_at_ms<=?
              AND (state!='expired' OR quarantine_deleted_at_ms IS NULL)
            ORDER BY expires_at_ms ASC
            LIMIT ?
            "#,
        )
        .bind(now_ms)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlite_error)?;
        rows.iter().map(session_from_row).collect()
    }
}

fn session_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<GuestReaderSession, GuestReaderError> {
    let expected: i64 = row.try_get("expected_byte_len").map_err(sqlite_error)?;
    let observed: Option<i64> = row.try_get("observed_byte_len").map_err(sqlite_error)?;
    Ok(GuestReaderSession {
        session_id: bytes_string(row.try_get("session_id").map_err(sqlite_error)?)?,
        access_token_hash: row.try_get("access_token_hash").map_err(sqlite_error)?,
        upload_id: bytes_string(row.try_get("upload_id").map_err(sqlite_error)?)?,
        reservation_id: bytes_string(row.try_get("reservation_id").map_err(sqlite_error)?)?,
        expected_byte_len: u64::try_from(expected)
            .map_err(|_| GuestReaderError::internal("guest_expected_length_invalid"))?,
        observed_byte_len: observed
            .map(u64::try_from)
            .transpose()
            .map_err(|_| GuestReaderError::internal("guest_observed_length_invalid"))?,
        storage_generation: optional_bytes_string(
            row.try_get("storage_generation").map_err(sqlite_error)?,
        )?,
        object_etag: optional_bytes_string(row.try_get("object_etag").map_err(sqlite_error)?)?,
        source_sha256: optional_bytes_string(row.try_get("source_sha256").map_err(sqlite_error)?)?,
        state: GuestSessionState::parse(
            row.try_get::<String, _>("state")
                .map_err(sqlite_error)?
                .as_str(),
        )?,
        classification: row.try_get("classification").map_err(sqlite_error)?,
        scene_json: row.try_get("scene_json").map_err(sqlite_error)?,
        terminal_code: row.try_get("terminal_code").map_err(sqlite_error)?,
        created_at_ms: row.try_get("created_at_ms").map_err(sqlite_error)?,
        updated_at_ms: row.try_get("updated_at_ms").map_err(sqlite_error)?,
        expires_at_ms: row.try_get("expires_at_ms").map_err(sqlite_error)?,
        quarantine_deleted_at_ms: row
            .try_get("quarantine_deleted_at_ms")
            .map_err(sqlite_error)?,
    })
}

fn open_response_from_stored(
    session: &GuestReaderSession,
) -> Result<GuestJson<GuestOpenResponse>, GuestReaderError> {
    let scene = session
        .scene_json
        .as_deref()
        .map(serde_json::from_slice::<Value>)
        .transpose()
        .map_err(|_| GuestReaderError::internal("guest_scene_corrupt"))?;
    let classification = match session.classification.as_deref() {
        Some("supported") => "supported",
        Some("partial") => "partial",
        Some("unsupported") => "unsupported",
        Some("rejected") | None => "rejected",
        Some(_) => return Err(GuestReaderError::internal("guest_classification_invalid")),
    };
    Ok(GuestJson(GuestOpenResponse {
        protocol_version: GUEST_PROTOCOL_V1,
        session_id: session.session_id.clone(),
        classification: classification.to_owned(),
        expires_at_ms: session.expires_at_ms,
        source_sha256: session.source_sha256.clone(),
        terminal_code: session.terminal_code.clone(),
        scene,
    }))
}

fn admission_request(
    session: &GuestReaderSession,
) -> Result<UploadAdmissionRequest, GuestReaderError> {
    Ok(UploadAdmissionRequest {
        reservation_id: session.reservation_id.clone(),
        tenant_id: GUEST_SERVICE_TENANT_ID.to_owned(),
        principal_id: GUEST_SERVICE_PRINCIPAL_ID.to_owned(),
        expected_bytes: i64::try_from(session.expected_byte_len)
            .map_err(|_| GuestReaderError::internal("guest_expected_length_invalid"))?,
        request_hash: admission_request_hash(&session.session_id, session.expected_byte_len),
    })
}

fn admission_request_hash(session_id: &str, expected_byte_len: u64) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"chaptera.reader-guest-upload-admission.v1\0");
    hasher.update(session_id.as_bytes());
    hasher.update(expected_byte_len.to_be_bytes());
    format!("{:x}", hasher.finalize())
}

fn content_length(headers: &HeaderMap) -> Result<u64, GuestReaderError> {
    headers
        .get(CONTENT_LENGTH)
        .ok_or_else(|| GuestReaderError::bad_request("content_length_required"))?
        .to_str()
        .ok()
        .and_then(|raw| raw.parse::<u64>().ok())
        .ok_or_else(|| GuestReaderError::bad_request("content_length_invalid"))
}

fn random_id(prefix: &str) -> Result<String, GuestReaderError> {
    let mut bytes = [0_u8; 16];
    OsRng
        .try_fill_bytes(&mut bytes)
        .map_err(|_| GuestReaderError::internal("guest_random_failed"))?;
    Ok(format!("{prefix}:{}", hex_bytes(&bytes)))
}

fn random_token() -> Result<String, GuestReaderError> {
    let mut bytes = [0_u8; 32];
    OsRng
        .try_fill_bytes(&mut bytes)
        .map_err(|_| GuestReaderError::internal("guest_random_failed"))?;
    Ok(hex_bytes(&bytes))
}

fn token_hash(token: &[u8]) -> [u8; 32] {
    Sha256::digest(token).into()
}

fn constant_time_eq(stored: &[u8], expected: &[u8; 32]) -> bool {
    if stored.len() != expected.len() {
        return false;
    }
    stored
        .iter()
        .zip(expected.iter())
        .fold(0_u8, |diff, (left, right)| diff | (left ^ right))
        == 0
}

fn opaque_suffix(value: &str) -> &str {
    value.split_once(':').map(|(_, suffix)| suffix).unwrap_or(value)
}

fn hex_bytes(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn bytes_string(bytes: Vec<u8>) -> Result<String, GuestReaderError> {
    String::from_utf8(bytes).map_err(|_| GuestReaderError::internal("guest_session_row_invalid"))
}

fn optional_bytes_string(bytes: Option<Vec<u8>>) -> Result<Option<String>, GuestReaderError> {
    bytes.map(bytes_string).transpose()
}

fn now_ms() -> Result<i64, GuestReaderError> {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| GuestReaderError::internal("clock_invalid"))?
        .as_millis();
    i64::try_from(millis).map_err(|_| GuestReaderError::internal("clock_out_of_range"))
}

fn add_duration(now_ms: i64, duration: Duration) -> Result<i64, GuestReaderError> {
    let delta = i64::try_from(duration.as_millis())
        .map_err(|_| GuestReaderError::internal("duration_out_of_range"))?;
    now_ms
        .checked_add(delta)
        .ok_or_else(|| GuestReaderError::internal("clock_out_of_range"))
}

fn retry_after_seconds(retry_at_ms: i64) -> Option<u64> {
    let now = now_ms().ok()?;
    let remaining = retry_at_ms.saturating_sub(now);
    let millis = u64::try_from(remaining.max(1)).ok()?;
    Some(millis.saturating_add(999) / 1000)
}

fn map_admission_error(error: UploadAdmissionError) -> GuestReaderError {
    match error.code {
        "upload_bytes_too_large" => GuestReaderError::payload_too_large("guest_upload_too_large"),
        "upload_principal_capacity" | "upload_tenant_capacity" => {
            GuestReaderError::rate_limited(error.code, error.retry_at_ms.unwrap_or(0))
        }
        _ => GuestReaderError::internal("guest_upload_admission_failed"),
    }
}

fn map_blob_error(error: BlobStoreError) -> GuestReaderError {
    match error.code {
        "invalid_upload_size" | "blob_input_overflow" | "blob_input_length_mismatch" => {
            GuestReaderError::bad_request("guest_upload_length_mismatch")
        }
        _ => GuestReaderError::internal("guest_quarantine_failed"),
    }
}

fn map_rate_error(_error: PublicRateLimitError) -> GuestReaderError {
    GuestReaderError::internal("guest_public_rate_failed")
}

fn map_scan_error(_error: crate::source_ingress::IngressError) -> GuestReaderError {
    GuestReaderError::unprocessable("guest_source_scan_failed")
}

fn map_scene_worker_error(_error: GuestSceneWorkerError) -> GuestReaderError {
    GuestReaderError::internal("guest_scene_worker_failed")
}

fn sqlite_error(error: impl fmt::Display) -> GuestReaderError {
    let _ = error;
    GuestReaderError::internal("guest_session_sqlite_failed")
}

#[derive(Debug)]
pub struct GuestReaderError {
    status: StatusCode,
    code: &'static str,
    retry_after_seconds: Option<u64>,
}

impl GuestReaderError {
    fn new(status: StatusCode, code: &'static str) -> Self {
        Self {
            status,
            code,
            retry_after_seconds: None,
        }
    }

    fn bad_request(code: &'static str) -> Self {
        Self::new(StatusCode::BAD_REQUEST, code)
    }

    fn unauthorized(code: &'static str) -> Self {
        Self::new(StatusCode::UNAUTHORIZED, code)
    }

    fn not_found(code: &'static str) -> Self {
        Self::new(StatusCode::NOT_FOUND, code)
    }

    fn conflict(code: &'static str) -> Self {
        Self::new(StatusCode::CONFLICT, code)
    }

    fn gone(code: &'static str) -> Self {
        Self::new(StatusCode::GONE, code)
    }

    fn payload_too_large(code: &'static str) -> Self {
        Self::new(StatusCode::PAYLOAD_TOO_LARGE, code)
    }

    fn unprocessable(code: &'static str) -> Self {
        Self::new(StatusCode::UNPROCESSABLE_ENTITY, code)
    }

    fn internal(code: &'static str) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, code)
    }

    fn rate_limited(code: &'static str, retry_at_ms: i64) -> Self {
        Self {
            status: StatusCode::TOO_MANY_REQUESTS,
            code,
            retry_after_seconds: retry_after_seconds(retry_at_ms),
        }
    }
}

impl fmt::Display for GuestReaderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.code)
    }
}

impl std::error::Error for GuestReaderError {}

impl IntoResponse for GuestReaderError {
    fn into_response(self) -> Response {
        let mut response =
            (self.status, Json(serde_json::json!({ "error": self.code }))).into_response();
        if let Some(seconds) = self.retry_after_seconds
            && let Ok(value) = HeaderValue::from_str(&seconds.max(1).to_string())
        {
            response.headers_mut().insert(RETRY_AFTER, value);
        }
        response
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        sync::atomic::{AtomicU64, Ordering},
    };

    use super::*;
    use crate::schema_migration::SqliteMigrationRuntime;

    static NEXT_PATH: AtomicU64 = AtomicU64::new(1);

    fn temp_path(label: &str) -> PathBuf {
        let n = NEXT_PATH.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "chaptera-guest-reader-{label}-{}-{n}.sqlite",
            std::process::id()
        ))
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

    fn session(now_ms: i64) -> GuestReaderSession {
        let token = token_hash(b"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
        GuestReaderSession {
            session_id: "guest:0123456789abcdef0123456789abcdef".to_owned(),
            access_token_hash: token.to_vec(),
            upload_id: "guest-upload:0123456789abcdef0123456789abcdef".to_owned(),
            reservation_id: "guest-reservation:0123456789abcdef0123456789abcdef".to_owned(),
            expected_byte_len: 1024,
            observed_byte_len: None,
            storage_generation: None,
            object_etag: None,
            source_sha256: None,
            state: GuestSessionState::Issued,
            classification: None,
            scene_json: None,
            terminal_code: None,
            created_at_ms: now_ms,
            updated_at_ms: now_ms,
            expires_at_ms: now_ms + 60_000,
            quarantine_deleted_at_ms: None,
        }
    }

    #[tokio::test]
    async fn migration_seeds_non_loginable_service_principal_and_guest_table() {
        let path = migrated_path("schema").await;
        let options = SqliteConnectOptions::new().filename(&path);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .unwrap();

        let principal_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM principals WHERE principal_id=? AND disabled_at_ms IS NULL",
        )
        .bind(GUEST_SERVICE_PRINCIPAL_ID.as_bytes())
        .fetch_one(&pool)
        .await
        .unwrap();
        let identity_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM principal_identities WHERE principal_id=?")
                .bind(GUEST_SERVICE_PRINCIPAL_ID.as_bytes())
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(principal_count, 1);
        assert_eq!(identity_count, 0);

        pool.close().await;
        let _ = fs::remove_file(path);
    }

    #[tokio::test]
    async fn session_store_never_persists_plain_access_token() {
        let path = migrated_path("token").await;
        let store = SqliteGuestReaderSessionStore::open(&path, 1, Duration::from_secs(5))
            .await
            .unwrap();
        let session = session(1_000);
        store.insert(&session).await.unwrap();

        let bytes = fs::read(&path).unwrap();
        assert!(
            !String::from_utf8_lossy(&bytes)
                .contains("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
        );
        assert_eq!(store.get(&session.session_id).await.unwrap().unwrap().state, GuestSessionState::Issued);

        store.close().await;
        let _ = fs::remove_file(path);
    }

    #[tokio::test]
    async fn stored_to_opening_is_single_claim() {
        let path = migrated_path("claim").await;
        let store = SqliteGuestReaderSessionStore::open(&path, 2, Duration::from_secs(5))
            .await
            .unwrap();
        let session = session(1_000);
        store.insert(&session).await.unwrap();
        store
            .mark_stored(&session.session_id, 1024, "generation-1", "etag-1", 1_001)
            .await
            .unwrap();

        assert_eq!(
            store
                .claim_open(&session.session_id, 1_002)
                .await
                .unwrap()
                .state,
            GuestSessionState::Opening
        );
        assert!(store.claim_open(&session.session_id, 1_003).await.is_err());

        store.close().await;
        let _ = fs::remove_file(path);
    }

    #[test]
    fn token_compare_is_constant_shape_and_distinguishes_values() {
        let a = token_hash(b"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
        let b = token_hash(b"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb");
        assert!(constant_time_eq(&a, &a));
        assert!(!constant_time_eq(&a, &b));
    }
}
