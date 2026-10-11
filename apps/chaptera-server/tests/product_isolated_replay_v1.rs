#![cfg(all(target_os = "linux", target_arch = "x86_64"))]

// Real subprocess admission: verify the same pinned PUB authoring graph is
// produced by the seccomp-confined worker, not a mock protocol object.

use std::{path::PathBuf, str::FromStr, time::Duration};

use chaptera_server::{
    product_replay_worker::{
        IsolatedMoveNodeIntentV1, IsolatedProductReplayProducer, IsolatedReaderSceneIntentV1,
    },
    revision_materializer::{
        EditorReplayEngine, PubEditorReplayEngine, cloud_revision_project, project_sha256,
    },
    source_baseline::SourceBaselineProducerConfig,
};
use pub_editor::{LengthEmu, Sha256Digest, open_mature_0x2c_editor};
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

    // The baseline itself must be derived by the confined process.
    // Unlike direct graph projection, this tests that the host never needs
    // to parse an untrusted PUB just to create the first EditorProject.
    let isolated_baseline = producer
        .baseline_project("document-sample3-isolated", &source_sha256, &source_bytes)
        .await
        .unwrap();
    assert_eq!(isolated_baseline, project);

    let alien_baseline = producer
        .baseline_project("document-sample3-isolated", &"0".repeat(64), &source_bytes)
        .await
        .unwrap_err();
    assert_eq!(alien_baseline.code, "product_replay_source_mismatch");

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

    // This is the true source-neutral rich scene from the same Linux worker,
    // not a mocked ReaderScene or an in-process HTTP parser.
    let baseline_scene = producer
        .project_reader_scene(
            "document-sample3-isolated",
            &source_sha256,
            &source_bytes,
            &project,
            &project_hash,
            &IsolatedReaderSceneIntentV1 {
                revision_id: "revision-sample3-baseline".into(),
                baseline_revision_id: "revision-sample3-baseline".into(),
            },
        )
        .await
        .unwrap();
    assert_eq!(
        baseline_scene["protocol_version"],
        "chaptera.reader-scene.v1"
    );
    assert_eq!(baseline_scene["document_id"], "document-sample3-isolated");
    assert_eq!(baseline_scene["source_hash"], source_sha256);
    assert_eq!(baseline_scene["revision_id"], "revision-sample3-baseline");
    assert_eq!(
        baseline_scene["scene_authority"],
        "server_viewer_projection"
    );
    assert!(
        baseline_scene["pages"]
            .as_array()
            .is_some_and(|pages| !pages.is_empty())
    );
    assert!(
        baseline_scene["nodes"]
            .as_array()
            .is_some_and(|nodes| !nodes.is_empty())
    );
    assert!(baseline_scene.get("project").is_none());
    assert!(baseline_scene.get("authoring_graph").is_none());

    // A real canonical moved-object revision must replay identically inside
    // the sandbox, not just the unedited import baseline.
    let (node_id, x_emu, y_emu) = expected
        .graph()
        .nodes
        .iter()
        .find_map(|(node_id, node)| {
            // Only scene-representable objects are admitted by the rich
            // Reader path; this mirrors real Product API acceptance.
            let node_json = serde_json::to_value(node_id).ok()?;
            if !baseline_scene["nodes"]
                .as_array()?
                .iter()
                .any(|scene_node| scene_node["node_id"] == node_json)
            {
                return None;
            }
            let x = node.header.bounds.x.get().checked_add(9_525)?;
            let y = node.header.bounds.y.get().checked_add(9_525)?;
            expected
                .can_move_node_to(*node_id, LengthEmu::new(x), LengthEmu::new(y))
                .ok()
                .map(|_| (*node_id, x, y))
        })
        .expect("Sample3 must have a movable object");
    let expected_operation = expected
        .move_node_to(node_id, LengthEmu::new(x_emu), LengthEmu::new(y_emu))
        .unwrap();
    let edited_project = cloud_revision_project(&expected.project());
    let intent = IsolatedMoveNodeIntentV1 {
        node_id: serde_json::to_value(node_id)
            .unwrap()
            .as_str()
            .unwrap()
            .to_owned(),
        x_emu,
        y_emu,
    };
    let (isolated_operation, isolated_project) = producer
        .move_node_to(
            "document-sample3-isolated",
            &source_sha256,
            &source_bytes,
            &project,
            &project_hash,
            &intent,
        )
        .await
        .unwrap();
    assert_eq!(isolated_operation, expected_operation);
    assert_eq!(isolated_project, edited_project);

    // This is a host-side source/revision fence, before any subprocess.
    let mismatched = producer
        .move_node_to(
            "document-sample3-isolated",
            &source_sha256,
            &source_bytes,
            &project,
            &"0".repeat(64),
            &intent,
        )
        .await
        .unwrap_err();
    assert_eq!(mismatched.code, "product_replay_project_hash_mismatch");
    let edited_sha = project_sha256(&edited_project).unwrap();
    let isolated_edited = producer
        .project_authoring_graph(
            "document-sample3-isolated",
            &source_sha256,
            &source_bytes,
            &edited_project,
            &edited_sha,
        )
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(&isolated_edited).unwrap(),
        serde_json::to_value(expected.graph()).unwrap()
    );

    // The edited revision must preserve the full scene structure while
    // faithfully reflecting the exact persisted geometry at the new head.
    let edited_scene = producer
        .project_reader_scene(
            "document-sample3-isolated",
            &source_sha256,
            &source_bytes,
            &edited_project,
            &edited_sha,
            &IsolatedReaderSceneIntentV1 {
                revision_id: "revision-sample3-moved".into(),
                baseline_revision_id: "revision-sample3-baseline".into(),
            },
        )
        .await
        .unwrap();
    assert_eq!(edited_scene["revision_id"], "revision-sample3-moved");
    let moved_scene_node = edited_scene["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|scene_node| scene_node["node_id"] == intent.node_id)
        .expect("rich Reader scene must preserve moved authoring node");
    assert_eq!(moved_scene_node["bounds"]["x"], x_emu);
    assert_eq!(moved_scene_node["bounds"]["y"], y_emu);
    assert!(edited_scene.get("authoring_graph").is_none());
    assert!(edited_scene.get("project").is_none());

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
