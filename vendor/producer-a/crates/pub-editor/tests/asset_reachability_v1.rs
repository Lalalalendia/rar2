use std::collections::BTreeMap;

use pub_editor::{
    EDITOR_PROJECT_VERSION_V0_12, EditOperation, EditorProjectAsset, EditorProjectError,
    EditorSession, LengthEmu, RectEmu, Sha256Digest,
};
use pub_model::{
    Affine2D, Document, DocumentId, Node, NodeHeader, NodeId, NodeKind, Page, PageId,
    ResolvedGraph, Size2D, SourceDescriptor,
};
use pub_reader::{PubExplicitShapePaintSource, PubResolvedGraph, PubResolvedNodePayload};

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

fn rect(x: i64, y: i64, width: i64, height: i64) -> RectEmu {
    RectEmu::new(
        LengthEmu::new(x),
        LengthEmu::new(y),
        LengthEmu::new(width),
        LengthEmu::new(height),
    )
}

fn image_graph() -> (PubResolvedGraph, NodeId) {
    let mut graph = graph();
    let page_id: PageId = canonical_id("11000000-0000-4000-8000-000000000001");
    let node_id: NodeId = canonical_id("22000000-0000-4000-8000-000000000001");

    graph.document.pages.push(page_id);
    graph.pages.insert(
        page_id,
        Page {
            id: page_id,
            size: Size2D::new(LengthEmu::new(5_000_000), LengthEmu::new(5_000_000)),
            bleed: None,
            margins: None,
            children: vec![node_id],
            extensions: Vec::new(),
        },
    );
    graph.nodes.insert(
        node_id,
        Node {
            kind: NodeKind::Shape,
            header: NodeHeader {
                id: node_id,
                parent_id: page_id.into_canonical(),
                bounds: rect(100_000, 200_000, 300_000, 400_000),
                transform: Affine2D::identity(),
                source_refs: Vec::new(),
                extensions: Vec::new(),
            },
            payload: PubResolvedNodePayload {
                contents_seq_num: 1,
                officeart_shape_type: Some(75),
                officeart_spid: Some(1),
                image_slot: Some(1),
                legacy_ole: None,
                explicit_image_crop: None,
                explicit_image_cardinal_rotation_degrees: None,
                explicit_paint: PubExplicitShapePaintSource::default(),
                effective_paint: None,
                story_frame: None,
                text_frame_inset: None,
                table_story: None,
                table: None,
            },
        },
    );

    (graph, node_id)
}

#[test]
fn current_image_resources_follow_public_replace_image_overlay() {
    let (base, node_id) = image_graph();
    let mut session = EditorSession::new(base).expect("session");
    let bytes = png_bytes(0x33);
    let sha = session
        .import_replacement_asset("image/png", bytes.clone())
        .expect("bounded PNG import");

    session
        .replace_image(node_id, sha)
        .expect("public ReplaceImage overlay");

    let resources = session
        .current_image_resources_v1()
        .expect("current image resources");
    assert_eq!(resources.len(), 1);
    assert_eq!(resources[0].node_ids, vec![node_id]);
    assert_eq!(resources[0].mime, "image/png");
    assert_eq!(resources[0].bytes, bytes);
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
    assert_eq!(project.schema_version, EDITOR_PROJECT_VERSION_V0_12);
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
