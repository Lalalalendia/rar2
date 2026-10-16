use std::{
    collections::HashSet,
    env, fmt,
    fs::{self as stdfs, File, OpenOptions},
    io::{BufWriter, Write},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use chaptera_failure_intake_protocol::FailureClassificationV1;
use chaptera_untrusted_pub_scan::install_post_read_filesystem_default_deny;
use pub_viewer::{
    ViewerProductOpenOutcome, open_pub_bundle, open_pub_or_salvage,
    viewer_geometry_environment_v0_1,
};
use rand::{RngCore, rngs::OsRng};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::{
    fs,
    io::{AsyncReadExt, AsyncWriteExt},
    process::Command,
    time::timeout,
};

#[cfg(test)]
use crate::safe_preview_scene::collect_worker_preview_samples;

use crate::{
    blob_store::BlobStoreService,
    config::CloudReaderFontResourceConfig,
    guest_intake_classifier::guest_failure_intake_evidence,
    product_replay_worker::{
        IsolatedReaderPreviewSidecarV1, PREVIEW_PART_COUNT, approved_reader_scene_shape,
        preview_part_name, read_preview_sidecar_parts, sanitize_reader_scene_browser_decoders,
        write_isolated_preview_sidecar,
    },
    reader_scene_v1::{
        MAX_SCENE_BYTES, ReaderConfiguredFontResourceV1, from_viewer_geometry_with_fonts,
    },
    safe_preview_scene::{
        WorkerSafePreviewAuditV1, WorkerSafePreviewSamplesV1, admit_host_reencoded_previews,
        collect_worker_preview_samples_with_audit,
    },
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
    #[serde(default)]
    bold: bool,
    #[serde(default)]
    italic: bool,
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
    pub scene: Option<Value>,
    pub salvage: Option<Value>,
    pub failure_classification: Option<FailureClassificationV1>,
    pub filesystem_confinement: bool,
    pub structural_scan_duration_us: u64,
    pub scene_duration_us: Option<u64>,
    /// Worker decoded samples stay in fixed-name private sidecars.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview_sidecar: Option<IsolatedReaderPreviewSidecarV1>,
    /// Host-only, never included in the wire receipt or public response.
    #[serde(skip)]
    pub preview_samples: Vec<WorkerSafePreviewSamplesV1>,
    /// Private source-free rejection census, never forwarded to HTTP.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) preview_audit: Option<WorkerSafePreviewAuditV1>,
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
                bold: font.bold,
                italic: font.italic,
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
        let mut receipt: GuestSceneWorkerReceiptV1 =
            serde_json::from_slice(&bytes).map_err(|_| {
                GuestSceneWorkerError::new(
                    "guest_scene_receipt_invalid",
                    "isolated guest scene receipt is malformed",
                )
            })?;
        validate_receipt(&receipt, session_id, &expected_sha256, expected_byte_len)?;
        if let Some(manifest) = receipt.preview_sidecar.as_ref() {
            if receipt.scene.is_none() {
                return Err(GuestSceneWorkerError::new(
                    "guest_scene_preview_sidecar_invalid",
                    "preview manifest cannot accompany a terminal guest receipt",
                ));
            }
            receipt.preview_samples = read_preview_sidecar_parts(manifest, &output_dir)
                .await
                .map_err(|_| {
                    GuestSceneWorkerError::new(
                        "guest_scene_preview_sidecar_invalid",
                        "host rejected isolated preview sidecar",
                    )
                })?;
        }
        if let Some(scene) = receipt.scene.as_mut() {
            // Strip worker-controlled image URLs and font containers first.
            // Reconstruct only host-pinned fonts and fresh encoded previews.
            let requested_font_ids = scene
                .get("fonts")
                .and_then(Value::as_array)
                .map(|fonts| {
                    fonts
                        .iter()
                        .filter_map(|font| font.get("resource_id").and_then(Value::as_str))
                        .map(str::to_owned)
                        .collect::<HashSet<_>>()
                })
                .unwrap_or_default();
            sanitize_reader_scene_browser_decoders(scene).map_err(|_| {
                GuestSceneWorkerError::new(
                    "guest_scene_browser_decoder_invalid",
                    "host could not strip unsafe worker resource URLs",
                )
            })?;
            restore_host_authorized_guest_fonts(
                scene,
                &requested_font_ids,
                &self.configured_fonts,
            )?;
            admit_host_reencoded_previews(scene, &receipt.preview_samples).map_err(|_| {
                GuestSceneWorkerError::new(
                    "guest_scene_safe_preview_invalid",
                    "host rejected untrusted preview pixel receipt",
                )
            })?;
            if serde_json::to_vec(scene)
                .map_err(|_| {
                    GuestSceneWorkerError::new(
                        "guest_scene_browser_decoder_invalid",
                        "sanitized guest scene could not be serialized",
                    )
                })?
                .len()
                > MAX_SCENE_BYTES
            {
                return Err(GuestSceneWorkerError::new(
                    "guest_scene_browser_decoder_invalid",
                    "sanitized guest scene exceeds Reader byte cap",
                ));
            }
            if scene["fidelity"]["state"] == "partial" {
                receipt.classification = "partial".to_owned();
            }
        }
        receipt.preview_samples.clear();
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

    // File descriptors preopened before post-read seccomp. Worker never
    // selects host paths for derivative resources.
    let mut preview_outputs = Vec::with_capacity(PREVIEW_PART_COUNT);
    for index in 0..PREVIEW_PART_COUNT {
        preview_outputs.push(
            OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(output_root.join(preview_part_name(index)))
                .map_err(|_| {
                    GuestSceneWorkerError::new(
                        "guest_scene_preview_sidecar_invalid",
                        "private preview sidecar could not be preopened",
                    )
                })?,
        );
    }

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
    let mut preview_samples = Vec::new();
    let mut preview_audit = WorkerSafePreviewAuditV1::default();

    let (
        classification,
        terminal_code,
        scene,
        salvage,
        structural_scan_duration_us,
        scene_duration_us,
    ) = match normal_open {
        Ok(bundle) => {
            // Decoding happens exclusively in the confined worker.
            let images = bundle.geometry.images.iter().filter_map(|image| {
                let id = serde_json::to_value(image.resource_id)
                    .ok()?
                    .as_str()?
                    .to_owned();
                Some((id, image.mime.as_str(), image.bytes.as_slice()))
            });
            let decorative = bundle
                .geometry
                .decorative_border_resources
                .iter()
                .filter_map(|image| {
                    let id = serde_json::to_value(image.resource_id)
                        .ok()?
                        .as_str()?
                        .to_owned();
                    Some((id, image.mime.as_str(), image.bytes.as_slice()))
                });
            (preview_samples, preview_audit) =
                collect_worker_preview_samples_with_audit(images.chain(decorative));
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
                        Some(scene),
                        None,
                        structural_scan_duration_us,
                        Some(duration_us(scene_started.elapsed())),
                    )
                }
                Err(_) => (
                    "unsupported".to_owned(),
                    Some("reader_scene_projection_failed".to_owned()),
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
                    structural_scan_duration_us,
                    None,
                ),
            }
        }
    };

    let failure_classification =
        guest_failure_intake_evidence(&source_bytes, &classification, terminal_code.as_deref())
            .map(|evidence| evidence.classification);

    let preview_sidecar = if scene.is_some() {
        write_isolated_preview_sidecar(&mut preview_outputs, &preview_samples).map_err(|_| {
            GuestSceneWorkerError::new(
                "guest_scene_preview_sidecar_invalid",
                "isolated preview sample sidecar could not be written",
            )
        })?
    } else {
        None
    };
    let preview_audit = scene.as_ref().map(|_| preview_audit);
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
        structural_scan_duration_us,
        scene_duration_us,
        preview_sidecar,
        preview_samples: Vec::new(),
        preview_audit,
    };
    write_receipt(output, &receipt)
}

/// Restore configured physical font bytes from host-side verification,
/// never from arbitrary worker JSON.
fn restore_host_authorized_guest_fonts(
    scene: &mut Value,
    requested: &HashSet<String>,
    configured: &[ReaderConfiguredFontResourceV1],
) -> Result<(), GuestSceneWorkerError> {
    if requested.is_empty() {
        return Ok(());
    }
    let fonts = scene
        .get_mut("fonts")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| {
            GuestSceneWorkerError::new(
                "guest_scene_browser_decoder_invalid",
                "sanitized guest scene has no font registry",
            )
        })?;
    for font in configured {
        if !requested.contains(&font.resource_id) {
            continue;
        }
        if format!("{:x}", Sha256::digest(&font.bytes)) != font.expected_sha256
            || font.mime != "font/ttf"
        {
            return Err(GuestSceneWorkerError::new(
                "guest_scene_browser_decoder_invalid",
                "configured guest font no longer matches its verified bytes",
            ));
        }
        fonts.push(json!({
            "resource_id": font.resource_id,
            "family_name": font.source_family,
            "mime": font.mime,
            "expected_sha256": font.expected_sha256,
            "availability": "inline_data_url",
            "inline_data_url": format!("data:font/ttf;base64,{}", BASE64.encode(&font.bytes)),
        }));
    }
    Ok(())
}

fn configured_font_resource_id(expected_sha256: &str, face_index: u32) -> String {
    format!("chaptera.cloud.configured-font.{expected_sha256}.face{face_index}")
}

fn load_configured_font_resources(
    configs: &[CloudReaderFontResourceConfig],
) -> Result<Vec<ReaderConfiguredFontResourceV1>, GuestSceneWorkerError> {
    let mut total_bytes = 0_usize;
    let mut resources = Vec::with_capacity(configs.len());
    let mut source_styles = std::collections::HashSet::new();
    let mut physical_faces = std::collections::HashSet::new();
    for config in configs {
        require_sha256(&config.expected_sha256)?;
        if config.face_index != 0 {
            return Err(GuestSceneWorkerError::new(
                "guest_scene_font_face_index_unsupported",
                "browser source-font transport requires physical face index zero",
            ));
        }
        if !source_styles.insert((
            config.source_family.trim().to_lowercase(),
            config.bold,
            config.italic,
        )) || !physical_faces.insert((config.expected_sha256.clone(), config.face_index))
        {
            return Err(GuestSceneWorkerError::new(
                "guest_scene_font_manifest_failed",
                "configured source font style/physical identity is duplicated",
            ));
        }
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
            bold: config.bold,
            italic: config.italic,
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
    let mut source_styles = std::collections::HashSet::new();
    let mut physical_faces = std::collections::HashSet::new();
    let mut resources = Vec::with_capacity(manifest.resources.len());
    for entry in manifest.resources {
        require_sha256(&entry.expected_sha256)?;
        let normalized_family = entry.source_family.trim().to_lowercase();
        if normalized_family.is_empty()
            || !source_styles.insert((normalized_family, entry.bold, entry.italic))
            || !physical_faces.insert((entry.expected_sha256.clone(), entry.face_index))
        {
            return Err(GuestSceneWorkerError::new(
                "guest_scene_font_manifest_failed",
                "configured font manifest contains invalid or duplicate source face/style",
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
        if !matches!(entry.mime.as_str(), "font/ttf" | "font/otf") || entry.face_index != 0 {
            return Err(GuestSceneWorkerError::new(
                "guest_scene_font_manifest_failed",
                "configured font manifest requires supported browser font face index zero",
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
            bold: entry.bold,
            italic: entry.italic,
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
                || scene.get("scene_authority").and_then(Value::as_str)
                    != Some("server_viewer_projection")
                || scene.get("document_id").and_then(Value::as_str) != Some(session_id)
                || scene.get("source_hash").and_then(Value::as_str) != Some(expected_sha256)
                || scene.get("revision_id").and_then(Value::as_str) != Some("guest:source")
                || !approved_reader_scene_shape(scene)
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
            if receipt.preview_sidecar.is_some() || receipt.scene.is_some() {
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
            if receipt.preview_sidecar.is_some()
                || receipt.scene.is_some()
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
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    fn configured_font_test_path() -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let suffix = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "chaptera-configured-font-test-{}-{suffix}.ttf",
            std::process::id()
        ))
    }

    fn safe_preview_guest_receipt() -> GuestSceneWorkerReceiptV1 {
        GuestSceneWorkerReceiptV1 {
            protocol_version: GUEST_SCENE_WORKER_V1.to_owned(),
            session_id: "guest:0123456789abcdef".to_owned(),
            source_sha256: "a".repeat(64),
            source_byte_len: 123,
            classification: "supported".to_owned(),
            terminal_code: None,
            scene: Some(json!({
                "protocol_version": "chaptera.reader-scene.v1",
                "document_id": "guest:0123456789abcdef",
                "source_hash": "a".repeat(64),
                "revision_id": "guest:source",
                "scene_authority": "server_viewer_projection",
                "stacking_fidelity": "source_back_to_front",
                "fidelity": {"state": "supported", "reasons": []},
                "pages": [{"page_id": "page-1", "order": 0, "width_emu": 952500, "height_emu": 952500}],
                "nodes": [{
                    "node_id": "image-node",
                    "page_id": "page-1",
                    "kind": "picture_frame",
                    "bounds": {},
                    "transform": {},
                    "resource_id": "image-a"
                }],
                "stories": [],
                "resources": [{
                    "resource_id": "image-a",
                    "mime": "image/png",
                    "availability": "inline_data_url",
                    "inline_data_url": "data:image/png;base64,UNTRUSTED_SOURCE_CONTAINER"
                }],
                "fonts": [{
                    "resource_id": "configured-face",
                    "family_name": "Untrusted worker name",
                    "mime": "font/ttf",
                    "expected_sha256": "f".repeat(64),
                    "availability": "inline_data_url",
                    "inline_data_url": "data:font/ttf;base64,UNTRUSTED_WORKER_FONT"
                }]
            })),
            salvage: None,
            failure_classification: None,
            filesystem_confinement: true,
            structural_scan_duration_us: 10,
            scene_duration_us: Some(100),
            preview_sidecar: None,
            preview_samples: Vec::new(),
            preview_audit: None,
        }
    }

    #[test]
    fn guest_safe_preview_rebuilds_only_host_owned_images_and_pinned_fonts() {
        let mut receipt = safe_preview_guest_receipt();
        validate_receipt(&receipt, "guest:0123456789abcdef", &"a".repeat(64), 123).unwrap();
        let mut png_bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut png_bytes, 2, 2);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            writer
                .write_image_data(&[
                    255, 0, 0, 255, 0, 255, 0, 128, 0, 0, 255, 255, 255, 255, 255, 255,
                ])
                .unwrap();
        }
        let previews = collect_worker_preview_samples(std::iter::once((
            "image-a".to_owned(),
            "image/png",
            png_bytes.as_slice(),
        )));
        assert_eq!(previews.len(), 1);
        let configured = ReaderConfiguredFontResourceV1 {
            source_family: "Host Pinned Family".to_owned(),
            resource_id: "configured-face".to_owned(),
            expected_sha256: chaptera_desktop_fallback_font_resource::EXPECTED_SHA256.to_owned(),
            face_index: 0,
            mime: "font/ttf".to_owned(),
            bytes: chaptera_desktop_fallback_font_resource::bytes().to_vec(),
        };
        let scene = receipt.scene.as_mut().unwrap();
        let requested = scene["fonts"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|font| font["resource_id"].as_str())
            .map(str::to_owned)
            .collect();
        sanitize_reader_scene_browser_decoders(scene).unwrap();
        restore_host_authorized_guest_fonts(scene, &requested, &[configured]).unwrap();
        admit_host_reencoded_previews(scene, &previews).unwrap();
        let encoded = serde_json::to_string(&scene).unwrap();
        assert!(!encoded.contains("UNTRUSTED_SOURCE_CONTAINER"));
        assert!(!encoded.contains("UNTRUSTED_WORKER_FONT"));
        assert!(
            scene["resources"][0]["inline_data_url"]
                .as_str()
                .unwrap()
                .starts_with("data:image/png;base64,")
        );
        assert!(scene["fonts"].as_array().unwrap().iter().any(|font| {
            font["family_name"] == "Host Pinned Family"
                && font["inline_data_url"]
                    .as_str()
                    .unwrap()
                    .starts_with("data:font/ttf;base64,")
        }));
        assert_eq!(scene["fidelity"]["state"], "partial");
        assert!(
            scene["fidelity"]["reasons"]
                .as_array()
                .unwrap()
                .iter()
                .any(|reason| reason == "safe_preview_color_untransformed")
        );
    }

    #[test]
    fn guest_worker_receipt_rejects_foreign_scene_identity_or_hidden_raw_fields() {
        let mut receipt = safe_preview_guest_receipt();
        receipt.scene.as_mut().unwrap()["document_id"] = json!("guest:foreign");
        assert!(
            validate_receipt(&receipt, "guest:0123456789abcdef", &"a".repeat(64), 123).is_err()
        );
        receipt.scene.as_mut().unwrap()["document_id"] = json!("guest:0123456789abcdef");
        receipt.scene.as_mut().unwrap()["resources"][0]["host_path"] = json!("/private/source.pub");
        assert!(
            validate_receipt(&receipt, "guest:0123456789abcdef", &"a".repeat(64), 123).is_err()
        );
    }

    #[test]
    fn configured_font_loader_admits_exact_bytes_and_rejects_hash_drift() {
        let path = configured_font_test_path();
        let bytes = chaptera_desktop_fallback_font_resource::bytes();
        stdfs::write(&path, bytes).expect("write redistribution-safe configured font fixture");

        let config = CloudReaderFontResourceConfig {
            source_family: "Chaptera Test Family".to_owned(),
            bold: false,
            italic: false,
            path: path.clone(),
            expected_sha256: chaptera_desktop_fallback_font_resource::EXPECTED_SHA256.to_owned(),
            face_index: 0,
            mime: "font/ttf".to_owned(),
        };

        let resources = load_configured_font_resources(std::slice::from_ref(&config))
            .expect("exact configured font bytes must be admitted");
        assert_eq!(resources.len(), 1);
        assert_eq!(resources[0].source_family, "Chaptera Test Family");
        assert!(!resources[0].bold);
        assert!(!resources[0].italic);
        assert_eq!(
            resources[0].resource_id,
            configured_font_resource_id(
                chaptera_desktop_fallback_font_resource::EXPECTED_SHA256,
                0
            )
        );
        assert_eq!(
            resources[0].expected_sha256,
            chaptera_desktop_fallback_font_resource::EXPECTED_SHA256
        );
        assert_eq!(resources[0].face_index, 0);
        assert_eq!(resources[0].mime, "font/ttf");
        assert_eq!(resources[0].bytes, bytes);

        let mut unsupported_face = config.clone();
        unsupported_face.face_index = 1;
        let error = load_configured_font_resources(&[unsupported_face])
            .expect_err("nonzero face is not safe for current browser transport");
        assert_eq!(error.code, "guest_scene_font_face_index_unsupported");

        let mut mismatch = config.clone();
        mismatch.expected_sha256 = "0".repeat(64);
        let error = load_configured_font_resources(&[mismatch])
            .expect_err("configured font hash drift must fail closed");
        assert_eq!(error.code, "guest_scene_font_hash_mismatch");

        stdfs::remove_file(&path).expect("remove configured font fixture");
        let error = load_configured_font_resources(&[config])
            .expect_err("missing configured font file must fail closed");
        assert_eq!(error.code, "guest_scene_font_read_failed");
    }

    #[test]
    fn configured_font_manifest_retains_explicit_style_and_old_regular_default() {
        let legacy = serde_json::json!({
            "source_family": "Example",
            "expected_sha256": "a".repeat(64),
            "face_index": 0,
            "mime": "font/ttf",
            "file_name": "font-0.ttf"
        });
        let old: GuestSceneFontManifestResourceV1 =
            serde_json::from_value(legacy).expect("old regular manifest remains readable");
        assert!(!old.bold && !old.italic);
        let mut styled = old;
        styled.bold = true;
        let json = serde_json::to_value(&styled).expect("serialize styled source face");
        assert_eq!(json["bold"], true);
        assert_eq!(json["italic"], false);
        let roundtrip: GuestSceneFontManifestResourceV1 =
            serde_json::from_value(json).expect("read styled source face");
        assert!(roundtrip.bold && !roundtrip.italic);
    }

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
            structural_scan_duration_us: 1,
            scene_duration_us: None,
            preview_sidecar: None,
            preview_samples: Vec::new(),
            preview_audit: None,
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
            structural_scan_duration_us: 1,
            scene_duration_us: None,
            preview_sidecar: None,
            preview_samples: Vec::new(),
            preview_audit: None,
        };
        assert!(validate_receipt(&receipt, "guest:0123456789abcdef", &"a".repeat(64), 1,).is_err());
    }
}
