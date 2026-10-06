use crate::{
    SetTableTrackExtentErrorV1, TableTrackTargetV1, plan_table_track_extent_v1,
};
use pub_model::{EffectiveTableGridV1, LengthEmu, NodeId, RectEmu};
use serde::{Deserialize, Serialize};

pub const TABLE_TRACK_EXTENT_HISTORY_V1: &str = "chaptera.table-track-extent-history.v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetTableTrackExtentHistoryV1 {
    pub protocol_version: String,
    pub table_id: NodeId,
    pub target: TableTrackTargetV1,
    pub before_extent: LengthEmu,
    pub after_extent: LengthEmu,
    pub before_bounds: RectEmu,
    pub after_bounds: RectEmu,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TableTrackExtentHistoryErrorV1 {
    WrongProtocol,
    TableIdentityMismatch,
    StaleGrid,
    StaleBounds,
    NonCanonicalAfterState,
    Transition(SetTableTrackExtentErrorV1),
}

impl From<SetTableTrackExtentErrorV1> for TableTrackExtentHistoryErrorV1 {
    fn from(value: SetTableTrackExtentErrorV1) -> Self {
        Self::Transition(value)
    }
}

pub fn canonical_table_track_extent_history_v1(
    grid: &EffectiveTableGridV1,
    bounds: RectEmu,
    target: TableTrackTargetV1,
    after_extent: LengthEmu,
) -> Result<SetTableTrackExtentHistoryV1, TableTrackExtentHistoryErrorV1> {
    let plan = plan_table_track_extent_v1(grid, bounds, target, after_extent)?;
    Ok(SetTableTrackExtentHistoryV1 {
        protocol_version: TABLE_TRACK_EXTENT_HISTORY_V1.into(),
        table_id: grid.table_id,
        target,
        before_extent: plan.before_extent,
        after_extent: plan.after_extent,
        before_bounds: plan.before_bounds,
        after_bounds: plan.after_bounds,
    })
}

pub fn apply_table_track_extent_history_forward_v1(
    grid: &EffectiveTableGridV1,
    bounds: RectEmu,
    history: &SetTableTrackExtentHistoryV1,
) -> Result<(EffectiveTableGridV1, RectEmu), TableTrackExtentHistoryErrorV1> {
    if history.protocol_version != TABLE_TRACK_EXTENT_HISTORY_V1 {
        return Err(TableTrackExtentHistoryErrorV1::WrongProtocol);
    }
    if grid.table_id != history.table_id {
        return Err(TableTrackExtentHistoryErrorV1::TableIdentityMismatch);
    }
    if bounds != history.before_bounds {
        return Err(TableTrackExtentHistoryErrorV1::StaleBounds);
    }

    let plan = plan_table_track_extent_v1(grid, bounds, history.target, history.after_extent)?;
    if plan.before_extent != history.before_extent {
        return Err(TableTrackExtentHistoryErrorV1::StaleGrid);
    }
    if plan.after_bounds != history.after_bounds {
        return Err(TableTrackExtentHistoryErrorV1::NonCanonicalAfterState);
    }

    Ok((plan.after_grid, plan.after_bounds))
}

pub fn apply_table_track_extent_history_inverse_v1(
    grid: &EffectiveTableGridV1,
    bounds: RectEmu,
    history: &SetTableTrackExtentHistoryV1,
) -> Result<(EffectiveTableGridV1, RectEmu), TableTrackExtentHistoryErrorV1> {
    if history.protocol_version != TABLE_TRACK_EXTENT_HISTORY_V1 {
        return Err(TableTrackExtentHistoryErrorV1::WrongProtocol);
    }
    if grid.table_id != history.table_id {
        return Err(TableTrackExtentHistoryErrorV1::TableIdentityMismatch);
    }
    if bounds != history.after_bounds {
        return Err(TableTrackExtentHistoryErrorV1::StaleBounds);
    }

    let plan = plan_table_track_extent_v1(grid, bounds, history.target, history.before_extent)?;
    if plan.before_extent != history.after_extent {
        return Err(TableTrackExtentHistoryErrorV1::StaleGrid);
    }
    if plan.after_bounds != history.before_bounds {
        return Err(TableTrackExtentHistoryErrorV1::NonCanonicalAfterState);
    }

    Ok((plan.after_grid, plan.after_bounds))
}
