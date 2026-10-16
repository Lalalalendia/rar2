#![cfg(all(target_os = "linux", target_arch = "x86_64"))]

// Exact source-bound Guest producer boundary: genuine public PUB and a real
// seccomp-confined chaptera worker. This is NOT simulated HTTP or Chromium.
// An original Publisher source never appears in retained workflow artifacts.
use std::{env, fs, path::PathBuf, str::FromStr, time::Duration};

use chaptera_server::{
    product_replay_worker::{
        IsolatedMoveNodesIntentV1, IsolatedProductReplayProducer, IsolatedReaderSceneIntentV1,
    },
    revision_materializer::{cloud_revision_project, project_sha256},
    source_baseline::SourceBaselineProducerConfig,
};
use pub_editor::{
    AuthoredEntityProvenanceV1, AuthoredShapePaintV1, AuthoredSolidFillV1,
    AuthoredSolidStrokeV1, EditOperation, LengthEmu, NodeId, RectEmu, Sha256Digest, Srgb8V1,
    open_mature_0x2c_editor,
};
use sha2::{Digest, Sha256};

const PINNED: &str = "6a825ba26ba35d6e885acdc62e859591ed37cb0ff7480b554b9cb362b644dfcf";
const A: &str = "019a2d80-1112-7001-8001-aabbccddee01";
const B: &str = "019a2d80-1112-7001-8001-aabbccddee02";
const C: &str = "019a2d80-1112-7001-8001-aabbccddee03";

fn paint() -> AuthoredShapePaintV1 {
    AuthoredShapePaintV1 {
        fill: AuthoredSolidFillV1 {
            visible: true,
            color: Srgb8V1 { r: 18, g: 52, b: 86 },
        },
        stroke: AuthoredSolidStrokeV1 {
            visible: true,
            color: Srgb8V1 { r: 170, g: 187, b: 204 },
            width_emu: 12_700,
        },
        provenance: AuthoredEntityProvenanceV1::AuthorCreated,
    }
}
fn rect(x: i64) -> RectEmu {
    RectEmu::new(
        LengthEmu::new(x), LengthEmu::new(100_000),
        LengthEmu::new(500_000), LengthEmu::new(250_000),
    )
}
fn id(value: &str) -> NodeId {
    serde_json::from_value(serde_json::Value::String(value.into())).expect("canonical NodeId")
}

#[tokio::test]
async fn original_pub_authored_distribute_survives_seccomp_worker_and_reader_scene() {
    let path = env::var("PR2790_PUBLIC_REAL_PUB")
        .expect("workflow must supply SHA-pinned original public PUB");
    let bytes = fs::read(path).expect("read bounded public original PUB");
    assert_eq!(bytes.len(), 291_840, "original public PUB size");
    let actual_hash = format!("{:x}", Sha256::digest(&bytes));
    assert_eq!(actual_hash, PINNED, "original public PUB hash changed");
    let mut editor = open_mature_0x2c_editor(
        &bytes, Sha256Digest::from_str(PINNED).expect("public digest"),
    ).expect("real Publisher parser");
    let original_graph = editor.graph().clone();
    let page_id = *original_graph.document.pages.first().expect("original first page");
    for (id_string, x) in [(A, 100_000_i64), (B, 300_000), (C, 900_000)] {
        editor.create_shape(id(id_string), page_id, rect(x), paint())
            .expect("create canonical direct Rectangle on source");
    }
    assert_eq!(editor.graph(), &original_graph);
    let base = cloud_revision_project(&editor.project());
    assert_eq!(base.operations.len(), 3);
    let base_sha = project_sha256(&base).expect("canonical base project SHA");
    let producer = IsolatedProductReplayProducer::new(SourceBaselineProducerConfig {
        isolation_python: PathBuf::from("python3"),
        isolation_harness: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tools/migration_pdf_worker_isolation.py"),
        worker_binary: PathBuf::from(env!("CARGO_BIN_EXE_chaptera")),
        worker_wall_timeout: Duration::from_secs(75),
        worker_address_space_mb: 2048,
        worker_cpu_seconds: 65,
        worker_open_files: 64,
        worker_output_file_mb: 32,
        temp_root: env::temp_dir(),
    }).expect("source-bound isolated producer");

    let document_id = "pr2790-pinned-public-real-pub";
    let selection = IsolatedMoveNodesIntentV1 {
        page_id: page_id.as_canonical().to_string(),
        selected_node_ids: [C, A, B].map(str::to_owned).to_vec(),
        mode: "distribute_horizontal".into(),
    };
    let (operation, after) = producer.move_nodes_to(
        document_id, PINNED, &bytes, &base, &base_sha, &selection,
    ).await.expect("real seccomp worker and independent host verifier must admit authored MoveNodes");
    let EditOperation::MoveNodes { page_id: actual_page, entries } = operation else {
        panic!("isolated worker must return canonical MoveNodes");
    };
    assert_eq!(actual_page, page_id);
    assert_eq!(entries.len(), 1, "change-only batch: exactly middle Rectangle");
    assert_eq!(entries[0].node_id, id(B));
    assert_eq!(entries[0].before, rect(300_000));
    assert_eq!(entries[0].after, rect(500_000));
    assert_eq!(after.operations.len(), 4);
    assert_eq!(after.operations[..3], base.operations[..]);
    assert_eq!(project_sha256(&after).unwrap().len(), 64);

    let expected_after_sha = project_sha256(&after).expect("changed project SHA");
    let scene = producer.project_reader_scene(
        document_id, PINNED, &bytes, &after, &expected_after_sha,
        &IsolatedReaderSceneIntentV1 {
            revision_id: format!("sha256:{}", "1".repeat(64)),
            baseline_revision_id: format!("sha256:{}", "0".repeat(64)),
        },
    ).await.expect("real isolated ReaderScene after MoveNodes");
    assert_eq!(scene["protocol_version"], "chaptera.reader-scene.v1");
    assert_eq!(scene["source_hash"], PINNED);
    let nodes = scene["nodes"].as_array().expect("Reader Scene node list");
    for (id_string, x) in [(A, 100_000), (B, 500_000), (C, 900_000)] {
        let matches = nodes.iter().filter(|node| node["node_id"] == id_string).collect::<Vec<_>>();
        assert_eq!(matches.len(), 1, "each moved/current authored Rectangle appears once");
        let bounds = &matches[0]["bounds"];
        assert_eq!(bounds["x"], x);
        assert_eq!(bounds["y"], 100_000);
        assert_eq!(bounds["width"], 500_000);
        assert_eq!(bounds["height"], 250_000);
    }
    // A stale base hash cannot trigger a separate mutation or grant.
    let denied = producer.move_nodes_to(
        document_id, PINNED, &bytes, &base, &"0".repeat(64), &selection,
    ).await.expect_err("mismatched canonical source-bound base project hash must fail");
    assert_eq!(denied.code, "product_replay_project_hash_mismatch");

    // A duplicate submitted identity is rejected before entering the worker.
    let mut duplicate = selection;
    duplicate.selected_node_ids = vec![A.into(), A.into(), B.into()];
    let denied = producer.move_nodes_to(
        document_id, PINNED, &bytes, &base, &base_sha, &duplicate,
    ).await.expect_err("duplicate selection must fail closed");
    assert_eq!(denied.code, "product_move_nodes_invalid");
    assert_eq!(format!("{:x}", Sha256::digest(&bytes)), PINNED);
}
