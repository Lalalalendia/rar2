use pub_editor::{
    AUTHORED_TABLE_SENTINEL_CONTENTS_SEQ_NUM_V1, AUTHORED_TABLE_SENTINEL_TEXT_ID_V1,
    CreateTableRuntimeV1, CreateTableRuntimeValidationError, LengthEmu, NodeId, PageId, RectEmu,
    StoryId, TableCellId, build_create_table_plan_v1, rebuild_authored_table_story_v1,
};
use pub_model::{NodeKind, TableColumnId, TableRowId};
use pub_reader::materialize_bounded_simple_table_cells;

fn canonical_id<T: serde::de::DeserializeOwned>(value: &str) -> T {
    serde_json::from_str(&format!("\"{}\"", value)).expect("canonical typed id")
}

fn node_id() -> NodeId {
    canonical_id("01890f47-1000-7abc-8def-0123456789ab")
}

fn story_id() -> StoryId {
    canonical_id("01890f47-1001-7abc-8def-0123456789ab")
}

fn page_id() -> PageId {
    canonical_id("11000000-0000-4000-8000-000000000001")
}

fn row_ids() -> Vec<TableRowId> {
    vec![
        canonical_id("01890f47-1010-7abc-8def-0123456789ab"),
        canonical_id("01890f47-1011-7abc-8def-0123456789ab"),
    ]
}

fn column_ids() -> Vec<TableColumnId> {
    vec![
        canonical_id("01890f47-1020-7abc-8def-0123456789ab"),
        canonical_id("01890f47-1021-7abc-8def-0123456789ab"),
    ]
}

fn cell_ids() -> Vec<TableCellId> {
    vec![
        canonical_id("01890f47-1030-7abc-8def-0123456789ab"),
        canonical_id("01890f47-1031-7abc-8def-0123456789ab"),
        canonical_id("01890f47-1032-7abc-8def-0123456789ab"),
        canonical_id("01890f47-1033-7abc-8def-0123456789ab"),
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

#[test]
fn authored_empty_2x2_table_is_one_normal_graph_table_with_durable_grid_ids() {
    let plan = build_create_table_plan_v1(&runtime()).expect("CreateTable plan");

    assert_eq!(plan.node.kind, NodeKind::Table);
    assert_eq!(plan.node.header.id, node_id());
    assert_eq!(plan.node.header.parent_id, page_id().into_canonical());
    assert_eq!(
        plan.node.payload.contents_seq_num,
        AUTHORED_TABLE_SENTINEL_CONTENTS_SEQ_NUM_V1
    );

    let owner = plan
        .node
        .payload
        .table_story
        .as_ref()
        .expect("table Story owner");
    assert_eq!(owner.text_id, AUTHORED_TABLE_SENTINEL_TEXT_ID_V1);
    assert_eq!(owner.story_id, Some(story_id()));
    assert!(owner.source_refs.is_empty());

    let table = plan.node.payload.table.as_ref().expect("table payload");
    assert_eq!(table.text_id, AUTHORED_TABLE_SENTINEL_TEXT_ID_V1);
    assert_eq!(table.rows, 2);
    assert_eq!(table.columns, 2);
    assert_eq!(table.story_id, Some(story_id()));
    assert!(table.source_refs.is_empty());
    assert!(table.border_segments.is_empty());

    assert_eq!(
        plan.grid
            .rows
            .iter()
            .map(|track| track.id)
            .collect::<Vec<_>>(),
        row_ids()
    );
    assert_eq!(
        plan.grid
            .columns
            .iter()
            .map(|track| track.id)
            .collect::<Vec<_>>(),
        column_ids()
    );
    assert_eq!(
        plan.grid
            .cells
            .iter()
            .map(|cell| cell.id)
            .collect::<Vec<_>>(),
        cell_ids()
    );
    assert!(
        plan.grid
            .rows
            .iter()
            .all(|track| track.extent == Some(LengthEmu::new(200_000)))
    );
    assert!(
        plan.grid
            .columns
            .iter()
            .all(|track| track.extent == Some(LengthEmu::new(300_000)))
    );
}

#[test]
fn empty_cells_materialize_empty_and_rebuild_to_identical_story_and_ranges() {
    let plan = build_create_table_plan_v1(&runtime()).expect("CreateTable plan");
    let table = plan.node.payload.table.as_ref().expect("table payload");
    let cells = materialize_bounded_simple_table_cells(table, &plan.story)
        .expect("empty authored cells must materialize");

    assert_eq!(cells.len(), 4);
    assert!(cells.iter().all(|cell| cell.text.is_empty()));

    let ordered = cells
        .iter()
        .map(|cell| (cell.id, cell.text.clone()))
        .collect::<Vec<_>>();
    let (rebuilt_story, rebuilt_ranges) =
        rebuild_authored_table_story_v1(&ordered).expect("rebuild authored table Story");
    assert_eq!(rebuilt_story, plan.story.text);
    for source in &table.cells {
        assert_eq!(
            rebuilt_ranges[&source.id],
            (source.utf16_start, source.utf16_end)
        );
    }
}

#[test]
fn nonuniform_bounds_fail_closed_instead_of_inventing_track_rounding() {
    let mut candidate = runtime();
    candidate.bounds = RectEmu::new(
        LengthEmu::new(100_000),
        LengthEmu::new(200_000),
        LengthEmu::new(600_001),
        LengthEmu::new(400_000),
    );
    assert_eq!(
        build_create_table_plan_v1(&candidate),
        Err(CreateTableRuntimeValidationError::NonUniformBounds)
    );
}

#[test]
fn duplicate_durable_identity_is_rejected() {
    let mut candidate = runtime();
    candidate.cell_ids[0] = candidate.cell_ids[1];
    assert_eq!(
        build_create_table_plan_v1(&candidate),
        Err(CreateTableRuntimeValidationError::DuplicateIdentity)
    );
}
