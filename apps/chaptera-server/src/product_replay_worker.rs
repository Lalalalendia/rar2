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
    sync::Arc,
};

use chaptera_untrusted_pub_scan::install_post_read_filesystem_default_deny;
use pub_editor::{EditorProject, Sha256Digest, open_mature_0x2c_editor};
use pub_reader::PubResolvedGraph;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use rand::{RngCore, rngs::OsRng};
use tokio::{fs as async_fs, io::AsyncWriteExt, process::Command, time::timeout};

use crate::{
    revision_materializer::{
        DocumentSourceAuthority, ExactRevisionMaterializationReceipt,
        ExactRevisionMaterializedState, ExactRevisionMaterializer, ExactRevisionMaterializerPort,
        ExactSourceLoader, MATERIALIZATION_RECEIPT_SCHEMA_V1, PubEditorReplayEngine,
        RevisionMaterializerError, project_sha256, validate_authorized_source,
    },
    source_baseline::SourceBaselineProducerConfig,
    sqlite_store::{RevisionEdge, SqliteRevisionStore},
};

pub const PRODUCT_REPLAY_WORKER_V1: &str = "chaptera.product-isolated-replay.v1";
pub const PRODUCT_MATERIALIZATION_WORKER_V1: &str =
    "chaptera.product-isolated-materialize.v1";
const MAX_SOURCE_BYTES: u64 = 256 * 1024 * 1024;
const MAX_PROJECT_BYTES: u64 = 32 * 1024 * 1024;
const MAX_GRAPH_BYTES: usize = 64 * 1024 * 1024;
const MAX_REPLAY_CHAIN_BYTES: u64 = 32 * 1024 * 1024;
const MAX_REPLAY_EDGES: usize = 4096;
const MAX_MATERIALIZATION_RECEIPT_BYTES: u64 = MAX_PROJECT_BYTES + 2 * 1024 * 1024;

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
    pub filesystem_confinement: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductMaterializationInputV1 {
    pub protocol_version: String,
    pub document_id: String,
    pub source_sha256: String,
    pub source_byte_len: u64,
    pub baseline_revision_id: String,
    pub baseline_cursor: i64,
    pub requested_revision_id: String,
    pub edges: Vec<RevisionEdge>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductMaterializationWorkerReceiptV1 {
    pub protocol_version: String,
    pub document_id: String,
    pub source_sha256: String,
    pub source_byte_len: u64,
    pub baseline_revision_id: String,
    pub baseline_cursor: i64,
    pub requested_revision_id: String,
    pub replayed_edges: usize,
    pub project_sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authoring_root_hash: Option<String>,
    pub project: EditorProject,
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
    Ok(())
}

fn validate_materialization_input(
    input: &ProductMaterializationInputV1,
    document_id: &str,
    source_sha256: &str,
    source_byte_len: u64,
) -> Result<(), ProductReplayWorkerError> {
    if input.protocol_version != PRODUCT_MATERIALIZATION_WORKER_V1
        || input.document_id != document_id
        || input.source_sha256 != source_sha256
        || input.source_byte_len != source_byte_len
        || input.baseline_cursor < 0
        || input.edges.len() > MAX_REPLAY_EDGES
    {
        return Err(ProductReplayWorkerError::new(
            "product_materialization_input_identity_mismatch",
            "isolated materialization input differs from authorized exact identity",
        ));
    }
    require_document_id(&input.baseline_revision_id)?;
    require_document_id(&input.requested_revision_id)?;

    let mut expected_parent = input.baseline_revision_id.as_str();
    let mut expected_cursor = input.baseline_cursor;
    for edge in &input.edges {
        if edge.document_id != input.document_id
            || edge.parent_revision != expected_parent
            || edge.parent_cursor != expected_cursor
            || edge.child_cursor != expected_cursor.checked_add(1).ok_or_else(|| {
                ProductReplayWorkerError::new(
                    "product_materialization_chain_invalid",
                    "revision cursor overflowed isolated replay policy",
                )
            })?
        {
            return Err(ProductReplayWorkerError::new(
                "product_materialization_chain_invalid",
                "revision chain is not contiguous from the authorized baseline",
            ));
        }
        require_document_id(&edge.child_revision)?;
        require_sha256(&edge.resulting_state_hash)?;
        if let Some(root) = &edge.authoring_root_hash {
            require_sha256(root)?;
        }
        expected_parent = edge.child_revision.as_str();
        expected_cursor = edge.child_cursor;
    }
    if expected_parent != input.requested_revision_id {
        return Err(ProductReplayWorkerError::new(
            "product_materialization_chain_invalid",
            "revision chain does not terminate at the requested revision",
        ));
    }
    Ok(())
}

fn validate_materialization_receipt(
    receipt: &ProductMaterializationWorkerReceiptV1,
    input: &ProductMaterializationInputV1,
) -> Result<(), ProductReplayWorkerError> {
    let expected_root = input
        .edges
        .last()
        .and_then(|edge| edge.authoring_root_hash.clone());
    if receipt.protocol_version != PRODUCT_MATERIALIZATION_WORKER_V1
        || receipt.document_id != input.document_id
        || receipt.source_sha256 != input.source_sha256
        || receipt.source_byte_len != input.source_byte_len
        || receipt.baseline_revision_id != input.baseline_revision_id
        || receipt.baseline_cursor != input.baseline_cursor
        || receipt.requested_revision_id != input.requested_revision_id
        || receipt.replayed_edges != input.edges.len()
        || receipt.authoring_root_hash != expected_root
        || !receipt.filesystem_confinement
    {
        return Err(ProductReplayWorkerError::new(
            "product_materialization_receipt_identity_mismatch",
            "isolated materialization receipt differs from the authorized revision chain",
        ));
    }
    require_sha256(&receipt.project_sha256)?;
    if project_sha256(&receipt.project).map_err(|_| {
        ProductReplayWorkerError::new(
            "product_materialization_receipt_invalid",
            "isolated materialized project hash could not be derived",
        )
    })? != receipt.project_sha256
    {
        return Err(ProductReplayWorkerError::new(
            "product_materialization_receipt_invalid",
            "isolated materialized project differs from its SHA-256 evidence",
        ));
    }
    let project_bytes = serde_json::to_vec(&receipt.project).map_err(|_| {
        ProductReplayWorkerError::new(
            "product_materialization_receipt_invalid",
            "isolated materialized project could not be serialized",
        )
    })?;
    if project_bytes.len() as u64 > MAX_PROJECT_BYTES {
        return Err(ProductReplayWorkerError::new(
            "product_materialization_receipt_invalid",
            "isolated materialized project exceeds output policy",
        ));
    }
    Ok(())
}

/// Invoke only through the existing no-network, resource-limited worker harness.
/// All filesystem inputs and the output descriptor are opened *before* the
/// post-read seccomp default-deny is installed.
pub fn run_product_replay_worker(
    document_id: &str,
    expected_source_sha256: &str,
    expected_source_byte_len: u64,
    project_path: &Path,
    expected_project_sha256: &str,
) -> Result<(), ProductReplayWorkerError> {
    require_document_id(document_id)?;
    require_sha256(expected_source_sha256)?;
    require_sha256(expected_project_sha256)?;
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

    let project_bytes = read_bounded(project_path, MAX_PROJECT_BYTES)?;
    let project: EditorProject = serde_json::from_slice(&project_bytes).map_err(|_| {
        ProductReplayWorkerError::new(
            "product_replay_project_invalid",
            "canonical project JSON is invalid",
        )
    })?;
    if project.source_hash.to_string() != expected_source_sha256 {
        return Err(ProductReplayWorkerError::new(
            "product_replay_project_source_mismatch",
            "canonical project is bound to a different immutable PUB",
        ));
    }
    let actual_project_hash = project_sha256(&project).map_err(|_| {
        ProductReplayWorkerError::new(
            "product_replay_project_invalid",
            "canonical project identity could not be derived",
        )
    })?;
    if actual_project_hash != expected_project_sha256 {
        return Err(ProductReplayWorkerError::new(
            "product_replay_project_hash_mismatch",
            "canonical project differs from the authorized revision",
        ));
    }

    // No further filesystem operations are permitted after this point.
    // The outer harness also installs syscall network deny and hard limits.
    install_post_read_filesystem_default_deny().map_err(|_| {
        ProductReplayWorkerError::new(
            "product_replay_sandbox_failed",
            "post-read filesystem default-deny could not be installed",
        )
    })?;

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
    session.apply_project(&project).map_err(|_| {
        ProductReplayWorkerError::new(
            "product_replay_operation_rejected",
            "isolated EditorSession rejected canonical project replay",
        )
    })?;

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
        project_sha256: actual_project_hash,
        authoring_graph_sha256: sha256_hex(&graph_bytes),
        authoring_graph: graph,
        filesystem_confinement: true,
    };
    validate_product_replay_receipt(
        &receipt,
        document_id,
        expected_source_sha256,
        expected_source_byte_len,
        expected_project_sha256,
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

pub fn run_product_materialization_worker(
    document_id: &str,
    expected_source_sha256: &str,
    expected_source_byte_len: u64,
    replay_json: &Path,
) -> Result<(), ProductReplayWorkerError> {
    require_document_id(document_id)?;
    require_sha256(expected_source_sha256)?;
    if expected_source_byte_len == 0 || expected_source_byte_len > MAX_SOURCE_BYTES {
        return Err(ProductReplayWorkerError::new(
            "product_materialization_input_limit",
            "authorized PUB input length is out of range",
        ));
    }

    let input_path = env::var("CHAPTERA_WORKER_INPUT").map_err(|_| {
        ProductReplayWorkerError::new(
            "product_materialization_environment_missing",
            "worker input was not provided by isolation harness",
        )
    })?;
    let output_root = env::var("CHAPTERA_WORKER_OUTPUT_DIR").map_err(|_| {
        ProductReplayWorkerError::new(
            "product_materialization_environment_missing",
            "worker output was not provided by isolation harness",
        )
    })?;
    let output_root = Path::new(&output_root);
    fs::create_dir_all(output_root).map_err(|_| {
        ProductReplayWorkerError::new(
            "product_materialization_output_failed",
            "worker output directory is unavailable",
        )
    })?;
    let output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output_root.join("result.json"))
        .map_err(|_| {
            ProductReplayWorkerError::new(
                "product_materialization_output_failed",
                "worker result could not be created",
            )
        })?;

    let source_bytes = read_bounded(Path::new(&input_path), expected_source_byte_len)?;
    if source_bytes.len() as u64 != expected_source_byte_len
        || sha256_hex(&source_bytes) != expected_source_sha256
    {
        return Err(ProductReplayWorkerError::new(
            "product_materialization_source_mismatch",
            "source bytes differ from the authorized immutable PUB identity",
        ));
    }

    let replay_bytes = read_bounded(replay_json, MAX_REPLAY_CHAIN_BYTES)?;
    let input: ProductMaterializationInputV1 =
        serde_json::from_slice(&replay_bytes).map_err(|_| {
            ProductReplayWorkerError::new(
                "product_materialization_input_invalid",
                "isolated revision chain JSON is malformed",
            )
        })?;
    validate_materialization_input(
        &input,
        document_id,
        expected_source_sha256,
        expected_source_byte_len,
    )?;

    install_post_read_filesystem_default_deny().map_err(|_| {
        ProductReplayWorkerError::new(
            "product_materialization_sandbox_failed",
            "post-read filesystem default-deny could not be installed",
        )
    })?;

    let (project, authoring_root_hash) = ExactRevisionMaterializer::replay_exact_chain(
        &PubEditorReplayEngine,
        &source_bytes,
        &input.document_id,
        &input.source_sha256,
        &input.edges,
    )
    .map_err(|_| {
        ProductReplayWorkerError::new(
            "product_materialization_replay_failed",
            "isolated exact revision replay failed",
        )
    })?;
    let project_sha256 = project_sha256(&project).map_err(|_| {
        ProductReplayWorkerError::new(
            "product_materialization_replay_failed",
            "isolated materialized project identity could not be derived",
        )
    })?;

    let receipt = ProductMaterializationWorkerReceiptV1 {
        protocol_version: PRODUCT_MATERIALIZATION_WORKER_V1.to_owned(),
        document_id: input.document_id.clone(),
        source_sha256: input.source_sha256.clone(),
        source_byte_len: input.source_byte_len,
        baseline_revision_id: input.baseline_revision_id.clone(),
        baseline_cursor: input.baseline_cursor,
        requested_revision_id: input.requested_revision_id.clone(),
        replayed_edges: input.edges.len(),
        project_sha256,
        authoring_root_hash,
        project,
        filesystem_confinement: true,
    };
    validate_materialization_receipt(&receipt, &input)?;

    let mut output = BufWriter::new(output);
    serde_json::to_writer(&mut output, &receipt).map_err(|_| {
        ProductReplayWorkerError::new(
            "product_materialization_output_failed",
            "isolated materialization receipt could not be written",
        )
    })?;
    output.write_all(b"\n").map_err(|_| {
        ProductReplayWorkerError::new(
            "product_materialization_output_failed",
            "isolated materialization receipt newline could not be written",
        )
    })?;
    output.flush().map_err(|_| {
        ProductReplayWorkerError::new(
            "product_materialization_output_failed",
            "isolated materialization receipt could not be flushed",
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

        let temp = PrivateReplayTemp::create(&self.config.temp_root)?;
        let source_path = temp.path.join("source.pub");
        let project_path = temp.path.join("project.json");
        let result_dir = temp.path.join("worker-result");
        for (path, bytes) in [
            (&source_path, source_bytes),
            (&project_path, project_bytes.as_slice()),
        ] {
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
            })?;
        }

        let worker = Command::new(&self.config.isolation_python)
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
            .arg(source_bytes.len().to_string())
            .arg("--project-json")
            .arg(&project_path)
            .arg("--expected-project-sha256")
            .arg(expected_project_sha256)
            .kill_on_drop(true)
            .output();
        let max_wall = self
            .config
            .worker_wall_timeout
            .checked_add(std::time::Duration::from_secs(5))
            .ok_or_else(|| {
                ProductReplayWorkerError::new(
                    "product_replay_config_invalid",
                    "worker deadline overflowed",
                )
            })?;
        let output = timeout(max_wall, worker)
            .await
            .map_err(|_| {
                ProductReplayWorkerError::new(
                    "product_replay_worker_timeout",
                    "isolated worker exceeded parent deadline",
                )
            })?
            .map_err(|_| {
                ProductReplayWorkerError::new(
                    "product_replay_worker_failed",
                    "isolated worker could not be launched",
                )
            })?;
        if !output.status.success() {
            return Err(ProductReplayWorkerError::new(
                "product_replay_worker_failed",
                "isolated worker failed without a usable graph receipt",
            ));
        }
        let receipt_path = result_dir.join("result.json");
        let metadata = async_fs::metadata(&receipt_path).await.map_err(|_| {
            ProductReplayWorkerError::new(
                "product_replay_receipt_missing",
                "isolated graph receipt is unavailable",
            )
        })?;
        if !metadata.is_file()
            || metadata.len() == 0
            || metadata.len() > MAX_GRAPH_BYTES as u64 + 1024 * 1024
        {
            return Err(ProductReplayWorkerError::new(
                "product_replay_receipt_invalid",
                "isolated graph receipt is outside bounded size",
            ));
        }
        let payload = async_fs::read(&receipt_path).await.map_err(|_| {
            ProductReplayWorkerError::new(
                "product_replay_receipt_missing",
                "isolated graph receipt could not be read",
            )
        })?;
        if payload.len() > MAX_GRAPH_BYTES + 1024 * 1024 {
            return Err(ProductReplayWorkerError::new(
                "product_replay_receipt_invalid",
                "isolated graph receipt exceeded size after read",
            ));
        }
        let receipt: ProductReplayWorkerReceiptV1 =
            serde_json::from_slice(&payload).map_err(|_| {
                ProductReplayWorkerError::new(
                    "product_replay_receipt_invalid",
                    "isolated graph receipt is malformed",
                )
            })?;
        validate_product_replay_receipt(
            &receipt,
            document_id,
            source_sha256,
            source_bytes.len() as u64,
            expected_project_sha256,
        )?;
        require_typed_graph(receipt.authoring_graph)
    }
}

#[derive(Clone)]
pub struct IsolatedProductMaterializationProducer {
    config: SourceBaselineProducerConfig,
}

impl IsolatedProductMaterializationProducer {
    pub fn new(config: SourceBaselineProducerConfig) -> Result<Self, ProductReplayWorkerError> {
        config.validate().map_err(|_| {
            ProductReplayWorkerError::new(
                "product_materialization_config_invalid",
                "isolated materialization worker configuration is invalid",
            )
        })?;
        Ok(Self { config })
    }

    pub async fn materialize_exact_project(
        &self,
        document_id: &str,
        source_sha256: &str,
        source_bytes: &[u8],
        baseline_revision_id: &str,
        baseline_cursor: i64,
        requested_revision_id: &str,
        edges: &[RevisionEdge],
    ) -> Result<ProductMaterializationWorkerReceiptV1, ProductReplayWorkerError> {
        require_document_id(document_id)?;
        require_sha256(source_sha256)?;
        if source_bytes.is_empty()
            || source_bytes.len() as u64 > MAX_SOURCE_BYTES
            || sha256_hex(source_bytes) != source_sha256
        {
            return Err(ProductReplayWorkerError::new(
                "product_materialization_source_mismatch",
                "verified source differs from isolated materialization request",
            ));
        }

        let input = ProductMaterializationInputV1 {
            protocol_version: PRODUCT_MATERIALIZATION_WORKER_V1.to_owned(),
            document_id: document_id.to_owned(),
            source_sha256: source_sha256.to_owned(),
            source_byte_len: source_bytes.len() as u64,
            baseline_revision_id: baseline_revision_id.to_owned(),
            baseline_cursor,
            requested_revision_id: requested_revision_id.to_owned(),
            edges: edges.to_vec(),
        };
        validate_materialization_input(
            &input,
            document_id,
            source_sha256,
            source_bytes.len() as u64,
        )?;
        let replay_bytes = serde_json::to_vec(&input).map_err(|_| {
            ProductReplayWorkerError::new(
                "product_materialization_input_invalid",
                "authorized revision chain could not be serialized",
            )
        })?;
        if replay_bytes.len() as u64 > MAX_REPLAY_CHAIN_BYTES {
            return Err(ProductReplayWorkerError::new(
                "product_materialization_input_limit",
                "authorized revision chain exceeds isolated worker input policy",
            ));
        }

        let temp = PrivateReplayTemp::create(&self.config.temp_root)?;
        let source_path = temp.path.join("source.pub");
        let replay_path = temp.path.join("replay.json");
        let result_dir = temp.path.join("worker-result");
        for (path, bytes) in [
            (&source_path, source_bytes),
            (&replay_path, replay_bytes.as_slice()),
        ] {
            let mut file = async_fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)
                .await
                .map_err(|_| {
                    ProductReplayWorkerError::new(
                        "product_materialization_temp_failed",
                        "private worker input could not be created",
                    )
                })?;
            file.write_all(bytes).await.map_err(|_| {
                ProductReplayWorkerError::new(
                    "product_materialization_temp_failed",
                    "private worker input write failed",
                )
            })?;
            file.sync_all().await.map_err(|_| {
                ProductReplayWorkerError::new(
                    "product_materialization_temp_failed",
                    "private worker input sync failed",
                )
            })?;
        }

        let worker = Command::new(&self.config.isolation_python)
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
            .arg("product-isolated-materialize")
            .arg("--document-id")
            .arg(document_id)
            .arg("--expected-sha256")
            .arg(source_sha256)
            .arg("--expected-byte-len")
            .arg(source_bytes.len().to_string())
            .arg("--replay-json")
            .arg(&replay_path)
            .kill_on_drop(true)
            .output();
        let max_wall = self
            .config
            .worker_wall_timeout
            .checked_add(std::time::Duration::from_secs(5))
            .ok_or_else(|| {
                ProductReplayWorkerError::new(
                    "product_materialization_config_invalid",
                    "worker deadline overflowed",
                )
            })?;
        let output = timeout(max_wall, worker)
            .await
            .map_err(|_| {
                ProductReplayWorkerError::new(
                    "product_materialization_worker_timeout",
                    "isolated materialization worker exceeded parent deadline",
                )
            })?
            .map_err(|_| {
                ProductReplayWorkerError::new(
                    "product_materialization_worker_failed",
                    "isolated materialization worker could not be launched",
                )
            })?;
        if !output.status.success() {
            return Err(ProductReplayWorkerError::new(
                "product_materialization_worker_failed",
                "isolated materialization worker failed without a usable receipt",
            ));
        }

        let receipt_path = result_dir.join("result.json");
        let metadata = async_fs::metadata(&receipt_path).await.map_err(|_| {
            ProductReplayWorkerError::new(
                "product_materialization_receipt_missing",
                "isolated materialization receipt is unavailable",
            )
        })?;
        if !metadata.is_file()
            || metadata.len() == 0
            || metadata.len() > MAX_MATERIALIZATION_RECEIPT_BYTES
        {
            return Err(ProductReplayWorkerError::new(
                "product_materialization_receipt_invalid",
                "isolated materialization receipt is outside bounded size",
            ));
        }
        let payload = async_fs::read(&receipt_path).await.map_err(|_| {
            ProductReplayWorkerError::new(
                "product_materialization_receipt_missing",
                "isolated materialization receipt could not be read",
            )
        })?;
        if payload.len() as u64 > MAX_MATERIALIZATION_RECEIPT_BYTES {
            return Err(ProductReplayWorkerError::new(
                "product_materialization_receipt_invalid",
                "isolated materialization receipt exceeded size after read",
            ));
        }
        let receipt: ProductMaterializationWorkerReceiptV1 =
            serde_json::from_slice(&payload).map_err(|_| {
                ProductReplayWorkerError::new(
                    "product_materialization_receipt_invalid",
                    "isolated materialization receipt is malformed",
                )
            })?;
        validate_materialization_receipt(&receipt, &input)?;
        Ok(receipt)
    }
}

pub struct IsolatedExactRevisionMaterializer {
    source_authority: Arc<dyn DocumentSourceAuthority>,
    source_loader: Arc<dyn ExactSourceLoader>,
    revision_store: SqliteRevisionStore,
    producer: IsolatedProductMaterializationProducer,
}

impl IsolatedExactRevisionMaterializer {
    pub fn new(
        source_authority: Arc<dyn DocumentSourceAuthority>,
        source_loader: Arc<dyn ExactSourceLoader>,
        revision_store: SqliteRevisionStore,
        config: SourceBaselineProducerConfig,
    ) -> Result<Self, ProductReplayWorkerError> {
        Ok(Self {
            source_authority,
            source_loader,
            revision_store,
            producer: IsolatedProductMaterializationProducer::new(config)?,
        })
    }
}

fn require_materializer_identifier(
    value: &str,
    field: &'static str,
) -> Result<(), RevisionMaterializerError> {
    if value.is_empty()
        || value.len() > 160
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'-'))
    {
        return Err(RevisionMaterializerError::new(
            "invalid_identifier",
            format!("{field} is not a bounded opaque identifier"),
        ));
    }
    Ok(())
}

#[async_trait::async_trait]
impl ExactRevisionMaterializerPort for IsolatedExactRevisionMaterializer {
    async fn materialize_state(
        &self,
        tenant_id: &str,
        document_id: &str,
        requested_revision_id: &str,
    ) -> Result<ExactRevisionMaterializedState, RevisionMaterializerError> {
        require_materializer_identifier(tenant_id, "tenant_id")?;
        require_materializer_identifier(document_id, "document_id")?;
        require_materializer_identifier(requested_revision_id, "requested_revision_id")?;

        let source = self
            .source_authority
            .resolve_document_source(tenant_id, document_id)
            .await?;
        validate_authorized_source(&source, tenant_id, document_id)?;

        let source_bytes = self.source_loader.load_exact_source(&source).await?;
        if source_bytes.len() as u64 != source.byte_len {
            return Err(RevisionMaterializerError::new(
                "source_length_mismatch",
                "loaded immutable source does not match the authorized byte length",
            ));
        }
        if sha256_hex(&source_bytes) != source.source_sha256 {
            return Err(RevisionMaterializerError::new(
                "source_hash_mismatch",
                "loaded immutable source does not match the authorized SHA-256",
            ));
        }

        let edges = self
            .revision_store
            .load_chain_to_revision(
                document_id,
                &source.baseline_revision_id,
                source.baseline_cursor,
                requested_revision_id,
            )
            .await
            .map_err(|error| RevisionMaterializerError::new(error.code, error.message))?;

        let isolated = self
            .producer
            .materialize_exact_project(
                document_id,
                &source.source_sha256,
                &source_bytes,
                &source.baseline_revision_id,
                source.baseline_cursor,
                requested_revision_id,
                &edges,
            )
            .await
            .map_err(|error| RevisionMaterializerError::new(error.code, error.message))?;

        let identity = self
            .revision_store
            .require_revision_identity(document_id, requested_revision_id)
            .await
            .map_err(|error| RevisionMaterializerError::new(error.code, error.message))?;

        let receipt = ExactRevisionMaterializationReceipt {
            schema_version: MATERIALIZATION_RECEIPT_SCHEMA_V1.to_owned(),
            tenant_id: tenant_id.to_owned(),
            document_id: document_id.to_owned(),
            source_binding_id: source.binding_id,
            source_sha256: source.source_sha256,
            baseline_revision_id: source.baseline_revision_id,
            baseline_cursor: source.baseline_cursor,
            requested_revision_id: requested_revision_id.to_owned(),
            canonical_revision_schema_version: identity.canonical_schema_version,
            canonical_authoring_revision_id: identity.canonical_revision_id,
            replayed_edges: isolated.replayed_edges,
            project_sha256: isolated.project_sha256,
            authoring_root_hash: isolated.authoring_root_hash,
            project: isolated.project,
        };
        Ok(ExactRevisionMaterializedState {
            receipt,
            source_bytes,
        })
    }
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
