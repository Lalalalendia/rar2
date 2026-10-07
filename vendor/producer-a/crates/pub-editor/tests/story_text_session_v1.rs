use std::collections::BTreeMap;

use pub_editor::{EditOperation, EditorError, EditorSession};
use pub_model::{
    Affine2D, CanonicalId, Document, DocumentId, LengthEmu, Node, NodeHeader, NodeId, NodeKind,
    Page, PageId, RectEmu, ResolvedGraph, Sha256Digest, Size2D, SourceDescriptor, Story, StoryId,
};
use pub_reader::{
    PubExplicitShapePaintSource, PubResolvedGraph, PubResolvedNodePayload, PubResolvedStoryFrame,
};

fn canonical(byte: u8) -> CanonicalId {
    CanonicalId::from_bytes([byte; 16])
}

fn document_id() -> DocumentId {
    DocumentId::from_canonical(canonical(0x10))
}

fn page_id() -> PageId {
    PageId::from_canonical(canonical(0x20))
}

fn frame_id() -> NodeId {
    NodeId::from_canonical(canonical(0x30))
}

fn story_id() -> StoryId {
    StoryId::from_canonical(canonical(0x40))
}

fn source_hash() -> Sha256Digest {
    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        .parse()
        .expect("valid source hash")
}

fn graph() -> PubResolvedGraph {
    let page_id = page_id();
    let frame_id = frame_id();
    let story_id = story_id();
    let source_hash = source_hash();

    let frame = Node {
        kind: NodeKind::TextFrame,
        header: NodeHeader {
            id: frame_id,
            parent_id: page_id.into_canonical(),
            bounds: RectEmu::new(
                LengthEmu::new(100_000),
                LengthEmu::new(100_000),
                LengthEmu::new(800_000),
                LengthEmu::new(300_000),
            ),
            transform: Affine2D::identity(),
            source_refs: Vec::new(),
            extensions: Vec::new(),
        },
        payload: PubResolvedNodePayload {
            contents_seq_num: 1,
            officeart_shape_type: Some(202),
            officeart_spid: Some(1),
            image_slot: None,
            legacy_ole: None,
            explicit_image_crop: None,
            explicit_image_cardinal_rotation_degrees: None,
            explicit_paint: PubExplicitShapePaintSource::default(),
            effective_paint: None,
            story_frame: Some(PubResolvedStoryFrame {
                story_id: Some(story_id),
                ordinal: 0,
                previous_frame: None,
                next_frame: None,
                vertical_alignment: None,
            }),
            text_frame_inset: None,
            table_story: None,
            table: None,
        },
    };

    ResolvedGraph {
        cdm_version: "0.1".into(),
        resolver_version: "story-text-session-test".into(),
        source: SourceDescriptor {
            format: "pub".into(),
            format_version: Some("0x2c".into()),
            adapter_version: "pub-rs/test".into(),
            source_hash,
        },
        document: Document {
            id: document_id(),
            format_origin: "pub".into(),
            source_hash,
            pages: vec![page_id],
            resources: Vec::new(),
            styles: Vec::new(),
        },
        pages: BTreeMap::from([(
            page_id,
            Page {
                id: page_id,
                size: Size2D::new(LengthEmu::new(4_000_000), LengthEmu::new(2_000_000)),
                bleed: None,
                margins: None,
                children: vec![frame_id],
                extensions: Vec::new(),
            },
        )]),
        nodes: BTreeMap::from([(frame_id, frame)]),
        stories: BTreeMap::from([(
            story_id,
            Story {
                id: story_id,
                text: "Hello world".into(),
                paragraphs: Vec::new(),
                runs: Vec::new(),
                fields: Vec::new(),
                hyperlinks: Vec::new(),
                source_refs: Vec::new(),
            },
        )]),
        paragraphs: BTreeMap::new(),
        text_runs: BTreeMap::new(),
        resources: BTreeMap::new(),
        styles: BTreeMap::new(),
        extensions: BTreeMap::new(),
    }
}

#[test]
fn replace_story_range_mutates_plain_story_through_session_owner() {
    let mut session = EditorSession::new(graph()).expect("session");
    session
        .can_enter_story_text_session(story_id())
        .expect("plain Story enters text session");
    session
        .can_replace_story_text(story_id())
        .expect("plain Story admits replacement");

    let operation = session
        .replace_story_range(story_id(), 6, 11, "world", "Chaptera")
        .expect("replace exact scalar range");

    assert!(matches!(
        operation,
        EditOperation::ReplaceStoryRange {
            story_id: actual,
            start_scalar: 6,
            end_scalar: 11,
            ..
        } if actual == story_id()
    ));
    assert_eq!(session.graph().stories[&story_id()].text, "Hello Chaptera");
}

#[test]
fn replace_story_text_replaces_full_plain_story_and_stale_range_fails() {
    let mut session = EditorSession::new(graph()).expect("session");
    session
        .replace_story_text(story_id(), "Chaptera")
        .expect("replace whole Story");
    assert_eq!(session.graph().stories[&story_id()].text, "Chaptera");

    assert!(matches!(
        session.replace_story_range(story_id(), 0, 8, "Hello", "x"),
        Err(EditorError::StaleOperation { story_id: actual }) if actual == story_id()
    ));
}
