//! Compile-only contract for core APIs consumed by the pub-editor adapter.
//!
//! This crate intentionally does not depend on `pub-editor`, Reader, export,
//! writer, IDML, or ODG. It is valid only as the downstream compatibility fence
//! for core-only diffs. Any pub-editor adapter/root change must still compile the
//! real `pub-editor` package.

use pub_editor_geometry_core::{
    GeometryNodeSnapshotV1, MAX_MOVE_NODES_V1, MAX_RESIZE_NODES_V1, MoveNodeBatchEntry,
    MoveNodesTransitionErrorV1, ResizeNodeBatchEntry, ResizeNodesTransitionErrorV1,
    validate_move_nodes_transition_v1, validate_resize_nodes_transition_v1,
};
use pub_editor_image_core::{
    ImageCropStateV1, ImageCropTransitionErrorV1, ImageReplacementTransitionErrorV1,
    apply_image_crop_forward_v1, apply_image_crop_inverse_v1, apply_image_replacement_forward_v1,
    apply_image_replacement_inverse_v1, effective_image_crop_state_v1,
};
use pub_editor_table_core::{
    SetTableTrackExtentErrorV1, SetTableTrackExtentHistoryV1, TABLE_TRACK_EXTENT_HISTORY_V1,
    TableTrackExtentHistoryErrorV1, TableTrackExtentPlanV1, TableTrackTargetV1,
    apply_table_track_extent_history_forward_v1, apply_table_track_extent_history_inverse_v1,
    canonical_table_track_extent_history_v1, plan_table_track_extent_v1, set_table_track_extent_v1,
};
use pub_editor_text_core::{
    StoryRangeTransitionErrorV1, apply_story_range_forward_v1, apply_story_range_inverse_v1,
    replace_scalar_range_text_v1, story_state_id_v1,
};
use pub_model::{
    Affine2D, CanonicalId, EffectiveTableGridV1, LengthEmu, NodeId, PageId, RectEmu, Sha256Digest,
    StoryId,
};
use serde::{Serialize, de::DeserializeOwned};
use std::collections::BTreeMap;

pub const COMPAT_MAX_MOVE_NODES_V1: usize = MAX_MOVE_NODES_V1;
pub const COMPAT_MAX_RESIZE_NODES_V1: usize = MAX_RESIZE_NODES_V1;
pub const COMPAT_TABLE_TRACK_EXTENT_HISTORY_V1: &str = TABLE_TRACK_EXTENT_HISTORY_V1;

pub type ValidateMoveNodesTransitionV1Fn = fn(
    PageId,
    &[MoveNodeBatchEntry],
    &[GeometryNodeSnapshotV1],
    bool,
) -> Result<(), MoveNodesTransitionErrorV1>;
pub const VALIDATE_MOVE_NODES_TRANSITION_V1: ValidateMoveNodesTransitionV1Fn =
    validate_move_nodes_transition_v1;

pub type ValidateResizeNodesTransitionV1Fn = fn(
    PageId,
    &[ResizeNodeBatchEntry],
    &[GeometryNodeSnapshotV1],
    bool,
) -> Result<(), ResizeNodesTransitionErrorV1>;
pub const VALIDATE_RESIZE_NODES_TRANSITION_V1: ValidateResizeNodesTransitionV1Fn =
    validate_resize_nodes_transition_v1;

pub type ApplyImageReplacementForwardV1Fn = fn(
    &mut BTreeMap<NodeId, Sha256Digest>,
    NodeId,
    Option<Sha256Digest>,
    Sha256Digest,
) -> Result<(), ImageReplacementTransitionErrorV1>;
pub const APPLY_IMAGE_REPLACEMENT_FORWARD_V1: ApplyImageReplacementForwardV1Fn =
    apply_image_replacement_forward_v1;

pub type ApplyImageReplacementInverseV1Fn = ApplyImageReplacementForwardV1Fn;
pub const APPLY_IMAGE_REPLACEMENT_INVERSE_V1: ApplyImageReplacementInverseV1Fn =
    apply_image_replacement_inverse_v1;

pub type EffectiveImageCropStateV1Fn = fn(
    Option<ImageCropStateV1>,
    &BTreeMap<NodeId, ImageCropStateV1>,
    NodeId,
) -> Option<ImageCropStateV1>;
pub const EFFECTIVE_IMAGE_CROP_STATE_V1: EffectiveImageCropStateV1Fn =
    effective_image_crop_state_v1;

pub type ApplyImageCropTransitionV1Fn = fn(
    Option<ImageCropStateV1>,
    &mut BTreeMap<NodeId, ImageCropStateV1>,
    NodeId,
    ImageCropStateV1,
    ImageCropStateV1,
) -> Result<(), ImageCropTransitionErrorV1>;
pub const APPLY_IMAGE_CROP_FORWARD_V1: ApplyImageCropTransitionV1Fn = apply_image_crop_forward_v1;
pub const APPLY_IMAGE_CROP_INVERSE_V1: ApplyImageCropTransitionV1Fn = apply_image_crop_inverse_v1;

pub type StoryStateIdV1Fn = fn(StoryId, &str) -> String;
pub const STORY_STATE_ID_V1: StoryStateIdV1Fn = story_state_id_v1;

pub type ReplaceScalarRangeTextV1Fn = fn(&str, u32, u32, &str, &str) -> Option<String>;
pub const REPLACE_SCALAR_RANGE_TEXT_V1: ReplaceScalarRangeTextV1Fn = replace_scalar_range_text_v1;

pub type ApplyStoryRangeForwardV1Fn = fn(
    &str,
    StoryId,
    u32,
    u32,
    &str,
    &str,
    &str,
    &str,
) -> Result<String, StoryRangeTransitionErrorV1>;
pub const APPLY_STORY_RANGE_FORWARD_V1: ApplyStoryRangeForwardV1Fn = apply_story_range_forward_v1;

pub type ApplyStoryRangeInverseV1Fn =
    fn(&str, StoryId, u32, &str, &str, &str, &str) -> Result<String, StoryRangeTransitionErrorV1>;
pub const APPLY_STORY_RANGE_INVERSE_V1: ApplyStoryRangeInverseV1Fn = apply_story_range_inverse_v1;

pub type SetTableTrackExtentV1Fn = fn(
    &EffectiveTableGridV1,
    TableTrackTargetV1,
    LengthEmu,
) -> Result<EffectiveTableGridV1, SetTableTrackExtentErrorV1>;
pub const SET_TABLE_TRACK_EXTENT_V1: SetTableTrackExtentV1Fn = set_table_track_extent_v1;

pub type PlanTableTrackExtentV1Fn =
    fn(
        &EffectiveTableGridV1,
        RectEmu,
        TableTrackTargetV1,
        LengthEmu,
    ) -> Result<TableTrackExtentPlanV1, SetTableTrackExtentErrorV1>;
pub const PLAN_TABLE_TRACK_EXTENT_V1: PlanTableTrackExtentV1Fn = plan_table_track_extent_v1;

pub type CanonicalTableTrackExtentHistoryV1Fn =
    fn(
        &EffectiveTableGridV1,
        RectEmu,
        TableTrackTargetV1,
        LengthEmu,
    ) -> Result<SetTableTrackExtentHistoryV1, TableTrackExtentHistoryErrorV1>;
pub const CANONICAL_TABLE_TRACK_EXTENT_HISTORY_V1: CanonicalTableTrackExtentHistoryV1Fn =
    canonical_table_track_extent_history_v1;

pub type ApplyTableTrackExtentHistoryV1Fn =
    fn(
        &EffectiveTableGridV1,
        RectEmu,
        &SetTableTrackExtentHistoryV1,
    ) -> Result<(EffectiveTableGridV1, RectEmu), TableTrackExtentHistoryErrorV1>;
pub const APPLY_TABLE_TRACK_EXTENT_HISTORY_FORWARD_V1: ApplyTableTrackExtentHistoryV1Fn =
    apply_table_track_extent_history_forward_v1;
pub const APPLY_TABLE_TRACK_EXTENT_HISTORY_INVERSE_V1: ApplyTableTrackExtentHistoryV1Fn =
    apply_table_track_extent_history_inverse_v1;

fn assert_wire_owned<T>()
where
    T: std::fmt::Debug + Clone + PartialEq + Eq + Serialize + DeserializeOwned,
{
}

fn assert_copy<T: Copy>() {}

pub fn assert_adapter_wire_traits() {
    assert_wire_owned::<MoveNodeBatchEntry>();
    assert_wire_owned::<ResizeNodeBatchEntry>();
    assert_wire_owned::<ImageCropStateV1>();
    assert_wire_owned::<TableTrackTargetV1>();
    assert_wire_owned::<SetTableTrackExtentHistoryV1>();

    // session_image.rs dereferences crop values borrowed from EditOperation.
    assert_copy::<ImageCropStateV1>();
}

pub fn assert_geometry_adapter_field_shape(
    move_entry: MoveNodeBatchEntry,
    resize_entry: ResizeNodeBatchEntry,
    snapshot: GeometryNodeSnapshotV1,
) {
    let MoveNodeBatchEntry {
        node_id,
        before,
        after,
    } = move_entry;
    let _: NodeId = node_id;
    let _: RectEmu = before;
    let _: RectEmu = after;

    let ResizeNodeBatchEntry {
        node_id,
        before,
        after,
    } = resize_entry;
    let _: NodeId = node_id;
    let _: RectEmu = before;
    let _: RectEmu = after;

    let GeometryNodeSnapshotV1 {
        node_id,
        parent_id,
        bounds,
        transform,
    } = snapshot;
    let _: NodeId = node_id;
    let _: CanonicalId = parent_id;
    let _: RectEmu = bounds;
    let _: Affine2D = transform;
}

pub fn move_transition_error_shape(error: MoveNodesTransitionErrorV1) -> &'static str {
    match error {
        MoveNodesTransitionErrorV1::Empty => "empty",
        MoveNodesTransitionErrorV1::TooLarge { found } => {
            let _: usize = found;
            "too_large"
        }
        MoveNodesTransitionErrorV1::Duplicate { node_id } => {
            let _: NodeId = node_id;
            "duplicate"
        }
        MoveNodesTransitionErrorV1::SizeChanged { node_id } => {
            let _: NodeId = node_id;
            "size_changed"
        }
        MoveNodesTransitionErrorV1::NoChange { node_id } => {
            let _: NodeId = node_id;
            "no_change"
        }
        MoveNodesTransitionErrorV1::NodeUnsupported { node_id } => {
            let _: NodeId = node_id;
            "node_unsupported"
        }
        MoveNodesTransitionErrorV1::PageMismatch { node_id, page_id } => {
            let _: NodeId = node_id;
            let _: PageId = page_id;
            "page_mismatch"
        }
        MoveNodesTransitionErrorV1::Stale { node_id } => {
            let _: NodeId = node_id;
            "stale"
        }
    }
}

pub fn resize_transition_error_shape(error: ResizeNodesTransitionErrorV1) -> &'static str {
    match error {
        ResizeNodesTransitionErrorV1::InvalidCount { found } => {
            let _: usize = found;
            "invalid_count"
        }
        ResizeNodesTransitionErrorV1::Duplicate { node_id } => {
            let _: NodeId = node_id;
            "duplicate"
        }
        ResizeNodesTransitionErrorV1::NotCanonical { node_id } => {
            let _: NodeId = node_id;
            "not_canonical"
        }
        ResizeNodesTransitionErrorV1::PageMismatch { node_id, page_id } => {
            let _: NodeId = node_id;
            let _: PageId = page_id;
            "page_mismatch"
        }
        ResizeNodesTransitionErrorV1::NodeUnsupported { node_id } => {
            let _: NodeId = node_id;
            "node_unsupported"
        }
        ResizeNodesTransitionErrorV1::NonPositive { node_id } => {
            let _: NodeId = node_id;
            "non_positive"
        }
        ResizeNodesTransitionErrorV1::Overflow { node_id } => {
            let _: NodeId = node_id;
            "overflow"
        }
        ResizeNodesTransitionErrorV1::Stale { node_id } => {
            let _: NodeId = node_id;
            "stale"
        }
        ResizeNodesTransitionErrorV1::NoSizeChange => "no_size_change",
    }
}

pub fn assert_image_crop_adapter_field_shape(crop: ImageCropStateV1) {
    let ImageCropStateV1 {
        top_raw,
        bottom_raw,
        left_raw,
        right_raw,
    } = crop;
    let _: Option<u32> = top_raw;
    let _: Option<u32> = bottom_raw;
    let _: Option<u32> = left_raw;
    let _: Option<u32> = right_raw;
}

pub fn assert_table_history_adapter_field_shape(history: SetTableTrackExtentHistoryV1) {
    let SetTableTrackExtentHistoryV1 {
        protocol_version,
        table_id,
        target,
        before_extent,
        after_extent,
        before_bounds,
        after_bounds,
    } = history;
    let _: String = protocol_version;
    let _: NodeId = table_id;
    let _: TableTrackTargetV1 = target;
    let _: LengthEmu = before_extent;
    let _: LengthEmu = after_extent;
    let _: RectEmu = before_bounds;
    let _: RectEmu = after_bounds;
}

pub fn table_track_target_variant_shape(target: TableTrackTargetV1) -> &'static str {
    match target {
        TableTrackTargetV1::Row(_) => "row",
        TableTrackTargetV1::Column(_) => "column",
    }
}
