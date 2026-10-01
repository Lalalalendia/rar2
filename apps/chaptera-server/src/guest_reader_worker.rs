use std::{
    env, fmt,
    fs::{self as stdfs, File, OpenOptions},
    io::{BufWriter, Write},
    path::{Path, PathBuf},
    time::Duration,
};

use chaptera_failure_intake_protocol::FailureClassificationV1;
use chaptera_untrusted_pub_scan::install_post_read_filesystem_default_deny;
use pub_viewer::{
    ViewerProductOpenOutcome, open_pub_bundle, open_pub_or_salvage,
    viewer_geometry_environment_v0_1,
};
use rand::{RngCore, rngs::OsRng};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::{
    fs,
    io::{AsyncReadExt, AsyncWriteExt},
    process::Command,
    time::timeout,
};

use crate::{
    blob_store::BlobStoreService,
    guest_intake_classifier::guest_failure_intake_evidence,
    reader_scene_v1::{
        ReaderTextFontProbeResource, from_viewer_geometry, from_viewer_geometry_with_font_probe,
    },
    source_ingress_security::SourceSecurityScannerConfig,
};

pub const GUEST_SCENE_WORKER_V1: &str = "chaptera.reader-guest-scene-worker.v1";
const MAX_RECEIPT_BYTES: u64 = 18 * 1024 * 1024;
const MAX_FONT_PROBE_BYTES: u64 = 4 * 1024 * 1024;
const COPY_BUFFER_BYTES: usize = 64 * 1024;
const GUEST_SCENE_FONT_PROBE_FAMILY_NAME: &str = "Caladea";

#[derive(Debug, Clone)]
pub struct GuestSceneFontProbeConfig {
    pub path: PathBuf,
    pub expected_sha256: String,
    pub source_family_sha256: String,
    pub resource_id: String,
}

#[derive(Debug)]
struct LoadedGuestSceneFontProbe {
    expected_sha256: String,
    source_family_sha256: String,
    resource_id: String,
    bytes: Vec<u8>,
}

impl LoadedGuestSceneFontProbe {
    fn load(config: &GuestSceneFontProbeConfig) -> Result<Self, GuestSceneWorkerError> {
        require_sha256(&config.expected_sha256)?;
        require_sha256(&config.source_family_sha256)?;
        require_ident(&config.resource_id, "probe_font_resource_id")?;
        let metadata = stdfs::metadata(&config.path).map_err(|_| {
            GuestSceneWorkerError::new(
                "guest_scene_font_probe_missing",
                "font probe input is unavailable",
            )
        })?;
        if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_FONT_PROBE_BYTES {
            return Err(GuestSceneWorkerError::new(
                "guest_scene_font_probe_invalid",
                "font probe input size is invalid",
            ));
        }
        let bytes = stdfs::read(&config.path).map_err(|_| {
            GuestSceneWorkerError::new(
                "guest_scene_font_probe_missing",
                "font probe input could not be read",
            )
        })?;
        let actual_sha256 = format!("{:x}", Sha256::digest(&bytes));
        if actual_sha256 != config.expected_sha256 {
            return Err(GuestSceneWorkerError::new(
                "guest_scene_font_probe_hash_mismatch",
                "font probe bytes differ from expected fingerprint",
            ));
        }
        Ok(Self {
            expected_sha256: config.expected_sha256.clone(),
            source_family_sha256: config.source_family_sha256.clone(),
            resource_id: config.resource_id.clone(),
            bytes,
        })
    }

    fn as_scene_resource(&self) -> ReaderTextFontProbeResource<'_> {
        ReaderTextFontProbeResource {
            resource_id: &self.resource_id,
            family_name: GUEST_SCENE_FONT_PROBE_FAMILY_NAME,
            expected_sha256: &self.expected_sha256,
            source_family_sha256: &self.source_family_sha256,
            face_index: 0,
            bytes: &self.bytes,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuestSceneWorkerError {
    pub code: &'static str,
    pub message: String,
}

impl GuestSceneWorkerError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl fmt::Display for GuestSceneWorkerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for GuestSceneWorkerError {}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GuestSceneWorkerReceiptV1 {
    pub protocol_version: String,
    pub session_id: String,
    pub source_sha256: String,
    pub source_byte_len: u64,
    pub classification: String,
    pub terminal_code: Option<String>,
    pub scene: Option<Value>,
    pub salvage: Option<Value>,
    pub failure_classification: Option<FailureClassificationV1>,
    pub filesystem_confinement: bool,
}

#[derive(Clone)]
pub struct IsolatedGuestSceneProducer {
    config: SourceSecurityScannerConfig,
    worker_binary: PathBuf,
}

impl IsolatedGuestSceneProducer {
    pub fn new(config: SourceSecurityScannerConfig) -> Result<Self, GuestSceneWorkerError> {
        config
            .validate()
            .map_err(|error| GuestSceneWorkerError::new(error.code, error.message))?;
        let worker_binary = env::current_exe().map_err(|_| {
            GuestSceneWorkerError::new(
                "guest_scene_worker_binary_missing",
                "current chaptera executable path is unavailable",
            )
        })?;
        Ok(Self {
            config,
            worker_binary,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn produce_from_quarantine(
        &self,
        blob_store: &BlobStoreService,
        tenant_id: &str,
        upload_id: &str,
        expected_generation: &str,
        expected_etag: &str,
        expected_byte_len: u64,
        session_id: &str,
    ) -> Result<GuestSceneWorkerReceiptV1, GuestSceneWorkerError> {
        require_ident(session_id, "session_id")?;
        if expected_byte_len == 0 || expected_byte_len > self.config.policy.max_file_bytes {
            return Err(GuestSceneWorkerError::new(
                "guest_scene_length_invalid",
                "guest scene input length is outside scanner policy",
            ));
        }

        let temp = GuestSceneTempDir::create(&self.config.temp_root).await?;
        let input_path = temp.path().join("source.pub");
        let output_dir = temp.path().join("worker-result");
        let mut source = blob_store
            .open_quarantine_exact(
                tenant_id,
                upload_id,
                expected_generation,
                expected_etag,
                expected_byte_len,
            )
            .await
            .map_err(|error| GuestSceneWorkerError::new(error.code, error.message))?;
        let mut output = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&input_path)
            .await
            .map_err(|_| {
                GuestSceneWorkerError::new(
                    "guest_scene_temp_failed",
                    "could not create private guest scene input",
                )
            })?;

        let mut copied = 0_u64;
        let mut hasher = Sha256::new();
        let mut buffer = vec![0_u8; COPY_BUFFER_BYTES];
        loop {
            let count = source.read(&mut buffer).await.map_err(|_| {
                GuestSceneWorkerError::new(
                    "guest_scene_quarantine_read_failed",
                    "exact quarantine input could not be read",
                )
            })?;
            if count == 0 {
                break;
            }
            copied = copied.checked_add(count as u64).ok_or_else(|| {
                GuestSceneWorkerError::new(
                    "guest_scene_length_overflow",
                    "guest scene copy length overflowed",
                )
            })?;
            if copied > expected_byte_len {
                return Err(GuestSceneWorkerError::new(
                    "guest_scene_length_mismatch",
                    "exact quarantine input exceeded expected length",
                ));
            }
            hasher.update(&buffer[..count]);
            output.write_all(&buffer[..count]).await.map_err(|_| {
                GuestSceneWorkerError::new(
                    "guest_scene_temp_failed",
                    "could not write private guest scene input",
                )
            })?;
        }
        output.flush().await.map_err(|_| {
            GuestSceneWorkerError::new(
                "guest_scene_temp_failed",
                "could not flush private guest scene input",
            )
        })?;
        output.sync_all().await.map_err(|_| {
            GuestSceneWorkerError::new(
                "guest_scene_temp_failed",
                "could not sync private guest scene input",
            )
        })?;
        drop(output);

        if copied != expected_byte_len {
            return Err(GuestSceneWorkerError::new(
                "guest_scene_length_mismatch",
                "exact quarantine input length differs from authority",
            ));
        }
        let expected_sha256 = format!("{:x}", hasher.finalize());

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
            .arg(&self.worker_binary)
            .arg("guest-reader-scene")
            .arg("--session-id")
            .arg(session_id)
            .arg("--expected-sha256")
            .arg(&expected_sha256)
            .arg("--expected-byte-len")
            .arg(expected_byte_len.to_string())
            .kill_on_drop(true)
            .output();

        let process_timeout = self
            .config
            .worker_wall_timeout
            .checked_add(Duration::from_secs(5))
            .ok_or_else(|| {
                GuestSceneWorkerError::new(
                    "guest_scene_config_invalid",
                    "guest scene worker timeout overflowed",
                )
            })?;
        let child_output = timeout(process_timeout, child)
            .await
            .map_err(|_| {
                GuestSceneWorkerError::new(
                    "guest_scene_worker_timeout",
                    "isolated guest scene worker exceeded parent timeout",
                )
            })?
            .map_err(|_| {
                GuestSceneWorkerError::new(
                    "guest_scene_worker_failed",
                    "isolated guest scene worker could not be started",
                )
            })?;
        if !child_output.status.success() {
            return Err(GuestSceneWorkerError::new(
                "guest_scene_worker_failed",
                "isolated guest scene worker rejected or failed",
            ));
        }

        let receipt_path = output_dir.join("result.json");
        let metadata = fs::metadata(&receipt_path).await.map_err(|_| {
            GuestSceneWorkerError::new(
                "guest_scene_receipt_missing",
                "isolated guest scene receipt is missing",
            )
        })?;
        if metadata.len() == 0 || metadata.len() > MAX_RECEIPT_BYTES {
            return Err(GuestSceneWorkerError::new(
                "guest_scene_receipt_invalid",
                "isolated guest scene receipt size is invalid",
            ));
        }
        let bytes = fs::read(receipt_path).await.map_err(|_| {
            GuestSceneWorkerError::new(
                "guest_scene_receipt_missing",
                "isolated guest scene receipt could not be read",
            )
        })?;
        let receipt: GuestSceneWorkerReceiptV1 = serde_json::from_slice(&bytes).map_err(|_| {
            GuestSceneWorkerError::new(
                "guest_scene_receipt_invalid",
                "isolated guest scene receipt is malformed",
            )
        })?;
        validate_receipt(&receipt, session_id, &expected_sha256, expected_byte_len)?;
        Ok(receipt)
    }
}

pub fn run_guest_scene_worker(
    session_id: &str,
    expected_sha256: &str,
    expected_byte_len: u64,
    font_probe: Option<&GuestSceneFontProbeConfig>,
) -> Result<(), GuestSceneWorkerError> {
    require_ident(session_id, "session_id")?;
    require_sha256(expected_sha256)?;
    if expected_byte_len == 0 {
        return Err(GuestSceneWorkerError::new(
            "guest_scene_length_invalid",
            "expected guest scene byte length must be positive",
        ));
    }

    let input_path = PathBuf::from(required_env("CHAPTERA_WORKER_INPUT")?);
    let output_root = PathBuf::from(required_env("CHAPTERA_WORKER_OUTPUT_DIR")?);
    stdfs::create_dir_all(&output_root).map_err(|_| {
        GuestSceneWorkerError::new(
            "guest_scene_worker_output_failed",
            "guest scene worker output directory is unavailable",
        )
    })?;
    let output = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(output_root.join("result.json"))
        .map(BufWriter::new)
        .map_err(|_| {
            GuestSceneWorkerError::new(
                "guest_scene_worker_output_failed",
                "guest scene worker result file could not be created",
            )
        })?;

    let metadata = stdfs::metadata(&input_path).map_err(|_| {
        GuestSceneWorkerError::new(
            "guest_scene_worker_input_failed",
            "authorized guest scene input is unavailable",
        )
    })?;
    if metadata.len() != expected_byte_len {
        return Err(GuestSceneWorkerError::new(
            "guest_scene_length_mismatch",
            "authorized guest scene input length differs from expected identity",
        ));
    }
    let source_bytes = stdfs::read(&input_path).map_err(|_| {
        GuestSceneWorkerError::new(
            "guest_scene_worker_input_failed",
            "authorized guest scene input could not be read",
        )
    })?;
    let actual_sha256 = format!("{:x}", Sha256::digest(&source_bytes));
    if actual_sha256 != expected_sha256 {
        return Err(GuestSceneWorkerError::new(
            "guest_scene_hash_mismatch",
            "authorized guest scene input hash differs from expected identity",
        ));
    }
    let loaded_font_probe = font_probe
        .map(LoadedGuestSceneFontProbe::load)
        .transpose()?;
    let scene_font_probe = loaded_font_probe
        .as_ref()
        .map(LoadedGuestSceneFontProbe::as_scene_resource);

    install_post_read_filesystem_default_deny().map_err(|_| {
        GuestSceneWorkerError::new(
            "guest_scene_sandbox_failed",
            "post-read filesystem default-deny could not be installed",
        )
    })?;

    let (classification, terminal_code, scene, salvage) =
        match open_pub_bundle(&source_bytes, viewer_geometry_environment_v0_1()) {
            Ok(bundle) => match match scene_font_probe.as_ref() {
                Some(probe) => from_viewer_geometry_with_font_probe(
                    session_id.to_owned(),
                    expected_sha256.to_owned(),
                    "guest:source".to_owned(),
                    &bundle.geometry,
                    &bundle.source_page_paint_orders,
                    Some(probe),
                ),
                None => from_viewer_geometry(
                    session_id.to_owned(),
                    expected_sha256.to_owned(),
                    "guest:source".to_owned(),
                    &bundle.geometry,
                    &bundle.source_page_paint_orders,
                ),
            } {
                Ok(scene) => {
                    let classification = if scene.fidelity.state == "supported" {
                        "supported"
                    } else {
                        "partial"
                    };
                    let scene = serde_json::to_value(scene).map_err(|_| {
                        GuestSceneWorkerError::new(
                            "guest_scene_worker_output_failed",
                            "Reader scene serialization failed",
                        )
                    })?;
                    (classification.to_owned(), None, Some(scene), None)
                }
                Err(_) => (
                    "unsupported".to_owned(),
                    Some("reader_scene_projection_failed".to_owned()),
                    None,
                    None,
                ),
            },
            Err(_) => {
                match open_pub_or_salvage(&source_bytes, viewer_geometry_environment_v0_1()) {
                    Ok(ViewerProductOpenOutcome::Salvage(partial_graph)) => {
                        let observation = serde_json::to_value(partial_graph).map_err(|_| {
                            GuestSceneWorkerError::new(
                                "guest_scene_worker_output_failed",
                                "Reader salvage observation serialization failed",
                            )
                        })?;
                        ("salvage".to_owned(), None, None, Some(observation))
                    }
                    Ok(ViewerProductOpenOutcome::Normal(_)) | Err(_) => (
                        "unsupported".to_owned(),
                        Some("reader_scene_open_failed".to_owned()),
                        None,
                        None,
                    ),
                }
            }
        };

    let failure_classification =
        guest_failure_intake_evidence(&source_bytes, &classification, terminal_code.as_deref())
            .map(|evidence| evidence.classification);

    let receipt = GuestSceneWorkerReceiptV1 {
        protocol_version: GUEST_SCENE_WORKER_V1.to_owned(),
        session_id: session_id.to_owned(),
        source_sha256: expected_sha256.to_owned(),
        source_byte_len: expected_byte_len,
        classification,
        terminal_code,
        scene,
        salvage,
        failure_classification,
        filesystem_confinement: true,
    };
    write_receipt(output, &receipt)
}

fn validate_receipt(
    receipt: &GuestSceneWorkerReceiptV1,
    session_id: &str,
    expected_sha256: &str,
    expected_byte_len: u64,
) -> Result<(), GuestSceneWorkerError> {
    if receipt.protocol_version != GUEST_SCENE_WORKER_V1
        || receipt.session_id != session_id
        || receipt.source_sha256 != expected_sha256
        || receipt.source_byte_len != expected_byte_len
        || !receipt.filesystem_confinement
    {
        return Err(GuestSceneWorkerError::new(
            "guest_scene_receipt_identity_mismatch",
            "isolated guest scene receipt differs from parent authority",
        ));
    }
    match receipt.classification.as_str() {
        "supported" | "partial" => {
            let scene = receipt.scene.as_ref().ok_or_else(|| {
                GuestSceneWorkerError::new(
                    "guest_scene_receipt_invalid",
                    "supported/partial receipt is missing Reader scene",
                )
            })?;
            if scene.get("protocol_version").and_then(Value::as_str)
                != Some("chaptera.reader-scene.v1")
            {
                return Err(GuestSceneWorkerError::new(
                    "guest_scene_receipt_invalid",
                    "worker scene uses an unsupported Reader scene protocol",
                ));
            }
            if receipt.salvage.is_some()
                || receipt.terminal_code.is_some()
                || receipt.failure_classification.is_some()
            {
                return Err(GuestSceneWorkerError::new(
                    "guest_scene_receipt_invalid",
                    "supported/partial receipt cannot carry terminal failure evidence",
                ));
            }
        }
        "salvage" => {
            if receipt.scene.is_some() {
                return Err(GuestSceneWorkerError::new(
                    "guest_scene_receipt_invalid",
                    "salvage receipt cannot masquerade as Reader scene",
                ));
            }
            let observation = receipt.salvage.as_ref().ok_or_else(|| {
                GuestSceneWorkerError::new(
                    "guest_scene_receipt_invalid",
                    "salvage receipt is missing Reader partial source graph",
                )
            })?;
            if observation.get("schema_version").and_then(Value::as_str)
                != Some("chaptera.reader-partial-source-graph.v1")
            {
                return Err(GuestSceneWorkerError::new(
                    "guest_scene_receipt_invalid",
                    "salvage receipt uses an unsupported observation protocol",
                ));
            }
            if observation.get("source_sha256").and_then(Value::as_str) != Some(expected_sha256)
                || receipt.terminal_code.is_some()
                || receipt.failure_classification.is_some()
            {
                return Err(GuestSceneWorkerError::new(
                    "guest_scene_receipt_invalid",
                    "salvage receipt identity/evidence shape is invalid",
                ));
            }
        }
        "unsupported" => {
            if receipt.scene.is_some()
                || receipt.salvage.is_some()
                || !matches!(
                    receipt.terminal_code.as_deref(),
                    Some("reader_scene_open_failed" | "reader_scene_projection_failed")
                )
            {
                return Err(GuestSceneWorkerError::new(
                    "guest_scene_receipt_invalid",
                    "unsupported receipt shape is invalid",
                ));
            }
            let failure_classification =
                receipt.failure_classification.as_ref().ok_or_else(|| {
                    GuestSceneWorkerError::new(
                        "guest_scene_receipt_invalid",
                        "unsupported receipt is missing server failure classification",
                    )
                })?;
            failure_classification.validate().map_err(|_| {
                GuestSceneWorkerError::new(
                    "guest_scene_receipt_invalid",
                    "unsupported receipt carries invalid failure classification",
                )
            })?;
        }
        _ => {
            return Err(GuestSceneWorkerError::new(
                "guest_scene_receipt_invalid",
                "worker returned unknown classification",
            ));
        }
    }
    Ok(())
}

fn write_receipt(
    mut output: BufWriter<File>,
    receipt: &GuestSceneWorkerReceiptV1,
) -> Result<(), GuestSceneWorkerError> {
    serde_json::to_writer(&mut output, receipt).map_err(|_| {
        GuestSceneWorkerError::new(
            "guest_scene_worker_output_failed",
            "guest scene receipt serialization failed",
        )
    })?;
    output.write_all(b"\n").map_err(|_| {
        GuestSceneWorkerError::new(
            "guest_scene_worker_output_failed",
            "guest scene receipt write failed",
        )
    })?;
    output.flush().map_err(|_| {
        GuestSceneWorkerError::new(
            "guest_scene_worker_output_failed",
            "guest scene receipt flush failed",
        )
    })
}

fn required_env(name: &str) -> Result<String, GuestSceneWorkerError> {
    env::var(name).map_err(|_| {
        GuestSceneWorkerError::new(
            "guest_scene_worker_environment_missing",
            format!("{name} is required"),
        )
    })
}

fn require_ident(value: &str, label: &str) -> Result<(), GuestSceneWorkerError> {
    if value.is_empty()
        || value.len() > 256
        || value
            .chars()
            .any(|character| character.is_control() || character.is_whitespace())
    {
        return Err(GuestSceneWorkerError::new(
            "guest_scene_identity_invalid",
            format!("{label} must be a bounded non-whitespace identity"),
        ));
    }
    Ok(())
}

fn require_sha256(value: &str) -> Result<(), GuestSceneWorkerError> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(GuestSceneWorkerError::new(
            "guest_scene_identity_invalid",
            "expected_sha256 must be 64 lowercase SHA-256 hex characters",
        ));
    }
    Ok(())
}

struct GuestSceneTempDir {
    path: PathBuf,
}

impl GuestSceneTempDir {
    async fn create(root: &Path) -> Result<Self, GuestSceneWorkerError> {
        fs::create_dir_all(root).await.map_err(|_| {
            GuestSceneWorkerError::new(
                "guest_scene_temp_failed",
                "guest scene temp root is unavailable",
            )
        })?;
        for _ in 0..8 {
            let mut random = [0_u8; 16];
            OsRng.try_fill_bytes(&mut random).map_err(|_| {
                GuestSceneWorkerError::new(
                    "guest_scene_random_failed",
                    "guest scene temp identity generation failed",
                )
            })?;
            let path = root.join(format!(
                "chaptera-guest-scene-{:032x}",
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
                                GuestSceneWorkerError::new(
                                    "guest_scene_temp_failed",
                                    "guest scene temp permissions could not be constrained",
                                )
                            })?;
                    }
                    return Ok(Self { path });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(_) => {
                    return Err(GuestSceneWorkerError::new(
                        "guest_scene_temp_failed",
                        "guest scene temp directory could not be created",
                    ));
                }
            }
        }
        Err(GuestSceneWorkerError::new(
            "guest_scene_temp_failed",
            "guest scene temp collision budget exhausted",
        ))
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for GuestSceneTempDir {
    fn drop(&mut self) {
        let _ = stdfs::remove_dir_all(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worker_receipt_accepts_source_neutral_salvage_observation() {
        let receipt = GuestSceneWorkerReceiptV1 {
            protocol_version: GUEST_SCENE_WORKER_V1.to_owned(),
            session_id: "guest:0123456789abcdef".to_owned(),
            source_sha256: "a".repeat(64),
            source_byte_len: 1,
            classification: "salvage".to_owned(),
            terminal_code: None,
            scene: None,
            salvage: Some(serde_json::json!({
                "schema_version":"chaptera.reader-partial-source-graph.v1",
                "source_sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "contents_family":null,
                "subsystems":{
                    "contents":"readable",
                    "quill":"absent",
                    "escher":"absent",
                    "escher_delay":"absent"
                },
                "facts":[],
                "gaps":["text_unavailable","image_facts_unavailable","geometry_facts_unavailable"]
            })),
            failure_classification: None,
            filesystem_confinement: true,
        };
        validate_receipt(&receipt, "guest:0123456789abcdef", &"a".repeat(64), 1)
            .expect("source-neutral salvage observation should cross worker boundary");
    }

    #[test]
    fn worker_receipt_rejects_scene_for_unsupported_classification() {
        let receipt = GuestSceneWorkerReceiptV1 {
            protocol_version: GUEST_SCENE_WORKER_V1.to_owned(),
            session_id: "guest:0123456789abcdef".to_owned(),
            source_sha256: "a".repeat(64),
            source_byte_len: 1,
            classification: "unsupported".to_owned(),
            terminal_code: Some("reader_scene_open_failed".to_owned()),
            scene: Some(serde_json::json!({"protocol_version":"chaptera.reader-scene.v1"})),
            salvage: None,
            failure_classification: None,
            filesystem_confinement: true,
        };
        assert!(validate_receipt(&receipt, "guest:0123456789abcdef", &"a".repeat(64), 1,).is_err());
    }
}
