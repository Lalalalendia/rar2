use std::collections::BTreeMap;

use pub_editor::{
    EDITOR_PROJECT_VERSION_CURRENT, EditOperation, EditorProjectAsset, EditorProjectError,
    EditorSession, Sha256Digest,
};
use pub_model::{Document, DocumentId, NodeId, ResolvedGraph, SourceDescriptor};
use pub_reader::PubResolvedGraph;

fn canonical_id<T: serde::de::DeserializeOwned>(value: &str) -> T {
    serde_json::from_str(&format!("\"{value}\"")).expect("canonical typed id")
}

fn graph() -> PubResolvedGraph {
    let source_hash = Sha256Digest::from_bytes([0x44; 32]);
    ResolvedGraph {
        cdm_version: "0.1".into(),
        resolver_version: "asset-reachability-test".into(),
        source: SourceDescriptor {
            format: "pub".into(),
            format_version: Some("0x2c".into()),
            adapter_version: "pub-rs/test".into(),
            source_hash,
        },
        document: Document {
            id: canonical_id::<DocumentId>("33000000-0000-4000-8000-000000000001"),
            format_origin: "pub".into(),
            source_hash,
            pages: Vec::new(),
            resources: Vec::new(),
            styles: Vec::new(),
        },
        pages: BTreeMap::new(),
        nodes: BTreeMap::new(),
        stories: BTreeMap::new(),
        paragraphs: BTreeMap::new(),
        text_runs: BTreeMap::new(),
        resources: BTreeMap::new(),
        styles: BTreeMap::new(),
        extensions: BTreeMap::new(),
    }
}

fn png_bytes(tag: u8) -> Vec<u8> {
    let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
    bytes.push(tag);
    bytes
}

#[test]
fn unused_runtime_import_is_not_durable_project_truth() {
    let mut session = EditorSession::new(graph()).expect("session");
    let sha = session
        .import_replacement_asset("image/png", png_bytes(0x11))
        .expect("bounded PNG import");

    assert_eq!(session.replacement_assets().count(), 1);
    assert!(
        session
            .replacement_assets()
            .any(|asset| asset.sha256 == sha)
    );

    let project = session.try_project().expect("project materialization");
    assert_eq!(project.schema_version, EDITOR_PROJECT_VERSION_CURRENT);
    assert!(
        project.assets.is_empty(),
        "runtime cache membership alone must not become durable project metadata"
    );
}

#[test]
fn current_project_rejects_extra_cache_shaped_asset_metadata() {
    let base = graph();
    let mut producer = EditorSession::new(base.clone()).expect("producer session");
    let bytes = png_bytes(0x22);
    let sha = producer
        .import_replacement_asset("image/png", bytes.clone())
        .expect("bounded PNG import");

    let mut project = producer.try_project().expect("project");
    assert!(project.operations.is_empty());
    project.assets.push(EditorProjectAsset {
        sha256: sha,
        mime: "image/png".to_owned(),
        byte_len: u64::try_from(bytes.len()).expect("bounded fixture"),
    });

    let mut replay = EditorSession::new(base).expect("replay session");
    let asset_bytes = BTreeMap::from([(sha, bytes)]);
    assert!(matches!(
        replay.apply_project_with_assets(&project, &asset_bytes),
        Err(EditorProjectError::AssetReachabilityMismatch {
            expected,
            found
        }) if expected.is_empty() && found == vec![sha]
    ));
}

#[test]
fn replace_image_retains_before_and_after_asset_identities() {
    let a = Sha256Digest::from_bytes([0x11; 32]);
    let b = Sha256Digest::from_bytes([0x22; 32]);
    let operation = EditOperation::ReplaceImage {
        node_id: canonical_id::<NodeId>("22000000-0000-4000-8000-000000000001"),
        before_asset: Some(a),
        after_asset: b,
    };

    assert_eq!(operation.durable_editor_asset_refs_v1(), vec![a, b]);
}
