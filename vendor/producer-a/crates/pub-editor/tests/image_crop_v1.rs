use std::collections::BTreeMap;

use pub_editor::{
    EDITOR_PROJECT_VERSION_CURRENT, EDITOR_PROJECT_VERSION_V0_19, EDITOR_PROJECT_VERSION_V0_24,
    EditOperation, EditorError, EditorSession, ImageCropStateV1, LengthEmu, NodeId, RectEmu,
    Sha256Digest,
};
use pub_model::{
    Affine2D, Document, DocumentId, Node, NodeHeader, NodeKind, Page, PageId, ResolvedGraph,
    Size2D, SourceDescriptor,
};
use pub_reader::{PubExplicitShapePaintSource, PubResolvedGraph, PubResolvedNodePayload};

fn node_id() -> NodeId {
    serde_json::from_str("\"20000000-0000-4000-8000-0000000000c1\"").expect("canonical NodeId JSON")
}

fn crop(top: u32, bottom: u32, left: u32, right: u32) -> ImageCropStateV1 {
    ImageCropStateV1 {
        top_raw: Some(top),
        bottom_raw: Some(bottom),
        left_raw: Some(left),
        right_raw: Some(right),
    }
}

fn canonical_id<T: serde::de::DeserializeOwned>(value: &str) -> T {
    serde_json::from_str(&format!("\"{value}\"")).expect("canonical typed id")
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
    let source_hash = Sha256Digest::from_bytes([0x44; 32]);
    let page_id: PageId = canonical_id("11000000-0000-4000-8000-000000000001");
    let node_id: NodeId = canonical_id("22000000-0000-4000-8000-000000000001");

    let page = Page {
        id: page_id,
        size: Size2D::new(LengthEmu::new(5_000_000), LengthEmu::new(5_000_000)),
        bleed: None,
        margins: None,
        children: vec![node_id],
        extensions: Vec::new(),
    };
    let node = Node {
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
    };

    (
        ResolvedGraph {
            cdm_version: "0.1".into(),
            resolver_version: "image-session-fast-test".into(),
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
                pages: vec![page_id],
                resources: Vec::new(),
                styles: Vec::new(),
            },
            pages: BTreeMap::from([(page_id, page)]),
            nodes: BTreeMap::from([(node_id, node)]),
            stories: BTreeMap::new(),
            paragraphs: BTreeMap::new(),
            text_runs: BTreeMap::new(),
            resources: BTreeMap::new(),
            styles: BTreeMap::new(),
            extensions: BTreeMap::new(),
        },
        node_id,
    )
}

fn png_bytes(tag: u8) -> Vec<u8> {
    let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
    bytes.push(tag);
    bytes
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
fn set_image_crop_has_canonical_wire_shape_and_v0_19_fence() {
    assert_eq!(EDITOR_PROJECT_VERSION_CURRENT, EDITOR_PROJECT_VERSION_V0_24);
    assert_eq!(EDITOR_PROJECT_VERSION_V0_19, "pub-editor-v0.19");

    let operation = EditOperation::SetImageCrop {
        node_id: node_id(),
        before: crop(1, 2, 3, 4),
        after: crop(5, 6, 7, 8),
    };
    let value = serde_json::to_value(operation).expect("serialize SetImageCrop");
    assert_eq!(value["kind"], "set_image_crop");
    assert_eq!(value["before"]["top_raw"], 1);
    assert_eq!(value["after"]["right_raw"], 8);
}

#[test]
fn image_crop_error_codes_are_stable() {
    let node_id = node_id();
    assert_eq!(
        EditorError::ImageCropUnsupported { node_id }.code(),
        "image_crop_unsupported"
    );
    assert_eq!(
        EditorError::ImageCropNoChange { node_id }.code(),
        "image_crop_no_change"
    );
    assert_eq!(
        EditorError::StaleImageCrop { node_id }.code(),
        "stale_image_crop"
    );
}
