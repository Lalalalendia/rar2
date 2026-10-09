use super::*;

pub(super) fn minimum_identity_project_schema_v1(operations: &[EditOperation]) -> &'static str {
    if operations
        .iter()
        .any(|operation| matches!(operation, EditOperation::AppendBlankPageV1 { .. }))
    {
        EDITOR_PROJECT_VERSION_V0_25
    } else if operations.iter().any(|operation| {
        matches!(
            operation,
            EditOperation::RegisterAuthoredPageIdentityV1 { .. }
        )
    }) {
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

pub(super) fn append_blank_page_persistence_requirements_v1(
    transition: &AppendBlankPageTransitionV1,
) -> Vec<PersistenceRequirement> {
    vec![
        PersistenceRequirement {
            feature: "page.created_identity".into(),
            origin: Some(transition.identity.page_id.into_canonical()),
            property_path: Some("page.identity".into()),
        },
        PersistenceRequirement {
            feature: "page.geometry".into(),
            origin: Some(transition.identity.page_id.into_canonical()),
            property_path: Some("page.size".into()),
        },
        PersistenceRequirement {
            feature: "document.page_membership".into(),
            origin: Some(transition.document_id.into_canonical()),
            property_path: Some("document.pages".into()),
        },
    ]
}

pub(super) fn display_page_append_error_v1(
    error: &EditorError,
    formatter: &mut fmt::Formatter<'_>,
) -> fmt::Result {
    match error {
        EditorError::PageAppendUnsupported { message } => {
            write!(formatter, "append blank page is unsupported: {message}")
        }
        EditorError::StalePageAppend => formatter.write_str(
            "current document/page membership no longer matches the append-page precondition",
        ),
        _ => unreachable!("page-append display helper receives only page-append errors"),
    }
}

pub(super) fn required_editor_asset_refs_v1(
    operations: &[EditOperation],
) -> BTreeSet<Sha256Digest> {
    operations
        .iter()
        .flat_map(EditOperation::durable_editor_asset_refs_v1)
        .collect()
}
