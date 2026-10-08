use std::collections::BTreeMap;

use pub_editor::{
    AuthoredEntityProvenanceV1, AuthoredShapeKindV1, AuthoredShapePaintV1, AuthoredShapeRuntimeV1,
    AuthoredShapeTransformV1, AuthoredSolidFillV1, AuthoredSolidStrokeV1, DUPLICATE_OFFSET_EMU_V1,
    DUPLICATE_PLACEMENT_POLICY_V1, DuplicateAuthoredRectangleErrorV1, EditOperation, EditorError,
    EditorSession, LengthEmu, NodeId, PageId, RectEmu, Srgb8V1,
    plan_duplicate_authored_rectangle_v1,
};
use pub_model::{
    Affine2D, Document, DocumentId, Node, NodeHeader, NodeKind, Page, ResolvedGraph, Sha256Digest,
    Size2D, SourceDescriptor,
};
use pub_reader::{PubExplicitShapePaintSource, PubResolvedGraph, PubResolvedNodePayload};

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

fn source_hash() -> Sha256Digest {
    "7777777777777777777777777777777777777777777777777777777777777777"
        .parse()
        .expect("sha")
}

fn page_id() -> PageId {
    canonical_id("11000000-0000-4000-8000-000000000001")
}

fn source_graph_node_id() -> NodeId {
    canonical_id("22000000-0000-4000-8000-000000000001")
}

fn authored_source_id() -> NodeId {
    canonical_id("01890f47-0d00-7abc-8def-0123456789ab")
}

fn duplicate_id() -> NodeId {
    canonical_id("01890f47-0d01-7abc-8def-0123456789ab")
}

fn duplicate_id_2() -> NodeId {
    canonical_id("01890f47-0d02-7abc-8def-0123456789ab")
}

fn invalid_non_v7_id() -> NodeId {
    canonical_id("22000000-0000-4000-8000-000000000002")
}

fn payload() -> PubResolvedNodePayload {
    PubResolvedNodePayload {
        contents_seq_num: 7,
        officeart_shape_type: Some(1),
        officeart_spid: Some(7),
        image_slot: None,
        legacy_ole: None,
        explicit_image_crop: None,
        explicit_image_cardinal_rotation_degrees: None,
        explicit_paint: PubExplicitShapePaintSource::default(),
        effective_paint: None,
        story_frame: None,
        text_frame_inset: None,
        table_story: None,
        table: None,
    }
}

fn graph() -> PubResolvedGraph {
    let page_id = page_id();
    let source_node = source_graph_node_id();
    let source_hash = source_hash();

    let mut pages = BTreeMap::new();
    pages.insert(
        page_id,
        Page {
            id: page_id,
            size: Size2D::new(LengthEmu::new(8_000_000), LengthEmu::new(10_000_000)),
            bleed: None,
            margins: None,
            children: vec![source_node],
            extensions: Vec::new(),
        },
    );

    let mut nodes = BTreeMap::new();
    nodes.insert(
        source_node,
        Node {
            kind: NodeKind::Shape,
            header: NodeHeader {
                id: source_node,
                parent_id: page_id.into_canonical(),
                bounds: rect(10_000, 20_000, 30_000, 40_000),
                transform: Affine2D::identity(),
                source_refs: Vec::new(),
                extensions: Vec::new(),
            },
            payload: payload(),
        },
    );

    ResolvedGraph {
        cdm_version: "0.1".into(),
        resolver_version: "duplicate-authored-rectangle-test".into(),
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
        pages,
        nodes,
        stories: BTreeMap::new(),
        paragraphs: BTreeMap::new(),
        text_runs: BTreeMap::new(),
        resources: BTreeMap::new(),
        styles: BTreeMap::new(),
        extensions: BTreeMap::new(),
    }
}

fn paint() -> AuthoredShapePaintV1 {
    AuthoredShapePaintV1 {
        fill: AuthoredSolidFillV1 {
            visible: true,
            color: Srgb8V1 {
                r: 10,
                g: 20,
                b: 30,
            },
        },
        stroke: AuthoredSolidStrokeV1 {
            visible: true,
            color: Srgb8V1 {
                r: 40,
                g: 50,
                b: 60,
            },
            width_emu: 12_700,
        },
        provenance: AuthoredEntityProvenanceV1::AuthorCreated,
    }
}

fn create_source(session: &mut EditorSession, bounds: RectEmu) {
    session
        .create_shape(authored_source_id(), page_id(), bounds, paint())
        .expect("source CreateShape");
}

#[test]
fn duplicate_is_exactly_one_create_shape_with_named_10pt_offset() {
    let mut session = EditorSession::new(graph()).expect("session");
    let source_bounds = rect(100, 200, 300, 400);
    create_source(&mut session, source_bounds);
    let before_operations = session.operations().len();

    session
        .can_duplicate_authored_rectangle(authored_source_id())
        .expect("duplicate admitted");
    let operation = session
        .duplicate_authored_rectangle(
            authored_source_id(),
            duplicate_id(),
            DUPLICATE_PLACEMENT_POLICY_V1,
        )
        .expect("duplicate");

    assert_eq!(session.operations().len(), before_operations + 1);
    assert!(matches!(
        operation,
        EditOperation::CreateShape { node_id, .. } if node_id == duplicate_id()
    ));

    let source = session
        .authored_shape(authored_source_id())
        .expect("source");
    let duplicate = session.authored_shape(duplicate_id()).expect("duplicate");
    assert_eq!(duplicate.paint, source.paint);
    assert_eq!(duplicate.shape_kind, source.shape_kind);
    assert_eq!(duplicate.transform, source.transform);
    assert_eq!(duplicate.bounds.width, source.bounds.width);
    assert_eq!(duplicate.bounds.height, source.bounds.height);
    assert_eq!(
        duplicate.bounds.x.get(),
        source.bounds.x.get() + DUPLICATE_OFFSET_EMU_V1
    );
    assert_eq!(
        duplicate.bounds.y.get(),
        source.bounds.y.get() + DUPLICATE_OFFSET_EMU_V1
    );
    assert_eq!(
        session.authored_stack(page_id()).expect("stack").members,
        vec![authored_source_id(), duplicate_id()]
    );
}

#[test]
fn undo_redo_and_fresh_replay_restore_same_duplicate_identity_and_state() {
    let base = graph();
    let mut session = EditorSession::new(base.clone()).expect("session");
    create_source(&mut session, rect(100, 200, 300, 400));
    session
        .duplicate_authored_rectangle(
            authored_source_id(),
            duplicate_id(),
            DUPLICATE_PLACEMENT_POLICY_V1,
        )
        .expect("duplicate");
    let expected = session
        .authored_shape(duplicate_id())
        .expect("duplicate state")
        .clone();
    let project = session.project();

    session.undo().expect("undo duplicate");
    assert!(session.authored_shape(duplicate_id()).is_none());
    assert_eq!(
        session.authored_stack(page_id()).expect("stack").members,
        vec![authored_source_id()]
    );

    session.redo().expect("redo duplicate");
    assert_eq!(session.authored_shape(duplicate_id()), Some(&expected));

    let mut reopened = EditorSession::new(base).expect("reopen");
    reopened.apply_project(&project).expect("replay");
    assert_eq!(reopened.authored_shape(duplicate_id()), Some(&expected));
    assert_eq!(
        reopened.authored_stack(page_id()).expect("stack").members,
        vec![authored_source_id(), duplicate_id()]
    );
}

#[test]
fn repeated_duplicate_allocates_distinct_identity_and_offsets_from_current_source() {
    let mut session = EditorSession::new(graph()).expect("session");
    create_source(&mut session, rect(100, 200, 300, 400));
    session
        .duplicate_authored_rectangle(
            authored_source_id(),
            duplicate_id(),
            DUPLICATE_PLACEMENT_POLICY_V1,
        )
        .expect("first duplicate");
    let first = session.authored_shape(duplicate_id()).unwrap().clone();

    session
        .duplicate_authored_rectangle(
            duplicate_id(),
            duplicate_id_2(),
            DUPLICATE_PLACEMENT_POLICY_V1,
        )
        .expect("second duplicate");
    let second = session.authored_shape(duplicate_id_2()).unwrap();
    assert_ne!(first.node_id, second.node_id);
    assert_eq!(
        second.bounds.x.get(),
        first.bounds.x.get() + DUPLICATE_OFFSET_EMU_V1
    );
    assert_eq!(
        second.bounds.y.get(),
        first.bounds.y.get() + DUPLICATE_OFFSET_EMU_V1
    );
    assert_eq!(
        session.authored_stack(page_id()).expect("stack").members,
        vec![authored_source_id(), duplicate_id(), duplicate_id_2()]
    );
}

#[test]
fn imported_collision_invalid_identity_policy_and_same_identity_fail_closed() {
    let mut session = EditorSession::new(graph()).expect("session");
    create_source(&mut session, rect(100, 200, 300, 400));
    let operations_before = session.operations().len();

    assert!(matches!(
        session.can_duplicate_authored_rectangle(source_graph_node_id()),
        Err(DuplicateAuthoredRectangleErrorV1::SourceUnsupported { .. })
    ));
    assert!(matches!(
        session.duplicate_authored_rectangle(
            authored_source_id(),
            invalid_non_v7_id(),
            DUPLICATE_PLACEMENT_POLICY_V1,
        ),
        Err(DuplicateAuthoredRectangleErrorV1::DestinationInvalid { .. })
    ));
    assert!(matches!(
        session.duplicate_authored_rectangle(
            authored_source_id(),
            authored_source_id(),
            DUPLICATE_PLACEMENT_POLICY_V1,
        ),
        Err(DuplicateAuthoredRectangleErrorV1::SameIdentity { .. })
    ));
    assert!(matches!(
        session.duplicate_authored_rectangle(
            authored_source_id(),
            duplicate_id(),
            "publisher.guess",
        ),
        Err(DuplicateAuthoredRectangleErrorV1::UnsupportedPlacementPolicy)
    ));

    session
        .create_shape(duplicate_id(), page_id(), rect(500, 500, 300, 400), paint())
        .expect("collision target");
    let operations_with_collision_target = session.operations().len();
    assert!(matches!(
        session.duplicate_authored_rectangle(
            authored_source_id(),
            duplicate_id(),
            DUPLICATE_PLACEMENT_POLICY_V1,
        ),
        Err(DuplicateAuthoredRectangleErrorV1::Commit(
            EditorError::CreateShapeIdCollision { .. }
        ))
    ));
    assert_eq!(operations_before + 1, operations_with_collision_target);
    assert_eq!(session.operations().len(), operations_with_collision_target);
}

#[test]
fn source_backed_and_offset_overflow_fail_before_commit() {
    let mut source = AuthoredShapeRuntimeV1 {
        node_id: authored_source_id(),
        page_id: page_id(),
        parent_id: page_id(),
        shape_kind: AuthoredShapeKindV1::Rectangle,
        bounds: rect(100, 200, 300, 400),
        transform: AuthoredShapeTransformV1::Identity,
        paint: paint(),
        provenance: AuthoredEntityProvenanceV1::SourceBacked,
    };
    assert!(matches!(
        plan_duplicate_authored_rectangle_v1(
            &source,
            duplicate_id(),
            DUPLICATE_PLACEMENT_POLICY_V1,
        ),
        Err(DuplicateAuthoredRectangleErrorV1::SourceUnsupported { .. })
    ));

    source.provenance = AuthoredEntityProvenanceV1::AuthorCreated;
    source.bounds = rect(9_007_199_254_640_991, 200, 50_000, 400);
    assert!(matches!(
        plan_duplicate_authored_rectangle_v1(
            &source,
            duplicate_id(),
            DUPLICATE_PLACEMENT_POLICY_V1,
        ),
        Err(DuplicateAuthoredRectangleErrorV1::BoundsOverflow { .. })
    ));
}
