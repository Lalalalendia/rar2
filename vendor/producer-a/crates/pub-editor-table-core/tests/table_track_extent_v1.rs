use pub_editor_table_core::{
    SetTableTrackExtentErrorV1, TableTrackTargetV1, plan_table_track_extent_v1,
    set_table_track_extent_v1,
};
use pub_model::{
    EFFECTIVE_TABLE_GRID_V1, EffectiveTableCellV1, EffectiveTableGridV1, EffectiveTableTrackV1,
    LengthEmu, NodeId, RectEmu, TableCellAddress, TableCellId, TableColumnId, TableRowId,
};

fn canonical_id<T: serde::de::DeserializeOwned>(value: &str) -> T {
    serde_json::from_str(&format!("\"{value}\"")).expect("canonical typed id")
}

fn row_id(index: u8) -> TableRowId {
    canonical_id(&format!("01890f47-710{index}-7abc-8def-0123456789ab"))
}

fn column_id(index: u8) -> TableColumnId {
    canonical_id(&format!("01890f47-720{index}-7abc-8def-0123456789ab"))
}

fn cell_id(index: u8) -> TableCellId {
    canonical_id(&format!("01890f47-730{index}-7abc-8def-0123456789ab"))
}

fn grid() -> EffectiveTableGridV1 {
    let rows = vec![
        EffectiveTableTrackV1 {
            id: row_id(0),
            index: 0,
            extent: Some(LengthEmu::new(200_000)),
        },
        EffectiveTableTrackV1 {
            id: row_id(1),
            index: 1,
            extent: Some(LengthEmu::new(200_000)),
        },
    ];
    let columns = vec![
        EffectiveTableTrackV1 {
            id: column_id(0),
            index: 0,
            extent: Some(LengthEmu::new(300_000)),
        },
        EffectiveTableTrackV1 {
            id: column_id(1),
            index: 1,
            extent: Some(LengthEmu::new(300_000)),
        },
    ];
    let cells = (0_u8..2)
        .flat_map(|row| {
            (0_u8..2).map(move |column| {
                let index = row * 2 + column;
                EffectiveTableCellV1 {
                    id: cell_id(index),
                    row_id: row_id(row),
                    column_id: column_id(column),
                    address: TableCellAddress {
                        row: u32::from(row),
                        column: u32::from(column),
                    },
                    row_span: 1,
                    column_span: 1,
                    story_id: None,
                    utf16_start: None,
                    utf16_end: None,
                }
            })
        })
        .collect();

    EffectiveTableGridV1 {
        version: EFFECTIVE_TABLE_GRID_V1.into(),
        table_id: canonical_id::<NodeId>("01890f47-7000-7abc-8def-0123456789ab"),
        rows,
        columns,
        cells,
    }
}

fn bounds() -> RectEmu {
    RectEmu::new(
        LengthEmu::new(100_000),
        LengthEmu::new(200_000),
        LengthEmu::new(600_000),
        LengthEmu::new(400_000),
    )
}

#[test]
fn row_and_column_plans_change_only_the_owned_axis() {
    let source = grid();

    let row_plan = plan_table_track_extent_v1(
        &source,
        bounds(),
        TableTrackTargetV1::Row(row_id(0)),
        LengthEmu::new(250_000),
    )
    .expect("row plan");
    assert_eq!(row_plan.before_extent, LengthEmu::new(200_000));
    assert_eq!(row_plan.after_grid.rows[0].extent, Some(LengthEmu::new(250_000)));
    assert_eq!(row_plan.after_grid.rows[1].extent, Some(LengthEmu::new(200_000)));
    assert_eq!(row_plan.after_bounds.width, LengthEmu::new(600_000));
    assert_eq!(row_plan.after_bounds.height, LengthEmu::new(450_000));

    let column_plan = plan_table_track_extent_v1(
        &source,
        bounds(),
        TableTrackTargetV1::Column(column_id(1)),
        LengthEmu::new(350_000),
    )
    .expect("column plan");
    assert_eq!(
        column_plan.after_grid.columns[1].extent,
        Some(LengthEmu::new(350_000))
    );
    assert_eq!(column_plan.after_bounds.width, LengthEmu::new(650_000));
    assert_eq!(column_plan.after_bounds.height, LengthEmu::new(400_000));

    assert_eq!(source, grid(), "planning must not mutate source");
}

#[test]
fn invalid_missing_unknown_and_noop_extents_fail_closed() {
    let source = grid();
    assert_eq!(
        set_table_track_extent_v1(
            &source,
            TableTrackTargetV1::Row(row_id(0)),
            LengthEmu::new(0),
        ),
        Err(SetTableTrackExtentErrorV1::InvalidExtent)
    );
    assert_eq!(
        set_table_track_extent_v1(
            &source,
            TableTrackTargetV1::Row(canonical_id(
                "01890f47-7199-7abc-8def-0123456789ab"
            )),
            LengthEmu::new(250_000),
        ),
        Err(SetTableTrackExtentErrorV1::TrackMissing)
    );
    assert_eq!(
        set_table_track_extent_v1(
            &source,
            TableTrackTargetV1::Row(row_id(0)),
            LengthEmu::new(200_000),
        ),
        Err(SetTableTrackExtentErrorV1::NoChange)
    );

    let mut unknown = source;
    unknown.rows[0].extent = None;
    assert_eq!(
        set_table_track_extent_v1(
            &unknown,
            TableTrackTargetV1::Row(row_id(0)),
            LengthEmu::new(250_000),
        ),
        Err(SetTableTrackExtentErrorV1::UnknownCurrentExtent)
    );
}
