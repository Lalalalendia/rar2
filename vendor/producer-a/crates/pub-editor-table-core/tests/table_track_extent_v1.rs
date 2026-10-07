use pub_editor_table_core::{
    SetTableTrackExtentErrorV1, TableTrackTargetV1, plan_table_track_extent_v1,
    set_table_track_extent_v1,
};
use pub_model::{
    EFFECTIVE_TABLE_GRID_V1, EffectiveTableCellV1, EffectiveTableGridV1, EffectiveTableTrackV1,
    LengthEmu, NodeId, RectEmu, StoryId, TableCellAddress, TableCellId, TableColumnId, TableRowId,
};

fn canonical_id<T: serde::de::DeserializeOwned>(value: &str) -> T {
    serde_json::from_str(&format!("\"{}\"", value)).expect("canonical typed id")
}

fn row_id(n: u8) -> TableRowId {
    canonical_id(&format!("11000000-0000-4000-8000-0000000000{:02}", n))
}

fn col_id(n: u8) -> TableColumnId {
    canonical_id(&format!("12000000-0000-4000-8000-0000000000{:02}", n))
}

fn cell_id(n: u8) -> TableCellId {
    canonical_id(&format!("13000000-0000-4000-8000-0000000000{:02}", n))
}

fn table_id() -> NodeId {
    canonical_id("14000000-0000-4000-8000-000000000001")
}

fn story_id() -> StoryId {
    canonical_id("15000000-0000-4000-8000-000000000001")
}

fn table_bounds() -> RectEmu {
    RectEmu::new(
        LengthEmu::new(10),
        LengthEmu::new(20),
        LengthEmu::new(400),
        LengthEmu::new(200),
    )
}

fn grid() -> EffectiveTableGridV1 {
    let rows = vec![
        EffectiveTableTrackV1 {
            id: row_id(1),
            index: 0,
            extent: Some(LengthEmu::new(100)),
        },
        EffectiveTableTrackV1 {
            id: row_id(2),
            index: 1,
            extent: Some(LengthEmu::new(100)),
        },
    ];
    let columns = vec![
        EffectiveTableTrackV1 {
            id: col_id(1),
            index: 0,
            extent: Some(LengthEmu::new(200)),
        },
        EffectiveTableTrackV1 {
            id: col_id(2),
            index: 1,
            extent: Some(LengthEmu::new(200)),
        },
    ];
    let mut cells = Vec::new();
    let mut n = 1;
    for (row, row_track) in rows.iter().enumerate() {
        for (column, column_track) in columns.iter().enumerate() {
            cells.push(EffectiveTableCellV1 {
                id: cell_id(n),
                row_id: row_track.id,
                column_id: column_track.id,
                address: TableCellAddress {
                    row: row as u32,
                    column: column as u32,
                },
                row_span: 1,
                column_span: 1,
                story_id: Some(story_id()),
                utf16_start: Some(0),
                utf16_end: Some(0),
            });
            n += 1;
        }
    }
    EffectiveTableGridV1 {
        version: EFFECTIVE_TABLE_GRID_V1.into(),
        table_id: table_id(),
        rows,
        columns,
        cells,
    }
}

#[test]
fn one_row_extent_changes_without_touching_siblings_or_identity() {
    let before = grid();
    let after = set_table_track_extent_v1(
        &before,
        TableTrackTargetV1::Row(row_id(1)),
        LengthEmu::new(150),
    )
    .expect("row extent");

    assert_eq!(after.table_id, before.table_id);
    assert_eq!(after.rows[0].extent, Some(LengthEmu::new(150)));
    assert_eq!(after.rows[1], before.rows[1]);
    assert_eq!(after.columns, before.columns);
    assert_eq!(after.cells, before.cells);
}

#[test]
fn one_column_extent_changes_without_touching_siblings_or_identity() {
    let before = grid();
    let after = set_table_track_extent_v1(
        &before,
        TableTrackTargetV1::Column(col_id(2)),
        LengthEmu::new(275),
    )
    .expect("column extent");

    assert_eq!(after.columns[1].extent, Some(LengthEmu::new(275)));
    assert_eq!(after.columns[0], before.columns[0]);
    assert_eq!(after.rows, before.rows);
    assert_eq!(after.cells, before.cells);
}

#[test]
fn row_resize_moves_only_outer_height_by_exact_delta() {
    let before = grid();
    let bounds = table_bounds();
    let plan = plan_table_track_extent_v1(
        &before,
        bounds,
        TableTrackTargetV1::Row(row_id(1)),
        LengthEmu::new(150),
    )
    .expect("row plan");

    assert_eq!(plan.before_extent, LengthEmu::new(100));
    assert_eq!(plan.after_extent, LengthEmu::new(150));
    assert_eq!(plan.before_bounds, bounds);
    assert_eq!(plan.after_bounds.x, bounds.x);
    assert_eq!(plan.after_bounds.y, bounds.y);
    assert_eq!(plan.after_bounds.width, bounds.width);
    assert_eq!(plan.after_bounds.height, LengthEmu::new(250));
    assert_eq!(plan.after_grid.rows[1], before.rows[1]);
    assert_eq!(plan.after_grid.columns, before.columns);
    assert_eq!(plan.after_grid.cells, before.cells);
}

#[test]
fn column_resize_moves_only_outer_width_by_exact_delta() {
    let before = grid();
    let bounds = table_bounds();
    let plan = plan_table_track_extent_v1(
        &before,
        bounds,
        TableTrackTargetV1::Column(col_id(2)),
        LengthEmu::new(275),
    )
    .expect("column plan");

    assert_eq!(plan.before_extent, LengthEmu::new(200));
    assert_eq!(plan.after_extent, LengthEmu::new(275));
    assert_eq!(plan.after_bounds.x, bounds.x);
    assert_eq!(plan.after_bounds.y, bounds.y);
    assert_eq!(plan.after_bounds.width, LengthEmu::new(475));
    assert_eq!(plan.after_bounds.height, bounds.height);
    assert_eq!(plan.after_grid.columns[0], before.columns[0]);
    assert_eq!(plan.after_grid.rows, before.rows);
    assert_eq!(plan.after_grid.cells, before.cells);
}

#[test]
fn invalid_unknown_missing_and_no_change_fail_closed() {
    let before = grid();
    assert_eq!(
        set_table_track_extent_v1(
            &before,
            TableTrackTargetV1::Row(row_id(1)),
            LengthEmu::new(0),
        ),
        Err(SetTableTrackExtentErrorV1::InvalidExtent)
    );
    assert_eq!(
        set_table_track_extent_v1(
            &before,
            TableTrackTargetV1::Column(col_id(9)),
            LengthEmu::new(300),
        ),
        Err(SetTableTrackExtentErrorV1::TrackMissing)
    );
    assert_eq!(
        set_table_track_extent_v1(
            &before,
            TableTrackTargetV1::Row(row_id(1)),
            LengthEmu::new(100),
        ),
        Err(SetTableTrackExtentErrorV1::NoChange)
    );

    let mut unknown = before;
    unknown.rows[0].extent = None;
    assert_eq!(
        set_table_track_extent_v1(
            &unknown,
            TableTrackTargetV1::Row(row_id(1)),
            LengthEmu::new(150),
        ),
        Err(SetTableTrackExtentErrorV1::UnknownCurrentExtent)
    );
}
