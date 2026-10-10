#![cfg(all(target_os = "linux", target_arch = "x86_64"))]

// Real subprocess admission: verify the same pinned PUB authoring graph is
// produced by the seccomp-confined worker, not a mock protocol object.

use std::{path::PathBuf, str::FromStr, time::Duration};

use chaptera_server::{
    product_replay_worker::{
        IsolatedProductMaterializationProducer, IsolatedProductReplayProducer,
    },
    revision_materializer::{EditorReplayEngine, PubEditorReplayEngine, project_sha256},
    source_baseline::SourceBaselineProducerConfig,
    sqlite_store::RevisionEdge,
};
use pub_editor::{Sha256Digest, open_mature_0x2c_editor};
use sha2::{Digest, Sha256};

fn sample3_pub() -> Vec<u8> {
    let source = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../vendor/producer-a/crates/pub-quill/tests/fixtures/Sample3.pub.b64"
    ));
    let digits = source
        .bytes()
        .filter(|b| !b.is_ascii_whitespace())
        .collect::<Vec<_>>();
    assert_eq!(digits.len() % 4, 0);
    let mut bytes = Vec::with_capacity(digits.len() / 4 * 3);
    for c in digits.chunks_exact(4) {
        let a = b64(c[0]);
        let b = b64(c[1]);
        let third = if c[2] == b'=' { 0 } else { b64(c[2]) };
        let fourth = if c[3] == b'=' { 0 } else { b64(c[3]) };
        bytes.push((a << 2) | (b >> 4));
        if c[2] != b'=' {
            bytes.push((b << 4) | (third >> 2));
        }
        if c[3] != b'=' {
            bytes.push((third << 6) | fourth);
        }
    }
    bytes
}

fn b64(value: u8) -> u8 {
    match value {
        b'A'..=b'Z' => value - b'A',
        b'a'..=b'z' => value - b'a' + 26,
        b'0'..=b'9' => value - b'0' + 52,
        b'+' => 62,
        b'/' => 63,
        _ => panic!("invalid pinned fixture base64"),
    }
}

#[tokio::test]
async fn pinned_sample3_exact_project_replays_in_real_seccomp_worker() {
    let source_bytes = sample3_pub();
    let source_sha256 = format!("{:x}", Sha256::digest(&source_bytes));
    let editor = PubEditorReplayEngine;
    let project = editor
        .baseline_project(&source_bytes, &source_sha256)
        .unwrap();
    let project_hash = project_sha256(&project).unwrap();

    let harness = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tools/migration_pdf_worker_isolation.py");
    let producer = IsolatedProductReplayProducer::new(SourceBaselineProducerConfig {
        isolation_python: PathBuf::from("python3"),
        isolation_harness: harness,
        worker_binary: PathBuf::from(env!("CARGO_BIN_EXE_chaptera")),
        worker_wall_timeout: Duration::from_secs(45),
        worker_address_space_mb: 2048,
        worker_cpu_seconds: 40,
        worker_open_files: 64,
        worker_output_file_mb: 64,
        temp_root: std::env::temp_dir(),
    })
    .unwrap();

    let actual = producer
        .project_authoring_graph(
            "document-sample3-isolated",
            &source_sha256,
            &source_bytes,
            &project,
            &project_hash,
        )
        .await
        .unwrap();
    let mut expected = open_mature_0x2c_editor(
        &source_bytes,
        Sha256Digest::from_str(&source_sha256).unwrap(),
    )
    .unwrap();
    expected.apply_project(&project).unwrap();
    let expected_graph = serde_json::to_value(expected.graph()).unwrap();
    assert_eq!(serde_json::to_value(&actual).unwrap(), expected_graph);

    // A forged project identity must be rejected before launching a worker.
    let denied = producer
        .project_authoring_graph(
            "document-sample3-isolated",
            &source_sha256,
            &source_bytes,
            &project,
            &"0".repeat(64),
        )
        .await
        .unwrap_err();
    assert_eq!(denied.code, "product_replay_project_hash_mismatch");
}

#[tokio::test]
async fn pinned_sample3_baseline_materializes_in_real_seccomp_worker() {
    let source_bytes = sample3_pub();
    let source_sha256 = format!("{:x}", Sha256::digest(&source_bytes));
    let editor = PubEditorReplayEngine;
    let expected = editor
        .baseline_project(&source_bytes, &source_sha256)
        .unwrap();

    let harness = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tools/migration_pdf_worker_isolation.py");
    let producer = IsolatedProductMaterializationProducer::new(SourceBaselineProducerConfig {
        isolation_python: PathBuf::from("python3"),
        isolation_harness: harness,
        worker_binary: PathBuf::from(env!("CARGO_BIN_EXE_chaptera")),
        worker_wall_timeout: Duration::from_secs(45),
        worker_address_space_mb: 2048,
        worker_cpu_seconds: 40,
        worker_open_files: 64,
        worker_output_file_mb: 64,
        temp_root: std::env::temp_dir(),
    })
    .unwrap();

    let receipt = producer
        .materialize_exact_project(
            "document-sample3-materialized",
            &source_sha256,
            &source_bytes,
            "revision-baseline",
            0,
            "revision-baseline",
            &[],
        )
        .await
        .unwrap();
    assert_eq!(receipt.project, expected);
    assert_eq!(receipt.project_sha256, project_sha256(&expected).unwrap());
    assert_eq!(receipt.replayed_edges, 0);
    assert_eq!(receipt.authoring_root_hash, None);
    assert!(receipt.filesystem_confinement);

    let malformed = RevisionEdge {
        document_id: "document-sample3-materialized".into(),
        parent_revision: "wrong-parent".into(),
        parent_cursor: 0,
        operation_id: "op-1".into(),
        request_hash: "c".repeat(64),
        canonical_event: Vec::new(),
        child_revision: "revision-next".into(),
        child_cursor: 1,
        resulting_state_hash: "d".repeat(64),
        authoring_root_hash: None,
        semantic_schema_version: 1,
        committed_at_ms: 1,
    };
    let denied = producer
        .materialize_exact_project(
            "document-sample3-materialized",
            &source_sha256,
            &source_bytes,
            "revision-baseline",
            0,
            "revision-next",
            &[malformed],
        )
        .await
        .unwrap_err();
    assert_eq!(denied.code, "product_materialization_chain_invalid");
}
