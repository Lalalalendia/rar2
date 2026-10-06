use pub_editor::{SetTableTrackExtentErrorV1, TableTrackTargetV1, set_table_track_extent_v1};
use pub_model::{
    EFFECTIVE_TABLE_GRID_V1, EffectiveTableCellV1, EffectiveTableGridV1, EffectiveTableTrackV1,
    LengthEmu, NodeId, StoryId, TableCellAddress, TableCellId, TableColumnId, TableRowId,
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
    for row in 0..2 {
        for column in 0..2 {
            cells.push(EffectiveTableCellV1 {
                id: cell_id(n),
                row_id: rows[row].id,
                column_id: columns[column].id,
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
fn invalid_or_unknown_track_fails_closed() {
    let before = grid();
    assert_eq!(
        set_table_track_extent_v1(
            &before,
            TableTrackTargetV1::Row(row_id(1)),
            LengthEmu::new(0)
        ),
        Err(SetTableTrackExtentErrorV1::InvalidExtent)
    );
    assert_eq!(
        set_table_track_extent_v1(
            &before,
            TableTrackTargetV1::Column(col_id(9)),
            LengthEmu::new(300)
        ),
        Err(SetTableTrackExtentErrorV1::TrackMissing)
    );
}
