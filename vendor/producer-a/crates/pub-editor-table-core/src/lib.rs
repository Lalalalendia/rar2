//! Compile-isolated table-track extent and history laws for pub-editor.
//!
//! This crate contains only session-neutral table-track primitives. It must not
//! depend on pub-editor, EditorSession, or persisted project orchestration.

mod table_track_extent_v1;
mod table_track_history_v1;

pub use table_track_extent_v1::{
    SetTableTrackExtentErrorV1, TableTrackExtentPlanV1, TableTrackTargetV1,
    plan_table_track_extent_v1, set_table_track_extent_v1,
};
pub use table_track_history_v1::{
    SetTableTrackExtentHistoryV1, TABLE_TRACK_EXTENT_HISTORY_V1, TableTrackExtentHistoryErrorV1,
    apply_table_track_extent_history_forward_v1, apply_table_track_extent_history_inverse_v1,
    canonical_table_track_extent_history_v1,
};
