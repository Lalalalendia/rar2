use std::collections::BTreeMap;

use pub_editor::{
    CreateTableRuntimeV1, EDITOR_PROJECT_VERSION_V0_20, EDITOR_PROJECT_VERSION_V0_21,
    EditOperation, EditorError, EditorProjectError, EditorSession, LengthEmu, NodeId, PageId,
    ParagraphId, RectEmu, StoryId, TableCellId, TableTrackTargetV1,
    apply_create_table_forward_v1,
};
use pub_model::{
    Document, DocumentId, ResolvedGraph, Sha256Digest, Size2D, SourceDescriptor, TableColumnId,
    TableRowId,
};
use pub_reader::{PubResolvedGraph, materialize_bounded_simple_table_cells};

fn canonical_id<T: serde::de::DeserializeOwned>(value: &str) -> T {
    serde_json::from_str(&format!("\"{}\"", value)).expect("canonical typed id")
}

fn source_hash() -> Sha256Digest {
    "cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd"
        .parse()
        .expect("sha")
}

fn page_id() -> PageId {
    canonical_id("11000000-0000-4000-8000-000000000001")
}

fn table_id() -> NodeId {
    canonical_id("01890f47-6000-7abc-8def-0123456789ab")
}

fn story_id() -> StoryId {
    canonical_id("01890f47-6001-7abc-8def-0123456789ab")
}

fn row_ids() -> Vec<TableRowId> {
    vec![
        canonical_id("01890f47-6010-7abc-8def-0123456789ab"),
        canonical_id("01890f47-6011-7abc-8def-0123456789ab"),
    ]
}

fn column_ids() -> Vec<TableColumnId> {
    vec![
        canonical_id("01890f47-6020-7abc-8def-0123456789ab"),
        canonical_id("01890f47-6021-7abc-8def-0123456789ab"),
    ]
}

fn cell_ids() -> Vec<TableCellId> {
    vec![
        canonical_id("01890f47-6030-7abc-8def-0123456789ab"),
        canonical_id("01890f47-6031-7abc-8def-0123456789ab"),
        canonical_id("01890f47-6032-7abc-8def-0123456789ab"),
        canonical_id("01890f47-6033-7abc-8def-0123456789ab"),
    ]
}

fn inserted_row_id() -> TableRowId {
    canonical_id("01890f47-6100-7abc-8def-0123456789ab")
}

fn inserted_row_cells() -> Vec<TableCellId> {
    vec![
        canonical_id("01890f47-6101-7abc-8def-0123456789ab"),
        canonical_id("01890f47-6102-7abc-8def-0123456789ab"),
    ]
}

fn inserted_column_id() -> TableColumnId {
    canonical_id("01890f47-6200-7abc-8def-0123456789ab")
}

fn inserted_column_cells() -> Vec<TableCellId> {
    vec![
        canonical_id("01890f47-6201-7abc-8def-0123456789ab"),
        canonical_id("01890f47-6202-7abc-8def-0123456789ab"),
    ]
}

fn runtime() -> CreateTableRuntimeV1 {
    CreateTableRuntimeV1 {
        node_id: table_id(),
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
        resolver_version: "table-rowcol-project-v1".into(),
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

fn materialized_text(session: &EditorSession, cell_id: TableCellId) -> String {
    let table = session.graph().nodes[&table_id()]
        .payload
        .table
        .as_ref()
        .expect("table");
    let story = &session.graph().stories[&story_id()];
    materialize_bounded_simple_table_cells(table, story)
        .expect("materialize")
        .into_iter()
        .find(|cell| cell.id == cell_id)
        .expect("cell")
        .text
}

#[test]
fn insert_row_is_v021_and_composes_with_prior_track_resize_and_cell_text() {
    let mut session = EditorSession::new(graph()).expect("session");
    session.create_table(runtime()).expect("CreateTable");
    session
        .set_table_track_extent_v1(
            table_id(),
            TableTrackTargetV1::Row(row_ids()[0]),
            LengthEmu::new(250_000),
        )
        .expect("resize first row");

    let new_cells = inserted_row_cells();
    let operation = session
        .insert_table_row_v1(
            table_id(),
            1,
            inserted_row_id(),
            new_cells.clone(),
            LengthEmu::new(225_000),
        )
        .expect("insert row");
    assert!(matches!(operation, EditOperation::InsertTableRow { .. }));

    let grid = session.current_table_grid_v1(table_id()).expect("grid");
    assert_eq!(grid.rows.len(), 3);
    assert_eq!(grid.rows[0].extent, Some(LengthEmu::new(250_000)));
    assert_eq!(grid.rows[1].id, inserted_row_id());
    assert_eq!(grid.rows[1].extent, Some(LengthEmu::new(225_000)));
    assert_eq!(grid.rows[2].id, row_ids()[1]);
    assert_eq!(
        session.current_table_bounds_v1(table_id()).expect("bounds"),
        RectEmu::new(
            LengthEmu::new(100_000),
            LengthEmu::new(200_000),
            LengthEmu::new(600_000),
            LengthEmu::new(675_000),
        )
    );

    session
        .replace_table_cell_text(table_id(), new_cells[0], "new row")
        .expect("edit inserted cell");
    assert_eq!(materialized_text(&session, new_cells[0]), "new row");

    let project = session.project();
    assert_eq!(project.schema_version, EDITOR_PROJECT_VERSION_V0_21);
    assert_eq!(project.operations.len(), 4);
    assert_eq!(project.table_grids[0], grid);

    let mut reopened = EditorSession::new(graph()).expect("fresh session");
    reopened.apply_project(&project).expect("replay");
    assert_eq!(reopened.project(), project);
    assert_eq!(materialized_text(&reopened, new_cells[0]), "new row");

    reopened.undo().expect("undo cell text");
    reopened.undo().expect("undo insert row");
    let before_grid = reopened
        .current_table_grid_v1(table_id())
        .expect("before grid");
    assert_eq!(before_grid.rows.len(), 2);
    assert_eq!(before_grid.rows[0].extent, Some(LengthEmu::new(250_000)));
    assert!(!before_grid.cells.iter().any(|cell| cell.id == new_cells[0]));

    reopened.redo().expect("redo insert row");
    reopened.redo().expect("redo cell text");
    assert_eq!(reopened.project(), project);
}

#[test]
fn delete_column_undo_restores_exact_ids_and_text() {
    let mut session = EditorSession::new(graph()).expect("session");
    session.create_table(runtime()).expect("CreateTable");
    session
        .replace_table_cell_text(table_id(), cell_ids()[1], "survivor")
        .expect("edit survivor");
    session
        .replace_table_cell_text(table_id(), cell_ids()[0], "removed")
        .expect("edit removed");

    let removed_column = column_ids()[0];
    let operation = session
        .delete_table_column_v1(table_id(), removed_column)
        .expect("delete column");
    assert!(matches!(operation, EditOperation::DeleteTableColumn { .. }));
    let after = session.current_table_grid_v1(table_id()).expect("after");
    assert_eq!(after.columns.len(), 1);
    assert_eq!(after.columns[0].id, column_ids()[1]);
    assert_eq!(after.cells[0].id, cell_ids()[1]);
    assert_eq!(materialized_text(&session, cell_ids()[1]), "survivor");

    session.undo().expect("undo delete");
    let restored = session.current_table_grid_v1(table_id()).expect("restored");
    assert_eq!(restored.columns, session.project().table_grids[0].columns);
    assert!(
        restored
            .columns
            .iter()
            .any(|column| column.id == removed_column)
    );
    assert_eq!(materialized_text(&session, cell_ids()[0]), "removed");
    assert_eq!(materialized_text(&session, cell_ids()[1]), "survivor");

    session.redo().expect("redo delete");
    let redone = session.current_table_grid_v1(table_id()).expect("redone");
    assert_eq!(redone.columns.len(), 1);
    assert!(
        !redone
            .columns
            .iter()
            .any(|column| column.id == removed_column)
    );
}

#[test]
fn insert_column_is_immediately_cell_editable_and_replays() {
    let mut session = EditorSession::new(graph()).expect("session");
    session.create_table(runtime()).expect("CreateTable");

    let new_cells = inserted_column_cells();
    session
        .insert_table_column_v1(
            table_id(),
            1,
            inserted_column_id(),
            new_cells.clone(),
            LengthEmu::new(275_000),
        )
        .expect("insert column");

    session
        .replace_table_cell_text(table_id(), new_cells[0], "new column")
        .expect("edit inserted column cell");
    assert_eq!(materialized_text(&session, new_cells[0]), "new column");

    let project = session.project();
    assert_eq!(project.schema_version, EDITOR_PROJECT_VERSION_V0_21);

    let mut reopened = EditorSession::new(graph()).expect("fresh session");
    reopened.apply_project(&project).expect("replay");
    assert_eq!(reopened.project(), project);
    assert_eq!(materialized_text(&reopened, new_cells[0]), "new column");

    reopened.undo().expect("undo cell text");
    reopened.undo().expect("undo insert column");
    let before = reopened.current_table_grid_v1(table_id()).expect("before");
    assert_eq!(before.columns.len(), 2);
    assert!(!before.cells.iter().any(|cell| cell.id == new_cells[0]));

    reopened.redo().expect("redo insert column");
    reopened.redo().expect("redo cell text");
    assert_eq!(reopened.project(), project);
}

#[test]
fn pre_v021_project_cannot_smuggle_table_rowcol_history() {
    let mut producer = EditorSession::new(graph()).expect("producer");
    producer.create_table(runtime()).expect("CreateTable");
    producer
        .insert_table_column_v1(
            table_id(),
            1,
            inserted_column_id(),
            inserted_column_cells(),
            LengthEmu::new(275_000),
        )
        .expect("insert column");

    let mut legacy = producer.project();
    assert_eq!(legacy.schema_version, EDITOR_PROJECT_VERSION_V0_21);
    legacy.schema_version = EDITOR_PROJECT_VERSION_V0_20.to_owned();

    let mut target = EditorSession::new(graph()).expect("target");
    assert!(matches!(
        target.apply_project(&legacy),
        Err(EditorProjectError::LegacyProjectCarriesTableRowColOperation { index: 1 })
    ));
    assert!(target.operations().is_empty());
}

#[test]
fn structural_lifecycle_rejects_rich_table_story() {
    let mut rich = graph();
    apply_create_table_forward_v1(&mut rich, &runtime()).expect("materialize source-like table");
    rich.stories
        .get_mut(&story_id())
        .expect("table story")
        .paragraphs
        .push(canonical_id::<ParagraphId>(
            "44000000-0000-4000-8000-000000000001",
        ));

    let mut session = EditorSession::new(rich).expect("session");
    assert!(matches!(
        session.insert_table_row_v1(
            table_id(),
            1,
            inserted_row_id(),
            inserted_row_cells(),
            LengthEmu::new(225_000),
        ),
        Err(EditorError::TableRowColUnsupported { node_id }) if node_id == table_id()
    ));
    assert!(session.operations().is_empty());
}

#[test]
fn generic_geometry_after_rowcol_fails_closed() {
    let mut session = EditorSession::new(graph()).expect("session");
    session.create_table(runtime()).expect("CreateTable");
    session
        .insert_table_column_v1(
            table_id(),
            1,
            inserted_column_id(),
            inserted_column_cells(),
            LengthEmu::new(275_000),
        )
        .expect("insert column");

    assert!(matches!(
        session.move_node_to(table_id(), LengthEmu::new(125_000), LengthEmu::new(225_000)),
        Err(EditorError::NodeMoveUnsupported { node_id }) if node_id == table_id()
    ));
    assert!(matches!(
        session.resize_node_to(
            table_id(),
            RectEmu::new(
                LengthEmu::new(100_000),
                LengthEmu::new(200_000),
                LengthEmu::new(875_000),
                LengthEmu::new(400_000),
            ),
        ),
        Err(EditorError::NodeResizeUnsupported { node_id }) if node_id == table_id()
    ));
}
