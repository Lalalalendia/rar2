use std::collections::BTreeMap;

use pub_editor::{
    AuthoredEntityProvenanceV1, AuthoredLineRuntimeV1, AuthoredSolidStrokeV1,
    EDITOR_PROJECT_VERSION_CURRENT, EDITOR_PROJECT_VERSION_V0_16, EDITOR_PROJECT_VERSION_V0_17,
    EDITOR_PROJECT_VERSION_V0_18, EDITOR_PROJECT_VERSION_V0_19, EditOperation, EditorError,
    EditorProject, EditorProjectError, EditorSession, LineGeometryV1, PointEmuV1, Srgb8V1,
    mature_0x2c_pub_persistence_target,
};
use pub_export::{PersistenceCompatibilityState, WriterCapabilityManifest};
use pub_model::{
    Document, DocumentId, LengthEmu, Node, NodeHeader, NodeId, NodeKind, Page, PageId, RectEmu,
    ResolvedGraph, Sha256Digest, Size2D, SourceDescriptor,
};
use pub_reader::{PubExplicitShapePaintSource, PubResolvedGraph, PubResolvedNodePayload};

fn canonical_id<T: serde::de::DeserializeOwned>(value: &str) -> T {
    serde_json::from_str(&format!("\"{value}\"")).expect("canonical typed id")
}

fn source_hash() -> Sha256Digest {
    "6666666666666666666666666666666666666666666666666666666666666666"
        .parse()
        .expect("sha")
}

fn page_id() -> PageId {
    canonical_id("11000000-0000-4000-8000-000000000001")
}

fn source_node_id() -> NodeId {
    canonical_id("22000000-0000-4000-8000-000000000001")
}

fn authored_node_id() -> NodeId {
    canonical_id("01890f47-0e00-7abc-8def-0123456789ab")
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

    let pages = BTreeMap::from([(
        page_id,
        Page {
            id: page_id,
            size: Size2D::new(LengthEmu::new(8_000_000), LengthEmu::new(10_000_000)),
            bleed: None,
            margins: None,
            children: vec![source_node],
            extensions: Vec::new(),
        },
    )]);

    let nodes = BTreeMap::from([(
        source_node,
        Node {
            kind: NodeKind::Shape,
            header: NodeHeader {
                id: source_node,
                parent_id: page_id.into_canonical(),
                bounds: RectEmu::new(
                    LengthEmu::new(10_000),
                    LengthEmu::new(20_000),
                    LengthEmu::new(30_000),
                    LengthEmu::new(40_000),
                ),
                transform: pub_model::Affine2D::identity(),
                source_refs: Vec::new(),
                extensions: Vec::new(),
            },
            payload: payload(),
        },
    )]);

    ResolvedGraph {
        cdm_version: "0.1".into(),
        resolver_version: "create-line-runtime-test".into(),
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

fn stroke() -> AuthoredSolidStrokeV1 {
    AuthoredSolidStrokeV1 {
        visible: true,
        color: Srgb8V1 {
            r: 40,
            g: 50,
            b: 60,
        },
        width_emu: 25_400,
    }
}

fn geometry(begin: (i64, i64), end: (i64, i64)) -> LineGeometryV1 {
    LineGeometryV1 {
        begin: PointEmuV1 {
            x: begin.0,
            y: begin.1,
        },
        end: PointEmuV1 { x: end.0, y: end.1 },
    }
}

fn line_from_operation(operation: &EditOperation) -> AuthoredLineRuntimeV1 {
    let EditOperation::CreateLine {
        node_id,
        page_id,
        parent_id,
        geometry,
        stroke,
        provenance,
    } = operation
    else {
        panic!("expected CreateLine")
    };
    AuthoredLineRuntimeV1 {
        node_id: *node_id,
        page_id: *page_id,
        parent_id: *parent_id,
        geometry: *geometry,
        stroke: stroke.clone(),
        provenance: *provenance,
    }
}

#[test]
fn create_line_is_one_v017_history_unit_and_source_graph_stays_immutable() {
    let base = graph();
    let mut session = EditorSession::new(base.clone()).expect("session");
    let node_id = authored_node_id();
    let ordered = geometry((400_000, 500_000), (100_000, 200_000));

    let operation = session
        .create_line(node_id, page_id(), ordered, stroke())
        .expect("CreateLine");

    assert!(matches!(operation, EditOperation::CreateLine { .. }));
    assert_eq!(session.operations(), std::slice::from_ref(&operation));
    assert_eq!(session.graph(), &base, "Line must stay in authored overlay");
    assert!(!session.graph().nodes.contains_key(&node_id));

    let authored = session.authored_line(node_id).expect("authored Line");
    assert_eq!(authored.geometry, ordered, "endpoint order is canonical");
    assert_eq!(authored.parent_id, page_id());
    assert_eq!(
        authored.provenance,
        AuthoredEntityProvenanceV1::AuthorCreated
    );

    let project = session.project();
    assert_eq!(EDITOR_PROJECT_VERSION_CURRENT, EDITOR_PROJECT_VERSION_V0_19);
    assert_eq!(project.schema_version, EDITOR_PROJECT_VERSION_V0_17);
    assert_eq!(project.operations, vec![operation.clone()]);
    assert_eq!(session.persistence_requirements().len(), 3);

    session.undo().expect("undo create");
    assert!(session.authored_line(node_id).is_none());
    assert_eq!(session.graph(), &base);
    assert!(matches!(session.undo(), Err(EditorError::NothingToUndo)));

    session.redo().expect("redo create");
    assert_eq!(
        session.authored_line(node_id),
        Some(&line_from_operation(&operation))
    );

    let mut reopened = EditorSession::new(base).expect("reopen");
    reopened.apply_project(&project).expect("project replay");
    assert_eq!(reopened.project(), project);
    assert_eq!(
        reopened.authored_line(node_id),
        session.authored_line(node_id)
    );
}

#[test]
fn endpoint_direction_and_zero_spans_survive_history_and_replay_exactly() {
    for ordered in [
        geometry((100, 100), (220, 100)),
        geometry((100, 100), (100, 220)),
        geometry((140, 140), (140, 140)),
        geometry((220, 160), (100, 100)),
    ] {
        let base = graph();
        let mut session = EditorSession::new(base.clone()).expect("session");
        let operation = session
            .create_line(authored_node_id(), page_id(), ordered, stroke())
            .expect("create");
        assert_eq!(
            session
                .authored_line(authored_node_id())
                .expect("line")
                .geometry,
            ordered
        );

        session.undo().expect("undo");
        session.redo().expect("redo");
        assert_eq!(
            session
                .authored_line(authored_node_id())
                .expect("redo line")
                .geometry,
            ordered
        );

        let project = session.project();
        let mut reopened = EditorSession::new(base).expect("fresh session");
        reopened.apply_project(&project).expect("replay");
        assert_eq!(
            reopened
                .authored_line(authored_node_id())
                .expect("reopened line")
                .geometry,
            ordered
        );
        assert_eq!(project.operations, vec![operation]);
    }
}

#[test]
fn create_line_rejects_invalid_page_identity_stroke_and_collisions() {
    let mut session = EditorSession::new(graph()).expect("session");
    let missing_page: PageId = canonical_id("11000000-0000-4000-8000-000000000099");

    assert!(matches!(
        session.create_line(
            authored_node_id(),
            missing_page,
            geometry((0, 0), (10, 10)),
            stroke(),
        ),
        Err(EditorError::CreateLinePageMissing { .. })
    ));

    assert!(matches!(
        session.create_line(
            source_node_id(),
            page_id(),
            geometry((0, 0), (10, 10)),
            stroke(),
        ),
        Err(EditorError::CreateLineIdCollision { .. })
    ));

    let non_v7: NodeId = canonical_id("22000000-0000-4000-8000-000000000099");
    assert!(matches!(
        session.create_line(non_v7, page_id(), geometry((0, 0), (10, 10)), stroke()),
        Err(EditorError::CreateLineInvalidNodeId { .. })
    ));

    let mut bad_stroke = stroke();
    bad_stroke.width_emu = 0;
    assert!(matches!(
        session.create_line(
            authored_node_id(),
            page_id(),
            geometry((0, 0), (10, 10)),
            bad_stroke,
        ),
        Err(EditorError::CreateLineInvalidStroke { .. })
    ));

    session
        .create_line(
            authored_node_id(),
            page_id(),
            geometry((0, 0), (10, 10)),
            stroke(),
        )
        .expect("first create");
    assert!(matches!(
        session.create_line(
            authored_node_id(),
            page_id(),
            geometry((20, 20), (30, 30)),
            stroke(),
        ),
        Err(EditorError::CreateLineIdCollision { .. })
    ));
}

#[test]
fn v016_project_cannot_smuggle_create_line_and_source_backed_provenance_fails_closed() {
    let operation = EditOperation::CreateLine {
        node_id: authored_node_id(),
        page_id: page_id(),
        parent_id: page_id(),
        geometry: geometry((0, 0), (100, 100)),
        stroke: stroke(),
        provenance: AuthoredEntityProvenanceV1::SourceBacked,
    };
    let mut seed = EditorSession::new(graph()).expect("seed session");
    seed.create_line(
        authored_node_id(),
        page_id(),
        geometry((0, 0), (100, 100)),
        stroke(),
    )
    .expect("seed valid CreateLine");
    let mut current = seed.project();
    assert_eq!(current.schema_version, EDITOR_PROJECT_VERSION_V0_17);
    current.operations = vec![operation.clone()];

    let mut session = EditorSession::new(graph()).expect("current session");
    assert!(matches!(
        session.apply_project(&current),
        Err(EditorProjectError::Operation {
            index: 0,
            error: EditorError::CreateLineInvalidProvenance { .. }
        })
    ));

    let legacy = EditorProject {
        schema_version: EDITOR_PROJECT_VERSION_V0_16.to_owned(),
        identity: None,
        ..current
    };
    let mut session = EditorSession::new(graph()).expect("legacy session");
    assert!(matches!(
        session.apply_project(&legacy),
        Err(EditorProjectError::LegacyProjectCarriesCreateLineOperation { index: 0 })
    ));
}

#[test]
fn native_pub_persistence_does_not_overclaim_line_support() {
    let mut session = EditorSession::new(graph()).expect("session");
    session
        .create_line(
            authored_node_id(),
            page_id(),
            geometry((100, 200), (300, 400)),
            stroke(),
        )
        .expect("create");

    let assessment = session
        .assess_mature_0x2c_pub_persistence(&WriterCapabilityManifest {
            target: mature_0x2c_pub_persistence_target(),
            writer_version: "test-no-create-line-writer".into(),
            features: BTreeMap::new(),
            scoped: Vec::new(),
        })
        .expect("assessment");

    assert_eq!(
        assessment.state,
        PersistenceCompatibilityState::NotEvaluated
    );
    assert!(assessment.items.iter().any(|item| {
        item.requirement.feature == "line.geometry.endpoints"
            && item.state == PersistenceCompatibilityState::NotEvaluated
    }));
    assert!(assessment.items.iter().any(|item| {
        item.requirement.feature == "line.stroke"
            && item.state == PersistenceCompatibilityState::NotEvaluated
    }));
}
