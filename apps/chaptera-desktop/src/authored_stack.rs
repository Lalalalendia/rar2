//! Desktop adapter from canonical Editor authored stacks into the viewer render lane.

use chaptera_viewer_render_plan::{
    AuthoredPageRenderLaneV1, AuthoredPageRenderNodeV1, PageRenderPlanV1, RenderSolidLineV1,
    apply_authored_page_render_lane_v1,
};

fn editor_authored_page_render_lane(
    editor: &pub_editor::EditorSession,
    page_id: pub_editor::PageId,
) -> Result<AuthoredPageRenderLaneV1, String> {
    let stack = editor
        .authored_stack(page_id)
        .ok_or_else(|| "authored lane page is absent from the editor graph".to_owned())?;
    let mut nodes = Vec::with_capacity(stack.members.len());

    for node_id in stack.members {
        let shape = editor
            .authored_shape(node_id)
            .ok_or_else(|| format!("authored lane member {node_id:?} has no authored shape"))?;
        if shape.page_id != page_id || shape.parent_id != page_id {
            return Err(format!(
                "authored lane member {node_id:?} does not belong directly to page {page_id:?}"
            ));
        }
        if shape.provenance != pub_editor::AuthoredEntityProvenanceV1::AuthorCreated
            || shape.paint.provenance != pub_editor::AuthoredEntityProvenanceV1::AuthorCreated
        {
            return Err(format!(
                "authored lane member {node_id:?} is not canonically AuthorCreated"
            ));
        }

        nodes.push(AuthoredPageRenderNodeV1 {
            node_id,
            bounds: shape.bounds,
            solid_fill_rgb: shape.paint.fill.visible.then_some([
                shape.paint.fill.color.r,
                shape.paint.fill.color.g,
                shape.paint.fill.color.b,
            ]),
            solid_line: shape.paint.stroke.visible.then_some(RenderSolidLineV1 {
                rgb: [
                    shape.paint.stroke.color.r,
                    shape.paint.stroke.color.g,
                    shape.paint.stroke.color.b,
                ],
                width_emu: shape.paint.stroke.width_emu,
            }),
        });
    }

    Ok(AuthoredPageRenderLaneV1 { page_id, nodes })
}

pub(super) fn apply_editor_authored_page_lane(
    plan: &mut PageRenderPlanV1,
    editor: &pub_editor::EditorSession,
) -> Result<(), String> {
    let lane = editor_authored_page_render_lane(editor, plan.page_id)?;
    apply_authored_page_render_lane_v1(plan, &lane).map_err(|error| error.to_string())
}
