#![cfg(all(target_os = "linux", target_arch = "x86_64"))]

// Exact external Publisher corpus acceptance for the Product Reader SAFE
// preview boundary. This is intentionally ignored outside its dedicated
// GitHub-hosted, SHA-pinned physical runner. No PUB, decoded pixels, story
// text, or browser-ready scene is committed or uploaded.

use std::{env, fs, path::PathBuf, time::Duration};

use chaptera_server::{
    product_replay_worker::{IsolatedProductReplayProducer, IsolatedReaderSceneIntentV1},
    revision_materializer::project_sha256,
    source_baseline::SourceBaselineProducerConfig,
};
use serde_json::json;
use sha2::{Digest, Sha256};

const PUB_SHA256_022: &str = "7079ad60fd8109796b84286d294cb490051a1517d8e52ac61e7c416dbd04428c";
const PUB_BYTES_022: usize = 5_012_480;

#[tokio::test]
#[ignore = "requires SHA-pinned 022 external real PUB and Linux seccomp worker"]
async fn exact_022_preview_reaches_product_host_with_source_neutral_pixels() {
    let fixture_path = env::var_os("CHAPTERA_PREVIEW_PROBE_PUB")
        .map(PathBuf::from)
        .expect("CHAPTERA_PREVIEW_PROBE_PUB must be set by the exact fixture acquisition");
    let scene_out = env::var_os("CHAPTERA_PREVIEW_PROBE_SCENE_OUT")
        .map(PathBuf::from)
        .expect("CHAPTERA_PREVIEW_PROBE_SCENE_OUT must be an absolute private runner path");
    let summary_out = env::var_os("CHAPTERA_PREVIEW_PROBE_SUMMARY_OUT")
        .map(PathBuf::from)
        .expect("CHAPTERA_PREVIEW_PROBE_SUMMARY_OUT must be an absolute private runner path");
    assert!(fixture_path.is_absolute());
    assert!(scene_out.is_absolute());
    assert!(summary_out.is_absolute());

    let pub_bytes = fs::read(&fixture_path).expect("must read pinned source PUB");
    assert_eq!(pub_bytes.len(), PUB_BYTES_022);
    assert_eq!(format!("{:x}", Sha256::digest(&pub_bytes)), PUB_SHA256_022);

    let harness = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tools/migration_pdf_worker_isolation.py");
    let producer = IsolatedProductReplayProducer::new(SourceBaselineProducerConfig {
        isolation_python: PathBuf::from("python3"),
        isolation_harness: harness,
        worker_binary: PathBuf::from(env!("CARGO_BIN_EXE_chaptera")),
        worker_wall_timeout: Duration::from_secs(90),
        worker_address_space_mb: 2048,
        worker_cpu_seconds: 75,
        worker_open_files: 64,
        worker_output_file_mb: 64,
        temp_root: env::temp_dir(),
    })
    .expect("pinned isolated producer must configure");

    const DOCUMENT_ID: &str = "document-022-safe-preview-evidence";
    const REVISION_ID: &str = "revision-022-pinned-baseline";
    let project = producer
        .baseline_project(DOCUMENT_ID, PUB_SHA256_022, &pub_bytes)
        .await
        .expect("022 baseline must derive in real seccomp worker");
    let project_hash = project_sha256(&project).expect("canonical baseline identity");
    let scene = producer
        .project_reader_scene(
            DOCUMENT_ID,
            PUB_SHA256_022,
            &pub_bytes,
            &project,
            &project_hash,
            &IsolatedReaderSceneIntentV1 {
                revision_id: REVISION_ID.into(),
                baseline_revision_id: REVISION_ID.into(),
            },
        )
        .await
        .expect("022 Reader Scene must reach host through isolated worker");

    assert_eq!(scene["protocol_version"], "chaptera.reader-scene.v1");
    assert_eq!(scene["source_hash"], PUB_SHA256_022);
    assert_eq!(scene["document_id"], DOCUMENT_ID);
    assert_eq!(scene["revision_id"], REVISION_ID);
    assert_eq!(scene["scene_authority"], "server_viewer_projection");
    assert_eq!(scene["pages"].as_array().unwrap().len(), 1);
    assert!(scene.get("project").is_none());
    assert!(scene.get("authoring_graph").is_none());

    let resources = scene["resources"]
        .as_array()
        .expect("022 image resources must exist");
    let picture_nodes = scene["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|node| node["kind"] == "picture_frame")
        .count();
    let mut admitted = 0_usize;
    let mut descriptor_only = 0_usize;
    let mut max_url_len = 0_usize;
    let mut admitted_ids = std::collections::HashSet::new();

    for resource in resources {
        let resource_id = resource["resource_id"]
            .as_str()
            .expect("resource ID string");
        match resource["availability"].as_str() {
            Some("inline_data_url") => {
                admitted += 1;
                let url = resource["inline_data_url"]
                    .as_str()
                    .expect("host-owned URL");
                assert!(
                    url.starts_with("data:image/png;base64,")
                        || url.starts_with("data:image/jpeg;base64,"),
                    "host must emit only freshly derived PNG/JPEG"
                );
                assert!(url.len() <= 7 * 1024 * 1024);
                assert!(
                    admitted_ids.insert(resource_id),
                    "no duplicate resource IDs"
                );
                max_url_len = max_url_len.max(url.len());
            }
            Some("descriptor_only") => {
                descriptor_only += 1;
                assert!(resource.get("inline_data_url").is_none());
            }
            _ => panic!("unexpected browser availability"),
        }
    }
    assert!(
        picture_nodes > 0,
        "022 must expose a real image-bearing scene node"
    );
    assert!(
        admitted > 0,
        "022 source-exact JPEG must be restored through host derivative"
    );
    assert!(
        scene["fidelity"]["reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason == "image_preview_partial_fidelity"),
        "safe re-encoding must retain partial color/fidelity disposition"
    );

    let report = json!({
        "schema": "chaptera.product-reader-safe-preview-exact-022.v1",
        "source_sha256": PUB_SHA256_022,
        "revision_id": REVISION_ID,
        "page_count": scene["pages"].as_array().unwrap().len(),
        "picture_frame_count": picture_nodes,
        "resource_count": resources.len(),
        "host_admitted_preview_count": admitted,
        "descriptor_only_count": descriptor_only,
        "max_host_data_url_len": max_url_len,
        "fidelity": "partial",
        "isolation": "real_seccomp",
        "source_bytes_forwarded": false,
    });
    fs::create_dir_all(scene_out.parent().expect("scene parent")).unwrap();
    fs::create_dir_all(summary_out.parent().expect("summary parent")).unwrap();
    fs::write(&scene_out, serde_json::to_vec(&scene).unwrap()).unwrap();
    fs::write(&summary_out, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    println!("SAFE_PREVIEW_022_SOURCE_NEUTRAL_RECEIPT {}", report);
}
