//! Compile-isolated author-created object/runtime laws shared by pub-editor.
//!
//! This crate contains only session-neutral authoring primitives. It must not
//! depend on pub-editor or EditorSession.

mod authored_stack_lifecycle_v1;
mod authored_stack_runtime_v1;
mod create_line_runtime_v1;
mod create_shape_runtime_v1;
mod create_table_runtime_v1;

pub use authored_stack_lifecycle_v1::{
    AUTHORED_STACK_PROTOCOL_V1, AuthoredStackLifecycleErrorV1, AuthoredStackLifecycleKindV1,
    AuthoredStackLifecycleTransitionV1, AuthoredStackV1,
    apply_authored_stack_transition_forward_v1, apply_authored_stack_transition_inverse_v1,
    authored_stack_state_id_v1, plan_create_line_append_v1, plan_create_shape_append_v1,
    plan_create_table_append_v1, plan_delete_shape_remove_v1, validate_authored_stack_v1,
};
pub use authored_stack_runtime_v1::{
    AuthoredStackReorderErrorV1, AuthoredStackReorderModeV1, AuthoredStackReorderTransitionV1,
    PAGE_ORDER_PROTOCOL_V1, PageOrderErrorV1, PageOrderTransitionV1,
    apply_authored_stack_reorder_forward_v1, apply_authored_stack_reorder_inverse_v1,
    apply_page_order_transition_forward_v1, apply_page_order_transition_inverse_v1,
    page_order_state_id_v1, plan_page_order_transition_v1, plan_reorder_authored_stack_v1,
    qualified_page_order_v1,
};
pub use create_line_runtime_v1::{
    AuthoredLineRuntimeV1, CreateLineRuntimeValidationError, LineGeometryV1, PointEmuV1,
    line_bounds_v1, validate_authored_line_runtime_v1,
};
pub use create_shape_runtime_v1::{
    AuthoredEntityProvenanceV1, AuthoredShapeKindV1, AuthoredShapePaintV1, AuthoredShapeRuntimeV1,
    AuthoredShapeTransformV1, AuthoredSolidFillV1, AuthoredSolidStrokeV1,
    CreateShapeRuntimeValidationError, Srgb8V1, is_editor_created_uuid_v7_node_id,
    validate_authored_shape_runtime_v1,
};
pub use create_table_runtime_v1::{
    AUTHORED_TABLE_SENTINEL_CONTENTS_SEQ_NUM_V1, AUTHORED_TABLE_SENTINEL_TEXT_ID_V1,
    AuthoredTableStoryRangesV1, CreateTablePlanV1, CreateTableRuntimeV1,
    CreateTableRuntimeValidationError, apply_create_table_forward_v1,
    apply_create_table_inverse_v1, build_create_table_plan_v1, rebuild_authored_table_story_v1,
    validate_create_table_runtime_v1,
};

// Private compatibility for internal module imports only.
pub use pub_model::{NodeId, PageId};
