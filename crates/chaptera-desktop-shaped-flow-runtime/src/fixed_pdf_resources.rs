use super::{
    CURRENT_FIXED_PDF_RESOURCE_INPUT_V1, DesktopShapedFlowRuntimeError,
    ExplicitDesktopFontResourceV1, build_current_story_layout_with_pages_v1,
};
use crate::fixed_pdf_pages::qualified_page_set_error_v1;
use pub_editor::{EditorCurrentImageResourceV1, EditorSession};
use pub_layout::BoundedShapedFlowScene;
use pub_model::{NodeId, PageId, StoryId};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CurrentFixedPdfStrokeV1 {
    pub rgb: [u8; 3],
    pub width_emu: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CurrentFixedPdfNodePaintV1 {
    pub node_id: NodeId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill_rgb: Option<[u8; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stroke: Option<CurrentFixedPdfStrokeV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CurrentFixedPdfFontV1 {
    pub fingerprint_sha256: String,
    pub face_index: u32,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CurrentFixedPdfResourceInputV1 {
    pub protocol_version: String,
    pub binding: Value,
    pub shaped_flow: BoundedShapedFlowScene,
    pub node_paints: Vec<CurrentFixedPdfNodePaintV1>,
    pub image_resources: Vec<EditorCurrentImageResourceV1>,
    pub font: CurrentFixedPdfFontV1,
}

pub fn build_current_fixed_pdf_resource_input_v1(
    editor: &EditorSession,
    primary_story_id: StoryId,
    binding: Value,
    font: &ExplicitDesktopFontResourceV1<'_>,
) -> Result<CurrentFixedPdfResourceInputV1, DesktopShapedFlowRuntimeError> {
    let page_ids = editor.graph().document.pages.clone();
    build_current_fixed_pdf_resource_input_for_pages_v1(
        editor,
        primary_story_id,
        &page_ids,
        binding,
        font,
    )
}

/// Builds fixed-output resources against an already-qualified page projection.
///
/// The caller owns page-role qualification. This keeps current-revision output
/// aligned with the Reader/customer-visible page set instead of silently
/// widening back to every recovered raw PAGE.
pub fn build_current_fixed_pdf_resource_input_for_pages_v1(
    editor: &EditorSession,
    primary_story_id: StoryId,
    page_ids: &[PageId],
    binding: Value,
    font: &ExplicitDesktopFontResourceV1<'_>,
) -> Result<CurrentFixedPdfResourceInputV1, DesktopShapedFlowRuntimeError> {
    qualified_page_set_error_v1(page_ids)?;
    let layout = build_current_story_layout_with_pages_v1(
        editor,
        primary_story_id,
        "fixed-pdf:current-editor-state",
        font,
        Some(page_ids),
    )?;
    let image_resources = editor.current_image_resources_v1().map_err(|error| {
        DesktopShapedFlowRuntimeError::new(
            "current_image_resources_failed",
            format!("current Editor image resources could not be materialized: {error}"),
        )
    })?;

    Ok(CurrentFixedPdfResourceInputV1 {
        protocol_version: CURRENT_FIXED_PDF_RESOURCE_INPUT_V1.to_owned(),
        binding,
        shaped_flow: layout.shaped_flow,
        // Keep paint admission explicit. The Stage-0.2 real run will tell us
        // whether the exact Move/Resize targets require a paint resource seam.
        node_paints: Vec::new(),
        image_resources,
        font: CurrentFixedPdfFontV1 {
            fingerprint_sha256: layout.font_fingerprint_sha256,
            face_index: font.face_index,
            bytes: font.bytes.to_vec(),
        },
    })
}

// CI routing negative control: fixed-PDF resource owner only.
