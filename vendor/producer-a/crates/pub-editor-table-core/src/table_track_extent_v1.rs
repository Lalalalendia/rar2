//! Steady-state control for table-track core fast-loop ownership.
use pub_model::{EffectiveTableGridV1, LengthEmu, RectEmu, TableColumnId, TableRowId};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "axis", content = "track_id", rename_all = "snake_case")]
pub enum TableTrackTargetV1 {
    Row(TableRowId),
    Column(TableColumnId),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableTrackExtentPlanV1 {
    pub target: TableTrackTargetV1,
    pub before_extent: LengthEmu,
    pub after_extent: LengthEmu,
    pub before_grid: EffectiveTableGridV1,
    pub after_grid: EffectiveTableGridV1,
    pub before_bounds: RectEmu,
    pub after_bounds: RectEmu,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SetTableTrackExtentErrorV1 {
    InvalidExtent,
    TrackMissing,
    AmbiguousTrack,
    UnknownCurrentExtent,
    NoChange,
    BoundsOverflow,
    InvalidResult,
}

fn current_extent_v1(
    grid: &EffectiveTableGridV1,
    target: TableTrackTargetV1,
) -> Result<LengthEmu, SetTableTrackExtentErrorV1> {
    let extents = match target {
        TableTrackTargetV1::Row(id) => grid
            .rows
            .iter()
            .filter(|track| track.id == id)
            .map(|track| track.extent)
            .collect::<Vec<_>>(),
        TableTrackTargetV1::Column(id) => grid
            .columns
            .iter()
            .filter(|track| track.id == id)
            .map(|track| track.extent)
            .collect::<Vec<_>>(),
    };

    match extents.as_slice() {
        [] => Err(SetTableTrackExtentErrorV1::TrackMissing),
        [None] => Err(SetTableTrackExtentErrorV1::UnknownCurrentExtent),
        [Some(extent)] => Ok(*extent),
        _ => Err(SetTableTrackExtentErrorV1::AmbiguousTrack),
    }
}

pub fn set_table_track_extent_v1(
    grid: &EffectiveTableGridV1,
    target: TableTrackTargetV1,
    extent: LengthEmu,
) -> Result<EffectiveTableGridV1, SetTableTrackExtentErrorV1> {
    if extent.get() <= 0 {
        return Err(SetTableTrackExtentErrorV1::InvalidExtent);
    }

    let before_extent = current_extent_v1(grid, target)?;
    if before_extent == extent {
        return Err(SetTableTrackExtentErrorV1::NoChange);
    }

    let mut next = grid.clone();
    match target {
        TableTrackTargetV1::Row(id) => {
            next.rows
                .iter_mut()
                .find(|track| track.id == id)
                .expect("current_extent_v1 proved row identity")
                .extent = Some(extent);
        }
        TableTrackTargetV1::Column(id) => {
            next.columns
                .iter_mut()
                .find(|track| track.id == id)
                .expect("current_extent_v1 proved column identity")
                .extent = Some(extent);
        }
    }

    next.validate()
        .map_err(|_| SetTableTrackExtentErrorV1::InvalidResult)?;
    Ok(next)
}

pub fn plan_table_track_extent_v1(
    grid: &EffectiveTableGridV1,
    table_bounds: RectEmu,
    target: TableTrackTargetV1,
    extent: LengthEmu,
) -> Result<TableTrackExtentPlanV1, SetTableTrackExtentErrorV1> {
    let before_extent = current_extent_v1(grid, target)?;
    let after_grid = set_table_track_extent_v1(grid, target, extent)?;
    let delta = extent
        .get()
        .checked_sub(before_extent.get())
        .ok_or(SetTableTrackExtentErrorV1::BoundsOverflow)?;

    let (width, height) = match target {
        TableTrackTargetV1::Row(_) => {
            let height = table_bounds
                .height
                .get()
                .checked_add(delta)
                .ok_or(SetTableTrackExtentErrorV1::BoundsOverflow)?;
            (table_bounds.width.get(), height)
        }
        TableTrackTargetV1::Column(_) => {
            let width = table_bounds
                .width
                .get()
                .checked_add(delta)
                .ok_or(SetTableTrackExtentErrorV1::BoundsOverflow)?;
            (width, table_bounds.height.get())
        }
    };

    if width <= 0 || height <= 0 {
        return Err(SetTableTrackExtentErrorV1::InvalidResult);
    }

    Ok(TableTrackExtentPlanV1 {
        target,
        before_extent,
        after_extent: extent,
        before_grid: grid.clone(),
        after_grid,
        before_bounds: table_bounds,
        after_bounds: RectEmu::new(
            table_bounds.x,
            table_bounds.y,
            LengthEmu::new(width),
            LengthEmu::new(height),
        ),
    })
}
