use std::collections::BTreeMap;

use pub_editor::{
    CreateTableRuntimeV1, EDITOR_PROJECT_VERSION_V0_18, EDITOR_PROJECT_VERSION_V0_19,
    EditOperation, EditorError, EditorProjectAsset, EditorProjectError, EditorSession, LengthEmu,
    NodeId, PageId, RectEmu, ResizeNodeBatchEntry, StoryId, TableCellId, TableTrackTargetV1,
};
use pub_model::{
    Document, DocumentId, ResolvedGraph, Sha256Digest, Size2D, SourceDescriptor, TableColumnId,
    TableRowId,
};
use pub_reader::PubResolvedGraph;

fn canonical_id<T: serde::de::DeserializeOwned>(value: &str) -> T {
    serde_json::from_str(&format!("\"{}\"", value)).expect("canonical typed id")
}

fn source_hash() -> Sha256Digest {
    "abababababababababababababababababababababababababababababababab"
        .parse()
        .expect("sha")
}

fn page_id() -> PageId {
    canonical_id("11000000-0000-4000-8000-000000000001")
}

fn node_id() -> NodeId {
    canonical_id("01890f47-3000-7abc-8def-0123456789ab")
}

fn story_id() -> StoryId {
    canonical_id("01890f47-3001-7abc-8def-0123456789ab")
}

fn row_ids() -> Vec<TableRowId> {
    vec![
        canonical_id("01890f47-3010-7abc-8def-0123456789ab"),
        canonical_id("01890f47-3011-7abc-8def-0123456789ab"),
    ]
}

fn column_ids() -> Vec<TableColumnId> {
    vec![
        canonical_id("01890f47-3020-7abc-8def-0123456789ab"),
        canonical_id("01890f47-3021-7abc-8def-0123456789ab"),
    ]
}

fn cell_ids() -> Vec<TableCellId> {
    vec![
        canonical_id("01890f47-3030-7abc-8def-0123456789ab"),
        canonical_id("01890f47-3031-7abc-8def-0123456789ab"),
        canonical_id("01890f47-3032-7abc-8def-0123456789ab"),
        canonical_id("01890f47-3033-7abc-8def-0123456789ab"),
    ]
}

fn runtime() -> CreateTableRuntimeV1 {
    CreateTableRuntimeV1 {
        node_id: node_id(),
        story_id: story_id(),
        page_id: page_id(),
        bounds: RectEmu::new(
            LengthEmu::new(100_000),
            LengthEmu::new(200_000),
            LengthEmu::new(600_000),
            LengthEmu::new(400_000),
        ),
        row_ids: row_ids(),
        column_ids: column_ids(),
        cell_ids: cell_ids(),
    }
}

fn graph() -> PubResolvedGraph {
    let source_hash = source_hash();
    let page_id = page_id();
    ResolvedGraph {
        cdm_version: "0.1".into(),
        resolver_version: "table-track-history-project-test".into(),
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
        pages: BTreeMap::from([(
            page_id,
            pub_model::Page {
                id: page_id,
                size: Size2D::new(LengthEmu::new(8_000_000), LengthEmu::new(10_000_000)),
                bleed: None,
                margins: None,
                children: Vec::new(),
                extensions: Vec::new(),
            },
        )]),
        nodes: BTreeMap::new(),
        stories: BTreeMap::new(),
        paragraphs: BTreeMap::new(),
        text_runs: BTreeMap::new(),
        resources: BTreeMap::new(),
        styles: BTreeMap::new(),
        extensions: BTreeMap::new(),
    }
}

#[test]
fn track_resize_is_v019_history_with_exact_undo_redo_and_fresh_replay() {
    let mut session = EditorSession::new(graph()).expect("session");
    session.create_table(runtime()).expect("CreateTable");

    let operation = session
        .set_table_track_extent_v1(
            node_id(),
            TableTrackTargetV1::Row(row_ids()[0]),
            LengthEmu::new(250_000),
        )
        .expect("resize row");
    assert!(matches!(
        operation,
        EditOperation::SetTableTrackExtent { .. }
    ));

    let current_grid = session
        .current_table_grid_v1(node_id())
        .expect("current grid");
    assert_eq!(current_grid.rows[0].extent, Some(LengthEmu::new(250_000)));
    assert_eq!(current_grid.rows[1].extent, Some(LengthEmu::new(200_000)));
    assert_eq!(
        current_grid.columns[0].extent,
        Some(LengthEmu::new(300_000))
    );
    assert_eq!(
        session
            .current_table_bounds_v1(node_id())
            .expect("current bounds"),
        RectEmu::new(
            LengthEmu::new(100_000),
            LengthEmu::new(200_000),
            LengthEmu::new(600_000),
            LengthEmu::new(450_000),
        )
    );

    let project = session.project();
    assert_eq!(project.schema_version, EDITOR_PROJECT_VERSION_V0_19);
    assert_eq!(project.operations.len(), 2);
    assert_eq!(
        project.table_grids[0].rows[0].extent,
        Some(LengthEmu::new(250_000))
    );

    session.undo().expect("undo track resize");
    assert_eq!(
        session
            .current_table_grid_v1(node_id())
            .expect("grid after undo")
            .rows[0]
            .extent,
        Some(LengthEmu::new(200_000))
    );
    assert_eq!(
        session
            .current_table_bounds_v1(node_id())
            .expect("bounds after undo")
            .height,
        LengthEmu::new(400_000)
    );

    session.redo().expect("redo track resize");
    assert_eq!(session.project(), project);

    let mut reopened = EditorSession::new(graph()).expect("fresh session");
    reopened.apply_project(&project).expect("project replay");
    assert_eq!(reopened.project(), project);
    assert_eq!(
        reopened
            .current_table_grid_v1(node_id())
            .expect("replayed grid")
            .rows[0]
            .extent,
        Some(LengthEmu::new(250_000))
    );
    assert_eq!(
        reopened
            .current_table_bounds_v1(node_id())
            .expect("replayed bounds")
            .height,
        LengthEmu::new(450_000)
    );
}

#[test]
fn v018_project_cannot_smuggle_track_resize_history() {
    let mut producer = EditorSession::new(graph()).expect("producer");
    producer.create_table(runtime()).expect("CreateTable");
    producer
        .set_table_track_extent_v1(
            node_id(),
            TableTrackTargetV1::Column(column_ids()[1]),
            LengthEmu::new(350_000),
        )
        .expect("resize column");
    let mut legacy = producer.project();
    legacy.schema_version = EDITOR_PROJECT_VERSION_V0_18.to_owned();

    let mut target = EditorSession::new(graph()).expect("target");
    assert!(matches!(
        target.apply_project(&legacy),
        Err(EditorProjectError::LegacyProjectCarriesTableTrackExtentOperation { index: 1 })
    ));
    assert!(target.operations().is_empty());
}

#[test]
fn v019_project_requires_identity_and_exact_asset_reachability() {
    let mut producer = EditorSession::new(graph()).expect("producer");
    producer.create_table(runtime()).expect("CreateTable");
    producer
        .set_table_track_extent_v1(
            node_id(),
            TableTrackTargetV1::Row(row_ids()[0]),
            LengthEmu::new(225_000),
        )
        .expect("resize row");

    let mut missing_identity = producer.project();
    assert_eq!(
        missing_identity.schema_version,
        EDITOR_PROJECT_VERSION_V0_19
    );
    missing_identity.identity = None;

    let mut target = EditorSession::new(graph()).expect("identity target");
    assert!(matches!(
        target.apply_project(&missing_identity),
        Err(EditorProjectError::MissingProjectIdentity)
    ));

    let mut extra_asset = producer.project();
    let fake_sha: Sha256Digest = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
        .parse()
        .expect("fake sha");
    extra_asset.assets.push(EditorProjectAsset {
        sha256: fake_sha,
        mime: "image/png".into(),
        byte_len: 8,
    });

    let mut target = EditorSession::new(graph()).expect("asset target");
    assert!(matches!(
        target.apply_project(&extra_asset),
        Err(EditorProjectError::AssetReachabilityMismatch { .. })
    ));
}

#[test]
fn v019_project_rejects_tampered_effective_table_grid() {
    let mut producer = EditorSession::new(graph()).expect("producer");
    producer.create_table(runtime()).expect("CreateTable");
    producer
        .set_table_track_extent_v1(
            node_id(),
            TableTrackTargetV1::Column(column_ids()[0]),
            LengthEmu::new(350_000),
        )
        .expect("resize column");

    let mut tampered = producer.project();
    assert_eq!(tampered.schema_version, EDITOR_PROJECT_VERSION_V0_19);
    tampered.table_grids[0].columns[0].extent = Some(LengthEmu::new(351_000));

    let mut target = EditorSession::new(graph()).expect("target");
    assert!(matches!(
        target.apply_project(&tampered),
        Err(EditorProjectError::TableGridMismatch)
    ));
    assert!(target.operations().is_empty());
}

#[test]
fn geometry_after_track_resize_fails_closed_until_composition_is_defined() {
    let mut session = EditorSession::new(graph()).expect("session");
    session.create_table(runtime()).expect("CreateTable");
    session
        .set_table_track_extent_v1(
            node_id(),
            TableTrackTargetV1::Row(row_ids()[0]),
            LengthEmu::new(250_000),
        )
        .expect("resize row");

    assert!(matches!(
        session.move_node_to(node_id(), LengthEmu::new(125_000), LengthEmu::new(225_000)),
        Err(EditorError::NodeMoveUnsupported { node_id: rejected }) if rejected == node_id()
    ));
    assert!(matches!(
        session.resize_node_to(
            node_id(),
            RectEmu::new(
                LengthEmu::new(100_000),
                LengthEmu::new(200_000),
                LengthEmu::new(650_000),
                LengthEmu::new(450_000),
            ),
        ),
        Err(EditorError::NodeResizeUnsupported { node_id: rejected }) if rejected == node_id()
    ));

    let mut persisted = session.project();
    let source_bounds = runtime().bounds;
    persisted.operations.push(EditOperation::ResizeNodes {
        page_id: page_id(),
        entries: vec![ResizeNodeBatchEntry {
            node_id: node_id(),
            before: source_bounds,
            after: RectEmu::new(
                source_bounds.x,
                source_bounds.y,
                LengthEmu::new(650_000),
                LengthEmu::new(450_000),
            ),
        }],
    });

    let mut reopened = EditorSession::new(graph()).expect("reopen");
    assert!(matches!(
        reopened.apply_project(&persisted),
        Err(EditorProjectError::Operation {
            index: 2,
            error: EditorError::NodeResizeUnsupported { node_id: rejected },
        }) if rejected == node_id()
    ));
    assert!(reopened.operations().is_empty());
}
