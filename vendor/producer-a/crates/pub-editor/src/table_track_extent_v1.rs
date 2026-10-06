use pub_model::{
    EffectiveTableGridV1, LengthEmu, TableColumnId, TableRowId,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableTrackTargetV1 {
    Row(TableRowId),
    Column(TableColumnId),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SetTableTrackExtentErrorV1 {
    InvalidExtent,
    TrackMissing,
    AmbiguousTrack,
    InvalidResult,
}

pub fn set_table_track_extent_v1(
    grid: &EffectiveTableGridV1,
    target: TableTrackTargetV1,
    extent: LengthEmu,
) -> Result<EffectiveTableGridV1, SetTableTrackExtentErrorV1> {
    if extent.get() <= 0 {
        return Err(SetTableTrackExtentErrorV1::InvalidExtent);
    }

    let mut next = grid.clone();
    let matches = match target {
        TableTrackTargetV1::Row(id) => {
            let matches = next.rows.iter_mut().filter(|track| track.id == id).collect::<Vec<_>>();
            if matches.len() == 1 {
                matches.into_iter().next().expect("one row").extent = Some(extent);
                1
            } else {
                matches.len()
            }
        }
        TableTrackTargetV1::Column(id) => {
            let matches = next
                .columns
                .iter_mut()
                .filter(|track| track.id == id)
                .collect::<Vec<_>>();
            if matches.len() == 1 {
                matches.into_iter().next().expect("one column").extent = Some(extent);
                1
            } else {
                matches.len()
            }
        }
    };

    match matches {
        0 => return Err(SetTableTrackExtentErrorV1::TrackMissing),
        1 => {}
        _ => return Err(SetTableTrackExtentErrorV1::AmbiguousTrack),
    }

    next.validate()
        .map_err(|_| SetTableTrackExtentErrorV1::InvalidResult)?;
    Ok(next)
}
