use std::{
    env, fmt, fs as stdfs,
    fs::{File, OpenOptions},
    io::{BufWriter, Write},
    path::{Path, PathBuf},
    str::FromStr,
    time::{SystemTime, UNIX_EPOCH},
};

use axum::{
    Json, Router,
    extract::{Path as AxumPath, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
};
use axum_extra::extract::cookie::CookieJar;
use chaptera_cdm_model::AUTHORING_REVISION_SCHEMA_V1;
use chaptera_untrusted_pub_scan::install_post_read_filesystem_default_deny;
use pub_editor::{EditorEditableTarget, EditorSession, Sha256Digest, open_mature_0x2c_editor};
use rand::{RngCore, rngs::OsRng};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use tokio::{fs, io::AsyncWriteExt, process::Command, time::timeout};

use crate::{
    auth_http::{AuthHttpError, AuthHttpState},
    authz_runtime::{AuthzError, CAP_EXPORT, SqliteAuthzAuthority},
    blob_store::BlobStoreService,
    export_executor::{IDML_BOUNDED_EDITABLE_PROFILE, ODG_BOUNDED_EDITABLE_PROFILE},
    jobs_runtime::{CreateExportJobRequestV1, JobsRuntime, JobsRuntimeError},
    source_authority::{SourceAuthorityError, SqliteDocumentSourceAuthority},
    source_baseline::SourceBaselineProducerConfig,
    sqlite_store::{SqliteRevisionStore, SqliteStoreError},
};

pub const MIGRATION_EDITABLE_ROUTE_REQUEST_V1: &str =
    "chaptera.migration-editable-route-request.v1";
pub const MIGRATION_EDITABLE_ROUTE_RESPONSE_V1: &str =
    "chaptera.migration-editable-route-response.v1";
pub const MIGRATION_EXPORT_CREATE_REQUEST_V1: &str =
    "chaptera.migration-export-create.v1";
pub const MIGRATION_EXPORT_CREATE_RESPONSE_V1: &str =
    "chaptera.migration-export-job.v1";
const MIGRATION_EDITABLE_ROUTE_RECEIPT_V1: &str = "chaptera.migration-editable-route-receipt.v1";
const MIGRATION_EXPORT_ENVIRONMENT_V1: &str = "chaptera.migration-export-environment.v1";
const RECEIPT_MAX_BYTES: u64 = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationEditableRouteError {
    pub code: &'static str,
    pub message: String,
}

impl MigrationEditableRouteError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl fmt::Display for MigrationEditableRouteError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for MigrationEditableRouteError {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MigrationEditableTargetAssessmentV1 {
    pub state: String,
    pub reason_code: String,
    pub declared_loss_count: u64,
    pub blocking_loss_count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MigrationEditableRouteReceiptV1 {
    protocol_version: String,
    document_id: String,
    source_sha256: String,
    source_byte_len: u64,
    open_state: String,
    idml: MigrationEditableTargetAssessmentV1,
    odg: MigrationEditableTargetAssessmentV1,
    filesystem_confinement: bool,
}

#[derive(Clone)]
pub struct IsolatedMigrationEditableRouteProducer {
    config: SourceBaselineProducerConfig,
    blob_store: BlobStoreService,
}

impl IsolatedMigrationEditableRouteProducer {
    pub fn new(
        config: SourceBaselineProducerConfig,
        blob_store: BlobStoreService,
    ) -> Result<Self, MigrationEditableRouteError> {
        config.validate().map_err(|error| {
            MigrationEditableRouteError::new("migration_route_config_invalid", error.message)
        })?;
        Ok(Self { config, blob_store })
    }

    async fn assess(
        &self,
        tenant_id: &str,
        binding_id: &str,
        document_id: &str,
        expected_source_sha256: &str,
        expected_source_byte_len: u64,
    ) -> Result<MigrationEditableRouteReceiptV1, MigrationEditableRouteError> {
        require_ident(tenant_id, "tenant_id")?;
        require_ident(binding_id, "binding_id")?;
        require_ident(document_id, "document_id")?;
        require_sha256(expected_source_sha256, "expected_source_sha256")?;
        if expected_source_byte_len == 0 {
            return Err(MigrationEditableRouteError::new(
                "migration_route_length_invalid",
                "expected source byte length must be positive",
            ));
        }

        let temp = MigrationRouteTempDir::create(&self.config.temp_root).await?;
        let input_path = temp.path().join("source.pub");
        let output_dir = temp.path().join("worker-result");
        let mut output = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&input_path)
            .await
            .map_err(|_| {
                MigrationEditableRouteError::new(
                    "migration_route_temp_failed",
                    "could not create private migration capability input",
                )
            })?;

        let copied = self
            .blob_store
            .stream_binding_verified(tenant_id, binding_id, &mut output)
            .await
            .map_err(|error| MigrationEditableRouteError::new(error.code, error.message))?;
        output.flush().await.map_err(|_| {
            MigrationEditableRouteError::new(
                "migration_route_temp_failed",
                "could not flush private migration capability input",
            )
        })?;
        output.sync_all().await.map_err(|_| {
            MigrationEditableRouteError::new(
                "migration_route_temp_failed",
                "could not sync private migration capability input",
            )
        })?;
        drop(output);

        if copied != expected_source_byte_len {
            return Err(MigrationEditableRouteError::new(
                "migration_route_length_mismatch",
                "verified durable source length differs from source authority",
            ));
        }
        let input_bytes = fs::read(&input_path).await.map_err(|_| {
            MigrationEditableRouteError::new(
                "migration_route_temp_failed",
                "private migration capability input could not be verified",
            )
        })?;
        let actual_sha256 = format!("{:x}", Sha256::digest(&input_bytes));
        if actual_sha256 != expected_source_sha256 {
            return Err(MigrationEditableRouteError::new(
                "migration_route_hash_mismatch",
                "verified durable source hash differs from source authority",
            ));
        }
        drop(input_bytes);

        let timeout_seconds = self.config.worker_wall_timeout.as_secs_f64().to_string();
        let child = Command::new(&self.config.isolation_python)
            .arg(&self.config.isolation_harness)
            .arg("run")
            .arg("--output-dir")
            .arg(&output_dir)
            .arg("--input")
            .arg(&input_path)
            .arg("--timeout")
            .arg(timeout_seconds)
            .arg("--address-space-mb")
            .arg(self.config.worker_address_space_mb.to_string())
            .arg("--cpu-seconds")
            .arg(self.config.worker_cpu_seconds.to_string())
            .arg("--open-files")
            .arg(self.config.worker_open_files.to_string())
            .arg("--output-file-mb")
            .arg(self.config.worker_output_file_mb.to_string())
            .arg("--clear-environment")
            .arg("--")
            .arg(&self.config.worker_binary)
            .arg("migration-editable-routes")
            .arg("--document-id")
            .arg(document_id)
            .arg("--expected-sha256")
            .arg(expected_source_sha256)
            .arg("--expected-byte-len")
            .arg(expected_source_byte_len.to_string())
            .kill_on_drop(true)
            .output();

        let process_timeout = self
            .config
            .worker_wall_timeout
            .checked_add(std::time::Duration::from_secs(5))
            .ok_or_else(|| {
                MigrationEditableRouteError::new(
                    "migration_route_config_invalid",
                    "worker timeout overflow",
                )
            })?;
        let child_output = timeout(process_timeout, child)
            .await
            .map_err(|_| {
                MigrationEditableRouteError::new(
                    "migration_route_worker_timeout",
                    "isolated migration capability worker exceeded parent timeout",
                )
            })?
            .map_err(|_| {
                MigrationEditableRouteError::new(
                    "migration_route_worker_failed",
                    "isolated migration capability worker could not be started",
                )
            })?;
        if !child_output.status.success() {
            return Err(MigrationEditableRouteError::new(
                "migration_route_worker_failed",
                "isolated migration capability worker rejected or failed",
            ));
        }

        let receipt_path = output_dir.join("result.json");
        let metadata = fs::metadata(&receipt_path).await.map_err(|_| {
            MigrationEditableRouteError::new(
                "migration_route_receipt_missing",
                "isolated migration capability receipt is missing",
            )
        })?;
        if metadata.len() == 0 || metadata.len() > RECEIPT_MAX_BYTES {
            return Err(MigrationEditableRouteError::new(
                "migration_route_receipt_invalid",
                "isolated migration capability receipt size is invalid",
            ));
        }
        let bytes = fs::read(&receipt_path).await.map_err(|_| {
            MigrationEditableRouteError::new(
                "migration_route_receipt_missing",
                "isolated migration capability receipt could not be read",
            )
        })?;
        let receipt: MigrationEditableRouteReceiptV1 =
            serde_json::from_slice(&bytes).map_err(|_| {
                MigrationEditableRouteError::new(
                    "migration_route_receipt_invalid",
                    "isolated migration capability receipt is malformed",
                )
            })?;
        validate_receipt(
            &receipt,
            document_id,
            expected_source_sha256,
            expected_source_byte_len,
        )?;
        Ok(receipt)
    }
}

fn target_assessment(
    session: &EditorSession,
    target: EditorEditableTarget,
) -> MigrationEditableTargetAssessmentV1 {
    match session.preview_editable_export(target, "migration:source") {
        Ok(preview) => {
            let counts = preview.report.counts;
            let declared_loss_count = counts
                .approximated
                .saturating_add(counts.flattened)
                .saturating_add(counts.rasterized)
                .saturating_add(counts.unsupported);
            if preview.report.can_serialize && counts.blocking == 0 {
                MigrationEditableTargetAssessmentV1 {
                    state: "available_with_declared_losses".to_owned(),
                    reason_code: "serializable".to_owned(),
                    declared_loss_count,
                    blocking_loss_count: 0,
                }
            } else {
                MigrationEditableTargetAssessmentV1 {
                    state: "unavailable".to_owned(),
                    reason_code: "blocking_losses".to_owned(),
                    declared_loss_count,
                    blocking_loss_count: counts.blocking,
                }
            }
        }
        Err(_) => MigrationEditableTargetAssessmentV1 {
            state: "not_verified".to_owned(),
            reason_code: "assessment_failed".to_owned(),
            declared_loss_count: 0,
            blocking_loss_count: 0,
        },
    }
}

fn unavailable_target(reason_code: &str) -> MigrationEditableTargetAssessmentV1 {
    MigrationEditableTargetAssessmentV1 {
        state: "unavailable".to_owned(),
        reason_code: reason_code.to_owned(),
        declared_loss_count: 0,
        blocking_loss_count: 0,
    }
}

pub fn run_migration_editable_routes_worker(
    document_id: &str,
    expected_source_sha256: &str,
    expected_source_byte_len: u64,
) -> Result<(), MigrationEditableRouteError> {
    require_ident(document_id, "document_id")?;
    require_sha256(expected_source_sha256, "expected_source_sha256")?;
    if expected_source_byte_len == 0 {
        return Err(MigrationEditableRouteError::new(
            "migration_route_length_invalid",
            "expected source byte length must be positive",
        ));
    }

    let input_path = PathBuf::from(required_env("CHAPTERA_WORKER_INPUT")?);
    let output_root = PathBuf::from(required_env("CHAPTERA_WORKER_OUTPUT_DIR")?);
    stdfs::create_dir_all(&output_root).map_err(|_| {
        MigrationEditableRouteError::new(
            "migration_route_worker_output_failed",
            "migration capability worker output directory is unavailable",
        )
    })?;
    let output = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(output_root.join("result.json"))
        .map(BufWriter::new)
        .map_err(|_| {
            MigrationEditableRouteError::new(
                "migration_route_worker_output_failed",
                "migration capability worker result file could not be created",
            )
        })?;

    let metadata = stdfs::metadata(&input_path).map_err(|_| {
        MigrationEditableRouteError::new(
            "migration_route_worker_input_failed",
            "authorized migration capability input is unavailable",
        )
    })?;
    if metadata.len() != expected_source_byte_len {
        return Err(MigrationEditableRouteError::new(
            "migration_route_length_mismatch",
            "authorized migration capability input length differs from expected identity",
        ));
    }
    let source_bytes = stdfs::read(&input_path).map_err(|_| {
        MigrationEditableRouteError::new(
            "migration_route_worker_input_failed",
            "authorized migration capability input could not be read",
        )
    })?;
    let actual_sha256 = format!("{:x}", Sha256::digest(&source_bytes));
    if actual_sha256 != expected_source_sha256 {
        return Err(MigrationEditableRouteError::new(
            "migration_route_hash_mismatch",
            "authorized migration capability input hash differs from expected identity",
        ));
    }

    install_post_read_filesystem_default_deny().map_err(|_| {
        MigrationEditableRouteError::new(
            "migration_route_sandbox_failed",
            "post-read filesystem default-deny could not be installed",
        )
    })?;

    let source_hash = Sha256Digest::from_str(expected_source_sha256).map_err(|_| {
        MigrationEditableRouteError::new(
            "migration_route_hash_invalid",
            "expected source SHA-256 could not be parsed",
        )
    })?;

    let (open_state, idml, odg) = match open_mature_0x2c_editor(&source_bytes, source_hash) {
        Ok(session) => (
            "admitted".to_owned(),
            target_assessment(&session, EditorEditableTarget::Idml),
            target_assessment(&session, EditorEditableTarget::Odg),
        ),
        Err(_) => (
            "not_admitted".to_owned(),
            unavailable_target("editor_profile_unavailable"),
            unavailable_target("editor_profile_unavailable"),
        ),
    };

    let receipt = MigrationEditableRouteReceiptV1 {
        protocol_version: MIGRATION_EDITABLE_ROUTE_RECEIPT_V1.to_owned(),
        document_id: document_id.to_owned(),
        source_sha256: expected_source_sha256.to_owned(),
        source_byte_len: expected_source_byte_len,
        open_state,
        idml,
        odg,
        filesystem_confinement: true,
    };
    validate_receipt(
        &receipt,
        document_id,
        expected_source_sha256,
        expected_source_byte_len,
    )?;
    write_receipt(output, &receipt)
}

fn write_receipt(
    mut output: BufWriter<File>,
    receipt: &MigrationEditableRouteReceiptV1,
) -> Result<(), MigrationEditableRouteError> {
    serde_json::to_writer(&mut output, receipt).map_err(|_| {
        MigrationEditableRouteError::new(
            "migration_route_worker_output_failed",
            "migration capability receipt serialization failed",
        )
    })?;
    output.write_all(b"\n").map_err(|_| {
        MigrationEditableRouteError::new(
            "migration_route_worker_output_failed",
            "migration capability receipt write failed",
        )
    })?;
    output.flush().map_err(|_| {
        MigrationEditableRouteError::new(
            "migration_route_worker_output_failed",
            "migration capability receipt flush failed",
        )
    })
}

fn validate_receipt(
    receipt: &MigrationEditableRouteReceiptV1,
    document_id: &str,
    expected_source_sha256: &str,
    expected_source_byte_len: u64,
) -> Result<(), MigrationEditableRouteError> {
    if receipt.protocol_version != MIGRATION_EDITABLE_ROUTE_RECEIPT_V1
        || receipt.document_id != document_id
        || receipt.source_sha256 != expected_source_sha256
        || receipt.source_byte_len != expected_source_byte_len
        || !receipt.filesystem_confinement
        || !matches!(receipt.open_state.as_str(), "admitted" | "not_admitted")
    {
        return Err(MigrationEditableRouteError::new(
            "migration_route_receipt_identity_mismatch",
            "migration capability receipt differs from authorized source identity/profile",
        ));
    }
    validate_target(&receipt.idml)?;
    validate_target(&receipt.odg)?;
    Ok(())
}

fn validate_target(
    target: &MigrationEditableTargetAssessmentV1,
) -> Result<(), MigrationEditableRouteError> {
    match target.state.as_str() {
        "available_with_declared_losses" => {
            if target.reason_code != "serializable" || target.blocking_loss_count != 0 {
                return Err(MigrationEditableRouteError::new(
                    "migration_route_receipt_invalid",
                    "available editable route has invalid reason/blocking counts",
                ));
            }
        }
        "unavailable" => {
            if !matches!(
                target.reason_code.as_str(),
                "blocking_losses" | "editor_profile_unavailable"
            ) {
                return Err(MigrationEditableRouteError::new(
                    "migration_route_receipt_invalid",
                    "unavailable editable route has an unsupported reason",
                ));
            }
        }
        "not_verified" => {
            if target.reason_code != "assessment_failed"
                || target.declared_loss_count != 0
                || target.blocking_loss_count != 0
            {
                return Err(MigrationEditableRouteError::new(
                    "migration_route_receipt_invalid",
                    "not-verified editable route has invalid public evidence",
                ));
            }
        }
        _ => {
            return Err(MigrationEditableRouteError::new(
                "migration_route_receipt_invalid",
                "editable route state is unsupported",
            ));
        }
    }
    Ok(())
}

fn required_env(name: &str) -> Result<String, MigrationEditableRouteError> {
    env::var(name).map_err(|_| {
        MigrationEditableRouteError::new(
            "migration_route_worker_environment_missing",
            format!("{name} is required"),
        )
    })
}

fn require_ident(value: &str, label: &str) -> Result<(), MigrationEditableRouteError> {
    if value.is_empty()
        || value.len() > 256
        || value
            .chars()
            .any(|ch| ch.is_control() || ch.is_whitespace())
    {
        return Err(MigrationEditableRouteError::new(
            "migration_route_identity_invalid",
            format!("{label} must be a bounded non-whitespace identity"),
        ));
    }
    Ok(())
}

fn require_sha256(value: &str, label: &str) -> Result<(), MigrationEditableRouteError> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        return Err(MigrationEditableRouteError::new(
            "migration_route_identity_invalid",
            format!("{label} must be 64 lowercase SHA-256 hex characters"),
        ));
    }
    Ok(())
}

struct MigrationRouteTempDir {
    path: PathBuf,
}

impl MigrationRouteTempDir {
    async fn create(root: &Path) -> Result<Self, MigrationEditableRouteError> {
        fs::create_dir_all(root).await.map_err(|_| {
            MigrationEditableRouteError::new(
                "migration_route_temp_failed",
                "migration capability temp root is unavailable",
            )
        })?;

        for _ in 0..8 {
            let mut random = [0_u8; 16];
            OsRng.try_fill_bytes(&mut random).map_err(|_| {
                MigrationEditableRouteError::new(
                    "migration_route_random_failed",
                    "migration capability temp identity generation failed",
                )
            })?;
            let path = root.join(format!(
                "chaptera-migration-route-{:032x}",
                u128::from_be_bytes(random)
            ));
            match fs::create_dir(&path).await {
                Ok(()) => {
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        fs::set_permissions(&path, stdfs::Permissions::from_mode(0o700))
                            .await
                            .map_err(|_| {
                                MigrationEditableRouteError::new(
                                    "migration_route_temp_failed",
                                    "migration capability temp permissions could not be constrained",
                                )
                            })?;
                    }
                    return Ok(Self { path });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(_) => {
                    return Err(MigrationEditableRouteError::new(
                        "migration_route_temp_failed",
                        "migration capability temp directory could not be created",
                    ));
                }
            }
        }
        Err(MigrationEditableRouteError::new(
            "migration_route_temp_failed",
            "migration capability temp collision budget exhausted",
        ))
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for MigrationRouteTempDir {
    fn drop(&mut self) {
        let _ = stdfs::remove_dir_all(&self.path);
    }
}

#[derive(Clone)]
pub struct MigrationEditableRouteHttpState {
    auth: AuthHttpState,
    source: SqliteDocumentSourceAuthority,
    authz: SqliteAuthzAuthority,
    revisions: SqliteRevisionStore,
    jobs: JobsRuntime,
    producer: IsolatedMigrationEditableRouteProducer,
}

impl MigrationEditableRouteHttpState {
    pub fn new(
        auth: AuthHttpState,
        source: SqliteDocumentSourceAuthority,
        authz: SqliteAuthzAuthority,
        revisions: SqliteRevisionStore,
        jobs: JobsRuntime,
        producer: IsolatedMigrationEditableRouteProducer,
    ) -> Self {
        Self {
            auth,
            source,
            authz,
            revisions,
            jobs,
            producer,
        }
    }
}

pub fn router(state: MigrationEditableRouteHttpState) -> Router {
    Router::new()
        .route(
            "/v1/migration/documents/{document_id}/editable-routes",
            post(assess_editable_routes),
        )
        .route(
            "/v1/migration/documents/{document_id}/exports",
            post(create_migration_export),
        )
        .with_state(state)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MigrationEditableRouteRequestV1 {
    protocol_version: String,
    document_id: String,
    source_sha256: String,
}

#[derive(Debug, Clone, Serialize)]
struct MigrationEditableRouteResponseV1 {
    protocol_version: &'static str,
    document_id: String,
    source_sha256: String,
    source_byte_len: u64,
    open_state: String,
    idml: MigrationEditableTargetAssessmentV1,
    odg: MigrationEditableTargetAssessmentV1,
}


#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MigrationExportCreateRequestV1 {
    protocol_version: String,
    document_id: String,
    source_sha256: String,
    target: String,
    client_request_id: String,
}

#[derive(Debug, Clone, Serialize)]
struct MigrationExportCreateResponseV1 {
    protocol_version: &'static str,
    document_id: String,
    source_sha256: String,
    target: String,
    target_profile: String,
    revision_id: String,
    job_id: String,
    status: String,
    declared_loss_count: u64,
    blocking_loss_count: u64,
}

async fn create_migration_export(
    State(state): State<MigrationEditableRouteHttpState>,
    AxumPath(document_id): AxumPath<String>,
    headers: HeaderMap,
    jar: CookieJar,
    Json(request): Json<MigrationExportCreateRequestV1>,
) -> Result<Json<MigrationExportCreateResponseV1>, MigrationEditableRouteHttpError> {
    validate_export_create_request(&request, &document_id)?;

    let principal = state
        .auth
        .authenticate_mutation_request(&headers, &jar)
        .await
        .map_err(MigrationEditableRouteHttpError::Auth)?;

    let source = state
        .source
        .resolve_by_document_id(&document_id)
        .await
        .map_err(MigrationEditableRouteHttpError::Source)?;
    if source.source_sha256 != request.source_sha256 {
        return Err(MigrationEditableRouteHttpError::conflict(
            "migration_export_source_hash_mismatch",
            "requested source_sha256 differs from durable source authority",
        ));
    }

    let now = now_ms()?;
    state
        .authz
        .authorize(
            &source.tenant_id,
            &document_id,
            &principal.principal_id,
            CAP_EXPORT,
            "migration:export-preflight",
            now,
        )
        .await
        .map_err(MigrationEditableRouteHttpError::Authz)?;

    let receipt = state
        .producer
        .assess(
            &source.tenant_id,
            &source.binding_id,
            &document_id,
            &source.source_sha256,
            source.byte_len,
        )
        .await
        .map_err(MigrationEditableRouteHttpError::Producer)?;

    let (target_profile, assessment) = admitted_target(&receipt, &request.target)?;

    let binding = state
        .revisions
        .require_revision_identity(&document_id, &source.baseline_revision_id)
        .await
        .map_err(MigrationEditableRouteHttpError::Store)?;
    if binding.canonical_schema_version != AUTHORING_REVISION_SCHEMA_V1 {
        return Err(MigrationEditableRouteHttpError::conflict(
            "migration_export_revision_schema_unsupported",
            "baseline revision is not bound to the supported canonical AuthoringRevision schema",
        ));
    }

    let snapshot = state
        .jobs
        .create_export(CreateExportJobRequestV1 {
            tenant_id: source.tenant_id,
            document_id: document_id.clone(),
            principal_id: principal.principal_id,
            exact_revision_id: source.baseline_revision_id.clone(),
            canonical_authoring_revision_id: binding.canonical_revision_id,
            target_profile: target_profile.to_owned(),
            layout_environment_id: migration_export_environment_id(target_profile),
            client_request_id: request.client_request_id.clone(),
            operation_id: format!("migration:export:create:{}", request.client_request_id),
            now_ms: now,
        })
        .await
        .map_err(MigrationEditableRouteHttpError::Jobs)?;

    Ok(Json(MigrationExportCreateResponseV1 {
        protocol_version: MIGRATION_EXPORT_CREATE_RESPONSE_V1,
        document_id,
        source_sha256: source.source_sha256,
        target: request.target,
        target_profile: target_profile.to_owned(),
        revision_id: source.baseline_revision_id,
        job_id: snapshot.job_id,
        status: snapshot.status,
        declared_loss_count: assessment.declared_loss_count,
        blocking_loss_count: assessment.blocking_loss_count,
    }))
}

fn validate_export_create_request(
    request: &MigrationExportCreateRequestV1,
    path_document_id: &str,
) -> Result<(), MigrationEditableRouteHttpError> {
    if request.protocol_version != MIGRATION_EXPORT_CREATE_REQUEST_V1 {
        return Err(MigrationEditableRouteHttpError::bad_request(
            "migration_export_protocol_invalid",
            "chaptera.migration-export-create.v1 is required",
        ));
    }
    if request.document_id != path_document_id {
        return Err(MigrationEditableRouteHttpError::bad_request(
            "migration_export_document_mismatch",
            "path document_id differs from request document_id",
        ));
    }
    require_ident(&request.document_id, "document_id")
        .map_err(MigrationEditableRouteHttpError::Producer)?;
    require_sha256(&request.source_sha256, "source_sha256")
        .map_err(MigrationEditableRouteHttpError::Producer)?;
    require_client_request_id(&request.client_request_id)?;
    target_profile(&request.target)?;
    Ok(())
}

fn require_client_request_id(
    value: &str,
) -> Result<(), MigrationEditableRouteHttpError> {
    if value.is_empty()
        || value.len() > 160
        || !value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'-')
        })
    {
        return Err(MigrationEditableRouteHttpError::bad_request(
            "migration_export_client_request_id_invalid",
            "client_request_id must be a bounded opaque identifier",
        ));
    }
    Ok(())
}

fn target_profile(target: &str) -> Result<&'static str, MigrationEditableRouteHttpError> {
    match target {
        "idml" => Ok(IDML_BOUNDED_EDITABLE_PROFILE),
        "odg" => Ok(ODG_BOUNDED_EDITABLE_PROFILE),
        _ => Err(MigrationEditableRouteHttpError::bad_request(
            "migration_export_target_invalid",
            "target must be idml or odg",
        )),
    }
}

fn admitted_target<'a>(
    receipt: &'a MigrationEditableRouteReceiptV1,
    target: &str,
) -> Result<(&'static str, &'a MigrationEditableTargetAssessmentV1), MigrationEditableRouteHttpError>
{
    let profile = target_profile(target)?;
    let assessment = match target {
        "idml" => &receipt.idml,
        "odg" => &receipt.odg,
        _ => unreachable!("target_profile validated target"),
    };
    if assessment.state != "available_with_declared_losses"
        || assessment.blocking_loss_count != 0
    {
        return Err(MigrationEditableRouteHttpError::conflict(
            "migration_export_route_unavailable",
            format!(
                "{target} route is {}; capability assessment must be available before materialization",
                assessment.state
            ),
        ));
    }
    Ok((profile, assessment))
}

fn migration_export_environment_id(target_profile: &str) -> String {
    let payload = format!("{MIGRATION_EXPORT_ENVIRONMENT_V1}\0{target_profile}");
    format!("sha256:{:x}", Sha256::digest(payload.as_bytes()))
}

async fn assess_editable_routes(
    State(state): State<MigrationEditableRouteHttpState>,
    AxumPath(document_id): AxumPath<String>,
    headers: HeaderMap,
    jar: CookieJar,
    Json(request): Json<MigrationEditableRouteRequestV1>,
) -> Result<Json<MigrationEditableRouteResponseV1>, MigrationEditableRouteHttpError> {
    validate_request(&request, &document_id)?;

    let principal = state
        .auth
        .authenticate_mutation_request(&headers, &jar)
        .await
        .map_err(MigrationEditableRouteHttpError::Auth)?;

    let source = state
        .source
        .resolve_by_document_id(&document_id)
        .await
        .map_err(MigrationEditableRouteHttpError::Source)?;
    if source.source_sha256 != request.source_sha256 {
        return Err(MigrationEditableRouteHttpError::conflict(
            "migration_route_source_hash_mismatch",
            "requested source_sha256 differs from durable source authority",
        ));
    }

    state
        .authz
        .authorize(
            &source.tenant_id,
            &document_id,
            &principal.principal_id,
            CAP_EXPORT,
            "migration:editable-routes",
            now_ms()?,
        )
        .await
        .map_err(MigrationEditableRouteHttpError::Authz)?;

    let receipt = state
        .producer
        .assess(
            &source.tenant_id,
            &source.binding_id,
            &document_id,
            &source.source_sha256,
            source.byte_len,
        )
        .await
        .map_err(MigrationEditableRouteHttpError::Producer)?;

    Ok(Json(MigrationEditableRouteResponseV1 {
        protocol_version: MIGRATION_EDITABLE_ROUTE_RESPONSE_V1,
        document_id: receipt.document_id,
        source_sha256: receipt.source_sha256,
        source_byte_len: receipt.source_byte_len,
        open_state: receipt.open_state,
        idml: receipt.idml,
        odg: receipt.odg,
    }))
}

fn validate_request(
    request: &MigrationEditableRouteRequestV1,
    path_document_id: &str,
) -> Result<(), MigrationEditableRouteHttpError> {
    if request.protocol_version != MIGRATION_EDITABLE_ROUTE_REQUEST_V1 {
        return Err(MigrationEditableRouteHttpError::bad_request(
            "migration_route_protocol_invalid",
            "chaptera.migration-editable-route-request.v1 is required",
        ));
    }
    if request.document_id != path_document_id {
        return Err(MigrationEditableRouteHttpError::bad_request(
            "migration_route_document_mismatch",
            "path document_id differs from request document_id",
        ));
    }
    require_ident(&request.document_id, "document_id")
        .map_err(MigrationEditableRouteHttpError::Producer)?;
    require_sha256(&request.source_sha256, "source_sha256")
        .map_err(MigrationEditableRouteHttpError::Producer)?;
    Ok(())
}

fn now_ms() -> Result<i64, MigrationEditableRouteHttpError> {
    let elapsed = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|_| {
        MigrationEditableRouteHttpError::internal(
            "clock_before_epoch",
            "system clock is before UNIX epoch",
        )
    })?;
    i64::try_from(elapsed.as_millis()).map_err(|_| {
        MigrationEditableRouteHttpError::internal("clock_overflow", "system clock does not fit i64")
    })
}

enum MigrationEditableRouteHttpError {
    Auth(AuthHttpError),
    Authz(AuthzError),
    Source(SourceAuthorityError),
    Store(SqliteStoreError),
    Jobs(JobsRuntimeError),
    Producer(MigrationEditableRouteError),
    Http {
        status: StatusCode,
        code: &'static str,
        message: String,
    },
}

impl MigrationEditableRouteHttpError {
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

    fn internal(code: &'static str, message: impl Into<String>) -> Self {
        Self::Http {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            code,
            message: message.into(),
        }
    }
}

impl IntoResponse for MigrationEditableRouteHttpError {
    fn into_response(self) -> Response {
        match self {
            Self::Auth(error) => error.into_response(),
            Self::Authz(error) => {
                let status = match error.code {
                    "authz_denied" | "authz_expired" => StatusCode::FORBIDDEN,
                    _ => StatusCode::INTERNAL_SERVER_ERROR,
                };
                (
                    status,
                    Json(json!({"error":{"code":error.code,"message":error.message}})),
                )
                    .into_response()
            }
            Self::Source(error) => {
                let status = if error.code == "document_source_not_found" {
                    StatusCode::NOT_FOUND
                } else {
                    StatusCode::INTERNAL_SERVER_ERROR
                };
                (
                    status,
                    Json(json!({"error":{"code":error.code,"message":error.message}})),
                )
                    .into_response()
            }
            Self::Store(error) => {
                let status = if error.code == "canonical_revision_unbound" {
                    StatusCode::CONFLICT
                } else {
                    StatusCode::INTERNAL_SERVER_ERROR
                };
                (
                    status,
                    Json(json!({"error":{"code":error.code,"message":error.message}})),
                )
                    .into_response()
            }
            Self::Jobs(error) => {
                let status = match error.code {
                    "grant_missing" | "authz_denied" | "authz_expired" => StatusCode::FORBIDDEN,
                    "idempotency_conflict"
                    | "job_scope_mismatch"
                    | "job_payload_scope_mismatch" => StatusCode::CONFLICT,
                    "invalid_client_request_id" | "invalid_operation_id" => {
                        StatusCode::BAD_REQUEST
                    }
                    _ => StatusCode::INTERNAL_SERVER_ERROR,
                };
                (
                    status,
                    Json(json!({"error":{"code":error.code,"message":error.message}})),
                )
                    .into_response()
            }
            Self::Producer(error) => {
                let status = match error.code {
                    "migration_route_identity_invalid" | "migration_route_length_invalid" => {
                        StatusCode::BAD_REQUEST
                    }
                    "migration_route_worker_timeout" => StatusCode::SERVICE_UNAVAILABLE,
                    _ => StatusCode::INTERNAL_SERVER_ERROR,
                };
                (
                    status,
                    Json(json!({"error":{"code":error.code,"message":error.message}})),
                )
                    .into_response()
            }
            Self::Http {
                status,
                code,
                message,
            } => (
                status,
                Json(json!({"error":{"code":code,"message":message}})),
            )
                .into_response(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOCUMENT_ID: &str = "document:one";
    const SOURCE_SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    #[test]
    fn migration_export_request_rejects_browser_revision_authority() {
        let value = json!({
            "protocol_version": MIGRATION_EXPORT_CREATE_REQUEST_V1,
            "document_id": DOCUMENT_ID,
            "source_sha256": SOURCE_SHA,
            "target": "idml",
            "client_request_id": "migration-one",
            "revision_id": "browser-chosen-revision",
            "canonical_authoring_revision_id": "c".repeat(64),
            "layout_environment_id": format!("sha256:{}", "d".repeat(64))
        });
        assert!(serde_json::from_value::<MigrationExportCreateRequestV1>(value).is_err());
    }

    #[test]
    fn migration_export_environment_is_stable_and_target_scoped() {
        let idml_one = migration_export_environment_id(IDML_BOUNDED_EDITABLE_PROFILE);
        let idml_two = migration_export_environment_id(IDML_BOUNDED_EDITABLE_PROFILE);
        let odg = migration_export_environment_id(ODG_BOUNDED_EDITABLE_PROFILE);
        assert_eq!(idml_one, idml_two);
        assert_ne!(idml_one, odg);
        assert!(idml_one.starts_with("sha256:"));
        assert_eq!(idml_one.len(), 71);
    }

    #[test]
    fn migration_export_requires_available_exact_target() {
        let receipt = MigrationEditableRouteReceiptV1 {
            protocol_version: MIGRATION_EDITABLE_ROUTE_RECEIPT_V1.into(),
            document_id: DOCUMENT_ID.into(),
            source_sha256: SOURCE_SHA.into(),
            source_byte_len: 123,
            open_state: "admitted".into(),
            idml: MigrationEditableTargetAssessmentV1 {
                state: "available_with_declared_losses".into(),
                reason_code: "serializable".into(),
                declared_loss_count: 4,
                blocking_loss_count: 0,
            },
            odg: MigrationEditableTargetAssessmentV1 {
                state: "unavailable".into(),
                reason_code: "blocking_losses".into(),
                declared_loss_count: 2,
                blocking_loss_count: 1,
            },
            filesystem_confinement: true,
        };

        let (profile, assessment) = admitted_target(&receipt, "idml").unwrap();
        assert_eq!(profile, IDML_BOUNDED_EDITABLE_PROFILE);
        assert_eq!(assessment.declared_loss_count, 4);

        let error = admitted_target(&receipt, "odg").unwrap_err();
        match error {
            MigrationEditableRouteHttpError::Http { code, .. } => {
                assert_eq!(code, "migration_export_route_unavailable");
            }
            _ => panic!("expected bounded HTTP conflict"),
        }
    }

    #[test]
    fn request_rejects_browser_authority_fields() {
        let value = json!({
            "protocol_version": MIGRATION_EDITABLE_ROUTE_REQUEST_V1,
            "document_id": DOCUMENT_ID,
            "source_sha256": SOURCE_SHA,
            "tenant_id": "tenant:browser",
            "target": "idml",
            "materialize": true
        });
        assert!(serde_json::from_value::<MigrationEditableRouteRequestV1>(value).is_err());
    }

    #[test]
    fn receipt_requires_exact_source_identity_and_confinement() {
        let receipt = MigrationEditableRouteReceiptV1 {
            protocol_version: MIGRATION_EDITABLE_ROUTE_RECEIPT_V1.into(),
            document_id: DOCUMENT_ID.into(),
            source_sha256: SOURCE_SHA.into(),
            source_byte_len: 123,
            open_state: "admitted".into(),
            idml: MigrationEditableTargetAssessmentV1 {
                state: "available_with_declared_losses".into(),
                reason_code: "serializable".into(),
                declared_loss_count: 3,
                blocking_loss_count: 0,
            },
            odg: MigrationEditableTargetAssessmentV1 {
                state: "unavailable".into(),
                reason_code: "blocking_losses".into(),
                declared_loss_count: 2,
                blocking_loss_count: 1,
            },
            filesystem_confinement: true,
        };
        validate_receipt(&receipt, DOCUMENT_ID, SOURCE_SHA, 123).unwrap();

        let mut wrong = receipt.clone();
        wrong.source_sha256 = "b".repeat(64);
        assert_eq!(
            validate_receipt(&wrong, DOCUMENT_ID, SOURCE_SHA, 123)
                .unwrap_err()
                .code,
            "migration_route_receipt_identity_mismatch"
        );

        let mut unconfined = receipt;
        unconfined.filesystem_confinement = false;
        assert_eq!(
            validate_receipt(&unconfined, DOCUMENT_ID, SOURCE_SHA, 123)
                .unwrap_err()
                .code,
            "migration_route_receipt_identity_mismatch"
        );
    }

    #[test]
    fn unavailable_profile_never_claims_target_bytes_or_loss_detail() {
        let target = unavailable_target("editor_profile_unavailable");
        assert_eq!(target.state, "unavailable");
        assert_eq!(target.declared_loss_count, 0);
        assert_eq!(target.blocking_loss_count, 0);
    }
}
