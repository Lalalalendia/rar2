//! Product Editor graph projection inside an isolated per-file worker.
//!
//! This command is an isolated building block for CLOUD-EDITOR-SEC #2565.
//! AuthN/AuthZ, RevisionStream authority and blob selection remain in the host;
//! the worker receives a verified immutable PUB plus a canonical EditorProject.

use std::{
    env, fmt,
    fs::{self, OpenOptions},
    io::{BufWriter, Write},
    path::Path,
    str::FromStr,
};

use chaptera_untrusted_pub_scan::install_post_read_filesystem_default_deny;
use pub_editor::{
    AuthoredEntityProvenanceV1, AuthoredShapeKindV1, AuthoredShapePaintV1,
    AuthoredShapeTransformV1, AuthoredSolidFillV1, AuthoredSolidStrokeV1, AuthoringTextPresetV1,
    EditOperation, EditorEditableTarget, EditorProject, LengthEmu, NodeId, PageId, RectEmu,
    Sha256Digest, Srgb8V1, StoryId, open_mature_0x2c_editor,
};
use pub_reader::PubResolvedGraph;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use rand::{RngCore, rngs::OsRng};
use tokio::{fs as async_fs, io::AsyncWriteExt, process::Command, time::timeout};

use crate::{
    reader_scene_v1::{MAX_SCENE_BYTES, READER_SCENE_V1},
    revision_materializer::{
        EditorReplayEngine, PubEditorReplayEngine, append_event_operation,
        cloud_replay_requires_local_identity, cloud_revision_project, project_sha256,
    },
    safe_preview_scene::{WorkerSafePreviewSamplesV1, admit_host_reencoded_previews},
    source_baseline::SourceBaselineProducerConfig,
};

pub const PRODUCT_REPLAY_WORKER_V1: &str = "chaptera.product-isolated-replay.v1";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IsolatedMoveNodeIntentV1 {
    pub node_id: String,
    pub x_emu: i64,
    pub y_emu: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IsolatedMoveNodeReceiptV1 {
    pub operation: EditOperation,
    pub resulting_project: EditorProject,
    pub resulting_project_sha256: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IsolatedResizeNodeIntentV1 {
    pub node_id: String,
    pub x_emu: i64,
    pub y_emu: i64,
    pub width_emu: i64,
    pub height_emu: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IsolatedResizeNodeReceiptV1 {
    pub operation: EditOperation,
    pub resulting_project: EditorProject,
    pub resulting_project_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IsolatedCreateShapeIntentV1 {
    // Both IDs are server-authorized. The browser must never mint NodeId
    // authority or be able to select another document's PageId.
    pub node_id: String,
    pub page_id: String,
    pub x_emu: i64,
    pub y_emu: i64,
    pub width_emu: i64,
    pub height_emu: i64,
    pub fill_rgb: [u8; 3],
    pub stroke_rgb: [u8; 3],
    pub stroke_width_emu: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IsolatedCreateShapeReceiptV1 {
    pub operation: EditOperation,
    pub resulting_project: EditorProject,
    pub resulting_project_sha256: String,
}

fn validate_create_shape_intent(
    intent: &IsolatedCreateShapeIntentV1,
) -> Result<(), ProductReplayWorkerError> {
    if intent.node_id.len() > 128
        || intent.node_id.as_bytes().get(14) != Some(&b'7')
        || intent.page_id.len() > 128
        || serde_json::from_value::<NodeId>(Value::String(intent.node_id.clone())).is_err()
        || serde_json::from_value::<PageId>(Value::String(intent.page_id.clone())).is_err()
        || intent.x_emu < 0
        || intent.y_emu < 0
        || intent.width_emu <= 0
        || intent.height_emu <= 0
        || intent.stroke_width_emu <= 0
        || intent.stroke_width_emu > 1_000_000
        || intent.x_emu.checked_add(intent.width_emu).is_none()
        || intent.y_emu.checked_add(intent.height_emu).is_none()
    {
        return Err(ProductReplayWorkerError::new(
            "product_create_shape_invalid",
            "CreateShape intent has invalid IDs, bounds or paint",
        ));
    }
    Ok(())
}

fn create_shape_paint(intent: &IsolatedCreateShapeIntentV1) -> AuthoredShapePaintV1 {
    AuthoredShapePaintV1 {
        fill: AuthoredSolidFillV1 {
            visible: true,
            color: Srgb8V1 {
                r: intent.fill_rgb[0],
                g: intent.fill_rgb[1],
                b: intent.fill_rgb[2],
            },
        },
        stroke: AuthoredSolidStrokeV1 {
            visible: true,
            color: Srgb8V1 {
                r: intent.stroke_rgb[0],
                g: intent.stroke_rgb[1],
                b: intent.stroke_rgb[2],
            },
            width_emu: intent.stroke_width_emu,
        },
        provenance: AuthoredEntityProvenanceV1::AuthorCreated,
    }
}

fn exact_create_shape_operation(
    intent: &IsolatedCreateShapeIntentV1,
) -> Result<EditOperation, ProductReplayWorkerError> {
    validate_create_shape_intent(intent)?;
    let node_id: NodeId =
        serde_json::from_value(Value::String(intent.node_id.clone())).map_err(|_| {
            ProductReplayWorkerError::new(
                "product_create_shape_invalid",
                "CreateShape NodeId is not canonical",
            )
        })?;
    let page_id: PageId =
        serde_json::from_value(Value::String(intent.page_id.clone())).map_err(|_| {
            ProductReplayWorkerError::new(
                "product_create_shape_invalid",
                "CreateShape PageId is not canonical",
            )
        })?;
    Ok(EditOperation::CreateShape {
        node_id,
        page_id,
        parent_id: page_id,
        shape_kind: AuthoredShapeKindV1::Rectangle,
        bounds: RectEmu::new(
            LengthEmu::new(intent.x_emu),
            LengthEmu::new(intent.y_emu),
            LengthEmu::new(intent.width_emu),
            LengthEmu::new(intent.height_emu),
        ),
        transform: AuthoredShapeTransformV1::Identity,
        paint: create_shape_paint(intent),
        provenance: AuthoredEntityProvenanceV1::AuthorCreated,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IsolatedCreateTextBoxIntentV1 {
    pub node_id: String,
    pub story_id: String,
    pub page_id: String,
    pub x_emu: i64,
    pub y_emu: i64,
    pub width_emu: i64,
    pub height_emu: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IsolatedCreateTextBoxReceiptV1 {
    pub operation: EditOperation,
    pub resulting_project: EditorProject,
    pub resulting_project_sha256: String,
}

fn temporary_textbox_preset_v1() -> AuthoringTextPresetV1 {
    // Server-selected canonical authoring preset, never supplied by a browser.
    AuthoringTextPresetV1 {
        resource_id: "chaptera.desktop.fallback-font.ubuntu-light.v1".to_owned(),
        font_fingerprint_sha256: "80307b8da7649aa4ee4d484b232140e3ce1ec0ca093073d3c53c8f5a5ced7a70"
            .to_owned(),
        face_index: 0,
        font_size_emu: LengthEmu::new(114_300),
        line_height_emu: LengthEmu::new(142_875),
    }
}

fn validate_create_textbox_intent(
    intent: &IsolatedCreateTextBoxIntentV1,
) -> Result<(), ProductReplayWorkerError> {
    let valid_v7 = |id: &str| {
        id.len() == 36
            && id.as_bytes().get(14) == Some(&b'7')
            && serde_json::from_value::<NodeId>(Value::String(id.to_owned())).is_ok()
    };
    if !valid_v7(&intent.node_id)
        || !valid_v7(&intent.story_id)
        || intent.node_id == intent.story_id
        || serde_json::from_value::<PageId>(Value::String(intent.page_id.clone())).is_err()
        || intent.x_emu < 0
        || intent.y_emu < 0
        || intent.width_emu <= 0
        || intent.height_emu <= 0
        || intent.x_emu.checked_add(intent.width_emu).is_none()
        || intent.y_emu.checked_add(intent.height_emu).is_none()
    {
        return Err(ProductReplayWorkerError::new(
            "product_create_textbox_invalid",
            "CreateTextBox intent has invalid IDs or bounds",
        ));
    }
    Ok(())
}

fn exact_create_textbox_operation(
    intent: &IsolatedCreateTextBoxIntentV1,
) -> Result<EditOperation, ProductReplayWorkerError> {
    validate_create_textbox_intent(intent)?;
    let typed = |id: &str| Value::String(id.to_owned());
    let node_id = serde_json::from_value::<NodeId>(typed(&intent.node_id)).map_err(|_| {
        ProductReplayWorkerError::new("product_create_textbox_invalid", "NodeId invalid")
    })?;
    let story_id = serde_json::from_value::<StoryId>(typed(&intent.story_id)).map_err(|_| {
        ProductReplayWorkerError::new("product_create_textbox_invalid", "StoryId invalid")
    })?;
    let page_id = serde_json::from_value::<PageId>(typed(&intent.page_id)).map_err(|_| {
        ProductReplayWorkerError::new("product_create_textbox_invalid", "PageId invalid")
    })?;
    Ok(EditOperation::CreateTextBox {
        node_id,
        story_id,
        page_id,
        bounds: RectEmu::new(
            LengthEmu::new(intent.x_emu),
            LengthEmu::new(intent.y_emu),
            LengthEmu::new(intent.width_emu),
            LengthEmu::new(intent.height_emu),
        ),
        text_preset: temporary_textbox_preset_v1(),
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IsolatedStoryRangeIntentV1 {
    pub story_id: String,
    pub start_scalar: u32,
    pub end_scalar: u32,
    pub replacement_text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IsolatedStoryRangeReceiptV1 {
    pub operation: EditOperation,
    pub resulting_project: EditorProject,
    pub resulting_project_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IsolatedEditableExportReceiptV1 {
    pub target: String,
    pub artifact_sha256: String,
    pub artifact_byte_len: usize,
    pub loss_report_sha256: String,
    pub loss_report: Value,
    #[serde(skip)]
    pub artifact_bytes: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IsolatedReaderSceneIntentV1 {
    pub revision_id: String,
    pub baseline_revision_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IsolatedReaderSceneReceiptV1 {
    pub revision_id: String,
    pub baseline_revision_id: String,
    pub scene_sha256: String,
    pub scene: Value,
    /// Out-of-band, bounded sample bytes. A compromised worker never chooses
    /// paths; the host reads fixed private sidecar slots and checks digest.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview_sidecar: Option<IsolatedReaderPreviewSidecarV1>,
    /// Populated by the authorized host from the sidecar after validation.
    /// Never serialize untrusted pixels inside the principal receipt JSON.
    #[serde(skip)]
    pub preview_samples: Vec<WorkerSafePreviewSamplesV1>,
}

const MAX_SOURCE_BYTES: u64 = 256 * 1024 * 1024;
const MAX_PROJECT_BYTES: u64 = 32 * 1024 * 1024;
const MAX_STORY_INTENT_BYTES: u64 = 64 * 1024;
const MAX_RESIZE_INTENT_BYTES: u64 = 1024;
const MAX_CREATE_SHAPE_INTENT_BYTES: u64 = 1024;
const MAX_CREATE_TEXTBOX_INTENT_BYTES: u64 = 768;
const MAX_GRAPH_BYTES: usize = 64 * 1024 * 1024;
const MAX_EDITED_EXPORT_BYTES: usize = 16 * 1024 * 1024;
const MAX_EDITED_LOSS_REPORT_BYTES: usize = 64 * 1024;
const EDITED_EXPORT_PART: &str = "edited-export.bin";
// An 8 MiB part stays well below the 32 MiB default per-file RLIMIT_FSIZE.
// Preview samples never inflate result.json; all fixed sidecar slots are
// opened before the isolated worker installs filesystem default-deny.
const PREVIEW_PART_BYTES: usize = 8 * 1024 * 1024;
pub(crate) const PREVIEW_PART_COUNT: usize = 8;
const PREVIEW_SIDECAR_MAX_BYTES: usize = PREVIEW_PART_BYTES * PREVIEW_PART_COUNT;
const PREVIEW_SIDECAR_V1: &str = "chaptera.reader-preview-samples-sidecar.v1";

async fn read_isolated_edited_export_sidecar(
    receipt: &mut ProductReplayWorkerReceiptV1,
    result_dir: &Path,
) -> Result<(), ProductReplayWorkerError> {
    let Some(edited) = receipt.editable_export.as_mut() else {
        return Ok(());
    };
    if !matches!(edited.target.as_str(), "idml" | "odg" | "pdf" | "pdf9up")
        || edited.artifact_byte_len == 0
        || edited.artifact_byte_len > MAX_EDITED_EXPORT_BYTES
    {
        return Err(ProductReplayWorkerError::new(
            "product_edited_export_receipt_invalid",
            "isolated editable export exceeds the admitted target/byte envelope",
        ));
    }
    require_sha256(&edited.artifact_sha256)?;
    require_sha256(&edited.loss_report_sha256)?;
    let loss = serde_json::to_vec(&edited.loss_report).map_err(|_| {
        ProductReplayWorkerError::new(
            "product_edited_export_receipt_invalid",
            "isolated LossReport serialization failed",
        )
    })?;
    if loss.len() > MAX_EDITED_LOSS_REPORT_BYTES || sha256_hex(&loss) != edited.loss_report_sha256 {
        return Err(ProductReplayWorkerError::new(
            "product_edited_export_receipt_invalid",
            "isolated LossReport differs from bounded checksum",
        ));
    }
    let path = result_dir.join(EDITED_EXPORT_PART);
    let metadata = async_fs::metadata(&path).await.map_err(|_| {
        ProductReplayWorkerError::new(
            "product_edited_export_receipt_missing",
            "isolated exported bytes were not produced",
        )
    })?;
    if !metadata.is_file() || metadata.len() != edited.artifact_byte_len as u64 {
        return Err(ProductReplayWorkerError::new(
            "product_edited_export_receipt_invalid",
            "isolated export sidecar length differs from receipt",
        ));
    }
    let bytes = async_fs::read(&path).await.map_err(|_| {
        ProductReplayWorkerError::new(
            "product_edited_export_receipt_invalid",
            "isolated export sidecar could not be read",
        )
    })?;
    if bytes.len() != edited.artifact_byte_len || sha256_hex(&bytes) != edited.artifact_sha256 {
        return Err(ProductReplayWorkerError::new(
            "product_edited_export_receipt_invalid",
            "isolated exported bytes differ from receipt digest",
        ));
    }
    if matches!(edited.target.as_str(), "pdf" | "pdf9up")
        && (edited.loss_report["schema_version"] != "chaptera.temporary-edited-fixed-pdf-loss.v1"
            || edited.loss_report["target"].as_str() != Some(edited.target.as_str())
            || edited.loss_report["can_serialize"] != true
            || edited.loss_report["current_editor_state"] != true
            || edited.loss_report["source_hash"].as_str() != Some(receipt.source_sha256.as_str())
            || edited.loss_report["counts"]["blocking"] != 0)
    {
        return Err(ProductReplayWorkerError::new(
            "product_edited_export_receipt_invalid",
            "isolated PDF LossReport does not match current source authority",
        ));
    }
    if matches!(edited.target.as_str(), "pdf" | "pdf9up") && !bytes.starts_with(b"%PDF-") {
        return Err(ProductReplayWorkerError::new(
            "product_edited_export_receipt_invalid",
            "isolated PDF lacks a valid physical PDF signature",
        ));
    }
    if edited.target == "pdf9up" {
        let counts = &edited.loss_report["counts"];
        if counts["pages"] != 1
            || [
                "blocking",
                "partial",
                "unsupported",
                "font_missing_glyph_lines",
                "layout_warnings",
                "pdf_warnings",
            ]
            .iter()
            .any(|name| counts[*name] != 0)
        {
            return Err(ProductReplayWorkerError::new(
                "product_edited_export_receipt_invalid",
                "nine-up export must be one A4 sheet without hidden visual loss",
            ));
        }
    }
    edited.artifact_bytes = bytes;
    Ok(())
}

pub(crate) fn preview_part_name(index: usize) -> String {
    format!("reader-preview-{index}.bin")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IsolatedReaderPreviewSidecarV1 {
    pub protocol_version: String,
    pub sha256: String,
    pub byte_len: usize,
    pub chunk_count: usize,
}

pub(crate) fn write_isolated_preview_sidecar(
    outputs: &mut [fs::File],
    previews: &[WorkerSafePreviewSamplesV1],
) -> Result<Option<IsolatedReaderPreviewSidecarV1>, ProductReplayWorkerError> {
    if previews.is_empty() {
        return Ok(None);
    }
    let bytes = serde_json::to_vec(previews).map_err(|_| {
        ProductReplayWorkerError::new(
            "product_reader_preview_sidecar_invalid",
            "isolated preview sample serialization failed",
        )
    })?;
    if bytes.len() > PREVIEW_SIDECAR_MAX_BYTES {
        // The source image remains descriptor-only; never fail the document.
        return Ok(None);
    }
    let chunks = bytes.len().div_ceil(PREVIEW_PART_BYTES);
    if chunks > outputs.len() {
        return Ok(None);
    }
    for (file, part) in outputs.iter_mut().zip(bytes.chunks(PREVIEW_PART_BYTES)) {
        file.write_all(part).map_err(|_| {
            ProductReplayWorkerError::new(
                "product_reader_preview_sidecar_invalid",
                "isolated preview sidecar write failed",
            )
        })?;
    }
    Ok(Some(IsolatedReaderPreviewSidecarV1 {
        protocol_version: PREVIEW_SIDECAR_V1.to_owned(),
        sha256: sha256_hex(&bytes),
        byte_len: bytes.len(),
        chunk_count: chunks,
    }))
}

async fn read_isolated_preview_sidecar(
    receipt: &mut ProductReplayWorkerReceiptV1,
    result_dir: &Path,
) -> Result<(), ProductReplayWorkerError> {
    let Some(scene) = receipt.reader_scene.as_mut() else {
        return Ok(());
    };
    let Some(info) = scene.preview_sidecar.as_ref() else {
        return Ok(());
    };
    scene.preview_samples = read_preview_sidecar_parts(info, result_dir).await?;
    Ok(())
}

/// Host-only: reject unchecked paths, symlinks, oversized data, and SHA drift.
pub(crate) async fn read_preview_sidecar_parts(
    info: &IsolatedReaderPreviewSidecarV1,
    result_dir: &Path,
) -> Result<Vec<WorkerSafePreviewSamplesV1>, ProductReplayWorkerError> {
    if info.protocol_version != PREVIEW_SIDECAR_V1
        || info.byte_len == 0
        || info.byte_len > PREVIEW_SIDECAR_MAX_BYTES
        || info.chunk_count == 0
        || info.chunk_count > PREVIEW_PART_COUNT
        || info.chunk_count != info.byte_len.div_ceil(PREVIEW_PART_BYTES)
        || require_sha256(&info.sha256).is_err()
    {
        return Err(ProductReplayWorkerError::new(
            "product_reader_preview_sidecar_invalid",
            "isolated preview sidecar manifest is invalid",
        ));
    }
    let mut combined = Vec::with_capacity(info.byte_len);
    for i in 0..info.chunk_count {
        let path = result_dir.join(preview_part_name(i));
        let metadata = async_fs::symlink_metadata(&path).await.map_err(|_| {
            ProductReplayWorkerError::new(
                "product_reader_preview_sidecar_invalid",
                "isolated preview sidecar is unavailable",
            )
        })?;
        let expected_len = (info.byte_len - combined.len()).min(PREVIEW_PART_BYTES);
        if !metadata.is_file() || metadata.len() != expected_len as u64 {
            return Err(ProductReplayWorkerError::new(
                "product_reader_preview_sidecar_invalid",
                "isolated preview sidecar has an unexpected type or size",
            ));
        }
        let part = async_fs::read(&path).await.map_err(|_| {
            ProductReplayWorkerError::new(
                "product_reader_preview_sidecar_invalid",
                "isolated preview sidecar could not be read",
            )
        })?;
        if part.len() != expected_len {
            return Err(ProductReplayWorkerError::new(
                "product_reader_preview_sidecar_invalid",
                "isolated preview sidecar length changed",
            ));
        }
        combined.extend_from_slice(&part);
    }
    if combined.len() != info.byte_len || sha256_hex(&combined) != info.sha256 {
        return Err(ProductReplayWorkerError::new(
            "product_reader_preview_sidecar_invalid",
            "isolated preview sidecar checksum mismatch",
        ));
    }
    serde_json::from_slice(&combined).map_err(|_| {
        ProductReplayWorkerError::new(
            "product_reader_preview_sidecar_invalid",
            "isolated preview sidecar JSON is invalid",
        )
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductReplayWorkerError {
    pub code: &'static str,
    pub message: &'static str,
}

impl ProductReplayWorkerError {
    fn new(code: &'static str, message: &'static str) -> Self {
        Self { code, message }
    }
}

impl fmt::Display for ProductReplayWorkerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for ProductReplayWorkerError {}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductReplayWorkerReceiptV1 {
    pub protocol_version: String,
    pub document_id: String,
    pub source_sha256: String,
    pub source_byte_len: u64,
    pub project_sha256: String,
    pub authoring_graph_sha256: String,
    pub authoring_graph: Value,
    /// Present only for the isolated baseline derivation mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline_project: Option<EditorProject>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub move_node: Option<IsolatedMoveNodeReceiptV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resize_node: Option<IsolatedResizeNodeReceiptV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub create_shape: Option<IsolatedCreateShapeReceiptV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub create_textbox: Option<IsolatedCreateTextBoxReceiptV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub story_range: Option<IsolatedStoryRangeReceiptV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reader_scene: Option<IsolatedReaderSceneReceiptV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub editable_export: Option<IsolatedEditableExportReceiptV1>,
    pub filesystem_confinement: bool,
}

fn require_sha256(value: &str) -> Result<(), ProductReplayWorkerError> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|b| b.is_ascii_digit() || matches!(b, b'a'..=b'f'))
    {
        return Err(ProductReplayWorkerError::new(
            "product_replay_identity_invalid",
            "SHA-256 must contain exactly 64 lowercase hex characters",
        ));
    }
    Ok(())
}

fn require_document_id(id: &str) -> Result<(), ProductReplayWorkerError> {
    if id.is_empty() || id.len() > 256 || id.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err(ProductReplayWorkerError::new(
            "product_replay_identity_invalid",
            "document identifier is not canonical",
        ));
    }
    Ok(())
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn read_bounded(path: &Path, limit: u64) -> Result<Vec<u8>, ProductReplayWorkerError> {
    let metadata = fs::metadata(path).map_err(|_| {
        ProductReplayWorkerError::new("product_replay_input_failed", "worker input is missing")
    })?;
    if !metadata.is_file() || metadata.len() > limit {
        return Err(ProductReplayWorkerError::new(
            "product_replay_input_limit",
            "worker input exceeds file/type limits",
        ));
    }
    let bytes = fs::read(path).map_err(|_| {
        ProductReplayWorkerError::new(
            "product_replay_input_failed",
            "worker input could not be read",
        )
    })?;
    if bytes.len() as u64 > limit {
        return Err(ProductReplayWorkerError::new(
            "product_replay_input_limit",
            "worker input exceeds size limit",
        ));
    }
    Ok(bytes)
}

/// Consume one validated StoryRange worker envelope without confusing the
/// expected Story payload with unrelated mutation authority.
fn take_exclusive_story_range_receipt(
    mut receipt: ProductReplayWorkerReceiptV1,
) -> Result<(IsolatedStoryRangeReceiptV1, Value), ProductReplayWorkerError> {
    if receipt.baseline_project.is_some()
        || receipt.move_node.is_some()
        || receipt.resize_node.is_some()
        || receipt.create_shape.is_some()
        || receipt.create_textbox.is_some()
        || receipt.reader_scene.is_some()
        || receipt.editable_export.is_some()
    {
        return Err(ProductReplayWorkerError::new(
            "product_replay_receipt_invalid",
            "isolated StoryRange response contains unrelated authority",
        ));
    }
    let mutation = receipt.story_range.take().ok_or_else(|| {
        ProductReplayWorkerError::new(
            "product_replay_receipt_invalid",
            "isolated StoryRange worker returned no mutation",
        )
    })?;
    Ok((mutation, receipt.authoring_graph))
}

/// Fail-closed receipt validator for a future host-side worker adapter.
/// A document-specific grant must be verified by the host before calling it.
pub fn validate_product_replay_receipt(
    receipt: &ProductReplayWorkerReceiptV1,
    document_id: &str,
    source_sha256: &str,
    source_byte_len: u64,
    expected_project_sha256: &str,
) -> Result<(), ProductReplayWorkerError> {
    if receipt.protocol_version != PRODUCT_REPLAY_WORKER_V1
        || receipt.document_id != document_id
        || receipt.source_sha256 != source_sha256
        || receipt.source_byte_len != source_byte_len
        || receipt.project_sha256 != expected_project_sha256
        || !receipt.filesystem_confinement
    {
        return Err(ProductReplayWorkerError::new(
            "product_replay_receipt_identity_mismatch",
            "isolated projection receipt differs from authorized exact identity",
        ));
    }

    let authority_count = usize::from(receipt.baseline_project.is_some())
        + usize::from(receipt.move_node.is_some())
        + usize::from(receipt.resize_node.is_some())
        + usize::from(receipt.create_shape.is_some())
        + usize::from(receipt.create_textbox.is_some())
        + usize::from(receipt.story_range.is_some())
        + usize::from(receipt.reader_scene.is_some())
        + usize::from(receipt.editable_export.is_some());
    if authority_count > 1 {
        return Err(ProductReplayWorkerError::new(
            "product_replay_receipt_invalid",
            "isolated receipt has multiple mutation authorities",
        ));
    }
    let serialized_graph = serde_json::to_vec(&receipt.authoring_graph).map_err(|_| {
        ProductReplayWorkerError::new(
            "product_replay_receipt_invalid",
            "isolated graph serialization failed",
        )
    })?;
    if serialized_graph.len() > MAX_GRAPH_BYTES
        || sha256_hex(&serialized_graph) != receipt.authoring_graph_sha256
    {
        return Err(ProductReplayWorkerError::new(
            "product_replay_receipt_invalid",
            "isolated graph does not match its bounded SHA-256 evidence",
        ));
    }
    if let Some(project) = &receipt.baseline_project
        && (project.source_hash.to_string() != source_sha256
            || project_sha256(project).map_err(|_| {
                ProductReplayWorkerError::new(
                    "product_replay_receipt_invalid",
                    "isolated baseline project could not be verified",
                )
            })? != expected_project_sha256)
    {
        return Err(ProductReplayWorkerError::new(
            "product_replay_receipt_invalid",
            "isolated baseline project does not match its source/project identity",
        ));
    }
    if let Some(move_node) = &receipt.move_node
        && (receipt.baseline_project.is_some()
            || !matches!(move_node.operation, EditOperation::MoveNode { .. })
            || move_node.resulting_project.source_hash.to_string() != source_sha256
            || project_sha256(&move_node.resulting_project).map_err(|_| {
                ProductReplayWorkerError::new(
                    "product_replay_receipt_invalid",
                    "isolated MoveNode project cannot be canonically hashed",
                )
            })? != move_node.resulting_project_sha256)
    {
        return Err(ProductReplayWorkerError::new(
            "product_replay_receipt_invalid",
            "isolated MoveNode receipt violates source and project identity",
        ));
    }
    if let Some(created) = &receipt.create_shape
        && (receipt.baseline_project.is_some()
            || receipt.move_node.is_some()
            || receipt.resize_node.is_some()
            || receipt.story_range.is_some()
            || receipt.reader_scene.is_some()
            || receipt.editable_export.is_some()
            || !matches!(created.operation, EditOperation::CreateShape { .. })
            || created.resulting_project.source_hash.to_string() != source_sha256
            || project_sha256(&created.resulting_project).map_err(|_| {
                ProductReplayWorkerError::new(
                    "product_replay_receipt_invalid",
                    "isolated CreateShape project cannot be canonically hashed",
                )
            })? != created.resulting_project_sha256)
    {
        return Err(ProductReplayWorkerError::new(
            "product_replay_receipt_invalid",
            "isolated CreateShape violates exclusive source/project authority",
        ));
    }
    if let Some(created) = &receipt.create_textbox
        && (!matches!(created.operation, EditOperation::CreateTextBox { .. })
            || created.resulting_project.source_hash.to_string() != source_sha256
            || project_sha256(&created.resulting_project).map_err(|_| {
                ProductReplayWorkerError::new(
                    "product_replay_receipt_invalid",
                    "unhashable TextBox project",
                )
            })? != created.resulting_project_sha256)
    {
        return Err(ProductReplayWorkerError::new(
            "product_replay_receipt_invalid",
            "CreateTextBox source/project identity invalid",
        ));
    }
    if let Some(resize) = &receipt.resize_node
        && (receipt.baseline_project.is_some()
            || receipt.move_node.is_some()
            || receipt.story_range.is_some()
            || receipt.reader_scene.is_some()
            || receipt.editable_export.is_some()
            || !matches!(resize.operation, EditOperation::ResizeNode { .. })
            || resize.resulting_project.source_hash.to_string() != source_sha256
            || project_sha256(&resize.resulting_project).map_err(|_| {
                ProductReplayWorkerError::new(
                    "product_replay_receipt_invalid",
                    "isolated ResizeNode project cannot be canonically hashed",
                )
            })? != resize.resulting_project_sha256)
    {
        return Err(ProductReplayWorkerError::new(
            "product_replay_receipt_invalid",
            "isolated ResizeNode receipt violates source or project identity",
        ));
    }
    if let Some(text) = &receipt.story_range
        && (receipt.baseline_project.is_some()
            || receipt.move_node.is_some()
            || receipt.resize_node.is_some()
            || receipt.reader_scene.is_some()
            || !matches!(text.operation, EditOperation::ReplaceStoryRange { .. })
            || text.resulting_project.source_hash.to_string() != source_sha256
            || project_sha256(&text.resulting_project).map_err(|_| {
                ProductReplayWorkerError::new(
                    "product_replay_receipt_invalid",
                    "isolated StoryRange project is not canonically hashable",
                )
            })? != text.resulting_project_sha256)
    {
        return Err(ProductReplayWorkerError::new(
            "product_replay_receipt_invalid",
            "isolated StoryRange receipt violates source or project identity",
        ));
    }
    if let Some(scene) = &receipt.reader_scene {
        let payload = serde_json::to_vec(&scene.scene).map_err(|_| {
            ProductReplayWorkerError::new(
                "product_replay_receipt_invalid",
                "Reader scene receipt could not be serialized",
            )
        })?;
        if receipt.baseline_project.is_some()
            || receipt.move_node.is_some()
            || receipt.resize_node.is_some()
            || receipt.story_range.is_some()
            || !valid_revision_id(&scene.revision_id)
            || !valid_revision_id(&scene.baseline_revision_id)
            || scene.scene["protocol_version"] != READER_SCENE_V1
            || scene.scene["scene_authority"] != "server_viewer_projection"
            || scene.scene["document_id"] != document_id
            || scene.scene["source_hash"] != source_sha256
            || scene.scene["revision_id"] != scene.revision_id
            || !approved_reader_scene_shape(&scene.scene)
            || payload.len() > MAX_SCENE_BYTES
            || sha256_hex(&payload) != scene.scene_sha256
        {
            return Err(ProductReplayWorkerError::new(
                "product_replay_receipt_invalid",
                "Reader scene disagrees with exact document, revision or bounded payload",
            ));
        }
    }
    Ok(())
}

/// Treat worker JSON as an untrusted wire envelope. In particular, never
/// forward a newly invented top-level key that could carry editor projects,
/// original PUB bytes, provider credentials or internal diagnostics.
pub(crate) fn approved_reader_scene_shape(scene: &Value) -> bool {
    let Some(object) = scene.as_object() else {
        return false;
    };
    const APPROVED: &[&str] = &[
        "protocol_version",
        "document_id",
        "source_hash",
        "revision_id",
        "scene_authority",
        "stacking_fidelity",
        "fidelity",
        "pages",
        "nodes",
        "stories",
        "resources",
        "fonts",
        "text_layout_fallback_counts",
        "diagnostics",
    ];
    if object.keys().any(|key| !APPROVED.contains(&key.as_str())) {
        return false;
    }
    // A valid top-level envelope is not sufficient: a compromised worker
    // could hide source paths, cookies or raw PUB data in an otherwise-valid
    // node/resource/story object and recompute the entire scene checksum.
    // Allow only the fields serialized by the public Reader Scene schema.
    const NODE_FIELDS: &[&str] = &[
        "node_id",
        "origin_node_id",
        "page_id",
        "parent_node_id",
        "kind",
        "bounds",
        "text_bounds",
        "transform",
        "paint",
        "decorative_border",
        "resource_id",
        "image_source_window",
        "image_content_rotation_degrees",
        "image_recolor",
        "table",
        "text",
        "text_layout",
        "preview_text_style",
    ];
    if !approved_scene_objects(
        &scene["pages"],
        &["page_id", "order", "width_emu", "height_emu"],
        &["page_id", "order", "width_emu", "height_emu"],
    ) || !approved_scene_objects(
        &scene["nodes"],
        NODE_FIELDS,
        &["node_id", "page_id", "kind", "bounds", "transform"],
    ) || !approved_scene_objects(
        &scene["stories"],
        &["story_id", "text", "text_fidelity"],
        &["story_id", "text", "text_fidelity"],
    ) || !scene.get("resources").is_none_or(|values| {
        approved_scene_objects(
            values,
            &["resource_id", "mime", "availability", "inline_data_url"],
            &["resource_id", "mime", "availability"],
        )
    }) || !scene.get("fonts").is_none_or(|values| {
        approved_scene_objects(
            values,
            &[
                "resource_id",
                "family_name",
                "mime",
                "expected_sha256",
                "availability",
                "inline_data_url",
            ],
            &[
                "resource_id",
                "family_name",
                "mime",
                "expected_sha256",
                "availability",
                "inline_data_url",
            ],
        )
    }) || !scene.get("diagnostics").is_none_or(|values| {
        approved_scene_objects(
            values,
            &["code", "severity", "origin_id", "message"],
            &["code", "severity", "message"],
        )
    }) {
        return false;
    }
    let Some(fidelity) = scene["fidelity"].as_object() else {
        return false;
    };
    if fidelity
        .keys()
        .any(|key| !["state", "reasons"].contains(&key.as_str()))
        || !scene["fidelity"]["reasons"]
            .as_array()
            .is_some_and(|reasons| reasons.iter().all(Value::is_string))
        || !scene
            .get("text_layout_fallback_counts")
            .is_none_or(|value| {
                value
                    .as_object()
                    .is_some_and(|counts| counts.values().all(|n| n.as_u64().is_some()))
            })
    {
        return false;
    }
    scene["stacking_fidelity"].is_string()
        && scene["fidelity"]["state"].is_string()
        && scene["fidelity"]["reasons"].is_array()
        && scene["pages"].is_array()
        && scene["nodes"].is_array()
        && scene["stories"].is_array()
        && scene.get("resources").is_none_or(Value::is_array)
        && scene.get("fonts").is_none_or(Value::is_array)
        && scene
            .get("text_layout_fallback_counts")
            .is_none_or(Value::is_object)
        && scene.get("diagnostics").is_none_or(Value::is_array)
}

fn approved_scene_objects(value: &Value, allowed: &[&str], required: &[&str]) -> bool {
    value.as_array().is_some_and(|items| {
        items.iter().all(|item| {
            item.as_object().is_some_and(|fields| {
                fields.keys().all(|key| allowed.contains(&key.as_str()))
                    && required.iter().all(|key| fields.contains_key(*key))
            })
        })
    })
}

fn trusted_base64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut encoded = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0];
        let b1 = chunk.get(1).copied().unwrap_or(0);
        let b2 = chunk.get(2).copied().unwrap_or(0);
        encoded.push(char::from(TABLE[usize::from(b0 >> 2)]));
        encoded.push(char::from(
            TABLE[usize::from(((b0 & 0x03) << 4) | (b1 >> 4))],
        ));
        if chunk.len() > 1 {
            encoded.push(char::from(
                TABLE[usize::from(((b1 & 0x0f) << 2) | (b2 >> 6))],
            ));
        } else {
            encoded.push('=');
        }
        if chunk.len() > 2 {
            encoded.push(char::from(TABLE[usize::from(b2 & 0x3f)]));
        } else {
            encoded.push('=');
        }
    }
    encoded
}

fn trusted_fallback_font_value() -> Result<Value, ProductReplayWorkerError> {
    chaptera_desktop_fallback_font_resource::validate().map_err(|_| {
        ProductReplayWorkerError::new(
            "product_reader_scene_trusted_font_invalid",
            "embedded trusted fallback font failed its pinned identity check",
        )
    })?;
    Ok(json!({
        "resource_id": chaptera_desktop_fallback_font_resource::RESOURCE_ID,
        "family_name": chaptera_desktop_fallback_font_resource::FAMILY_NAME,
        "mime": "font/ttf",
        "expected_sha256": chaptera_desktop_fallback_font_resource::EXPECTED_SHA256,
        "availability": "inline_data_url",
        "inline_data_url": format!(
            "data:font/ttf;base64,{}",
            trusted_base64_encode(chaptera_desktop_fallback_font_resource::bytes())
        ),
    }))
}

/// Treat the isolated parser/Viewer output as hostile even after schema/hash
/// validation. Browser decoder inputs are a separate trust boundary:
/// - image payloads from the worker are never forwarded as data URLs;
/// - font payloads are reconstructed from the server's pinned embedded font,
///   never copied from worker JSON.
///
/// Text/diagnostic strings remain data-only and are rendered with textContent.
pub(crate) fn sanitize_reader_scene_browser_decoders(
    scene: &mut Value,
) -> Result<(), ProductReplayWorkerError> {
    let object = scene.as_object_mut().ok_or_else(|| {
        ProductReplayWorkerError::new(
            "product_reader_scene_resource_invalid",
            "Reader scene browser envelope is not an object",
        )
    })?;

    let has_image_resources = if let Some(resources) = object.get_mut("resources") {
        let resources = resources.as_array_mut().ok_or_else(|| {
            ProductReplayWorkerError::new(
                "product_reader_scene_resource_invalid",
                "Reader scene resources must be an array",
            )
        })?;
        for resource in resources.iter_mut() {
            let fields = resource.as_object_mut().ok_or_else(|| {
                ProductReplayWorkerError::new(
                    "product_reader_scene_resource_invalid",
                    "Reader image resource must be an object",
                )
            })?;
            fields.remove("inline_data_url");
            fields.insert(
                "availability".to_owned(),
                Value::String("descriptor_only".to_owned()),
            );
        }
        !resources.is_empty()
    } else {
        false
    };

    if has_image_resources {
        let fidelity = object
            .get_mut("fidelity")
            .and_then(Value::as_object_mut)
            .ok_or_else(|| {
                ProductReplayWorkerError::new(
                    "product_reader_scene_resource_invalid",
                    "Reader scene fidelity envelope is invalid",
                )
            })?;
        fidelity.insert("state".to_owned(), Value::String("partial".to_owned()));
        let reasons = fidelity
            .get_mut("reasons")
            .and_then(Value::as_array_mut)
            .ok_or_else(|| {
                ProductReplayWorkerError::new(
                    "product_reader_scene_resource_invalid",
                    "Reader scene fidelity reasons are invalid",
                )
            })?;
        if !reasons
            .iter()
            .any(|reason| reason.as_str() == Some("image_resource_not_inline"))
        {
            reasons.push(Value::String("image_resource_not_inline".to_owned()));
        }
    }

    if let Some(fonts) = object.get_mut("fonts") {
        let fonts = fonts.as_array_mut().ok_or_else(|| {
            ProductReplayWorkerError::new(
                "product_reader_scene_resource_invalid",
                "Reader scene fonts must be an array",
            )
        })?;
        if !fonts.is_empty() {
            fonts.clear();
            fonts.push(trusted_fallback_font_value()?);
        }
    }

    let payload = serde_json::to_vec(scene).map_err(|_| {
        ProductReplayWorkerError::new(
            "product_reader_scene_resource_invalid",
            "sanitized Reader scene could not be serialized",
        )
    })?;
    if payload.len() > MAX_SCENE_BYTES {
        return Err(ProductReplayWorkerError::new(
            "product_reader_scene_resource_invalid",
            "sanitized Reader scene exceeds the browser response limit",
        ));
    }
    Ok(())
}

fn valid_revision_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && !value
            .chars()
            .any(|ch| ch.is_control() || ch.is_whitespace())
}

/// Invoke only through the existing no-network, resource-limited worker harness.
/// All filesystem inputs and the output descriptor are opened *before* the
/// post-read seccomp default-deny is installed.
#[expect(
    clippy::too_many_arguments,
    reason = "isolated execution trust inputs stay explicit for the worker contract"
)]
pub fn run_product_replay_worker(
    document_id: &str,
    expected_source_sha256: &str,
    expected_source_byte_len: u64,
    project_path: Option<&Path>,
    expected_project_sha256: Option<&str>,
    move_node_intent: Option<IsolatedMoveNodeIntentV1>,
    reader_scene_intent: Option<IsolatedReaderSceneIntentV1>,
    story_intent_path: Option<&Path>,
    editable_export_target: Option<&str>,
    resize_intent_path: Option<&Path>,
    create_shape_intent_path: Option<&Path>,
    create_textbox_intent_path: Option<&Path>,
) -> Result<(), ProductReplayWorkerError> {
    require_document_id(document_id)?;
    require_sha256(expected_source_sha256)?;
    if project_path.is_some() != expected_project_sha256.is_some() {
        return Err(ProductReplayWorkerError::new(
            "product_replay_identity_invalid",
            "project input and expected SHA-256 must be supplied together",
        ));
    }
    if let Some(expected_project_sha256) = expected_project_sha256 {
        require_sha256(expected_project_sha256)?;
    }
    if usize::from(move_node_intent.is_some())
        + usize::from(reader_scene_intent.is_some())
        + usize::from(story_intent_path.is_some())
        + usize::from(editable_export_target.is_some())
        + usize::from(resize_intent_path.is_some())
        + usize::from(create_shape_intent_path.is_some())
        + usize::from(create_textbox_intent_path.is_some())
        > 1
    {
        return Err(ProductReplayWorkerError::new(
            "product_replay_identity_invalid",
            "isolated Product worker admits only one mutation or Scene command",
        ));
    }
    if create_textbox_intent_path.is_some() && project_path.is_none() {
        return Err(ProductReplayWorkerError::new(
            "product_create_textbox_invalid",
            "CreateTextBox requires a verified base project",
        ));
    }
    if create_shape_intent_path.is_some() && project_path.is_none() {
        return Err(ProductReplayWorkerError::new(
            "product_create_shape_invalid",
            "CreateShape requires an authorized exact base project",
        ));
    }
    if resize_intent_path.is_some() && project_path.is_none() {
        return Err(ProductReplayWorkerError::new(
            "product_resize_node_invalid",
            "ResizeNode requires an authorized exact base project",
        ));
    }
    if story_intent_path.is_some() && project_path.is_none() {
        return Err(ProductReplayWorkerError::new(
            "product_story_range_invalid",
            "StoryRange requires an authorized exact base project",
        ));
    }
    if editable_export_target.is_some() && project_path.is_none() {
        return Err(ProductReplayWorkerError::new(
            "product_edited_export_invalid",
            "edited export requires a verified canonical project",
        ));
    }
    if let Some(target) = editable_export_target
        && !matches!(target, "idml" | "odg" | "pdf" | "pdf9up")
    {
        return Err(ProductReplayWorkerError::new(
            "product_edited_export_invalid",
            "only bounded IDML, ODG or PDF export is admitted",
        ));
    }
    if move_node_intent.is_some() && project_path.is_none() {
        return Err(ProductReplayWorkerError::new(
            "product_replay_identity_invalid",
            "MoveNode requires an authorized exact source-bound project",
        ));
    }
    if let Some(scene) = &reader_scene_intent
        && (project_path.is_none()
            || move_node_intent.is_some()
            || !valid_revision_id(&scene.revision_id)
            || !valid_revision_id(&scene.baseline_revision_id))
    {
        return Err(ProductReplayWorkerError::new(
            "product_replay_identity_invalid",
            "Reader scene requires an exact project and exclusive revision identities",
        ));
    }
    if expected_source_byte_len == 0 || expected_source_byte_len > MAX_SOURCE_BYTES {
        return Err(ProductReplayWorkerError::new(
            "product_replay_input_limit",
            "authorized PUB input length is out of range",
        ));
    }

    let input_path = env::var("CHAPTERA_WORKER_INPUT").map_err(|_| {
        ProductReplayWorkerError::new(
            "product_replay_environment_missing",
            "worker input was not provided by isolation harness",
        )
    })?;
    let output_root = env::var("CHAPTERA_WORKER_OUTPUT_DIR").map_err(|_| {
        ProductReplayWorkerError::new(
            "product_replay_environment_missing",
            "worker output was not provided by isolation harness",
        )
    })?;
    let output_root = Path::new(&output_root);
    fs::create_dir_all(output_root).map_err(|_| {
        ProductReplayWorkerError::new(
            "product_replay_output_failed",
            "worker output directory is unavailable",
        )
    })?;
    let output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output_root.join("result.json"))
        .map_err(|_| {
            ProductReplayWorkerError::new(
                "product_replay_output_failed",
                "worker result could not be created",
            )
        })?;

    let mut edited_export_output = if editable_export_target.is_some() {
        Some(
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(output_root.join(EDITED_EXPORT_PART))
                .map_err(|_| {
                    ProductReplayWorkerError::new(
                        "product_edited_export_output_failed",
                        "isolated sidecar could not be opened before sandbox",
                    )
                })?,
        )
    } else {
        None
    };
    let mut preview_outputs = Vec::new();
    if reader_scene_intent.is_some() {
        for i in 0..PREVIEW_PART_COUNT {
            let file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(output_root.join(preview_part_name(i)))
                .map_err(|_| {
                    ProductReplayWorkerError::new(
                        "product_reader_preview_sidecar_invalid",
                        "isolated preview sidecar slot could not be opened",
                    )
                })?;
            preview_outputs.push(file);
        }
    }

    let source_bytes = read_bounded(Path::new(&input_path), expected_source_byte_len)?;
    if source_bytes.len() as u64 != expected_source_byte_len
        || sha256_hex(&source_bytes) != expected_source_sha256
    {
        return Err(ProductReplayWorkerError::new(
            "product_replay_source_mismatch",
            "source bytes differ from the authorized immutable PUB identity",
        ));
    }

    let project = match (project_path, expected_project_sha256) {
        (Some(project_path), Some(expected_sha)) => {
            let project_bytes = read_bounded(project_path, MAX_PROJECT_BYTES)?;
            let project: EditorProject = serde_json::from_slice(&project_bytes).map_err(|_| {
                ProductReplayWorkerError::new(
                    "product_replay_project_invalid",
                    "canonical project JSON is invalid",
                )
            })?;
            if project.source_hash.to_string() != expected_source_sha256
                || project_sha256(&project).map_err(|_| {
                    ProductReplayWorkerError::new(
                        "product_replay_project_invalid",
                        "canonical project identity could not be derived",
                    )
                })? != expected_sha
            {
                return Err(ProductReplayWorkerError::new(
                    "product_replay_project_hash_mismatch",
                    "project/source identity differs from authorized revision",
                ));
            }
            Some(project)
        }
        (None, None) => None,
        _ => unreachable!("paired project input validated before read"),
    };

    // The Story replacement is private input, never copied into process argv,
    // CI receipts or command-line telemetry. Read only before seccomp deny.
    let resize_intent = if let Some(path) = resize_intent_path {
        let bytes = read_bounded(path, MAX_RESIZE_INTENT_BYTES)?;
        let intent: IsolatedResizeNodeIntentV1 = serde_json::from_slice(&bytes).map_err(|_| {
            ProductReplayWorkerError::new(
                "product_resize_node_invalid",
                "isolated ResizeNode intent is not a typed private JSON envelope",
            )
        })?;
        if intent.node_id.is_empty()
            || intent.node_id.len() > 128
            || intent.width_emu <= 0
            || intent.height_emu <= 0
            || intent.x_emu.checked_add(intent.width_emu).is_none()
            || intent.y_emu.checked_add(intent.height_emu).is_none()
        {
            return Err(ProductReplayWorkerError::new(
                "product_resize_node_invalid",
                "isolated ResizeNode intent has non-positive or overflowed bounds",
            ));
        }
        Some(intent)
    } else {
        None
    };
    let story_intent = if let Some(path) = story_intent_path {
        let bytes = read_bounded(path, MAX_STORY_INTENT_BYTES)?;
        let intent: IsolatedStoryRangeIntentV1 = serde_json::from_slice(&bytes).map_err(|_| {
            ProductReplayWorkerError::new(
                "product_story_range_invalid",
                "isolated StoryRange intent is not a typed private JSON envelope",
            )
        })?;
        if intent.story_id.is_empty()
            || intent.story_id.len() > 128
            || intent.end_scalar < intent.start_scalar
            || intent.replacement_text.len() > 8192
            || intent.replacement_text.chars().any(|c| c.is_control())
        {
            return Err(ProductReplayWorkerError::new(
                "product_story_range_invalid",
                "isolated StoryRange exceeds bounded plain-text admission",
            ));
        }
        Some(intent)
    } else {
        None
    };

    let create_shape_intent = if let Some(path) = create_shape_intent_path {
        let bytes = read_bounded(path, MAX_CREATE_SHAPE_INTENT_BYTES)?;
        let intent: IsolatedCreateShapeIntentV1 = serde_json::from_slice(&bytes).map_err(|_| {
            ProductReplayWorkerError::new(
                "product_create_shape_invalid",
                "CreateShape must be a typed private JSON envelope",
            )
        })?;
        validate_create_shape_intent(&intent)?;
        Some(intent)
    } else {
        None
    };

    let create_textbox_intent = if let Some(path) = create_textbox_intent_path {
        let bytes = read_bounded(path, MAX_CREATE_TEXTBOX_INTENT_BYTES)?;
        let intent: IsolatedCreateTextBoxIntentV1 =
            serde_json::from_slice(&bytes).map_err(|_| {
                ProductReplayWorkerError::new(
                    "product_create_textbox_invalid",
                    "CreateTextBox private JSON is not typed",
                )
            })?;
        validate_create_textbox_intent(&intent)?;
        Some(intent)
    } else {
        None
    };

    // No further filesystem operations are permitted after this point.
    // The outer harness also installs syscall network deny and hard limits.
    install_post_read_filesystem_default_deny().map_err(|_| {
        ProductReplayWorkerError::new(
            "product_replay_sandbox_failed",
            "post-read filesystem default-deny could not be installed",
        )
    })?;

    let replay_engine = PubEditorReplayEngine;
    let (project, baseline_mode) = if let Some(project) = project {
        let replayed = replay_engine
            .replay_project(&source_bytes, expected_source_sha256, &project)
            .map_err(|_| {
                ProductReplayWorkerError::new(
                    "product_replay_operation_rejected",
                    "canonical EditorSession did not reproduce the exact project",
                )
            })?;
        if replayed != project {
            return Err(ProductReplayWorkerError::new(
                "product_replay_operation_rejected",
                "isolated EditorSession replay differs from authorized project",
            ));
        }
        (project, false)
    } else {
        let baseline = replay_engine
            .baseline_project(&source_bytes, expected_source_sha256)
            .map_err(|_| {
                ProductReplayWorkerError::new(
                    "product_replay_source_unsupported",
                    "isolated EditorSession could not derive a baseline from PUB",
                )
            })?;
        (baseline, true)
    };
    let actual_project_hash = project_sha256(&project).map_err(|_| {
        ProductReplayWorkerError::new(
            "product_replay_project_invalid",
            "isolated project identity could not be derived",
        )
    })?;
    if let Some(expected_sha) = expected_project_sha256
        && actual_project_hash != expected_sha
    {
        return Err(ProductReplayWorkerError::new(
            "product_replay_project_hash_mismatch",
            "isolated replay returned an unexpected canonical project",
        ));
    }

    let source_hash = Sha256Digest::from_str(expected_source_sha256).map_err(|_| {
        ProductReplayWorkerError::new(
            "product_replay_identity_invalid",
            "source SHA-256 is not canonical",
        )
    })?;
    let mut session = open_mature_0x2c_editor(&source_bytes, source_hash).map_err(|_| {
        ProductReplayWorkerError::new(
            "product_replay_source_unsupported",
            "isolated EditorSession could not open this PUB",
        )
    })?;
    let mut local_replay = project.clone();
    if local_replay.identity.is_none()
        && cloud_replay_requires_local_identity(&local_replay.schema_version)
    {
        // Apply precisely the same version-gated local identity law as the
        // authoritative replay engine. Old schema versions retain None.
        local_replay.identity = session.project().identity;
    }
    session.apply_project(&local_replay).map_err(|_| {
        ProductReplayWorkerError::new(
            "product_replay_operation_rejected",
            "isolated EditorSession rejected canonical project replay",
        )
    })?;

    let move_node = if let Some(intent) = move_node_intent {
        let node_id: NodeId =
            serde_json::from_value(Value::String(intent.node_id)).map_err(|_| {
                ProductReplayWorkerError::new(
                    "product_move_node_invalid",
                    "MoveNode target is not a canonical node identifier",
                )
            })?;
        let operation = session
            .move_node_to(
                node_id,
                LengthEmu::new(intent.x_emu),
                LengthEmu::new(intent.y_emu),
            )
            .map_err(|_| {
                ProductReplayWorkerError::new(
                    "product_move_node_rejected",
                    "isolated canonical EditorSession rejected requested MoveNode",
                )
            })?;
        if !matches!(operation, EditOperation::MoveNode { .. }) {
            return Err(ProductReplayWorkerError::new(
                "product_move_node_invalid",
                "isolated EditorSession emitted an unexpected operation kind",
            ));
        }
        let resulting_project = cloud_revision_project(&session.project());
        let expected_project =
            append_event_operation(project.clone(), operation.clone()).map_err(|_| {
                ProductReplayWorkerError::new(
                    "product_move_node_invalid",
                    "isolated MoveNode cannot be appended to canonical source project",
                )
            })?;
        if expected_project != resulting_project {
            return Err(ProductReplayWorkerError::new(
                "product_move_node_invalid",
                "isolated MoveNode result differs from canonical event append",
            ));
        }
        let resulting_project_sha256 = project_sha256(&resulting_project).map_err(|_| {
            ProductReplayWorkerError::new(
                "product_move_node_invalid",
                "isolated MoveNode resulting project has invalid canonical hash",
            )
        })?;
        Some(IsolatedMoveNodeReceiptV1 {
            operation,
            resulting_project,
            resulting_project_sha256,
        })
    } else {
        None
    };

    let resize_node = if let Some(intent) = resize_intent {
        let node_id: NodeId =
            serde_json::from_value(Value::String(intent.node_id)).map_err(|_| {
                ProductReplayWorkerError::new(
                    "product_resize_node_invalid",
                    "ResizeNode target is not a canonical node identifier",
                )
            })?;
        let bounds = RectEmu::new(
            LengthEmu::new(intent.x_emu),
            LengthEmu::new(intent.y_emu),
            LengthEmu::new(intent.width_emu),
            LengthEmu::new(intent.height_emu),
        );
        let operation = session.resize_node_to(node_id, bounds).map_err(|_| {
            ProductReplayWorkerError::new(
                "product_resize_node_rejected",
                "isolated canonical EditorSession rejected requested ResizeNode",
            )
        })?;
        if !matches!(operation, EditOperation::ResizeNode { .. }) {
            return Err(ProductReplayWorkerError::new(
                "product_resize_node_invalid",
                "isolated EditorSession emitted an unexpected operation kind",
            ));
        }
        let resulting_project = cloud_revision_project(&session.project());
        let expected_project =
            append_event_operation(project.clone(), operation.clone()).map_err(|_| {
                ProductReplayWorkerError::new(
                    "product_resize_node_invalid",
                    "isolated ResizeNode cannot be appended to canonical source project",
                )
            })?;
        if expected_project != resulting_project {
            return Err(ProductReplayWorkerError::new(
                "product_resize_node_invalid",
                "isolated ResizeNode result differs from canonical event append",
            ));
        }
        let resulting_project_sha256 = project_sha256(&resulting_project).map_err(|_| {
            ProductReplayWorkerError::new(
                "product_resize_node_invalid",
                "isolated ResizeNode resulting project has invalid canonical hash",
            )
        })?;
        Some(IsolatedResizeNodeReceiptV1 {
            operation,
            resulting_project,
            resulting_project_sha256,
        })
    } else {
        None
    };

    let create_shape = if let Some(intent) = create_shape_intent {
        let expected_operation = exact_create_shape_operation(&intent)?;
        let EditOperation::CreateShape {
            node_id,
            page_id,
            bounds,
            paint,
            ..
        } = &expected_operation
        else {
            unreachable!("canonical CreateShape intent creates one CreateShape operation")
        };
        // Only canonical, page-owned rectangles wholly inside the selected
        // existing page are admitted; HTML/SVG never gets authoring authority.
        let page = session.graph().pages.get(page_id).ok_or_else(|| {
            ProductReplayWorkerError::new(
                "product_create_shape_rejected",
                "CreateShape page is absent from the source graph",
            )
        })?;
        if bounds
            .right()
            .is_none_or(|x| x.get() > page.size.width.get())
            || bounds
                .bottom()
                .is_none_or(|y| y.get() > page.size.height.get())
        {
            return Err(ProductReplayWorkerError::new(
                "product_create_shape_rejected",
                "CreateShape bounds exceed canonical page geometry",
            ));
        }
        let operation = session
            .create_shape(*node_id, *page_id, *bounds, paint.clone())
            .map_err(|_| {
                ProductReplayWorkerError::new(
                    "product_create_shape_rejected",
                    "isolated EditorSession rejected the authored rectangle",
                )
            })?;
        if operation != expected_operation
            || !matches!(
                session.authored_shape(*node_id),
                Some(shape) if shape.page_id == *page_id
                    && shape.bounds == *bounds && shape.paint == *paint
            )
        {
            return Err(ProductReplayWorkerError::new(
                "product_create_shape_invalid",
                "isolated authored overlay differs from canonical CreateShape",
            ));
        }
        let resulting_project = cloud_revision_project(&session.project());
        let expected_project =
            append_event_operation(project.clone(), operation.clone()).map_err(|_| {
                ProductReplayWorkerError::new(
                    "product_create_shape_invalid",
                    "authored rectangle cannot be appended to canonical project",
                )
            })?;
        if expected_project != resulting_project {
            return Err(ProductReplayWorkerError::new(
                "product_create_shape_invalid",
                "authored rectangle differs from exact event replay",
            ));
        }
        let resulting_project_sha256 = project_sha256(&resulting_project).map_err(|_| {
            ProductReplayWorkerError::new(
                "product_create_shape_invalid",
                "CreateShape resulting project is not canonically hashable",
            )
        })?;
        Some(IsolatedCreateShapeReceiptV1 {
            operation,
            resulting_project,
            resulting_project_sha256,
        })
    } else {
        None
    };

    let create_textbox = if let Some(intent) = create_textbox_intent {
        let expected = exact_create_textbox_operation(&intent)?;
        let EditOperation::CreateTextBox {
            node_id,
            story_id,
            page_id,
            bounds,
            text_preset,
        } = &expected
        else {
            unreachable!("CreateTextBox intent builds one canonical operation")
        };
        let page = session.graph().pages.get(page_id).ok_or_else(|| {
            ProductReplayWorkerError::new(
                "product_create_textbox_rejected",
                "TextFrame page missing",
            )
        })?;
        if bounds
            .right()
            .is_none_or(|x| x.get() > page.size.width.get())
            || bounds
                .bottom()
                .is_none_or(|y| y.get() > page.size.height.get())
        {
            return Err(ProductReplayWorkerError::new(
                "product_create_textbox_rejected",
                "TextFrame extends beyond canonical page",
            ));
        }
        let operation = session
            .create_text_box(*node_id, *story_id, *page_id, *bounds, text_preset.clone())
            .map_err(|_| {
                ProductReplayWorkerError::new(
                    "product_create_textbox_rejected",
                    "isolated EditorSession rejected TextFrame",
                )
            })?;
        let proof = session
            .prove_author_created_story_v1(*story_id)
            .map_err(|_| {
                ProductReplayWorkerError::new(
                    "product_create_textbox_invalid",
                    "author-created Story has no canonical proof",
                )
            })?;
        if operation != expected
            || proof.frame_id != *node_id
            || proof.page_id != *page_id
            || proof.text_preset != *text_preset
        {
            return Err(ProductReplayWorkerError::new(
                "product_create_textbox_invalid",
                "TextFrame/Story differ from canonical operation",
            ));
        }
        let resulting_project = cloud_revision_project(&session.project());
        let expected_project =
            append_event_operation(project.clone(), operation.clone()).map_err(|_| {
                ProductReplayWorkerError::new(
                    "product_create_textbox_invalid",
                    "TextFrame event append failed",
                )
            })?;
        if expected_project != resulting_project {
            return Err(ProductReplayWorkerError::new(
                "product_create_textbox_invalid",
                "TextFrame exact project replay differs",
            ));
        }
        let resulting_project_sha256 = project_sha256(&resulting_project).map_err(|_| {
            ProductReplayWorkerError::new(
                "product_create_textbox_invalid",
                "TextFrame canonical hash invalid",
            )
        })?;
        Some(IsolatedCreateTextBoxReceiptV1 {
            operation,
            resulting_project,
            resulting_project_sha256,
        })
    } else {
        None
    };

    let story_range = if let Some(intent) = story_intent {
        // Only the canonical EditorSession sees PUB-derived Story text.
        // Browser input cannot name or forge expected_before.
        let story_id: StoryId =
            serde_json::from_value(Value::String(intent.story_id)).map_err(|_| {
                ProductReplayWorkerError::new(
                    "product_story_range_invalid",
                    "StoryRange target is not a canonical StoryId",
                )
            })?;
        let story = session.graph().stories.get(&story_id).ok_or_else(|| {
            ProductReplayWorkerError::new(
                "product_story_range_rejected",
                "isolated EditorSession has no requested Story",
            )
        })?;
        let start = intent.start_scalar as usize;
        let end = intent.end_scalar as usize;
        if end > story.text.chars().count() {
            return Err(ProductReplayWorkerError::new(
                "product_story_range_rejected",
                "requested scalar range exceeds the canonical Story",
            ));
        }
        let expected_before = story
            .text
            .chars()
            .skip(start)
            .take(end - start)
            .collect::<String>();
        let operation = session
            .replace_story_range(
                story_id,
                intent.start_scalar,
                intent.end_scalar,
                expected_before,
                intent.replacement_text,
            )
            .map_err(|_| {
                ProductReplayWorkerError::new(
                    "product_story_range_rejected",
                    "isolated canonical EditorSession rejected the Story mutation",
                )
            })?;
        if !matches!(operation, EditOperation::ReplaceStoryRange { .. }) {
            return Err(ProductReplayWorkerError::new(
                "product_story_range_invalid",
                "isolated EditorSession emitted an unexpected mutation kind",
            ));
        }
        let resulting_project = cloud_revision_project(&session.project());
        let expected_project =
            append_event_operation(project.clone(), operation.clone()).map_err(|_| {
                ProductReplayWorkerError::new(
                    "product_story_range_invalid",
                    "Story mutation could not be appended as an exact canonical revision",
                )
            })?;
        if expected_project != resulting_project {
            return Err(ProductReplayWorkerError::new(
                "product_story_range_invalid",
                "Story mutation differs from exact canonical event replay",
            ));
        }
        let resulting_project_sha256 = project_sha256(&resulting_project).map_err(|_| {
            ProductReplayWorkerError::new(
                "product_story_range_invalid",
                "Story mutation project cannot be canonically hashed",
            )
        })?;
        Some(IsolatedStoryRangeReceiptV1 {
            operation,
            resulting_project,
            resulting_project_sha256,
        })
    } else {
        None
    };

    let reader_scene = if let Some(intent) = reader_scene_intent {
        // This is the only execution point for the source-neutral Viewer.
        // It runs AFTER seccomp filesystem deny and under the no-network
        // process limits; the host never calls open_pub_bundle.
        let (scene, preview_samples) =
            crate::product_api_http::render_reader_scene_in_isolated_worker(
                document_id,
                expected_source_sha256,
                &intent.revision_id,
                &intent.baseline_revision_id,
                &source_bytes,
                &project,
            )
            .map_err(|_| {
                ProductReplayWorkerError::new(
                    "product_reader_scene_rejected",
                    "isolated Viewer failed to project the exact authorized revision",
                )
            })?;
        let bytes = serde_json::to_vec(&scene).map_err(|_| {
            ProductReplayWorkerError::new(
                "product_reader_scene_invalid",
                "isolated Reader scene cannot be serialized",
            )
        })?;
        if bytes.len() > MAX_SCENE_BYTES {
            return Err(ProductReplayWorkerError::new(
                "product_reader_scene_limit",
                "isolated Reader scene exceeds the bounded response envelope",
            ));
        }
        let sidecar = write_isolated_preview_sidecar(&mut preview_outputs, &preview_samples)?;
        Some(IsolatedReaderSceneReceiptV1 {
            revision_id: intent.revision_id,
            baseline_revision_id: intent.baseline_revision_id,
            scene_sha256: sha256_hex(&bytes),
            scene,
            preview_sidecar: sidecar,
            preview_samples: Vec::new(),
        })
    } else {
        None
    };

    let editable_export = if let Some(target_name) = editable_export_target {
        let (artifact, loss_report) = if matches!(target_name, "pdf" | "pdf9up") {
            // The only PDF renderer used here is the existing source-neutral
            // pub-pdf. It receives exact current EditorSession state, not a
            // fresh source-only conversion of immutable PUB bytes.
            let rendered = if target_name == "pdf9up" {
                crate::temporary_fixed_pdf::render_current_fixed_pdf_a4_card_9up(
                    &source_bytes,
                    expected_source_sha256,
                    &session,
                )
            } else {
                crate::temporary_fixed_pdf::render_current_fixed_pdf(
                    &source_bytes,
                    expected_source_sha256,
                    &session,
                )
            }
            .map_err(|_| {
                ProductReplayWorkerError::new(
                    "product_edited_export_blocked",
                    "current edited revision cannot be rendered to bounded fixed PDF",
                )
            })?;
            (rendered.bytes, rendered.loss_report)
        } else {
            let target = match target_name {
                "idml" => EditorEditableTarget::Idml,
                "odg" => EditorEditableTarget::Odg,
                _ => {
                    return Err(ProductReplayWorkerError::new(
                        "product_edited_export_invalid",
                        "unknown edited export target",
                    ));
                }
            };
            let label = format!("{document_id}@{actual_project_hash}");
            let preview = session
                .preview_editable_export(target, label.clone())
                .map_err(|_| {
                    ProductReplayWorkerError::new(
                        "product_edited_export_blocked",
                        "current revision target preflight rejected",
                    )
                })?;
            if !preview.report.can_serialize || preview.report.counts.blocking != 0 {
                return Err(ProductReplayWorkerError::new(
                    "product_edited_export_blocked",
                    "current revision has blocking semantic losses",
                ));
            }
            let exported = session.export_editable(target, label).map_err(|_| {
                ProductReplayWorkerError::new(
                    "product_edited_export_blocked",
                    "current revision could not be serialized",
                )
            })?;
            let loss_report = serde_json::to_value(exported.report).map_err(|_| {
                ProductReplayWorkerError::new(
                    "product_edited_export_receipt_invalid",
                    "LossReport could not be encoded",
                )
            })?;
            (exported.bytes, loss_report)
        };
        if artifact.is_empty() || artifact.len() > MAX_EDITED_EXPORT_BYTES {
            return Err(ProductReplayWorkerError::new(
                "product_edited_export_limit",
                "edited artifact exceeds bounded response limit",
            ));
        }
        let loss_bytes = serde_json::to_vec(&loss_report).map_err(|_| {
            ProductReplayWorkerError::new(
                "product_edited_export_receipt_invalid",
                "LossReport serialization failed",
            )
        })?;
        if loss_bytes.len() > MAX_EDITED_LOSS_REPORT_BYTES {
            return Err(ProductReplayWorkerError::new(
                "product_edited_export_limit",
                "private LossReport exceeds bounded receipt",
            ));
        }
        let output = edited_export_output.as_mut().ok_or_else(|| {
            ProductReplayWorkerError::new(
                "product_edited_export_output_failed",
                "output descriptor not prepared before sandbox",
            )
        })?;
        output.write_all(&artifact).map_err(|_| {
            ProductReplayWorkerError::new(
                "product_edited_export_output_failed",
                "edited artifact sidecar write failed",
            )
        })?;
        output.flush().map_err(|_| {
            ProductReplayWorkerError::new(
                "product_edited_export_output_failed",
                "edited artifact sidecar flush failed",
            )
        })?;
        Some(IsolatedEditableExportReceiptV1 {
            target: target_name.to_owned(),
            artifact_sha256: sha256_hex(&artifact),
            artifact_byte_len: artifact.len(),
            loss_report_sha256: sha256_hex(&loss_bytes),
            loss_report,
            artifact_bytes: Vec::new(),
        })
    } else {
        None
    };

    let graph: Value = serde_json::to_value(session.graph()).map_err(|_| {
        ProductReplayWorkerError::new(
            "product_replay_projection_failed",
            "isolated authoring graph could not be serialized",
        )
    })?;
    let graph_bytes = serde_json::to_vec(&graph).map_err(|_| {
        ProductReplayWorkerError::new(
            "product_replay_projection_failed",
            "isolated authoring graph serialization failed",
        )
    })?;
    if graph_bytes.len() > MAX_GRAPH_BYTES {
        return Err(ProductReplayWorkerError::new(
            "product_replay_output_limit",
            "isolated authoring graph exceeds output policy",
        ));
    }

    let receipt = ProductReplayWorkerReceiptV1 {
        protocol_version: PRODUCT_REPLAY_WORKER_V1.to_owned(),
        document_id: document_id.to_owned(),
        source_sha256: expected_source_sha256.to_owned(),
        source_byte_len: expected_source_byte_len,
        project_sha256: actual_project_hash.clone(),
        authoring_graph_sha256: sha256_hex(&graph_bytes),
        authoring_graph: graph,
        baseline_project: if baseline_mode { Some(project) } else { None },
        move_node,
        resize_node,
        create_shape,
        create_textbox,
        story_range,
        reader_scene,
        editable_export,
        filesystem_confinement: true,
    };
    validate_product_replay_receipt(
        &receipt,
        document_id,
        expected_source_sha256,
        expected_source_byte_len,
        &actual_project_hash,
    )?;

    let mut output = BufWriter::new(output);
    serde_json::to_writer(&mut output, &receipt).map_err(|_| {
        ProductReplayWorkerError::new(
            "product_replay_output_failed",
            "isolated projection receipt could not be written",
        )
    })?;
    output.write_all(b"\n").map_err(|_| {
        ProductReplayWorkerError::new(
            "product_replay_output_failed",
            "isolated projection receipt newline could not be written",
        )
    })?;
    output.flush().map_err(|_| {
        ProductReplayWorkerError::new(
            "product_replay_output_failed",
            "isolated projection receipt could not be flushed",
        )
    })
}

/// Production-side adapter. Inputs are selected and authorized by the main
/// server; this adapter never resolves tenant, blob IDs, or access grants.
#[derive(Clone)]
pub struct IsolatedProductReplayProducer {
    config: SourceBaselineProducerConfig,
}

impl IsolatedProductReplayProducer {
    pub fn new(config: SourceBaselineProducerConfig) -> Result<Self, ProductReplayWorkerError> {
        config.validate().map_err(|_| {
            ProductReplayWorkerError::new(
                "product_replay_config_invalid",
                "isolated product worker configuration is invalid",
            )
        })?;
        Ok(Self { config })
    }

    pub async fn project_authoring_graph(
        &self,
        document_id: &str,
        source_sha256: &str,
        source_bytes: &[u8],
        project: &EditorProject,
        expected_project_sha256: &str,
    ) -> Result<PubResolvedGraph, ProductReplayWorkerError> {
        require_document_id(document_id)?;
        require_sha256(source_sha256)?;
        require_sha256(expected_project_sha256)?;
        if source_bytes.is_empty()
            || source_bytes.len() as u64 > MAX_SOURCE_BYTES
            || sha256_hex(source_bytes) != source_sha256
        {
            return Err(ProductReplayWorkerError::new(
                "product_replay_source_mismatch",
                "verified source differs from worker request",
            ));
        }
        let project_bytes = serde_json::to_vec(project).map_err(|_| {
            ProductReplayWorkerError::new(
                "product_replay_project_invalid",
                "canonical project could not be serialized",
            )
        })?;
        if project_bytes.len() as u64 > MAX_PROJECT_BYTES
            || project_sha256(project).map_err(|_| {
                ProductReplayWorkerError::new(
                    "product_replay_project_invalid",
                    "canonical project hash could not be derived",
                )
            })? != expected_project_sha256
        {
            return Err(ProductReplayWorkerError::new(
                "product_replay_project_hash_mismatch",
                "worker project differs from authorized revision",
            ));
        }

        let receipt = self
            .invoke_job(
                document_id,
                source_sha256,
                source_bytes,
                Some((&project_bytes, expected_project_sha256)),
                None,
                None,
                None,
                None,
                None,
                None,
                None,
            )
            .await?;
        validate_product_replay_receipt(
            &receipt,
            document_id,
            source_sha256,
            source_bytes.len() as u64,
            expected_project_sha256,
        )?;
        if receipt.baseline_project.is_some()
            || receipt.move_node.is_some()
            || receipt.reader_scene.is_some()
            || receipt.story_range.is_some()
        {
            return Err(ProductReplayWorkerError::new(
                "product_replay_receipt_invalid",
                "projection worker returned an unexpected baseline or mutation",
            ));
        }
        require_typed_graph(receipt.authoring_graph)
    }

    /// First canonical source projection: the host must never parse PUB in
    /// order to manufacture a baseline EditorProject.
    pub async fn baseline_project(
        &self,
        document_id: &str,
        source_sha256: &str,
        source_bytes: &[u8],
    ) -> Result<EditorProject, ProductReplayWorkerError> {
        let receipt = self
            .invoke_job(
                document_id,
                source_sha256,
                source_bytes,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
            )
            .await?;
        if receipt.move_node.is_some()
            || receipt.reader_scene.is_some()
            || receipt.story_range.is_some()
        {
            return Err(ProductReplayWorkerError::new(
                "product_replay_receipt_invalid",
                "baseline worker unexpectedly returned a mutation or Reader scene",
            ));
        }
        let project = receipt.baseline_project.as_ref().ok_or_else(|| {
            ProductReplayWorkerError::new(
                "product_replay_receipt_invalid",
                "baseline worker did not return a canonical project",
            )
        })?;
        let expected_project_hash = project_sha256(project).map_err(|_| {
            ProductReplayWorkerError::new(
                "product_replay_receipt_invalid",
                "baseline project hash could not be derived",
            )
        })?;
        validate_product_replay_receipt(
            &receipt,
            document_id,
            source_sha256,
            source_bytes.len() as u64,
            &expected_project_hash,
        )?;
        let _ = require_typed_graph(receipt.authoring_graph)?;
        if !project.assets.is_empty() {
            return Err(ProductReplayWorkerError::new(
                "product_replay_asset_unsupported",
                "baseline project references external unresolved asset data",
            ));
        }
        Ok(project.clone())
    }

    /// Return the same rich Reader Scene envelope as the legacy source-neutral
    /// Viewer. The caller has already authorized the source and selected the
    /// exact current RevisionStream head; the host never parses a PUB here.
    pub async fn project_reader_scene(
        &self,
        document_id: &str,
        source_sha256: &str,
        source_bytes: &[u8],
        project: &EditorProject,
        expected_project_sha256: &str,
        intent: &IsolatedReaderSceneIntentV1,
    ) -> Result<Value, ProductReplayWorkerError> {
        if !valid_revision_id(&intent.revision_id)
            || !valid_revision_id(&intent.baseline_revision_id)
        {
            return Err(ProductReplayWorkerError::new(
                "product_reader_scene_invalid",
                "Reader scene revision identity is invalid",
            ));
        }
        let project_bytes = serde_json::to_vec(project).map_err(|_| {
            ProductReplayWorkerError::new(
                "product_replay_project_invalid",
                "canonical project cannot be serialized",
            )
        })?;
        if project_bytes.len() as u64 > MAX_PROJECT_BYTES
            || project_sha256(project).map_err(|_| {
                ProductReplayWorkerError::new(
                    "product_replay_project_invalid",
                    "canonical Reader project hash is invalid",
                )
            })? != expected_project_sha256
        {
            return Err(ProductReplayWorkerError::new(
                "product_replay_project_hash_mismatch",
                "Reader Scene input is not the exact authorized revision project",
            ));
        }
        let receipt = self
            .invoke_job(
                document_id,
                source_sha256,
                source_bytes,
                Some((&project_bytes, expected_project_sha256)),
                None,
                Some(intent),
                None,
                None,
                None,
                None,
                None,
            )
            .await?;
        validate_product_replay_receipt(
            &receipt,
            document_id,
            source_sha256,
            source_bytes.len() as u64,
            expected_project_sha256,
        )?;
        if receipt.baseline_project.is_some()
            || receipt.move_node.is_some()
            || receipt.story_range.is_some()
        {
            return Err(ProductReplayWorkerError::new(
                "product_replay_receipt_invalid",
                "Reader scene worker returned baseline or geometry mutation",
            ));
        }
        let mut scene = receipt.reader_scene.ok_or_else(|| {
            ProductReplayWorkerError::new(
                "product_replay_receipt_invalid",
                "Reader scene worker did not return an exact scene",
            )
        })?;
        if scene.revision_id != intent.revision_id
            || scene.baseline_revision_id != intent.baseline_revision_id
            || scene.scene["revision_id"] != intent.revision_id
            || scene.scene["document_id"] != document_id
            || scene.scene["source_hash"] != source_sha256
        {
            return Err(ProductReplayWorkerError::new(
                "product_replay_receipt_identity_mismatch",
                "Reader scene receipt was replayed for another document or revision",
            ));
        }
        sanitize_reader_scene_browser_decoders(&mut scene.scene)?;
        admit_host_reencoded_previews(&mut scene.scene, &scene.preview_samples).map_err(
            |code| ProductReplayWorkerError::new("product_reader_safe_preview_invalid", code),
        )?;
        Ok(scene.scene)
    }

    /// Exact geometry mutation is performed inside the confined worker.
    /// The server independently checks the typed operation and deterministic
    /// project transition before submitting the durable RevisionStream edge.
    pub async fn move_node_to(
        &self,
        document_id: &str,
        source_sha256: &str,
        source_bytes: &[u8],
        base_project: &EditorProject,
        expected_base_sha256: &str,
        intent: &IsolatedMoveNodeIntentV1,
    ) -> Result<(EditOperation, EditorProject), ProductReplayWorkerError> {
        let project_bytes = serde_json::to_vec(base_project).map_err(|_| {
            ProductReplayWorkerError::new(
                "product_replay_project_invalid",
                "canonical base project could not be serialized",
            )
        })?;
        if project_bytes.len() as u64 > MAX_PROJECT_BYTES
            || project_sha256(base_project).map_err(|_| {
                ProductReplayWorkerError::new(
                    "product_replay_project_invalid",
                    "canonical base project hash could not be derived",
                )
            })? != expected_base_sha256
        {
            return Err(ProductReplayWorkerError::new(
                "product_replay_project_hash_mismatch",
                "MoveNode base is not the exact authorized revision",
            ));
        }
        let receipt = self
            .invoke_job(
                document_id,
                source_sha256,
                source_bytes,
                Some((&project_bytes, expected_base_sha256)),
                Some(intent),
                None,
                None,
                None,
                None,
                None,
                None,
            )
            .await?;
        validate_product_replay_receipt(
            &receipt,
            document_id,
            source_sha256,
            source_bytes.len() as u64,
            expected_base_sha256,
        )?;
        if receipt.baseline_project.is_some()
            || receipt.reader_scene.is_some()
            || receipt.story_range.is_some()
        {
            return Err(ProductReplayWorkerError::new(
                "product_replay_receipt_invalid",
                "isolated MoveNode response unexpectedly included a baseline or Reader scene",
            ));
        }
        let mutation = receipt.move_node.ok_or_else(|| {
            ProductReplayWorkerError::new(
                "product_replay_receipt_invalid",
                "isolated MoveNode receipt does not contain a mutation",
            )
        })?;
        let node_id: NodeId = serde_json::from_value(Value::String(intent.node_id.clone()))
            .map_err(|_| {
                ProductReplayWorkerError::new(
                    "product_move_node_invalid",
                    "MoveNode target identifier is not canonical",
                )
            })?;
        match &mutation.operation {
            EditOperation::MoveNode {
                node_id: actual_id,
                after,
                ..
            } if *actual_id == node_id
                && after.x.get() == intent.x_emu
                && after.y.get() == intent.y_emu => {}
            _ => {
                return Err(ProductReplayWorkerError::new(
                    "product_replay_receipt_invalid",
                    "isolated MoveNode operation differs from authorized intent",
                ));
            }
        }
        let expected_project =
            append_event_operation(base_project.clone(), mutation.operation.clone()).map_err(
                |_| {
                    ProductReplayWorkerError::new(
                        "product_replay_receipt_invalid",
                        "isolated MoveNode cannot form a canonical project transition",
                    )
                },
            )?;
        if expected_project != mutation.resulting_project {
            return Err(ProductReplayWorkerError::new(
                "product_replay_receipt_invalid",
                "isolated MoveNode project differs from the exact event append",
            ));
        }
        let graph = require_typed_graph(receipt.authoring_graph)?;
        let bounds = graph.nodes.get(&node_id).map(|node| node.header.bounds);
        if !matches!(bounds, Some(bounds) if bounds.x.get() == intent.x_emu
            && bounds.y.get() == intent.y_emu)
        {
            return Err(ProductReplayWorkerError::new(
                "product_replay_receipt_invalid",
                "isolated MoveNode graph does not represent its claimed geometry",
            ));
        }
        Ok((mutation.operation, mutation.resulting_project))
    }

    /// A bounded, page-owned identity-transform ResizeNode, applied only by
    /// the isolated canonical EditorSession from the exact authorized project.
    pub async fn resize_node_to(
        &self,
        document_id: &str,
        source_sha256: &str,
        source_bytes: &[u8],
        base_project: &EditorProject,
        expected_base_sha256: &str,
        intent: &IsolatedResizeNodeIntentV1,
    ) -> Result<(EditOperation, EditorProject), ProductReplayWorkerError> {
        if intent.width_emu <= 0
            || intent.height_emu <= 0
            || intent.x_emu.checked_add(intent.width_emu).is_none()
            || intent.y_emu.checked_add(intent.height_emu).is_none()
        {
            return Err(ProductReplayWorkerError::new(
                "product_resize_node_invalid",
                "ResizeNode after bounds must be positive and non-overflowing",
            ));
        }
        let project_bytes = serde_json::to_vec(base_project).map_err(|_| {
            ProductReplayWorkerError::new(
                "product_replay_project_invalid",
                "canonical ResizeNode base project could not be serialized",
            )
        })?;
        if project_bytes.len() as u64 > MAX_PROJECT_BYTES
            || project_sha256(base_project).map_err(|_| {
                ProductReplayWorkerError::new(
                    "product_replay_project_invalid",
                    "canonical ResizeNode base project hash could not be derived",
                )
            })? != expected_base_sha256
        {
            return Err(ProductReplayWorkerError::new(
                "product_replay_project_hash_mismatch",
                "ResizeNode base is not the exact authorized revision",
            ));
        }
        let receipt = self
            .invoke_job(
                document_id,
                source_sha256,
                source_bytes,
                Some((&project_bytes, expected_base_sha256)),
                None,
                None,
                None,
                None,
                Some(intent),
                None,
                None,
            )
            .await?;
        validate_product_replay_receipt(
            &receipt,
            document_id,
            source_sha256,
            source_bytes.len() as u64,
            expected_base_sha256,
        )?;
        if receipt.baseline_project.is_some()
            || receipt.move_node.is_some()
            || receipt.story_range.is_some()
            || receipt.reader_scene.is_some()
            || receipt.editable_export.is_some()
        {
            return Err(ProductReplayWorkerError::new(
                "product_replay_receipt_invalid",
                "isolated ResizeNode returned unrelated authority",
            ));
        }
        let mutation = receipt.resize_node.ok_or_else(|| {
            ProductReplayWorkerError::new(
                "product_replay_receipt_invalid",
                "isolated ResizeNode receipt does not contain a mutation",
            )
        })?;
        let node_id: NodeId = serde_json::from_value(Value::String(intent.node_id.clone()))
            .map_err(|_| {
                ProductReplayWorkerError::new(
                    "product_resize_node_invalid",
                    "ResizeNode target identifier is not canonical",
                )
            })?;
        match &mutation.operation {
            EditOperation::ResizeNode {
                node_id: actual_id,
                after,
                ..
            } if *actual_id == node_id
                && after.x.get() == intent.x_emu
                && after.y.get() == intent.y_emu
                && after.width.get() == intent.width_emu
                && after.height.get() == intent.height_emu => {}
            _ => {
                return Err(ProductReplayWorkerError::new(
                    "product_replay_receipt_invalid",
                    "isolated ResizeNode differs from authorized intent",
                ));
            }
        }
        let expected_project =
            append_event_operation(base_project.clone(), mutation.operation.clone()).map_err(
                |_| {
                    ProductReplayWorkerError::new(
                        "product_replay_receipt_invalid",
                        "isolated ResizeNode cannot form a canonical project transition",
                    )
                },
            )?;
        if expected_project != mutation.resulting_project {
            return Err(ProductReplayWorkerError::new(
                "product_replay_receipt_invalid",
                "isolated ResizeNode project differs from exact event append",
            ));
        }
        let graph = require_typed_graph(receipt.authoring_graph)?;
        let bounds = graph.nodes.get(&node_id).map(|node| node.header.bounds);
        if !matches!(bounds, Some(bounds) if bounds.x.get() == intent.x_emu
            && bounds.y.get() == intent.y_emu
            && bounds.width.get() == intent.width_emu
            && bounds.height.get() == intent.height_emu)
        {
            return Err(ProductReplayWorkerError::new(
                "product_replay_receipt_invalid",
                "isolated ResizeNode graph does not represent its claimed geometry",
            ));
        }
        Ok((mutation.operation, mutation.resulting_project))
    }

    /// Isolated, source-bound authored Rectangle mutation. The host must mint a
    /// collision-resistant UUIDv7 NodeId and authorize the exact PageId before
    /// invoking this method. No mutable PUB bytes or source parsing on the host.
    pub async fn create_shape(
        &self,
        document_id: &str,
        source_sha256: &str,
        source_bytes: &[u8],
        base_project: &EditorProject,
        expected_base_sha256: &str,
        intent: &IsolatedCreateShapeIntentV1,
    ) -> Result<(EditOperation, EditorProject), ProductReplayWorkerError> {
        let expected = exact_create_shape_operation(intent)?;
        let project_bytes = serde_json::to_vec(base_project).map_err(|_| {
            ProductReplayWorkerError::new(
                "product_replay_project_invalid",
                "canonical CreateShape base project could not be serialized",
            )
        })?;
        if project_bytes.len() as u64 > MAX_PROJECT_BYTES
            || project_sha256(base_project).map_err(|_| {
                ProductReplayWorkerError::new(
                    "product_replay_project_invalid",
                    "CreateShape base identity cannot be derived",
                )
            })? != expected_base_sha256
        {
            return Err(ProductReplayWorkerError::new(
                "product_replay_project_hash_mismatch",
                "CreateShape must target the exact current source-bound project",
            ));
        }
        let receipt = self
            .invoke_job(
                document_id,
                source_sha256,
                source_bytes,
                Some((&project_bytes, expected_base_sha256)),
                None,
                None,
                None,
                None,
                None,
                Some(intent),
                None,
            )
            .await?;
        validate_product_replay_receipt(
            &receipt,
            document_id,
            source_sha256,
            source_bytes.len() as u64,
            expected_base_sha256,
        )?;
        if receipt.baseline_project.is_some()
            || receipt.move_node.is_some()
            || receipt.resize_node.is_some()
            || receipt.story_range.is_some()
            || receipt.reader_scene.is_some()
            || receipt.editable_export.is_some()
        {
            return Err(ProductReplayWorkerError::new(
                "product_replay_receipt_invalid",
                "CreateShape worker returned unrelated authority",
            ));
        }
        let created = receipt.create_shape.ok_or_else(|| {
            ProductReplayWorkerError::new(
                "product_replay_receipt_invalid",
                "CreateShape worker did not produce a mutation",
            )
        })?;
        if created.operation != expected {
            return Err(ProductReplayWorkerError::new(
                "product_replay_receipt_invalid",
                "CreateShape operation differs from authorized intent",
            ));
        }
        let expected_project =
            append_event_operation(base_project.clone(), created.operation.clone()).map_err(
                |_| {
                    ProductReplayWorkerError::new(
                        "product_replay_receipt_invalid",
                        "CreateShape cannot form an exact event append",
                    )
                },
            )?;
        if expected_project != created.resulting_project {
            return Err(ProductReplayWorkerError::new(
                "product_replay_receipt_invalid",
                "CreateShape project differs from canonical replay",
            ));
        }
        let _ = require_typed_graph(receipt.authoring_graph)?;
        Ok((created.operation, created.resulting_project))
    }

    /// Source-backed, bounded plain Story text. The isolated EditorSession
    /// derives expected_before and owns eligibility; the host verifies the
    /// complete canonical event before any authorized RevisionStream commit.
    pub async fn create_textbox(
        &self,
        document_id: &str,
        source_sha256: &str,
        source_bytes: &[u8],
        base_project: &EditorProject,
        expected_base_sha256: &str,
        intent: &IsolatedCreateTextBoxIntentV1,
    ) -> Result<(EditOperation, EditorProject), ProductReplayWorkerError> {
        let expected = exact_create_textbox_operation(intent)?;
        let project_bytes = serde_json::to_vec(base_project).map_err(|_| {
            ProductReplayWorkerError::new(
                "product_replay_project_invalid",
                "canonical CreateTextBox base project could not be serialized",
            )
        })?;
        if project_bytes.len() as u64 > MAX_PROJECT_BYTES
            || project_sha256(base_project).map_err(|_| {
                ProductReplayWorkerError::new(
                    "product_replay_project_invalid",
                    "CreateTextBox base identity cannot be derived",
                )
            })? != expected_base_sha256
        {
            return Err(ProductReplayWorkerError::new(
                "product_replay_project_hash_mismatch",
                "CreateTextBox must target the exact current source-bound project",
            ));
        }
        let receipt = self
            .invoke_job(
                document_id,
                source_sha256,
                source_bytes,
                Some((&project_bytes, expected_base_sha256)),
                None,
                None,
                None,
                None,
                None,
                None,
                Some(intent),
            )
            .await?;
        validate_product_replay_receipt(
            &receipt,
            document_id,
            source_sha256,
            source_bytes.len() as u64,
            expected_base_sha256,
        )?;
        if receipt.baseline_project.is_some()
            || receipt.move_node.is_some()
            || receipt.resize_node.is_some()
            || receipt.story_range.is_some()
            || receipt.reader_scene.is_some()
            || receipt.editable_export.is_some()
        {
            return Err(ProductReplayWorkerError::new(
                "product_replay_receipt_invalid",
                "CreateTextBox worker returned unrelated authority",
            ));
        }
        let created = receipt.create_textbox.ok_or_else(|| {
            ProductReplayWorkerError::new(
                "product_replay_receipt_invalid",
                "CreateTextBox worker did not produce a mutation",
            )
        })?;
        if created.operation != expected {
            return Err(ProductReplayWorkerError::new(
                "product_replay_receipt_invalid",
                "CreateTextBox operation differs from authorized intent",
            ));
        }
        let expected_project =
            append_event_operation(base_project.clone(), created.operation.clone()).map_err(
                |_| {
                    ProductReplayWorkerError::new(
                        "product_replay_receipt_invalid",
                        "CreateTextBox cannot form an exact event append",
                    )
                },
            )?;
        if expected_project != created.resulting_project {
            return Err(ProductReplayWorkerError::new(
                "product_replay_receipt_invalid",
                "CreateTextBox project differs from canonical replay",
            ));
        }
        let _ = require_typed_graph(receipt.authoring_graph)?;
        Ok((created.operation, created.resulting_project))
    }

    /// Source-backed, bounded plain Story text. The isolated EditorSession
    /// derives expected_before and owns eligibility; the host verifies the
    /// complete canonical event before any authorized RevisionStream commit.
    pub async fn replace_story_range(
        &self,
        document_id: &str,
        source_sha256: &str,
        source_bytes: &[u8],
        base_project: &EditorProject,
        expected_base_sha256: &str,
        intent: &IsolatedStoryRangeIntentV1,
    ) -> Result<(EditOperation, EditorProject), ProductReplayWorkerError> {
        if intent.end_scalar < intent.start_scalar
            || intent.replacement_text.len() > 8192
            || intent.replacement_text.chars().any(|ch| ch.is_control())
        {
            return Err(ProductReplayWorkerError::new(
                "product_story_range_invalid",
                "unsupported plain Story range intent",
            ));
        }
        let project_bytes = serde_json::to_vec(base_project).map_err(|_| {
            ProductReplayWorkerError::new(
                "product_replay_project_invalid",
                "canonical Story base project could not be serialized",
            )
        })?;
        if project_bytes.len() as u64 > MAX_PROJECT_BYTES
            || project_sha256(base_project).map_err(|_| {
                ProductReplayWorkerError::new(
                    "product_replay_project_invalid",
                    "canonical Story base project hash could not be derived",
                )
            })? != expected_base_sha256
        {
            return Err(ProductReplayWorkerError::new(
                "product_replay_project_hash_mismatch",
                "StoryRange base is not the exact authorized revision",
            ));
        }
        let receipt = self
            .invoke_job(
                document_id,
                source_sha256,
                source_bytes,
                Some((&project_bytes, expected_base_sha256)),
                None,
                None,
                Some(intent),
                None,
                None,
                None,
                None,
            )
            .await?;
        validate_product_replay_receipt(
            &receipt,
            document_id,
            source_sha256,
            source_bytes.len() as u64,
            expected_base_sha256,
        )?;
        let (mutation, authoring_graph) = take_exclusive_story_range_receipt(receipt)?;
        let story_id: StoryId = serde_json::from_value(Value::String(intent.story_id.clone()))
            .map_err(|_| {
                ProductReplayWorkerError::new(
                    "product_story_range_invalid",
                    "requested StoryId is not canonical",
                )
            })?;
        match &mutation.operation {
            EditOperation::ReplaceStoryRange {
                story_id: actual_id,
                start_scalar,
                end_scalar,
                replacement_text,
                ..
            } if *actual_id == story_id
                && *start_scalar == intent.start_scalar
                && *end_scalar == intent.end_scalar
                && replacement_text == &intent.replacement_text => {}
            _ => {
                return Err(ProductReplayWorkerError::new(
                    "product_replay_receipt_invalid",
                    "isolated Story operation differs from authorized browser range",
                ));
            }
        }
        let expected_project =
            append_event_operation(base_project.clone(), mutation.operation.clone()).map_err(
                |_| {
                    ProductReplayWorkerError::new(
                        "product_replay_receipt_invalid",
                        "isolated Story operation is not an appendable canonical event",
                    )
                },
            )?;
        if expected_project != mutation.resulting_project {
            return Err(ProductReplayWorkerError::new(
                "product_replay_receipt_invalid",
                "isolated Story project differs from the exact event append",
            ));
        }
        let graph = require_typed_graph(authoring_graph)?;
        if !graph.stories.contains_key(&story_id) {
            return Err(ProductReplayWorkerError::new(
                "product_replay_receipt_invalid",
                "isolated Story graph omitted the mutated Story",
            ));
        }
        Ok((mutation.operation, mutation.resulting_project))
    }

    /// Produces one bounded physical artifact from the exact edited revision.
    /// The original source and private worker scratch remain ephemeral.
    pub async fn export_editable_current(
        &self,
        document_id: &str,
        source_sha256: &str,
        source_bytes: &[u8],
        project: &EditorProject,
        project_hash: &str,
        target: &str,
    ) -> Result<IsolatedEditableExportReceiptV1, ProductReplayWorkerError> {
        require_document_id(document_id)?;
        require_sha256(source_sha256)?;
        require_sha256(project_hash)?;
        if !matches!(target, "idml" | "odg" | "pdf" | "pdf9up") {
            return Err(ProductReplayWorkerError::new(
                "product_edited_export_invalid",
                "temporary output admits only IDML, ODG and PDF",
            ));
        }
        let project_bytes = serde_json::to_vec(project).map_err(|_| {
            ProductReplayWorkerError::new(
                "product_edited_export_invalid",
                "canonical edited project cannot be encoded",
            )
        })?;
        if project_bytes.len() > MAX_PROJECT_BYTES as usize
            || project_sha256(project).map_err(|_| {
                ProductReplayWorkerError::new(
                    "product_edited_export_invalid",
                    "canonical project hash is unavailable",
                )
            })? != project_hash
        {
            return Err(ProductReplayWorkerError::new(
                "product_edited_export_invalid",
                "exact edited project identity mismatch",
            ));
        }
        let receipt = self
            .invoke_job(
                document_id,
                source_sha256,
                source_bytes,
                Some((&project_bytes, project_hash)),
                None,
                None,
                None,
                Some(target),
                None,
                None,
                None,
            )
            .await?;
        validate_product_replay_receipt(
            &receipt,
            document_id,
            source_sha256,
            source_bytes.len() as u64,
            project_hash,
        )?;
        if receipt.baseline_project.is_some()
            || receipt.move_node.is_some()
            || receipt.story_range.is_some()
            || receipt.reader_scene.is_some()
        {
            return Err(ProductReplayWorkerError::new(
                "product_edited_export_receipt_invalid",
                "worker returned unrelated authority during edited export",
            ));
        }
        let edited = receipt.editable_export.ok_or_else(|| {
            ProductReplayWorkerError::new(
                "product_edited_export_receipt_invalid",
                "worker returned no edited artifact",
            )
        })?;
        if edited.target != target
            || edited.artifact_bytes.len() != edited.artifact_byte_len
            || edited.artifact_bytes.is_empty()
            || sha256_hex(&edited.artifact_bytes) != edited.artifact_sha256
        {
            return Err(ProductReplayWorkerError::new(
                "product_edited_export_receipt_invalid",
                "physical edited bytes do not match worker receipt",
            ));
        }
        Ok(edited)
    }

    /// Executes only the existing per-file isolation harness. The host may
    /// serialize verified bytes but must not interpret the source PUB.
    #[expect(
        clippy::too_many_arguments,
        reason = "each independent source and mutation authority input is verified"
    )]
    async fn invoke_job(
        &self,
        document_id: &str,
        source_sha256: &str,
        source_bytes: &[u8],
        project: Option<(&[u8], &str)>,
        move_node: Option<&IsolatedMoveNodeIntentV1>,
        reader_scene: Option<&IsolatedReaderSceneIntentV1>,
        story_range: Option<&IsolatedStoryRangeIntentV1>,
        editable_export_target: Option<&str>,
        resize_node: Option<&IsolatedResizeNodeIntentV1>,
        create_shape: Option<&IsolatedCreateShapeIntentV1>,
        create_textbox: Option<&IsolatedCreateTextBoxIntentV1>,
    ) -> Result<ProductReplayWorkerReceiptV1, ProductReplayWorkerError> {
        require_document_id(document_id)?;
        require_sha256(source_sha256)?;
        if source_bytes.is_empty()
            || source_bytes.len() as u64 > MAX_SOURCE_BYTES
            || sha256_hex(source_bytes) != source_sha256
        {
            return Err(ProductReplayWorkerError::new(
                "product_replay_source_mismatch",
                "verified source differs from worker request",
            ));
        }
        if usize::from(move_node.is_some())
            + usize::from(reader_scene.is_some())
            + usize::from(story_range.is_some())
            + usize::from(editable_export_target.is_some())
            + usize::from(resize_node.is_some())
            + usize::from(create_shape.is_some())
            + usize::from(create_textbox.is_some())
            > 1
        {
            return Err(ProductReplayWorkerError::new(
                "product_replay_identity_invalid",
                "Product worker must receive only one Scene or mutation command",
            ));
        }
        if reader_scene.is_some() && (project.is_none() || move_node.is_some()) {
            return Err(ProductReplayWorkerError::new(
                "product_reader_scene_invalid",
                "Reader scene request must have an exact project without a mutation",
            ));
        }
        if editable_export_target.is_some() && project.is_none() {
            return Err(ProductReplayWorkerError::new(
                "product_edited_export_invalid",
                "edited output requires a canonical verified project",
            ));
        }
        if let Some(target) = editable_export_target
            && !matches!(target, "idml" | "odg" | "pdf" | "pdf9up")
        {
            return Err(ProductReplayWorkerError::new(
                "product_edited_export_invalid",
                "unknown edited target",
            ));
        }
        if create_textbox.is_some() && project.is_none() {
            return Err(ProductReplayWorkerError::new(
                "product_create_textbox_invalid",
                "CreateTextBox needs a verified project",
            ));
        }
        if create_shape.is_some() && project.is_none() {
            return Err(ProductReplayWorkerError::new(
                "product_create_shape_invalid",
                "CreateShape requires an authorized source-bound project",
            ));
        }
        if resize_node.is_some() && project.is_none() {
            return Err(ProductReplayWorkerError::new(
                "product_resize_node_invalid",
                "ResizeNode requires a verified exact base project",
            ));
        }
        if story_range.is_some() && project.is_none() {
            return Err(ProductReplayWorkerError::new(
                "product_story_range_invalid",
                "StoryRange requires a verified project and source",
            ));
        }
        if move_node.is_some() && project.is_none() {
            return Err(ProductReplayWorkerError::new(
                "product_move_node_invalid",
                "MoveNode requires a verified project and source",
            ));
        }
        if let Some((bytes, expected_hash)) = project {
            require_sha256(expected_hash)?;
            if bytes.len() as u64 > MAX_PROJECT_BYTES {
                return Err(ProductReplayWorkerError::new(
                    "product_replay_input_limit",
                    "canonical project exceeds worker input limits",
                ));
            }
        }

        let temp = PrivateReplayTemp::create(&self.config.temp_root)?;
        let source_path = temp.path.join("source.pub");
        let project_path = temp.path.join("project.json");
        let story_intent_path = temp.path.join("story-intent.json");
        let resize_intent_path = temp.path.join("resize-intent.json");
        let create_shape_intent_path = temp.path.join("create-shape-intent.json");
        let create_textbox_intent_path = temp.path.join("create-textbox-intent.json");
        let result_dir = temp.path.join("worker-result");
        write_private_replay_input(&source_path, source_bytes).await?;
        if let Some((project_bytes, _)) = project {
            write_private_replay_input(&project_path, project_bytes).await?;
        }
        if let Some(intent) = create_shape {
            validate_create_shape_intent(intent)?;
            let bytes = serde_json::to_vec(intent).map_err(|_| {
                ProductReplayWorkerError::new(
                    "product_create_shape_invalid",
                    "private CreateShape intent cannot be serialized",
                )
            })?;
            if bytes.len() as u64 > MAX_CREATE_SHAPE_INTENT_BYTES {
                return Err(ProductReplayWorkerError::new(
                    "product_create_shape_invalid",
                    "private CreateShape intent exceeds bounded envelope",
                ));
            }
            write_private_replay_input(&create_shape_intent_path, &bytes).await?;
        }
        if let Some(intent) = create_textbox {
            validate_create_textbox_intent(intent)?;
            let bytes = serde_json::to_vec(intent).map_err(|_| {
                ProductReplayWorkerError::new(
                    "product_create_textbox_invalid",
                    "private TextFrame intent cannot be serialized",
                )
            })?;
            if bytes.len() as u64 > MAX_CREATE_TEXTBOX_INTENT_BYTES {
                return Err(ProductReplayWorkerError::new(
                    "product_create_textbox_invalid",
                    "private TextFrame intent too large",
                ));
            }
            write_private_replay_input(&create_textbox_intent_path, &bytes).await?;
        }
        if let Some(intent) = resize_node {
            let bytes = serde_json::to_vec(intent).map_err(|_| {
                ProductReplayWorkerError::new(
                    "product_resize_node_invalid",
                    "private ResizeNode intent could not be serialized",
                )
            })?;
            if bytes.len() as u64 > MAX_RESIZE_INTENT_BYTES {
                return Err(ProductReplayWorkerError::new(
                    "product_resize_node_invalid",
                    "private ResizeNode intent exceeded limits",
                ));
            }
            write_private_replay_input(&resize_intent_path, &bytes).await?;
        }
        if let Some(intent) = story_range {
            let bytes = serde_json::to_vec(intent).map_err(|_| {
                ProductReplayWorkerError::new(
                    "product_story_range_invalid",
                    "private StoryRange intent could not be serialized",
                )
            })?;
            if bytes.len() as u64 > MAX_STORY_INTENT_BYTES {
                return Err(ProductReplayWorkerError::new(
                    "product_story_range_invalid",
                    "private StoryRange intent exceeded limits",
                ));
            }
            write_private_replay_input(&story_intent_path, &bytes).await?;
        }

        let mut command = Command::new(&self.config.isolation_python);
        command
            .arg(&self.config.isolation_harness)
            .arg("run")
            .arg("--output-dir")
            .arg(&result_dir)
            .arg("--input")
            .arg(&source_path)
            .arg("--timeout")
            .arg(self.config.worker_wall_timeout.as_secs_f64().to_string())
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
            .arg("product-isolated-replay")
            .arg("--document-id")
            .arg(document_id)
            .arg("--expected-sha256")
            .arg(source_sha256)
            .arg("--expected-byte-len")
            .arg(source_bytes.len().to_string());
        if let Some((_, expected_hash)) = project {
            command
                .arg("--project-json")
                .arg(&project_path)
                .arg("--expected-project-sha256")
                .arg(expected_hash);
        }
        if let Some(move_node) = move_node {
            command
                .arg("--move-node-id")
                .arg(&move_node.node_id)
                .arg("--move-x-emu")
                .arg(move_node.x_emu.to_string())
                .arg("--move-y-emu")
                .arg(move_node.y_emu.to_string());
        }
        if story_range.is_some() {
            command.arg("--story-intent-json").arg(&story_intent_path);
        }
        if resize_node.is_some() {
            command.arg("--resize-intent-json").arg(&resize_intent_path);
        }
        if create_shape.is_some() {
            command
                .arg("--create-shape-intent-json")
                .arg(&create_shape_intent_path);
        }
        if create_textbox.is_some() {
            command
                .arg("--create-textbox-intent-json")
                .arg(&create_textbox_intent_path);
        }
        if let Some(target) = editable_export_target {
            command.arg("--editable-export-target").arg(target);
        }
        if let Some(scene) = reader_scene {
            command
                .arg("--scene-revision-id")
                .arg(&scene.revision_id)
                .arg("--scene-baseline-revision-id")
                .arg(&scene.baseline_revision_id);
        }
        let deadline = self
            .config
            .worker_wall_timeout
            .checked_add(std::time::Duration::from_secs(5))
            .ok_or_else(|| {
                ProductReplayWorkerError::new(
                    "product_replay_config_invalid",
                    "isolated worker deadline overflowed",
                )
            })?;
        let status = timeout(deadline, command.kill_on_drop(true).output())
            .await
            .map_err(|_| {
                ProductReplayWorkerError::new(
                    "product_replay_worker_timeout",
                    "isolated worker exceeded the parent deadline",
                )
            })?
            .map_err(|_| {
                ProductReplayWorkerError::new(
                    "product_replay_worker_failed",
                    "isolated worker could not be launched",
                )
            })?;
        if !status.status.success() {
            return Err(ProductReplayWorkerError::new(
                "product_replay_worker_failed",
                "isolated worker failed without a usable receipt",
            ));
        }

        let receipt_path = result_dir.join("result.json");
        let metadata = async_fs::metadata(&receipt_path).await.map_err(|_| {
            ProductReplayWorkerError::new(
                "product_replay_receipt_missing",
                "isolated worker receipt is unavailable",
            )
        })?;
        let limit = MAX_GRAPH_BYTES as u64 + MAX_PROJECT_BYTES + 1024 * 1024;
        if !metadata.is_file() || metadata.len() == 0 || metadata.len() > limit {
            return Err(ProductReplayWorkerError::new(
                "product_replay_receipt_invalid",
                "isolated worker receipt exceeds bounded file limits",
            ));
        }
        let payload = async_fs::read(&receipt_path).await.map_err(|_| {
            ProductReplayWorkerError::new(
                "product_replay_receipt_missing",
                "isolated worker receipt could not be read",
            )
        })?;
        if payload.len() as u64 > limit {
            return Err(ProductReplayWorkerError::new(
                "product_replay_receipt_invalid",
                "isolated worker receipt grew beyond size limits",
            ));
        }
        let mut receipt: ProductReplayWorkerReceiptV1 =
            serde_json::from_slice(&payload).map_err(|_| {
                ProductReplayWorkerError::new(
                    "product_replay_receipt_invalid",
                    "isolated worker receipt is malformed",
                )
            })?;
        read_isolated_preview_sidecar(&mut receipt, &result_dir).await?;
        read_isolated_edited_export_sidecar(&mut receipt, &result_dir).await?;
        Ok(receipt)
    }
}

async fn write_private_replay_input(
    path: &Path,
    bytes: &[u8],
) -> Result<(), ProductReplayWorkerError> {
    let mut file = async_fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .await
        .map_err(|_| {
            ProductReplayWorkerError::new(
                "product_replay_temp_failed",
                "private worker input could not be created",
            )
        })?;
    file.write_all(bytes).await.map_err(|_| {
        ProductReplayWorkerError::new(
            "product_replay_temp_failed",
            "private worker input write failed",
        )
    })?;
    file.sync_all().await.map_err(|_| {
        ProductReplayWorkerError::new(
            "product_replay_temp_failed",
            "private worker input sync failed",
        )
    })
}

fn require_typed_graph(graph: Value) -> Result<PubResolvedGraph, ProductReplayWorkerError> {
    serde_json::from_value(graph).map_err(|_| {
        ProductReplayWorkerError::new(
            "product_replay_receipt_invalid",
            "isolated graph does not match typed authoring graph schema",
        )
    })
}

/// Private staging is removed whether a worker succeeds, fails, or is cancelled.
struct PrivateReplayTemp {
    path: std::path::PathBuf,
}

impl PrivateReplayTemp {
    fn create(root: &Path) -> Result<Self, ProductReplayWorkerError> {
        fs::create_dir_all(root).map_err(|_| {
            ProductReplayWorkerError::new(
                "product_replay_temp_failed",
                "configured private temp root is unavailable",
            )
        })?;
        for _ in 0..4 {
            let path = root.join(format!(
                "product-replay-{}-{:016x}",
                std::process::id(),
                OsRng.next_u64()
            ));
            let mut builder = fs::DirBuilder::new();
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            match builder.create(&path) {
                Ok(()) => return Ok(Self { path }),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(_) => break,
            }
        }
        Err(ProductReplayWorkerError::new(
            "product_replay_temp_failed",
            "could not allocate private replay job directory",
        ))
    }
}

impl Drop for PrivateReplayTemp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn receipt() -> ProductReplayWorkerReceiptV1 {
        let graph = serde_json::json!({"nodes": [{"node_id": "test"}]});
        ProductReplayWorkerReceiptV1 {
            protocol_version: PRODUCT_REPLAY_WORKER_V1.to_owned(),
            document_id: "document-a".to_owned(),
            source_sha256: "a".repeat(64),
            source_byte_len: 512,
            project_sha256: "b".repeat(64),
            authoring_graph_sha256: sha256_hex(&serde_json::to_vec(&graph).unwrap()),
            authoring_graph: graph,
            baseline_project: None,
            move_node: None,
            resize_node: None,
            create_shape: None,
            create_textbox: None,
            story_range: None,
            reader_scene: None,
            editable_export: None,
            filesystem_confinement: true,
        }
    }

    #[test]
    fn create_shape_private_intent_denies_nonpositive_geometry_and_unknown_fields() {
        let base = IsolatedCreateShapeIntentV1 {
            node_id: "019a2d80-1112-7001-8001-aabbccddee01".to_owned(),
            page_id: "019a2d80-1112-7001-8001-aabbccddee02".to_owned(),
            x_emu: 914_400,
            y_emu: 914_400,
            width_emu: 1_828_800,
            height_emu: 914_400,
            fill_rgb: [18, 52, 86],
            stroke_rgb: [170, 187, 204],
            stroke_width_emu: 12_700,
        };
        validate_create_shape_intent(&base).unwrap();
        assert!(matches!(
            exact_create_shape_operation(&base).unwrap(),
            EditOperation::CreateShape { .. }
        ));
        let mut invalid = base.clone();
        invalid.width_emu = 0;
        assert_eq!(
            validate_create_shape_intent(&invalid).unwrap_err().code,
            "product_create_shape_invalid"
        );
        invalid = base.clone();
        invalid.x_emu = i64::MAX;
        assert!(validate_create_shape_intent(&invalid).is_err());
        invalid = base.clone();
        invalid.stroke_width_emu = 0;
        assert!(validate_create_shape_intent(&invalid).is_err());
        invalid = base.clone();
        invalid.node_id = "019a2d80-1112-4001-8001-aabbccddee01".to_owned();
        assert!(validate_create_shape_intent(&invalid).is_err());
        let mut object = serde_json::to_value(base).unwrap();
        object["unexpected"] = Value::Bool(true);
        assert!(serde_json::from_value::<IsolatedCreateShapeIntentV1>(object).is_err());
    }

    #[test]
    fn isolated_story_range_receipt_accepts_story_but_rejects_missing_or_mixed_authority() {
        let source_sha256 = "a".repeat(64);
        let project = EditorProject {
            schema_version: "pub-editor-v0.11".to_owned(),
            source_hash: Sha256Digest::from_str(&source_sha256).unwrap(),
            identity: None,
            assets: Vec::new(),
            table_grids: Vec::new(),
            operations: Vec::new(),
        };
        let story_id: StoryId = serde_json::from_value(Value::String(
            "40000000-0000-4000-8000-000000000001".to_owned(),
        ))
        .unwrap();
        let mut valid = receipt();
        valid.story_range = Some(IsolatedStoryRangeReceiptV1 {
            operation: EditOperation::ReplaceStoryRange {
                story_id,
                start_scalar: 0,
                end_scalar: 1,
                expected_before: "A".to_owned(),
                replacement_text: "B".to_owned(),
                before_story_state_id: format!("sha256:{}", "1".repeat(64)),
                after_story_state_id: format!("sha256:{}", "2".repeat(64)),
            },
            resulting_project_sha256: project_sha256(&project).unwrap(),
            resulting_project: project.clone(),
        });
        // The source, project and graph checks pass before host-only
        // exclusive Story extraction. A synthetic graph is not E2E PUB proof.
        validate_product_replay_receipt(&valid, "document-a", &source_sha256, 512, &"b".repeat(64))
            .unwrap();
        let (mutation, graph) = take_exclusive_story_range_receipt(valid.clone()).unwrap();
        assert!(matches!(
            mutation.operation,
            EditOperation::ReplaceStoryRange { .. }
        ));
        assert_eq!(graph, valid.authoring_graph);

        assert_eq!(
            take_exclusive_story_range_receipt(receipt())
                .unwrap_err()
                .code,
            "product_replay_receipt_invalid"
        );
        let mut mixed = valid;
        mixed.baseline_project = Some(project);
        assert_eq!(
            take_exclusive_story_range_receipt(mixed).unwrap_err().code,
            "product_replay_receipt_invalid"
        );
    }

    #[tokio::test]
    async fn isolated_preview_sidecar_roundtrips_and_rejects_tampering() {
        let mut encoded_png = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut encoded_png, 2, 2);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            writer
                .write_image_data(&[
                    255, 0, 0, 255, 0, 0, 255, 128, 0, 0, 255, 255, 50, 50, 50, 255,
                ])
                .unwrap();
        }
        let previews = crate::safe_preview_scene::collect_worker_preview_samples(std::iter::once(
            ("image-a".to_owned(), "image/png", encoded_png.as_slice()),
        ));
        assert_eq!(previews.len(), 1);

        let root = PrivateReplayTemp::create(&std::env::temp_dir()).unwrap();
        let mut files = Vec::new();
        for i in 0..PREVIEW_PART_COUNT {
            files.push(
                OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(root.path.join(preview_part_name(i)))
                    .unwrap(),
            );
        }
        let manifest = write_isolated_preview_sidecar(&mut files, &previews)
            .unwrap()
            .unwrap();
        assert_eq!(manifest.chunk_count, 1);
        let mut receipt = receipt();
        receipt.reader_scene = Some(IsolatedReaderSceneReceiptV1 {
            revision_id: "revision-a".to_owned(),
            baseline_revision_id: "revision-a".to_owned(),
            scene_sha256: "c".repeat(64),
            scene: json!({}),
            preview_sidecar: Some(manifest),
            preview_samples: Vec::new(),
        });
        // The private receipt never inlines decoded pixels.
        let serialized = serde_json::to_value(&receipt).unwrap();
        assert!(serialized["reader_scene"].get("preview_samples").is_none());
        read_isolated_preview_sidecar(&mut receipt, &root.path)
            .await
            .unwrap();
        assert_eq!(
            receipt.reader_scene.as_ref().unwrap().preview_samples[0].samples_base64,
            previews[0].samples_base64
        );

        let part_path = root.path.join(preview_part_name(0));
        let mut tampered = fs::read(&part_path).unwrap();
        tampered[0] ^= 1;
        fs::write(part_path, tampered).unwrap();
        assert_eq!(
            read_isolated_preview_sidecar(&mut receipt, &root.path)
                .await
                .unwrap_err()
                .code,
            "product_reader_preview_sidecar_invalid"
        );
    }

    #[test]
    fn preview_sidecar_splits_at_eight_mebibytes_without_unbounded_json() {
        let mut encoded_png = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut encoded_png, 1, 1);
            encoder.set_color(png::ColorType::Rgb);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(&[11, 22, 33]).unwrap();
        }
        let mut previews = crate::safe_preview_scene::collect_worker_preview_samples(
            std::iter::once(("image-a".to_owned(), "image/png", encoded_png.as_slice())),
        );
        assert_eq!(previews.len(), 1);
        // The sidecar writer exercises transport, not pixel admission.
        previews[0].samples_base64 = "A".repeat(PREVIEW_PART_BYTES + 1);
        let root = PrivateReplayTemp::create(&std::env::temp_dir()).unwrap();
        let mut files = Vec::new();
        for i in 0..PREVIEW_PART_COUNT {
            files.push(
                OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(root.path.join(preview_part_name(i)))
                    .unwrap(),
            );
        }
        let sidecar = write_isolated_preview_sidecar(&mut files, &previews)
            .unwrap()
            .unwrap();
        assert_eq!(sidecar.chunk_count, 2);
        assert_eq!(
            fs::metadata(root.path.join(preview_part_name(0)))
                .unwrap()
                .len(),
            PREVIEW_PART_BYTES as u64
        );
        assert!(
            fs::metadata(root.path.join(preview_part_name(1)))
                .unwrap()
                .len()
                < PREVIEW_PART_BYTES as u64
        );
    }

    #[test]
    fn rejects_tampered_or_unconfined_graph_receipts() {
        let valid = receipt();
        assert!(
            validate_product_replay_receipt(
                &valid,
                "document-a",
                &"a".repeat(64),
                512,
                &"b".repeat(64)
            )
            .is_ok()
        );

        let mut tampered = valid.clone();
        tampered.authoring_graph = serde_json::json!({"nodes": [{"node_id": "injected"}]});
        assert_eq!(
            validate_product_replay_receipt(
                &tampered,
                "document-a",
                &"a".repeat(64),
                512,
                &"b".repeat(64)
            )
            .unwrap_err()
            .code,
            "product_replay_receipt_invalid"
        );
        let mut unconfined = valid.clone();
        unconfined.filesystem_confinement = false;
        assert_eq!(
            validate_product_replay_receipt(
                &unconfined,
                "document-a",
                &"a".repeat(64),
                512,
                &"b".repeat(64)
            )
            .unwrap_err()
            .code,
            "product_replay_receipt_identity_mismatch"
        );
        let mut alien = valid;
        alien.project_sha256 = "c".repeat(64);
        assert_eq!(
            validate_product_replay_receipt(
                &alien,
                "document-a",
                &"a".repeat(64),
                512,
                &"b".repeat(64)
            )
            .unwrap_err()
            .code,
            "product_replay_receipt_identity_mismatch"
        );
    }

    #[test]
    fn browser_decoder_boundary_strips_worker_images_and_rebuilds_trusted_font() {
        let mut scene = serde_json::json!({
            "protocol_version": READER_SCENE_V1,
            "scene_authority": "server_viewer_projection",
            "document_id": "document-a",
            "source_hash": "a".repeat(64),
            "revision_id": "revision-a",
            "stacking_fidelity": "source",
            "fidelity": {"state": "supported", "reasons": []},
            "pages": [],
            "nodes": [],
            "stories": [],
            "resources": [{
                "resource_id": "resource-a",
                "mime": "image/png",
                "availability": "inline_data_url",
                "inline_data_url": "data:image/png;base64,ATTACKER_CONTROLLED"
            }],
            "fonts": [{
                "resource_id": "worker-forged-font",
                "family_name": "Forged",
                "mime": "font/ttf",
                "expected_sha256": "b".repeat(64),
                "availability": "inline_data_url",
                "inline_data_url": "data:font/ttf;base64,ATTACKER_CONTROLLED"
            }]
        });

        sanitize_reader_scene_browser_decoders(&mut scene).unwrap();

        let image = &scene["resources"][0];
        assert_eq!(image["availability"], "descriptor_only");
        assert!(image.get("inline_data_url").is_none());
        assert_eq!(scene["fidelity"]["state"], "partial");
        assert!(
            scene["fidelity"]["reasons"]
                .as_array()
                .unwrap()
                .iter()
                .any(|reason| reason == "image_resource_not_inline")
        );

        let fonts = scene["fonts"].as_array().unwrap();
        assert_eq!(fonts.len(), 1);
        let font = &fonts[0];
        assert_eq!(
            font["resource_id"],
            chaptera_desktop_fallback_font_resource::RESOURCE_ID
        );
        assert_eq!(
            font["family_name"],
            chaptera_desktop_fallback_font_resource::FAMILY_NAME
        );
        assert_eq!(
            font["expected_sha256"],
            chaptera_desktop_fallback_font_resource::EXPECTED_SHA256
        );
        assert_eq!(font["mime"], "font/ttf");
        assert!(
            font["inline_data_url"]
                .as_str()
                .is_some_and(|value| value.starts_with("data:font/ttf;base64,"))
        );
        assert!(
            !font["inline_data_url"]
                .as_str()
                .unwrap()
                .contains("ATTACKER_CONTROLLED")
        );
    }

    #[test]
    fn reader_scene_receipt_rejects_replayed_revision_and_bad_shape() {
        let mut expected = receipt();
        let scene = serde_json::json!({
            "protocol_version": READER_SCENE_V1,
            "scene_authority": "server_viewer_projection",
            "document_id": "document-a",
            "source_hash": "a".repeat(64),
            "revision_id": "revision-a",
            "stacking_fidelity": "source",
            "fidelity": {"state": "Exact", "reasons": []},
            "pages": [],
            "nodes": [],
            "stories": []
        });
        expected.reader_scene = Some(IsolatedReaderSceneReceiptV1 {
            revision_id: "revision-a".into(),
            baseline_revision_id: "revision-a".into(),
            scene_sha256: sha256_hex(&serde_json::to_vec(&scene).unwrap()),
            scene,
            preview_sidecar: None,
            preview_samples: Vec::new(),
        });
        assert!(
            validate_product_replay_receipt(
                &expected,
                "document-a",
                &"a".repeat(64),
                512,
                &"b".repeat(64)
            )
            .is_ok()
        );

        let mut other_revision = expected.clone();
        let refscene = other_revision.reader_scene.as_mut().unwrap();
        refscene.scene["revision_id"] = Value::String("revision-other".into());
        // Even when a malicious worker recomputes the checksum, its revision
        // payload must be bound to the declared exact revision identity.
        refscene.scene_sha256 = sha256_hex(&serde_json::to_vec(&refscene.scene).unwrap());
        assert_eq!(
            validate_product_replay_receipt(
                &other_revision,
                "document-a",
                &"a".repeat(64),
                512,
                &"b".repeat(64)
            )
            .unwrap_err()
            .code,
            "product_replay_receipt_invalid"
        );

        let mut leaked = expected.clone();
        let leak = leaked.reader_scene.as_mut().unwrap();
        leak.scene["project"] = serde_json::json!({"source_file": "should-not-leak"});
        leak.scene_sha256 = sha256_hex(&serde_json::to_vec(&leak.scene).unwrap());
        assert_eq!(
            validate_product_replay_receipt(
                &leaked,
                "document-a",
                &"a".repeat(64),
                512,
                &"b".repeat(64)
            )
            .unwrap_err()
            .code,
            "product_replay_receipt_invalid"
        );

        // Recomputing a valid scene SHA must never make private fields in
        // child collections eligible for browser delivery.
        for (collection, injected) in [
            (
                "pages",
                serde_json::json!({
                    "page_id": "page-a",
                    "order": 0,
                    "width_emu": 100,
                    "height_emu": 100,
                    "private_file_path": "/srv/private/source.pub",
                }),
            ),
            (
                "stories",
                serde_json::json!({
                    "story_id": "story-a",
                    "text": "visible",
                    "text_fidelity": "source",
                    "raw_pub_bytes": "secret",
                }),
            ),
            (
                "resources",
                serde_json::json!({
                    "resource_id": "resource-a",
                    "mime": "image/png",
                    "availability": "available",
                    "provider_token": "secret",
                }),
            ),
            (
                "nodes",
                serde_json::json!({
                    "node_id": "node-a",
                    "page_id": "page-a",
                    "kind": "shape",
                    "bounds": {"x": 0, "y": 0, "width": 10, "height": 10},
                    "transform": {"a": "1", "b": "0", "c": "0", "d": "1", "tx": 0, "ty": 0},
                    "editor_project": {"private": "secret"},
                }),
            ),
        ] {
            let mut forged = expected.clone();
            let scene = forged.reader_scene.as_mut().unwrap();
            scene.scene[collection] = serde_json::json!([injected]);
            scene.scene_sha256 = sha256_hex(&serde_json::to_vec(&scene.scene).unwrap());
            assert_eq!(
                validate_product_replay_receipt(
                    &forged,
                    "document-a",
                    &"a".repeat(64),
                    512,
                    &"b".repeat(64),
                )
                .unwrap_err()
                .code,
                "product_replay_receipt_invalid",
                "nested leak in {collection} should be rejected",
            );
        }

        let mut bad_shape = expected.clone();
        let bad = bad_shape.reader_scene.as_mut().unwrap();
        bad.scene["nodes"] = Value::String("forged nodes".into());
        bad.scene_sha256 = sha256_hex(&serde_json::to_vec(&bad.scene).unwrap());
        assert_eq!(
            validate_product_replay_receipt(
                &bad_shape,
                "document-a",
                &"a".repeat(64),
                512,
                &"b".repeat(64)
            )
            .unwrap_err()
            .code,
            "product_replay_receipt_invalid"
        );
    }

    #[test]
    fn rejects_untyped_projection_even_when_it_has_a_self_consistent_checksum() {
        let receipt = receipt();
        // A malicious worker could recompute a checksum over arbitrary JSON;
        // accepting a checksum alone is not a validated authoring graph.
        validate_product_replay_receipt(
            &receipt,
            "document-a",
            &"a".repeat(64),
            512,
            &"b".repeat(64),
        )
        .unwrap();
        assert_eq!(
            require_typed_graph(receipt.authoring_graph)
                .unwrap_err()
                .code,
            "product_replay_receipt_invalid"
        );
    }

    #[test]
    fn worker_identity_validation_rejects_paths_and_noncanonical_hashes() {
        assert!(require_document_id("document:one").is_ok());
        for id in ["", "spaces in id", "foo\nbar", "x\t"] {
            assert!(require_document_id(id).is_err());
        }
        for hash in ["a".repeat(63), "A".repeat(64), "z".repeat(64)] {
            assert!(require_sha256(&hash).is_err());
        }
    }
}
