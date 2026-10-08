use std::collections::BTreeMap;

use pub_editor::{
    AuthoredEntityProvenanceV1, AuthoredShapePaintV1, AuthoredSolidFillV1, AuthoredSolidStrokeV1,
    EDITOR_PROJECT_VERSION_CURRENT, EDITOR_PROJECT_VERSION_V0_11, EDITOR_PROJECT_VERSION_V0_12,
    EDITOR_PROJECT_VERSION_V0_24, EditOperation, EditorError, EditorProjectError, EditorSession,
    LengthEmu, RectEmu, Srgb8V1, authored_shape_state_id_v1,
};
use pub_model::{
    Affine2D, Document, DocumentId, Node, NodeHeader, NodeId, NodeKind, Page, PageId,
    ResolvedGraph, Sha256Digest, Size2D, SourceDescriptor,
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
    "6666666666666666666666666666666666666666666666666666666666666666"
        .parse()
        .expect("sha")
}

fn page_id() -> PageId {
    canonical_id("11000000-0000-4000-8000-000000000001")
}

fn missing_page_id() -> PageId {
    canonical_id("11000000-0000-4000-8000-000000000099")
}

fn source_node_id() -> NodeId {
    canonical_id("22000000-0000-4000-8000-000000000001")
}

fn authored_node_id() -> NodeId {
    canonical_id("01890f47-0d00-7abc-8def-0123456789ab")
}

fn missing_authored_node_id() -> NodeId {
    canonical_id("01890f47-0d01-7abc-8def-0123456789ab")
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
    let source_node = source_node_id();
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
        resolver_version: "delete-node-runtime-test".into(),
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
                r: 11,
                g: 22,
                b: 33,
            },
        },
        stroke: AuthoredSolidStrokeV1 {
            visible: true,
            color: Srgb8V1 {
                r: 44,
                g: 55,
                b: 66,
            },
            width_emu: 12_700,
        },
        provenance: AuthoredEntityProvenanceV1::AuthorCreated,
    }
}

fn create_authored_rectangle(session: &mut EditorSession) -> EditOperation {
    session
        .create_shape(
            authored_node_id(),
            page_id(),
            rect(-100_000, 250_000, 900_000, 600_000),
            paint(),
        )
        .expect("CreateShape")
}

#[test]
fn delete_node_is_one_v0_12_history_unit_with_exact_undo_redo_and_replay() {
    assert_eq!(EDITOR_PROJECT_VERSION_CURRENT, EDITOR_PROJECT_VERSION_V0_24);
    let base = graph();
    let mut session = EditorSession::new(base.clone()).expect("session");
    let create = create_authored_rectangle(&mut session);
    let node_id = authored_node_id();
    let before = session
        .authored_shape(node_id)
        .expect("authored shape")
        .clone();

    let delete = session.delete_node(node_id).expect("DeleteNode");
    let EditOperation::DeleteNode {
        node_id: deleted_node,
        page_id: deleted_page,
        before: persisted_before,
        before_state_id,
    } = &delete
    else {
        panic!("expected DeleteNode")
    };
    assert_eq!(*deleted_node, node_id);
    assert_eq!(*deleted_page, page_id());
    assert_eq!(persisted_before, &before);
    assert_eq!(before_state_id, &authored_shape_state_id_v1(&before));

    assert!(session.authored_shape(node_id).is_none());
    assert_eq!(
        session.graph(),
        &base,
        "DeleteNode must not mutate source graph"
    );
    assert_eq!(session.operations(), &[create.clone(), delete.clone()]);

    let project = session.project();
    assert_eq!(project.schema_version, EDITOR_PROJECT_VERSION_V0_12);
    assert_eq!(project.operations, vec![create.clone(), delete.clone()]);
    assert_eq!(session.persistence_requirements().len(), 4);

    assert_eq!(session.undo().expect("undo DeleteNode"), &delete);
    assert_eq!(session.authored_shape(node_id), Some(&before));
    assert_eq!(session.graph(), &base);

    assert_eq!(session.redo().expect("redo DeleteNode"), &delete);
    assert!(session.authored_shape(node_id).is_none());
    assert_eq!(session.graph(), &base);

    let mut reopened = EditorSession::new(base).expect("reopen");
    reopened.apply_project(&project).expect("replay project");
    assert_eq!(reopened.project(), project);
    assert!(reopened.authored_shape(node_id).is_none());
    assert_eq!(reopened.operations(), &[create, delete]);
}

#[test]
fn delete_node_wire_carries_exact_full_before_entity_and_state_identity() {
    let mut session = EditorSession::new(graph()).expect("session");
    create_authored_rectangle(&mut session);
    let before = session
        .authored_shape(authored_node_id())
        .expect("authored shape")
        .clone();
    let operation = session.delete_node(authored_node_id()).expect("DeleteNode");
    let value = serde_json::to_value(operation).expect("wire");

    assert_eq!(value["kind"], "delete_node");
    assert_eq!(
        value["node_id"],
        serde_json::to_value(authored_node_id()).expect("node id")
    );
    assert_eq!(
        value["page_id"],
        serde_json::to_value(page_id()).expect("page id")
    );
    assert_eq!(
        value["before"],
        serde_json::to_value(&before).expect("before entity")
    );
    assert_eq!(
        value["before_state_id"],
        authored_shape_state_id_v1(&before)
    );
}

#[test]
fn source_missing_and_already_deleted_nodes_fail_closed_without_new_history() {
    let mut session = EditorSession::new(graph()).expect("session");

    assert!(matches!(
        session.delete_node(source_node_id()),
        Err(EditorError::NodeDeleteUnsupported { .. })
    ));
    assert!(matches!(
        session.delete_node(missing_authored_node_id()),
        Err(EditorError::NodeDeleteUnsupported { .. })
    ));
    assert!(session.operations().is_empty());

    create_authored_rectangle(&mut session);
    session
        .delete_node(authored_node_id())
        .expect("first delete");
    let operation_count = session.operations().len();

    assert!(matches!(
        session.delete_node(authored_node_id()),
        Err(EditorError::NodeDeleteUnsupported { .. })
    ));
    assert_eq!(session.operations().len(), operation_count);
}

#[test]
fn stale_or_wrong_page_delete_state_is_rejected_transactionally() {
    let mut producer = EditorSession::new(graph()).expect("producer");
    create_authored_rectangle(&mut producer);
    producer
        .delete_node(authored_node_id())
        .expect("producer delete");
    let canonical = producer.project();

    let mut stale = canonical.clone();
    let EditOperation::DeleteNode { before, .. } = &mut stale.operations[1] else {
        panic!("expected DeleteNode")
    };
    before.bounds = rect(1, 2, 3, 4);

    let mut target = EditorSession::new(graph()).expect("stale target");
    assert!(matches!(
        target.apply_project(&stale),
        Err(EditorProjectError::Operation {
            index: 1,
            error: EditorError::StaleNodeDelete { .. }
        })
    ));
    assert!(target.operations().is_empty());
    assert!(target.authored_shape(authored_node_id()).is_none());

    let mut wrong_page = canonical.clone();
    let EditOperation::DeleteNode { page_id, .. } = &mut wrong_page.operations[1] else {
        panic!("expected DeleteNode")
    };
    *page_id = missing_page_id();

    let mut target = EditorSession::new(graph()).expect("page target");
    assert!(matches!(
        target.apply_project(&wrong_page),
        Err(EditorProjectError::Operation {
            index: 1,
            error: EditorError::NodeDeletePageMismatch { .. }
        })
    ));
    assert!(target.operations().is_empty());
}

#[test]
fn v0_11_project_cannot_smuggle_delete_node() {
    let mut producer = EditorSession::new(graph()).expect("producer");
    create_authored_rectangle(&mut producer);
    producer
        .delete_node(authored_node_id())
        .expect("producer delete");
    let mut project = producer.project();
    assert_eq!(project.schema_version, EDITOR_PROJECT_VERSION_V0_12);
    project.schema_version = EDITOR_PROJECT_VERSION_V0_11.to_owned();

    let mut target = EditorSession::new(graph()).expect("target");
    assert!(matches!(
        target.apply_project(&project),
        Err(EditorProjectError::LegacyProjectCarriesDeleteNodeOperation { index: 1 })
    ));
    assert!(target.operations().is_empty());
}

#[test]
fn undo_restores_same_uuidv7_entity_not_a_reconstructed_new_identity() {
    let mut session = EditorSession::new(graph()).expect("session");
    create_authored_rectangle(&mut session);
    let before = session
        .authored_shape(authored_node_id())
        .expect("authored")
        .clone();

    session.delete_node(authored_node_id()).expect("DeleteNode");
    session.undo().expect("undo");
    let restored = session
        .authored_shape(authored_node_id())
        .expect("restored");

    assert_eq!(restored, &before);
    assert_eq!(restored.node_id, authored_node_id());
    assert_eq!(
        authored_shape_state_id_v1(restored),
        authored_shape_state_id_v1(&before)
    );
}

#[test]
fn forked_project_preserves_delete_node_under_v0_12() {
    let mut producer = EditorSession::new(graph()).expect("producer");
    create_authored_rectangle(&mut producer);
    producer
        .delete_node(authored_node_id())
        .expect("producer delete");

    let forked = producer.fork_project_next_issue().expect("fork project");
    assert_eq!(forked.schema_version, EDITOR_PROJECT_VERSION_V0_12);
    assert!(matches!(
        forked.operations.last(),
        Some(EditOperation::DeleteNode { .. })
    ));

    let mut reopened = EditorSession::new(graph()).expect("reopen fork");
    reopened.apply_project(&forked).expect("replay fork");
    assert!(reopened.authored_shape(authored_node_id()).is_none());
    assert_eq!(reopened.project(), forked);
}

#[test]
fn delete_node_remains_an_explicit_native_pub_persistence_requirement() {
    let mut session = EditorSession::new(graph()).expect("session");
    create_authored_rectangle(&mut session);
    session.delete_node(authored_node_id()).expect("DeleteNode");

    let requirements = session
        .effective_pub_persistence_requirements()
        .expect("effective persistence requirements");
    assert!(requirements.iter().any(|requirement| {
        requirement.feature == "node.deleted_identity"
            && requirement.origin == Some(authored_node_id().into_canonical())
            && requirement.property_path.as_deref() == Some("node")
    }));
    assert!(
        session
            .effective_ordinary_story_text_mutations()
            .expect("story mutations")
            .is_empty(),
        "DeleteNode must not be misclassified as a Story text mutation"
    );
}

#[test]
fn create_delete_history_roundtrips_through_full_undo_redo_stack() {
    let mut session = EditorSession::new(graph()).expect("session");
    let create = create_authored_rectangle(&mut session);
    let before = session
        .authored_shape(authored_node_id())
        .expect("created shape")
        .clone();
    let delete = session.delete_node(authored_node_id()).expect("DeleteNode");

    assert_eq!(session.undo().expect("undo delete"), &delete);
    assert_eq!(session.authored_shape(authored_node_id()), Some(&before));

    assert_eq!(session.undo().expect("undo create"), &create);
    assert!(session.authored_shape(authored_node_id()).is_none());
    assert!(matches!(session.undo(), Err(EditorError::NothingToUndo)));

    assert_eq!(session.redo().expect("redo create"), &create);
    assert_eq!(session.authored_shape(authored_node_id()), Some(&before));

    assert_eq!(session.redo().expect("redo delete"), &delete);
    assert!(session.authored_shape(authored_node_id()).is_none());
    assert!(matches!(session.redo(), Err(EditorError::NothingToRedo)));
}
