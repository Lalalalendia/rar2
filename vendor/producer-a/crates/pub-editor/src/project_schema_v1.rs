use super::*;

pub(super) fn minimum_identity_project_schema_v1(operations: &[EditOperation]) -> &'static str {
    if operations
        .iter()
        .any(|operation| matches!(operation, EditOperation::RulerGuideV1 { .. }))
    {
        EDITOR_PROJECT_VERSION_V0_24
    } else if operations
        .iter()
        .any(|operation| matches!(operation, EditOperation::ReorderPagesV1 { .. }))
    {
        EDITOR_PROJECT_VERSION_V0_23
    } else if operations
        .iter()
        .any(|operation| matches!(operation, EditOperation::LinkTextFrameTail { .. }))
    {
        EDITOR_PROJECT_VERSION_V0_22
    } else if operations
        .iter()
        .any(|operation| table_rowcol_history_v1(operation).is_some())
    {
        EDITOR_PROJECT_VERSION_V0_21
    } else if operations
        .iter()
        .any(|operation| matches!(operation, EditOperation::SetTableTrackExtent { .. }))
    {
        EDITOR_PROJECT_VERSION_V0_20
    } else if operations
        .iter()
        .any(|operation| matches!(operation, EditOperation::SetImageCrop { .. }))
    {
        EDITOR_PROJECT_VERSION_V0_19
    } else if operations
        .iter()
        .any(|operation| matches!(operation, EditOperation::CreateTable { .. }))
    {
        EDITOR_PROJECT_VERSION_V0_18
    } else if operations
        .iter()
        .any(|operation| matches!(operation, EditOperation::CreateLine { .. }))
    {
        EDITOR_PROJECT_VERSION_V0_17
    } else if operations.iter().any(is_scoped_text_format_operation_v1) {
        EDITOR_PROJECT_VERSION_V0_16
    } else if operations.iter().any(|operation| {
        matches!(
            operation,
            EditOperation::SetParagraphAlignmentOverride { .. }
                | EditOperation::ClearParagraphAlignmentOverride { .. }
        )
    }) {
        EDITOR_PROJECT_VERSION_V0_15
    } else if operations
        .iter()
        .any(|operation| text_format_operation_story_id_v1(operation).is_some())
    {
        EDITOR_PROJECT_VERSION_V0_14
    } else if operations
        .iter()
        .any(|operation| matches!(operation, EditOperation::ReorderAuthoredStack { .. }))
    {
        EDITOR_PROJECT_VERSION_V0_13
    } else {
        EDITOR_PROJECT_VERSION_V0_12
    }
}
