use pub_editor_table_core::{
    TABLE_TRACK_EXTENT_HISTORY_V1, TableTrackExtentHistoryErrorV1, TableTrackTargetV1,
    apply_table_track_extent_history_forward_v1, apply_table_track_extent_history_inverse_v1,
    canonical_table_track_extent_history_v1,
};
use pub_model::{
    EFFECTIVE_TABLE_GRID_V1, EffectiveTableCellV1, EffectiveTableGridV1, EffectiveTableTrackV1,
    LengthEmu, NodeId, RectEmu, TableCellAddress, TableCellId, TableColumnId, TableRowId,
};

fn canonical_id<T: serde::de::DeserializeOwned>(value: &str) -> T {
    serde_json::from_str(&format!("\"{value}\"")).expect("canonical typed id")
}

fn row_id(index: u8) -> TableRowId {
    canonical_id(&format!("01890f47-810{index}-7abc-8def-0123456789ab"))
}

fn column_id(index: u8) -> TableColumnId {
    canonical_id(&format!("01890f47-820{index}-7abc-8def-0123456789ab"))
}

fn cell_id(index: u8) -> TableCellId {
    canonical_id(&format!("01890f47-830{index}-7abc-8def-0123456789ab"))
}

fn grid() -> EffectiveTableGridV1 {
    EffectiveTableGridV1 {
        version: EFFECTIVE_TABLE_GRID_V1.into(),
        table_id: canonical_id::<NodeId>("01890f47-8000-7abc-8def-0123456789ab"),
        rows: vec![
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
        ],
        columns: vec![
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
        ],
        cells: (0_u8..2)
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
            .collect(),
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
fn canonical_history_roundtrips_exact_grid_and_bounds() {
    let source = grid();
    let source_bounds = bounds();
    let history = canonical_table_track_extent_history_v1(
        &source,
        source_bounds,
        TableTrackTargetV1::Column(column_id(1)),
        LengthEmu::new(350_000),
    )
    .expect("canonical history");

    assert_eq!(history.protocol_version, TABLE_TRACK_EXTENT_HISTORY_V1);
    assert_eq!(history.before_extent, LengthEmu::new(300_000));
    assert_eq!(history.after_extent, LengthEmu::new(350_000));
    assert_eq!(history.after_bounds.width, LengthEmu::new(650_000));

    let (after, after_bounds) =
        apply_table_track_extent_history_forward_v1(&source, source_bounds, &history)
            .expect("forward");
    assert_eq!(after.columns[1].extent, Some(LengthEmu::new(350_000)));
    assert_eq!(after_bounds, history.after_bounds);

    let (restored, restored_bounds) =
        apply_table_track_extent_history_inverse_v1(&after, after_bounds, &history)
            .expect("inverse");
    assert_eq!(restored, source);
    assert_eq!(restored_bounds, source_bounds);
}

#[test]
fn stale_or_tampered_history_is_rejected() {
    let source = grid();
    let source_bounds = bounds();
    let history = canonical_table_track_extent_history_v1(
        &source,
        source_bounds,
        TableTrackTargetV1::Row(row_id(0)),
        LengthEmu::new(250_000),
    )
    .expect("history");

    let mut wrong_protocol = history.clone();
    wrong_protocol.protocol_version = "chaptera.table-track-extent-history.bad".into();
    assert_eq!(
        apply_table_track_extent_history_forward_v1(&source, source_bounds, &wrong_protocol),
        Err(TableTrackExtentHistoryErrorV1::WrongProtocol)
    );

    let stale_bounds = RectEmu::new(
        source_bounds.x,
        source_bounds.y,
        source_bounds.width,
        LengthEmu::new(401_000),
    );
    assert_eq!(
        apply_table_track_extent_history_forward_v1(&source, stale_bounds, &history),
        Err(TableTrackExtentHistoryErrorV1::StaleBounds)
    );

    let mut tampered = history.clone();
    tampered.after_bounds = RectEmu::new(
        tampered.after_bounds.x,
        tampered.after_bounds.y,
        tampered.after_bounds.width,
        LengthEmu::new(tampered.after_bounds.height.get() + 1),
    );
    assert_eq!(
        apply_table_track_extent_history_forward_v1(&source, source_bounds, &tampered),
        Err(TableTrackExtentHistoryErrorV1::NonCanonicalAfterState)
    );
}
