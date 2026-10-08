use pub_editor_table_core::{
    TABLE_TRACK_EXTENT_HISTORY_V1, TableTrackExtentHistoryErrorV1, TableTrackTargetV1,
    apply_table_track_extent_history_forward_v1, apply_table_track_extent_history_inverse_v1,
    canonical_table_track_extent_history_v1,
};
use pub_model::{
    EFFECTIVE_TABLE_GRID_V1, EffectiveTableCellV1, EffectiveTableGridV1, EffectiveTableTrackV1,
    LengthEmu, NodeId, RectEmu, StoryId, TableCellAddress, TableCellId, TableColumnId, TableRowId,
};

fn canonical_id<T: serde::de::DeserializeOwned>(value: &str) -> T {
    serde_json::from_str(&format!("\"{}\"", value)).expect("canonical typed id")
}

fn row_id(n: u8) -> TableRowId {
    canonical_id(&format!("21000000-0000-4000-8000-0000000000{:02}", n))
}

fn col_id(n: u8) -> TableColumnId {
    canonical_id(&format!("22000000-0000-4000-8000-0000000000{:02}", n))
}

fn cell_id(n: u8) -> TableCellId {
    canonical_id(&format!("23000000-0000-4000-8000-0000000000{:02}", n))
}

fn table_id() -> NodeId {
    canonical_id("24000000-0000-4000-8000-000000000001")
}

fn story_id() -> StoryId {
    canonical_id("25000000-0000-4000-8000-000000000001")
}

fn bounds() -> RectEmu {
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
    let mut next_id = 1;
    for (row, row_track) in rows.iter().enumerate() {
        for (column, column_track) in columns.iter().enumerate() {
            cells.push(EffectiveTableCellV1 {
                id: cell_id(next_id),
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
            next_id += 1;
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
fn canonical_history_round_trips_row_resize_exactly() {
    let before_grid = grid();
    let before_bounds = bounds();
    let history = canonical_table_track_extent_history_v1(
        &before_grid,
        before_bounds,
        TableTrackTargetV1::Row(row_id(1)),
        LengthEmu::new(150),
    )
    .expect("history");

    assert_eq!(history.protocol_version, TABLE_TRACK_EXTENT_HISTORY_V1);
    assert_eq!(history.before_extent, LengthEmu::new(100));
    assert_eq!(history.after_extent, LengthEmu::new(150));

    let (after_grid, after_bounds) =
        apply_table_track_extent_history_forward_v1(&before_grid, before_bounds, &history)
            .expect("forward");
    assert_eq!(after_grid.rows[0].extent, Some(LengthEmu::new(150)));
    assert_eq!(after_bounds.height, LengthEmu::new(250));

    let (reopened_grid, reopened_bounds) =
        apply_table_track_extent_history_inverse_v1(&after_grid, after_bounds, &history)
            .expect("inverse");
    assert_eq!(reopened_grid, before_grid);
    assert_eq!(reopened_bounds, before_bounds);
}

#[test]
fn canonical_history_round_trips_column_resize_exactly() {
    let before_grid = grid();
    let before_bounds = bounds();
    let history = canonical_table_track_extent_history_v1(
        &before_grid,
        before_bounds,
        TableTrackTargetV1::Column(col_id(2)),
        LengthEmu::new(275),
    )
    .expect("history");

    let (after_grid, after_bounds) =
        apply_table_track_extent_history_forward_v1(&before_grid, before_bounds, &history)
            .expect("forward");
    assert_eq!(after_grid.columns[1].extent, Some(LengthEmu::new(275)));
    assert_eq!(after_bounds.width, LengthEmu::new(475));

    let (restored_grid, restored_bounds) =
        apply_table_track_extent_history_inverse_v1(&after_grid, after_bounds, &history)
            .expect("inverse");
    assert_eq!(restored_grid, before_grid);
    assert_eq!(restored_bounds, before_bounds);
}

#[test]
fn stale_or_tampered_history_fails_closed() {
    let before_grid = grid();
    let before_bounds = bounds();
    let mut history = canonical_table_track_extent_history_v1(
        &before_grid,
        before_bounds,
        TableTrackTargetV1::Row(row_id(2)),
        LengthEmu::new(125),
    )
    .expect("history");

    let stale_bounds = RectEmu::new(
        before_bounds.x,
        before_bounds.y,
        before_bounds.width,
        LengthEmu::new(201),
    );
    assert_eq!(
        apply_table_track_extent_history_forward_v1(&before_grid, stale_bounds, &history),
        Err(TableTrackExtentHistoryErrorV1::StaleBounds)
    );

    history.after_bounds.height = LengthEmu::new(999);
    assert_eq!(
        apply_table_track_extent_history_forward_v1(&before_grid, before_bounds, &history),
        Err(TableTrackExtentHistoryErrorV1::NonCanonicalAfterState)
    );
}
