use std::collections::BTreeMap;

use pub_editor::{
    AuthoredEntityProvenanceV1, AuthoredShapePaintV1, AuthoredSolidFillV1, AuthoredSolidStrokeV1,
    AuthoredStackReorderModeV1, EDITOR_PROJECT_VERSION_CURRENT, EDITOR_PROJECT_VERSION_V0_10,
    EDITOR_PROJECT_VERSION_V0_12, EDITOR_PROJECT_VERSION_V0_13, EDITOR_PROJECT_VERSION_V0_17,
    EDITOR_PROJECT_VERSION_V0_21, EditOperation, EditorError, EditorProjectError, EditorSession,
    LengthEmu, LineGeometryV1, PointEmuV1, RectEmu, Srgb8V1,
};
use pub_model::{
    Document, DocumentId, NodeId, Page, PageId, ResolvedGraph, Sha256Digest, Size2D,
    SourceDescriptor,
};
use pub_reader::PubResolvedGraph;

fn canonical_id<T: serde::de::DeserializeOwned>(value: &str) -> T {
    serde_json::from_str(&format!("\"{value}\"")).expect("canonical typed id")
}

fn source_hash() -> Sha256Digest {
    "7777777777777777777777777777777777777777777777777777777777777777"
        .parse()
        .expect("sha")
}

fn page_id() -> PageId {
    canonical_id("11000000-0000-4000-8000-000000000001")
}

fn node_a() -> NodeId {
    canonical_id("01890f47-0d00-7abc-8def-0123456789ab")
}

fn node_b() -> NodeId {
    canonical_id("01890f47-0d01-7abc-8def-0123456789ab")
}

fn line_node() -> NodeId {
    canonical_id("01890f47-0d02-7abc-8def-0123456789ab")
}

fn rect(x: i64) -> RectEmu {
    RectEmu::new(
        LengthEmu::new(x),
        LengthEmu::new(20_000),
        LengthEmu::new(300_000),
        LengthEmu::new(400_000),
    )
}

fn graph() -> PubResolvedGraph {
    let page_id = page_id();
    let source_hash = source_hash();
    let mut pages = BTreeMap::new();
    pages.insert(
        page_id,
        Page {
            id: page_id,
            size: Size2D::new(LengthEmu::new(8_000_000), LengthEmu::new(10_000_000)),
            bleed: None,
            margins: None,
            children: Vec::new(),
            extensions: Vec::new(),
        },
    );

    ResolvedGraph {
        cdm_version: "0.1".into(),
        resolver_version: "authored-stack-runtime-test".into(),
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
        nodes: BTreeMap::new(),
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
            color: Srgb8V1 { r: 1, g: 2, b: 3 },
        },
        stroke: AuthoredSolidStrokeV1 {
            visible: true,
            color: Srgb8V1 { r: 4, g: 5, b: 6 },
            width_emu: 12_700,
        },
        provenance: AuthoredEntityProvenanceV1::AuthorCreated,
    }
}

fn create_two(session: &mut EditorSession) {
    session
        .create_shape(node_a(), page_id(), rect(10_000), paint())
        .expect("create A");
    session
        .create_shape(node_b(), page_id(), rect(500_000), paint())
        .expect("create B");
}

#[test]
fn create_line_is_one_v0_17_history_unit_with_exact_undo_redo_and_replay() {
    let geometry = LineGeometryV1 {
        begin: PointEmuV1 {
            x: 400_000,
            y: 800_000,
        },
        end: PointEmuV1 {
            x: 100_000,
            y: 200_000,
        },
    };
    let stroke = paint().stroke;

    let mut session = EditorSession::new(graph()).expect("session");
    let operation = session
        .create_line(line_node(), page_id(), geometry, stroke.clone())
        .expect("create line");

    assert!(matches!(operation, EditOperation::CreateLine { .. }));
    let line = session.authored_line(line_node()).expect("current line");
    assert_eq!(line.geometry, geometry);
    assert_eq!(line.stroke, stroke);
    assert_eq!(line.page_id, page_id());
    assert_eq!(line.parent_id, page_id());
    assert_eq!(
        session.authored_stack(page_id()).expect("stack").members,
        vec![line_node()]
    );

    let project = session.project();
    assert_eq!(project.schema_version, EDITOR_PROJECT_VERSION_V0_17);
    assert_eq!(project.operations, vec![operation.clone()]);

    session.undo().expect("undo line");
    assert!(session.authored_line(line_node()).is_none());
    assert!(
        session
            .authored_stack(page_id())
            .expect("stack after undo")
            .members
            .is_empty()
    );

    session.redo().expect("redo line");
    let redone = session.authored_line(line_node()).expect("redone line");
    assert_eq!(redone.geometry, geometry);
    assert_eq!(redone.stroke, stroke);
    assert_eq!(
        session
            .authored_stack(page_id())
            .expect("redone stack")
            .members,
        vec![line_node()]
    );

    let mut fresh = EditorSession::new(graph()).expect("fresh session");
    fresh.apply_project(&project).expect("fresh project replay");
    let reopened = fresh.authored_line(line_node()).expect("replayed line");
    assert_eq!(reopened.geometry, geometry);
    assert_eq!(reopened.stroke, stroke);
    assert_eq!(
        fresh
            .authored_stack(page_id())
            .expect("replayed stack")
            .members,
        vec![line_node()]
    );
    assert_eq!(fresh.project(), project);
    assert_eq!(fresh.source_hash(), source_hash());
}

#[test]
fn identity_less_legacy_session_cannot_emit_v0_17_create_line_project() {
    let mut legacy_project = EditorSession::new(graph())
        .expect("legacy producer")
        .project();
    legacy_project.schema_version = EDITOR_PROJECT_VERSION_V0_10.to_owned();
    legacy_project.identity = None;

    let mut session = EditorSession::new(graph()).expect("legacy target");
    session
        .apply_project(&legacy_project)
        .expect("empty legacy project remains readable");
    assert!(session.operations().is_empty());

    session
        .create_line(
            line_node(),
            page_id(),
            LineGeometryV1 {
                begin: PointEmuV1 { x: 100, y: 200 },
                end: PointEmuV1 { x: 300, y: 400 },
            },
            paint().stroke,
        )
        .expect("runtime Line edit remains admissible");

    assert!(matches!(
        session.try_project(),
        Err(EditorProjectError::MissingProjectIdentity)
    ));
}

#[test]
fn create_delete_history_remains_v0_12_while_current_schema_advances() {
    assert_eq!(EDITOR_PROJECT_VERSION_CURRENT, EDITOR_PROJECT_VERSION_V0_21);

    let mut session = EditorSession::new(graph()).expect("session");
    create_two(&mut session);
    assert_eq!(
        session.authored_stack(page_id()).expect("stack").members,
        vec![node_a(), node_b()]
    );
    session.delete_node(node_a()).expect("delete A");
    assert_eq!(
        session.authored_stack(page_id()).expect("stack").members,
        vec![node_b()]
    );

    let project = session.project();
    assert_eq!(project.schema_version, EDITOR_PROJECT_VERSION_V0_12);

    let mut reopened = EditorSession::new(graph()).expect("reopen");
    reopened.apply_project(&project).expect("replay v0.12");
    assert_eq!(reopened.project(), project);
    assert_eq!(
        reopened.authored_stack(page_id()).expect("stack").members,
        vec![node_b()]
    );

    reopened.undo().expect("undo legacy delete after replay");
    assert_eq!(
        reopened.authored_stack(page_id()).expect("stack").members,
        vec![node_a(), node_b()]
    );
}

#[test]
fn reorder_is_one_v0_13_history_unit_with_exact_undo_redo_and_replay() {
    let mut session = EditorSession::new(graph()).expect("session");
    create_two(&mut session);

    let reorder = session
        .reorder_authored_stack(page_id(), node_a(), AuthoredStackReorderModeV1::StepForward)
        .expect("step A forward");
    assert!(matches!(
        reorder,
        EditOperation::ReorderAuthoredStack { .. }
    ));
    assert_eq!(
        session.authored_stack(page_id()).expect("stack").members,
        vec![node_b(), node_a()]
    );

    let project = session.project();
    assert_eq!(project.schema_version, EDITOR_PROJECT_VERSION_V0_13);
    assert_eq!(project.operations.len(), 3);

    session.undo().expect("undo reorder");
    assert_eq!(
        session.authored_stack(page_id()).expect("stack").members,
        vec![node_a(), node_b()]
    );
    session.redo().expect("redo reorder");
    assert_eq!(
        session.authored_stack(page_id()).expect("stack").members,
        vec![node_b(), node_a()]
    );

    let mut reopened = EditorSession::new(graph()).expect("reopen");
    reopened.apply_project(&project).expect("replay v0.13");
    assert_eq!(reopened.project(), project);
    assert_eq!(
        reopened.authored_stack(page_id()).expect("stack").members,
        vec![node_b(), node_a()]
    );
}

#[test]
fn all_four_reorder_modes_are_exact_and_noop_edges_fail_closed() {
    let mut session = EditorSession::new(graph()).expect("session");
    create_two(&mut session);

    assert!(matches!(
        session.reorder_authored_stack(
            page_id(),
            node_b(),
            AuthoredStackReorderModeV1::StepForward
        ),
        Err(EditorError::AuthoredStackReorderNoChange { .. })
    ));
    assert_eq!(session.operations().len(), 2);

    session
        .reorder_authored_stack(page_id(), node_b(), AuthoredStackReorderModeV1::ToBack)
        .expect("B to back");
    assert_eq!(
        session.authored_stack(page_id()).expect("stack").members,
        vec![node_b(), node_a()]
    );
    session
        .reorder_authored_stack(page_id(), node_b(), AuthoredStackReorderModeV1::StepForward)
        .expect("B forward");
    assert_eq!(
        session.authored_stack(page_id()).expect("stack").members,
        vec![node_a(), node_b()]
    );
    session
        .reorder_authored_stack(
            page_id(),
            node_b(),
            AuthoredStackReorderModeV1::StepBackward,
        )
        .expect("B backward");
    assert_eq!(
        session.authored_stack(page_id()).expect("stack").members,
        vec![node_b(), node_a()]
    );
    session
        .reorder_authored_stack(page_id(), node_b(), AuthoredStackReorderModeV1::ToFront)
        .expect("B to front");
    assert_eq!(
        session.authored_stack(page_id()).expect("stack").members,
        vec![node_a(), node_b()]
    );
}

#[test]
fn tampered_or_pre_v0_13_reorder_project_fails_transactionally() {
    let mut producer = EditorSession::new(graph()).expect("producer");
    create_two(&mut producer);
    producer
        .reorder_authored_stack(page_id(), node_a(), AuthoredStackReorderModeV1::StepForward)
        .expect("reorder");
    let project = producer.project();

    let mut legacy = project.clone();
    legacy.schema_version = EDITOR_PROJECT_VERSION_V0_12.to_owned();
    let mut target = EditorSession::new(graph()).expect("legacy target");
    assert!(matches!(
        target.apply_project(&legacy),
        Err(EditorProjectError::LegacyProjectCarriesReorderAuthoredStackOperation { index: 2 })
    ));
    assert!(target.operations().is_empty());
    assert!(
        target
            .authored_stack(page_id())
            .expect("stack")
            .members
            .is_empty()
    );

    let mut tampered = project.clone();
    let EditOperation::ReorderAuthoredStack { transition } = &mut tampered.operations[2] else {
        panic!("reorder operation")
    };
    transition.after.members.reverse();

    let mut target = EditorSession::new(graph()).expect("tamper target");
    assert!(matches!(
        target.apply_project(&tampered),
        Err(EditorProjectError::Operation {
            index: 2,
            error: EditorError::StaleAuthoredStack { .. }
        }) | Err(EditorProjectError::OperationMismatch { index: 2 })
    ));
    assert!(target.operations().is_empty());
    assert!(
        target
            .authored_stack(page_id())
            .expect("stack")
            .members
            .is_empty()
    );
}
