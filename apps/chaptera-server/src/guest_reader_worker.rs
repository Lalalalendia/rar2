use std::{
    env, fmt,
    fs::{self as stdfs, File, OpenOptions},
    io::{BufWriter, Write},
    path::{Path, PathBuf},
    time::{Duration, Instant},
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
    config::CloudReaderFontResourceConfig,
    guest_intake_classifier::guest_failure_intake_evidence,
    reader_scene_v1::{ReaderConfiguredFontResourceV1, from_viewer_geometry_with_fonts},
    source_ingress_security::SourceSecurityScannerConfig,
};

pub const GUEST_SCENE_WORKER_V1: &str = "chaptera.reader-guest-scene-worker.v1";
const MAX_RECEIPT_BYTES: u64 = 18 * 1024 * 1024;
const COPY_BUFFER_BYTES: usize = 64 * 1024;
const MAX_CONFIGURED_FONT_BYTES: usize = 4 * 1024 * 1024;
const MAX_CONFIGURED_FONT_TOTAL_BYTES: usize = 4 * 1024 * 1024;
const MAX_FONT_MANIFEST_BYTES: u64 = 64 * 1024;
const GUEST_FONT_MANIFEST_V1: &str = "chaptera.reader-configured-font-manifest.v1";

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
struct GuestSceneFontManifestV1 {
    protocol_version: String,
    resources: Vec<GuestSceneFontManifestResourceV1>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct GuestSceneFontManifestResourceV1 {
    source_family: String,
    expected_sha256: String,
    face_index: u32,
    mime: String,
    file_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GuestSceneWorkerReceiptV1 {
    pub protocol_version: String,
    pub session_id: String,
    pub source_sha256: String,
    pub source_byte_len: u64,
    pub classification: String,
    pub terminal_code: Option<String>,
    pub projection_failure_class: Option<String>,
    pub scene: Option<Value>,
    pub salvage: Option<Value>,
    pub failure_classification: Option<FailureClassificationV1>,
    pub filesystem_confinement: bool,
    pub structural_scan_duration_us: u64,
    pub scene_duration_us: Option<u64>,
}

#[derive(Clone)]
pub struct IsolatedGuestSceneProducer {
    config: SourceSecurityScannerConfig,
    worker_binary: PathBuf,
    configured_fonts: Vec<ReaderConfiguredFontResourceV1>,
}

impl IsolatedGuestSceneProducer {
    pub fn new(config: SourceSecurityScannerConfig) -> Result<Self, GuestSceneWorkerError> {
        Self::new_with_fonts(config, &[])
    }

    pub fn new_with_fonts(
        config: SourceSecurityScannerConfig,
        font_configs: &[CloudReaderFontResourceConfig],
    ) -> Result<Self, GuestSceneWorkerError> {
        config
            .validate()
            .map_err(|error| GuestSceneWorkerError::new(error.code, error.message))?;
        let worker_binary = env::current_exe().map_err(|_| {
            GuestSceneWorkerError::new(
                "guest_scene_worker_binary_missing",
                "current chaptera executable path is unavailable",
            )
        })?;
        let configured_fonts = load_configured_font_resources(font_configs)?;
        Ok(Self {
            config,
            worker_binary,
            configured_fonts,
        })
    }

    async fn materialize_font_manifest(
        &self,
        temp_root: &Path,
    ) -> Result<Option<PathBuf>, GuestSceneWorkerError> {
        if self.configured_fonts.is_empty() {
            return Ok(None);
        }

        let font_root = temp_root.join("fonts");
        fs::create_dir(&font_root).await.map_err(|_| {
            GuestSceneWorkerError::new(
                "guest_scene_font_temp_failed",
                "private configured font directory could not be created",
            )
        })?;
        let mut resources = Vec::with_capacity(self.configured_fonts.len());
        for (index, font) in self.configured_fonts.iter().enumerate() {
            let file_name = format!("font-{index}.bin");
            let path = font_root.join(&file_name);
            let mut output = fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&path)
                .await
                .map_err(|_| {
                    GuestSceneWorkerError::new(
                        "guest_scene_font_temp_failed",
                        "private configured font file could not be created",
                    )
                })?;
            output.write_all(&font.bytes).await.map_err(|_| {
                GuestSceneWorkerError::new(
                    "guest_scene_font_temp_failed",
                    "private configured font file could not be written",
                )
            })?;
            output.flush().await.map_err(|_| {
                GuestSceneWorkerError::new(
                    "guest_scene_font_temp_failed",
                    "private configured font file could not be flushed",
                )
            })?;
            drop(output);
            resources.push(GuestSceneFontManifestResourceV1 {
                source_family: font.source_family.clone(),
                expected_sha256: font.expected_sha256.clone(),
                face_index: font.face_index,
                mime: font.mime.clone(),
                file_name,
            });
        }

        let manifest = GuestSceneFontManifestV1 {
            protocol_version: GUEST_FONT_MANIFEST_V1.to_owned(),
            resources,
        };
        let bytes = serde_json::to_vec(&manifest).map_err(|_| {
            GuestSceneWorkerError::new(
                "guest_scene_font_manifest_failed",
                "configured font manifest could not be serialized",
            )
        })?;
        if bytes.len() as u64 > MAX_FONT_MANIFEST_BYTES {
            return Err(GuestSceneWorkerError::new(
                "guest_scene_font_manifest_failed",
                "configured font manifest exceeds its bounded size",
            ));
        }
        let path = font_root.join("manifest.json");
        fs::write(&path, bytes).await.map_err(|_| {
            GuestSceneWorkerError::new(
                "guest_scene_font_manifest_failed",
                "configured font manifest could not be written",
            )
        })?;
        Ok(Some(path))
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
        let font_manifest = self.materialize_font_manifest(temp.path()).await?;

        let timeout_seconds = self.config.worker_wall_timeout.as_secs_f64().to_string();
        let mut command = Command::new(&self.config.isolation_python);
        command
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
            .arg(expected_byte_len.to_string());
        if let Some(font_manifest) = font_manifest.as_ref() {
            command.arg("--font-registry").arg(font_manifest);
        }
        let child = command.kill_on_drop(true).output();

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
    font_registry_path: Option<&Path>,
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
    let configured_fonts = load_worker_font_manifest(font_registry_path)?;

    install_post_read_filesystem_default_deny().map_err(|_| {
        GuestSceneWorkerError::new(
            "guest_scene_sandbox_failed",
            "post-read filesystem default-deny could not be installed",
        )
    })?;

    let structural_scan_started = Instant::now();
    let normal_open = open_pub_bundle(&source_bytes, viewer_geometry_environment_v0_1());

    let (
        classification,
        terminal_code,
        projection_failure_class,
        scene,
        salvage,
        structural_scan_duration_us,
        scene_duration_us,
    ) = match normal_open {
        Ok(bundle) => {
            let structural_scan_duration_us = duration_us(structural_scan_started.elapsed());
            let scene_started = Instant::now();
            match from_viewer_geometry_with_fonts(
                session_id.to_owned(),
                expected_sha256.to_owned(),
                "guest:source".to_owned(),
                &bundle.geometry,
                &bundle.source_page_paint_orders,
                &configured_fonts,
            ) {
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
                    (
                        classification.to_owned(),
                        None,
                        None,
                        Some(scene),
                        None,
                        structural_scan_duration_us,
                        Some(duration_us(scene_started.elapsed())),
                    )
                }
                Err(error) => (
                    "unsupported".to_owned(),
                    Some("reader_scene_projection_failed".to_owned()),
                    Some(classify_projection_failure(&error).to_owned()),
                    None,
                    None,
                    structural_scan_duration_us,
                    Some(duration_us(scene_started.elapsed())),
                ),
            }
        }
        Err(_) => {
            let fallback = open_pub_or_salvage(&source_bytes, viewer_geometry_environment_v0_1());
            let structural_scan_duration_us = duration_us(structural_scan_started.elapsed());
            match fallback {
                Ok(ViewerProductOpenOutcome::Salvage(partial_graph)) => {
                    let observation = serde_json::to_value(partial_graph).map_err(|_| {
                        GuestSceneWorkerError::new(
                            "guest_scene_worker_output_failed",
                            "Reader salvage observation serialization failed",
                        )
                    })?;
                    (
                        "salvage".to_owned(),
                        None,
                        None,
                        None,
                        Some(observation),
                        structural_scan_duration_us,
                        None,
                    )
                }
                Ok(ViewerProductOpenOutcome::Normal(_)) | Err(_) => (
                    "unsupported".to_owned(),
                    Some("reader_scene_open_failed".to_owned()),
                    None,
                    None,
                    None,
                    structural_scan_duration_us,
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
        projection_failure_class,
        scene,
        salvage,
        failure_classification,
        filesystem_confinement: true,
        structural_scan_duration_us,
        scene_duration_us,
    };
    write_receipt(output, &receipt)
}

fn classify_projection_failure(error: &str) -> &'static str {
    if error.contains("projected Scene") || error.contains("projected instance") {
        "projected_instance"
    } else if error.contains("image resource")
        || error.contains("image placement")
        || error.contains("image source window")
        || error.contains("image recolor")
    {
        "image_binding"
    } else if error.contains("table") {
        "table_binding"
    } else if error.contains("story frame")
        || error.contains("text fragment")
        || error.contains("text bounds")
        || error.contains("text layout")
    {
        "text_binding"
    } else if error.contains("page") {
        "page_binding"
    } else if error.contains("Viewer node") || error.contains("parent") || error.contains("node id")
    {
        "node_geometry"
    } else if error.contains("paint") {
        "paint_binding"
    } else if error.contains("source hash") {
        "source_identity"
    } else if error.contains("fallback font") || error.contains("font resource") {
        "font_binding"
    } else {
        "other"
    }
}

fn configured_font_resource_id(expected_sha256: &str, face_index: u32) -> String {
    format!("chaptera.cloud.configured-font.{expected_sha256}.face{face_index}")
}

fn load_configured_font_resources(
    configs: &[CloudReaderFontResourceConfig],
) -> Result<Vec<ReaderConfiguredFontResourceV1>, GuestSceneWorkerError> {
    let mut total_bytes = 0_usize;
    let mut resources = Vec::with_capacity(configs.len());
    for config in configs {
        require_sha256(&config.expected_sha256)?;
        let bytes = stdfs::read(&config.path).map_err(|_| {
            GuestSceneWorkerError::new(
                "guest_scene_font_read_failed",
                "configured Reader font resource could not be read",
            )
        })?;
        if bytes.is_empty() || bytes.len() > MAX_CONFIGURED_FONT_BYTES {
            return Err(GuestSceneWorkerError::new(
                "guest_scene_font_size_invalid",
                "configured Reader font resource exceeds bounded size",
            ));
        }
        total_bytes = total_bytes.checked_add(bytes.len()).ok_or_else(|| {
            GuestSceneWorkerError::new(
                "guest_scene_font_size_invalid",
                "configured Reader font byte count overflowed",
            )
        })?;
        if total_bytes > MAX_CONFIGURED_FONT_TOTAL_BYTES {
            return Err(GuestSceneWorkerError::new(
                "guest_scene_font_size_invalid",
                "configured Reader fonts exceed total bounded size",
            ));
        }
        let actual_sha256 = format!("{:x}", Sha256::digest(&bytes));
        if actual_sha256 != config.expected_sha256 {
            return Err(GuestSceneWorkerError::new(
                "guest_scene_font_hash_mismatch",
                "configured Reader font resource differs from expected SHA-256",
            ));
        }
        resources.push(ReaderConfiguredFontResourceV1 {
            source_family: config.source_family.trim().to_owned(),
            resource_id: configured_font_resource_id(&config.expected_sha256, config.face_index),
            expected_sha256: config.expected_sha256.clone(),
            face_index: config.face_index,
            mime: config.mime.clone(),
            bytes,
        });
    }
    Ok(resources)
}

fn load_worker_font_manifest(
    manifest_path: Option<&Path>,
) -> Result<Vec<ReaderConfiguredFontResourceV1>, GuestSceneWorkerError> {
    let Some(manifest_path) = manifest_path else {
        return Ok(Vec::new());
    };
    let metadata = stdfs::metadata(manifest_path).map_err(|_| {
        GuestSceneWorkerError::new(
            "guest_scene_font_manifest_failed",
            "configured font manifest is unavailable",
        )
    })?;
    if metadata.len() == 0 || metadata.len() > MAX_FONT_MANIFEST_BYTES {
        return Err(GuestSceneWorkerError::new(
            "guest_scene_font_manifest_failed",
            "configured font manifest size is invalid",
        ));
    }
    let bytes = stdfs::read(manifest_path).map_err(|_| {
        GuestSceneWorkerError::new(
            "guest_scene_font_manifest_failed",
            "configured font manifest could not be read",
        )
    })?;
    let manifest: GuestSceneFontManifestV1 = serde_json::from_slice(&bytes).map_err(|_| {
        GuestSceneWorkerError::new(
            "guest_scene_font_manifest_failed",
            "configured font manifest is malformed",
        )
    })?;
    if manifest.protocol_version != GUEST_FONT_MANIFEST_V1 || manifest.resources.len() > 16 {
        return Err(GuestSceneWorkerError::new(
            "guest_scene_font_manifest_failed",
            "configured font manifest protocol or resource count is invalid",
        ));
    }
    let root = manifest_path.parent().ok_or_else(|| {
        GuestSceneWorkerError::new(
            "guest_scene_font_manifest_failed",
            "configured font manifest has no private root",
        )
    })?;
    let mut total_bytes = 0_usize;
    let mut families = std::collections::HashSet::new();
    let mut resources = Vec::with_capacity(manifest.resources.len());
    for entry in manifest.resources {
        require_sha256(&entry.expected_sha256)?;
        let normalized_family = entry.source_family.trim().to_lowercase();
        if normalized_family.is_empty() || !families.insert(normalized_family) {
            return Err(GuestSceneWorkerError::new(
                "guest_scene_font_manifest_failed",
                "configured font manifest contains invalid or duplicate source family",
            ));
        }
        if entry.file_name.is_empty()
            || entry.file_name.contains('/')
            || entry.file_name.contains('\\')
            || entry.file_name == "."
            || entry.file_name == ".."
        {
            return Err(GuestSceneWorkerError::new(
                "guest_scene_font_manifest_failed",
                "configured font manifest contains an unsafe file name",
            ));
        }
        if !matches!(entry.mime.as_str(), "font/ttf" | "font/otf") || entry.face_index > 31 {
            return Err(GuestSceneWorkerError::new(
                "guest_scene_font_manifest_failed",
                "configured font manifest resource metadata is invalid",
            ));
        }
        let font_bytes = stdfs::read(root.join(&entry.file_name)).map_err(|_| {
            GuestSceneWorkerError::new(
                "guest_scene_font_read_failed",
                "private configured font resource could not be read",
            )
        })?;
        if font_bytes.is_empty() || font_bytes.len() > MAX_CONFIGURED_FONT_BYTES {
            return Err(GuestSceneWorkerError::new(
                "guest_scene_font_size_invalid",
                "private configured font resource exceeds bounded size",
            ));
        }
        total_bytes = total_bytes.checked_add(font_bytes.len()).ok_or_else(|| {
            GuestSceneWorkerError::new(
                "guest_scene_font_size_invalid",
                "private configured font byte count overflowed",
            )
        })?;
        if total_bytes > MAX_CONFIGURED_FONT_TOTAL_BYTES {
            return Err(GuestSceneWorkerError::new(
                "guest_scene_font_size_invalid",
                "private configured fonts exceed total bounded size",
            ));
        }
        let actual_sha256 = format!("{:x}", Sha256::digest(&font_bytes));
        if actual_sha256 != entry.expected_sha256 {
            return Err(GuestSceneWorkerError::new(
                "guest_scene_font_hash_mismatch",
                "private configured font resource differs from expected SHA-256",
            ));
        }
        resources.push(ReaderConfiguredFontResourceV1 {
            source_family: entry.source_family.trim().to_owned(),
            resource_id: configured_font_resource_id(&entry.expected_sha256, entry.face_index),
            expected_sha256: entry.expected_sha256,
            face_index: entry.face_index,
            mime: entry.mime,
            bytes: font_bytes,
        });
    }
    Ok(resources)
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
    let scene_timing_expected = matches!(receipt.classification.as_str(), "supported" | "partial")
        || receipt.terminal_code.as_deref() == Some("reader_scene_projection_failed");
    let projection_failure_expected =
        receipt.terminal_code.as_deref() == Some("reader_scene_projection_failed");
    if receipt.projection_failure_class.is_some() != projection_failure_expected {
        return Err(GuestSceneWorkerError::new(
            "guest_scene_receipt_identity_mismatch",
            "isolated guest scene projection failure class differs from terminal authority",
        ));
    }
    if receipt.scene_duration_us.is_some() != scene_timing_expected {
        return Err(GuestSceneWorkerError::new(
            "guest_scene_receipt_identity_mismatch",
            "isolated guest scene timing differs from classification authority",
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
                || receipt.projection_failure_class.is_some()
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
                || receipt.projection_failure_class.is_some()
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

fn duration_us(duration: Duration) -> u64 {
    u64::try_from(duration.as_micros()).unwrap_or(u64::MAX)
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
            projection_failure_class: None,
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
            structural_scan_duration_us: 1,
            scene_duration_us: None,
        };
        validate_receipt(&receipt, "guest:0123456789abcdef", &"a".repeat(64), 1)
            .expect("source-neutral salvage observation should cross worker boundary");
    }

    #[test]
    fn projection_failure_classifier_is_source_neutral() {
        assert_eq!(
            classify_projection_failure(
                "projected Scene instance abc targets unknown Viewer frame def"
            ),
            "projected_instance"
        );
        assert_eq!(
            classify_projection_failure("image placement references unknown node secret"),
            "image_binding"
        );
        assert_eq!(
            classify_projection_failure("table secret cell has non-positive resolved bounds"),
            "table_binding"
        );
        assert_eq!(
            classify_projection_failure("story frame references unknown node secret"),
            "text_binding"
        );
        assert_eq!(
            classify_projection_failure("duplicate Viewer page id secret"),
            "page_binding"
        );
        assert_eq!(classify_projection_failure("unclassified secret"), "other");
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
            projection_failure_class: None,
            scene: Some(serde_json::json!({"protocol_version":"chaptera.reader-scene.v1"})),
            salvage: None,
            failure_classification: None,
            filesystem_confinement: true,
            structural_scan_duration_us: 1,
            scene_duration_us: None,
        };
        assert!(validate_receipt(&receipt, "guest:0123456789abcdef", &"a".repeat(64), 1,).is_err());
    }
}
