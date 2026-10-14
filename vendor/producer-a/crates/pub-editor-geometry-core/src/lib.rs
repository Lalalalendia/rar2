//! Compile-isolated Move/Resize transition laws shared by pub-editor.
//!
//! This crate contains only session-neutral geometry primitives. It must not
//! depend on pub-editor, EditorSession, or PubResolvedGraph.

mod batch_transition_v1;

pub use batch_transition_v1::{
    GeometryNodeSnapshotV1, MAX_MOVE_NODES_V1, MAX_RESIZE_NODES_V1, MoveNodeBatchEntry,
    MoveNodesTransitionErrorV1, ResizeNodeBatchEntry, ResizeNodesTransitionErrorV1,
    validate_move_nodes_transition_v1, validate_resize_nodes_transition_v1,
};
