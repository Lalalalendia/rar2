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
    EditOperation, EditorProject, LengthEmu, NodeId, Sha256Digest, open_mature_0x2c_editor,
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
}

const MAX_SOURCE_BYTES: u64 = 256 * 1024 * 1024;
const MAX_PROJECT_BYTES: u64 = 32 * 1024 * 1024;
const MAX_GRAPH_BYTES: usize = 64 * 1024 * 1024;

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
    pub reader_scene: Option<IsolatedReaderSceneReceiptV1>,
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
    if let Some(scene) = &receipt.reader_scene {
        let payload = serde_json::to_vec(&scene.scene).map_err(|_| {
            ProductReplayWorkerError::new(
                "product_replay_receipt_invalid",
                "Reader scene receipt could not be serialized",
            )
        })?;
        if receipt.baseline_project.is_some()
            || receipt.move_node.is_some()
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
fn approved_reader_scene_shape(scene: &Value) -> bool {
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
fn sanitize_reader_scene_browser_decoders(
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
pub fn run_product_replay_worker(
    document_id: &str,
    expected_source_sha256: &str,
    expected_source_byte_len: u64,
    project_path: Option<&Path>,
    expected_project_sha256: Option<&str>,
    move_node_intent: Option<IsolatedMoveNodeIntentV1>,
    reader_scene_intent: Option<IsolatedReaderSceneIntentV1>,
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

    let reader_scene = if let Some(intent) = reader_scene_intent {
        // This is the only execution point for the source-neutral Viewer.
        // It runs AFTER seccomp filesystem deny and under the no-network
        // process limits; the host never calls open_pub_bundle.
        let scene = crate::product_api_http::render_reader_scene_in_isolated_worker(
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
        Some(IsolatedReaderSceneReceiptV1 {
            revision_id: intent.revision_id,
            baseline_revision_id: intent.baseline_revision_id,
            scene_sha256: sha256_hex(&bytes),
            scene,
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
        reader_scene,
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
            .invoke_job(document_id, source_sha256, source_bytes, None, None, None)
            .await?;
        if receipt.move_node.is_some() || receipt.reader_scene.is_some() {
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
            )
            .await?;
        validate_product_replay_receipt(
            &receipt,
            document_id,
            source_sha256,
            source_bytes.len() as u64,
            expected_project_sha256,
        )?;
        if receipt.baseline_project.is_some() || receipt.move_node.is_some() {
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
            )
            .await?;
        validate_product_replay_receipt(
            &receipt,
            document_id,
            source_sha256,
            source_bytes.len() as u64,
            expected_base_sha256,
        )?;
        if receipt.baseline_project.is_some() || receipt.reader_scene.is_some() {
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

    /// Executes only the existing per-file isolation harness. The host may
    /// serialize verified bytes but must not interpret the source PUB.
    async fn invoke_job(
        &self,
        document_id: &str,
        source_sha256: &str,
        source_bytes: &[u8],
        project: Option<(&[u8], &str)>,
        move_node: Option<&IsolatedMoveNodeIntentV1>,
        reader_scene: Option<&IsolatedReaderSceneIntentV1>,
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
        if reader_scene.is_some() && (project.is_none() || move_node.is_some()) {
            return Err(ProductReplayWorkerError::new(
                "product_reader_scene_invalid",
                "Reader scene request must have an exact project without a mutation",
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
        let result_dir = temp.path.join("worker-result");
        write_private_replay_input(&source_path, source_bytes).await?;
        if let Some((project_bytes, _)) = project {
            write_private_replay_input(&project_path, project_bytes).await?;
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
        serde_json::from_slice(&payload).map_err(|_| {
            ProductReplayWorkerError::new(
                "product_replay_receipt_invalid",
                "isolated worker receipt is malformed",
            )
        })
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
            reader_scene: None,
            filesystem_confinement: true,
        }
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
