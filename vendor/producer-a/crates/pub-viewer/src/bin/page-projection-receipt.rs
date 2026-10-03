use anyhow::{Context, Result};
use pub_viewer::{open_mature_0x2c_geometry, viewer_geometry_environment_v0_1};
use serde_json::json;
use std::{env, fs, path::PathBuf};

fn main() -> Result<()> {
    let mut args = env::args_os().skip(1);
    let source = PathBuf::from(
        args.next()
            .context("usage: page-projection-receipt SOURCE.pub OUTPUT.json")?,
    );
    let output = PathBuf::from(
        args.next()
            .context("usage: page-projection-receipt SOURCE.pub OUTPUT.json")?,
    );

    let bytes = fs::read(&source).with_context(|| format!("read {}", source.display()))?;
    let visual = open_mature_0x2c_geometry(&bytes, viewer_geometry_environment_v0_1())
        .context("open PUB through current Viewer page projection")?;

    let per_page = visual
        .document
        .pages
        .iter()
        .map(|page| {
            let page_parent = page.id.into_canonical();
            let direct_node_ids = visual
                .scene
                .nodes
                .iter()
                .filter(|node| node.parent_origin == page_parent)
                .map(|node| node.origin)
                .collect::<std::collections::BTreeSet<_>>();

            let direct_scene_node_count = direct_node_ids.len();
            let story_frame_count = visual
                .story_frames
                .iter()
                .filter(|frame| direct_node_ids.contains(&frame.frame_id))
                .count();
            let text_fragment_count = visual
                .text_fragments
                .iter()
                .filter(|fragment| direct_node_ids.contains(&fragment.frame_id))
                .count();
            let table_count = visual
                .tables
                .iter()
                .filter(|table| direct_node_ids.contains(&table.node_id))
                .count();
            let paint_count = visual
                .paints
                .iter()
                .filter(|paint| direct_node_ids.contains(&paint.node_id))
                .count();
            let image_node_use_count = visual
                .images
                .iter()
                .map(|image| {
                    image
                        .node_ids
                        .iter()
                        .filter(|node_id| direct_node_ids.contains(node_id))
                        .count()
                })
                .sum::<usize>();
            let image_placement_count = visual
                .images
                .iter()
                .map(|image| {
                    image
                        .placements
                        .iter()
                        .filter(|placement| direct_node_ids.contains(&placement.node_id))
                        .count()
                })
                .sum::<usize>();

            json!({
                "viewer_page_index": page.index,
                "document_ordinal_zero_based": page.index - 1,
                "direct_scene_node_count": direct_scene_node_count,
                "story_frame_count": story_frame_count,
                "text_fragment_count": text_fragment_count,
                "table_count": table_count,
                "paint_count": paint_count,
                "image_node_use_count": image_node_use_count,
                "image_placement_count": image_placement_count,
            })
        })
        .collect::<Vec<_>>();

    let receipt = json!({
        "schema": "chaptera.viewer-page-projection-receipt.v2",
        "viewer_page_count": visual.document.pages.len(),
        "viewer_page_indices": visual.document.pages.iter().map(|page| page.index).collect::<Vec<_>>(),
        "scene_surface_count": visual.scene.surfaces.len(),
        "per_page": per_page,
        "diagnostic_codes": visual.document.diagnostics.iter().map(|diagnostic| diagnostic.code.clone()).collect::<Vec<_>>(),
        "claims": {
            "page_identity_emitted": false,
            "node_identity_emitted": false,
            "story_text_emitted": false,
            "counts_are_direct_page_local_scene_membership_only": true,
        },
    });

    fs::write(
        &output,
        serde_json::to_vec_pretty(&receipt).context("serialize page projection receipt")?,
    )
    .with_context(|| format!("write {}", output.display()))?;
    Ok(())
}
