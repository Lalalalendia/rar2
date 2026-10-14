use std::collections::BTreeMap;

use pub_editor::{
    CreateTableRuntimeV1, EDITOR_PROJECT_VERSION_V0_17, EDITOR_PROJECT_VERSION_V0_18,
    EditOperation, EditorProjectError, EditorSession, LengthEmu, NodeId, PageId, RectEmu, StoryId,
    TableCellId,
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
    "9999999999999999999999999999999999999999999999999999999999999999"
        .parse()
        .expect("sha")
}

fn page_id() -> PageId {
    canonical_id("11000000-0000-4000-8000-000000000001")
}

fn node_id() -> NodeId {
    canonical_id("01890f47-2000-7abc-8def-0123456789ab")
}

fn story_id() -> StoryId {
    canonical_id("01890f47-2001-7abc-8def-0123456789ab")
}

fn row_ids() -> Vec<TableRowId> {
    vec![
        canonical_id("01890f47-2010-7abc-8def-0123456789ab"),
        canonical_id("01890f47-2011-7abc-8def-0123456789ab"),
    ]
}

fn column_ids() -> Vec<TableColumnId> {
    vec![
        canonical_id("01890f47-2020-7abc-8def-0123456789ab"),
        canonical_id("01890f47-2021-7abc-8def-0123456789ab"),
    ]
}

fn cell_ids() -> Vec<TableCellId> {
    vec![
        canonical_id("01890f47-2030-7abc-8def-0123456789ab"),
        canonical_id("01890f47-2031-7abc-8def-0123456789ab"),
        canonical_id("01890f47-2032-7abc-8def-0123456789ab"),
        canonical_id("01890f47-2033-7abc-8def-0123456789ab"),
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
        resolver_version: "create-table-history-test".into(),
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
fn create_table_and_cell_edit_survive_undo_redo_and_fresh_project_replay() {
    let table = runtime();
    let target_cell = cell_ids()[1];
    let mut session = EditorSession::new(graph()).expect("session");

    let create = session.create_table(table.clone()).expect("CreateTable");
    assert_eq!(
        create,
        EditOperation::CreateTable {
            table: table.clone()
        }
    );
    assert!(session.graph().nodes.contains_key(&node_id()));
    assert!(session.graph().stories.contains_key(&story_id()));
    assert_eq!(
        session.authored_stack(page_id()).expect("stack").members,
        vec![node_id()]
    );
    session
        .can_replace_table_cell_text(node_id(), target_cell)
        .expect("fresh empty cell is immediately editable");

    let edit = session
        .replace_table_cell_text(node_id(), target_cell, "hello")
        .expect("edit created cell");
    assert!(matches!(edit, EditOperation::ReplaceTableCellText { .. }));

    let project = session.project();
    assert_eq!(project.schema_version, EDITOR_PROJECT_VERSION_V0_18);
    assert_eq!(project.operations.len(), 2);
    assert_eq!(project.table_grids.len(), 1);
    assert_eq!(
        project.table_grids[0]
            .rows
            .iter()
            .map(|track| track.id)
            .collect::<Vec<_>>(),
        row_ids()
    );
    assert_eq!(
        project.table_grids[0]
            .columns
            .iter()
            .map(|track| track.id)
            .collect::<Vec<_>>(),
        column_ids()
    );

    session.undo().expect("undo cell edit");
    session.undo().expect("undo CreateTable");
    assert!(!session.graph().nodes.contains_key(&node_id()));
    assert!(!session.graph().stories.contains_key(&story_id()));
    assert!(
        session
            .authored_stack(page_id())
            .expect("empty stack")
            .members
            .is_empty()
    );

    session.redo().expect("redo CreateTable");
    session.redo().expect("redo cell edit");
    assert_eq!(session.project(), project);

    let mut reopened = EditorSession::new(graph()).expect("fresh session");
    reopened.apply_project(&project).expect("project replay");
    assert_eq!(reopened.project(), project);

    let cells = reopened.editable_table_cells_for_story(story_id());
    let target = cells
        .iter()
        .find(|cell| cell.cell_id == target_cell)
        .expect("edited cell after reopen");
    assert_eq!(target.text, "hello");
}

#[test]
fn v017_project_cannot_smuggle_create_table_history() {
    let mut producer = EditorSession::new(graph()).expect("producer");
    producer.create_table(runtime()).expect("CreateTable");
    let mut legacy = producer.project();
    legacy.schema_version = EDITOR_PROJECT_VERSION_V0_17.to_owned();

    let mut target = EditorSession::new(graph()).expect("target");
    assert!(matches!(
        target.apply_project(&legacy),
        Err(EditorProjectError::LegacyProjectCarriesCreateTableOperation { index: 0 })
    ));
    assert!(target.operations().is_empty());
    assert!(!target.graph().nodes.contains_key(&node_id()));
}
