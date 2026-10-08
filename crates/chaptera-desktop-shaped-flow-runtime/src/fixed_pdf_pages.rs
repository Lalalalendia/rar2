use super::DesktopShapedFlowRuntimeError;
use pub_editor::EditorSession;
use pub_layout::{BoundedAuthoringSlice, BoundedGuideInput};
use pub_model::{PageId, PublisherGuideRole};
use std::collections::BTreeSet;

pub(crate) fn qualified_page_set_error_v1(
    page_ids: &[PageId],
) -> Result<(), DesktopShapedFlowRuntimeError> {
    if page_ids.is_empty() {
        return Err(DesktopShapedFlowRuntimeError::new(
            "qualified_pages_missing",
            "current fixed-PDF input requires at least one qualified customer page",
        ));
    }
    Ok(())
}

pub(crate) fn append_current_authored_ruler_guides_v1(
    editor: &EditorSession,
    authoring: &mut BoundedAuthoringSlice,
) -> Result<(), DesktopShapedFlowRuntimeError> {
    let guides = editor.current_authored_ruler_guides_v1().map_err(|error| {
        DesktopShapedFlowRuntimeError::new(
            "authoring_guide_projection_failed",
            format!("current authored ruler guides could not be resolved: {error}"),
        )
    })?;
    authoring
        .guides
        .extend(guides.into_iter().map(|authored| BoundedGuideInput {
            page_id: authored.page_id,
            guide: authored.guide,
            provenance: PublisherGuideRole::PageRulerGuide,
        }));
    Ok(())
}

pub(crate) fn bounded_authoring_slice_for_pages_v1(
    editor: &EditorSession,
    page_ids: &[PageId],
) -> Result<BoundedAuthoringSlice, DesktopShapedFlowRuntimeError> {
    qualified_page_set_error_v1(page_ids)?;
    let mut authoring =
        pub_viewer::bounded_authoring_slice_from_resolved(editor.graph()).map_err(|error| {
            DesktopShapedFlowRuntimeError::new(
                "authoring_projection_failed",
                format!("resolved graph could not enter bounded layout projection: {error}"),
            )
        })?;
    append_current_authored_ruler_guides_v1(editor, &mut authoring)?;

    let requested_pages = page_ids.iter().copied().collect::<BTreeSet<_>>();
    let available_pages = authoring
        .pages
        .iter()
        .map(|page| page.id)
        .collect::<BTreeSet<_>>();
    if let Some(page_id) = page_ids
        .iter()
        .find(|page_id| !available_pages.contains(page_id))
    {
        return Err(DesktopShapedFlowRuntimeError::new(
            "authoring_projection_failed",
            format!("layout projection missing document page {page_id:?}"),
        ));
    }

    authoring
        .pages
        .retain(|page| requested_pages.contains(&page.id));
    let requested_page_origins = page_ids
        .iter()
        .map(|page_id| page_id.into_canonical())
        .collect::<BTreeSet<_>>();
    authoring
        .node_geometry
        .retain(|node| requested_page_origins.contains(&node.parent_origin));
    let requested_nodes = authoring
        .node_geometry
        .iter()
        .map(|node| node.node_id)
        .collect::<BTreeSet<_>>();
    authoring
        .story_frames
        .retain(|frame| requested_nodes.contains(&frame.frame_id));
    authoring
        .tables
        .retain(|table| requested_nodes.contains(&table.node_id));
    authoring
        .guides
        .retain(|guide| requested_pages.contains(&guide.page_id));

    Ok(authoring)
}
