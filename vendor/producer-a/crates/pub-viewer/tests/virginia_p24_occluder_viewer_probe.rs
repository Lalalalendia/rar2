use std::{collections::BTreeSet, env, fs, path::PathBuf};

use pub_model::RectEmu;
use pub_viewer::{open_pub_bundle, viewer_geometry_environment_v0_1};
use serde::Serialize;

#[derive(Debug, Serialize)]
struct Receipt {
    schema: &'static str,
    viewer_page: u32,
    candidate_story_fill_count: usize,
    later_overlapping_image_count: usize,
    scene_survivor_count: usize,
    embedded_image_materialized_count: usize,
    viewer_paint_entry_count: usize,
    viewer_visible_paint_count: usize,
    claims: Claims,
}

#[derive(Debug, Serialize)]
struct Claims {
    exact_source_identity_checked: bool,
    node_ids_emitted: bool,
    coordinates_emitted: bool,
    colors_emitted: bool,
    text_emitted: bool,
    pdf_used_as_semantic_authority: bool,
}

fn overlaps(a: RectEmu, b: RectEmu) -> bool {
    let (Some(ar), Some(ab), Some(br), Some(bb)) = (a.right(), a.bottom(), b.right(), b.bottom())
    else {
        return false;
    };
    a.x.get() < br.get() && b.x.get() < ar.get() && a.y.get() < bb.get() && b.y.get() < ab.get()
}

#[test]
#[ignore = "requires exact public Virginia Remplacante fixture and receipt path"]
fn exact_virginia_p24_later_image_viewer_survival_probe() {
    let fixture = PathBuf::from(
        env::var("CHAPTERA_VIRGINIA_RESTORED_FILL_FIXTURE")
            .expect("CHAPTERA_VIRGINIA_RESTORED_FILL_FIXTURE"),
    );
    let output = PathBuf::from(
        env::var("CHAPTERA_VIRGINIA_OCCLUSION_VIEWER_OUT")
            .expect("CHAPTERA_VIRGINIA_OCCLUSION_VIEWER_OUT"),
    );
    let expected_sha = env::var("CHAPTERA_VIRGINIA_RESTORED_FILL_SHA256")
        .expect("CHAPTERA_VIRGINIA_RESTORED_FILL_SHA256");

    let bytes = fs::read(&fixture).expect("read exact Virginia Remplacante PUB");
    let bundle = open_pub_bundle(&bytes, viewer_geometry_environment_v0_1())
        .expect("open exact Virginia Remplacante through Viewer bundle");
    assert_eq!(
        bundle.geometry.document.source.source_hash.to_string(),
        expected_sha,
        "exact Virginia source identity"
    );

    let viewer_page = 24_u32;
    let page = bundle
        .geometry
        .document
        .pages
        .get((viewer_page - 1) as usize)
        .expect("Viewer p24 exists");
    let order = bundle
        .source_page_paint_orders
        .iter()
        .find(|order| order.page_id == page.id)
        .expect("Viewer p24 has bounded source paint order");

    let graph = &bundle.resolved_graph;
    let mut candidate_story_fill_count = 0_usize;
    let mut later_images = BTreeSet::new();

    for (rank, node_id) in order.node_ids.iter().copied().enumerate() {
        let Some(node) = graph.nodes.get(&node_id) else {
            continue;
        };
        if node.payload.story_frame.is_none() {
            continue;
        }
        let Some(paint) = node.payload.effective_paint.as_ref() else {
            continue;
        };
        let complete_visible_solid = paint.fill.solid.as_ref().is_some_and(|value| value.value)
            && paint.fill.visible.as_ref().is_some_and(|value| value.value)
            && paint.fill.color_rgb.is_some();
        if !complete_visible_solid {
            continue;
        }
        candidate_story_fill_count += 1;

        for later_id in order.node_ids.iter().copied().skip(rank + 1) {
            let Some(later) = graph.nodes.get(&later_id) else {
                continue;
            };
            if later.payload.image_slot.is_none()
                || !overlaps(node.header.bounds, later.header.bounds)
            {
                continue;
            }
            later_images.insert(later_id);
        }
    }

    let scene_survivor_count = later_images
        .iter()
        .filter(|node_id| {
            bundle
                .geometry
                .scene
                .nodes
                .iter()
                .any(|node| node.origin == **node_id)
        })
        .count();

    let embedded_image_materialized_count = later_images
        .iter()
        .filter(|node_id| {
            bundle
                .geometry
                .images
                .iter()
                .any(|image| image.node_ids.contains(node_id))
        })
        .count();

    let viewer_paint_entry_count = later_images
        .iter()
        .filter(|node_id| {
            bundle
                .geometry
                .paints
                .iter()
                .any(|paint| paint.node_id == **node_id)
        })
        .count();

    let viewer_visible_paint_count = later_images
        .iter()
        .filter(|node_id| {
            bundle.geometry.paints.iter().any(|paint| {
                paint.node_id == **node_id
                    && (paint.solid_fill_rgb.is_some() || paint.solid_line.is_some())
            })
        })
        .count();

    let receipt = Receipt {
        schema: "chaptera.virginia-p24-occluder-viewer-survival.v1",
        viewer_page,
        candidate_story_fill_count,
        later_overlapping_image_count: later_images.len(),
        scene_survivor_count,
        embedded_image_materialized_count,
        viewer_paint_entry_count,
        viewer_visible_paint_count,
        claims: Claims {
            exact_source_identity_checked: true,
            node_ids_emitted: false,
            coordinates_emitted: false,
            colors_emitted: false,
            text_emitted: false,
            pdf_used_as_semantic_authority: false,
        },
    };

    assert!(
        receipt.later_overlapping_image_count > 0,
        "p24 must retain the Stage-L later-overlapping image discriminator"
    );

    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).expect("create Viewer occlusion receipt directory");
    }
    fs::write(
        &output,
        serde_json::to_vec_pretty(&receipt).expect("serialize Viewer occlusion receipt"),
    )
    .expect("write Viewer occlusion receipt");

    println!(
        "VIRGINIA_P24_OCCLUDER_VIEWER_SURVIVAL candidates={} later_images={} scene={} embedded={} paint={} visible_paint={}",
        receipt.candidate_story_fill_count,
        receipt.later_overlapping_image_count,
        receipt.scene_survivor_count,
        receipt.embedded_image_materialized_count,
        receipt.viewer_paint_entry_count,
        receipt.viewer_visible_paint_count,
    );
}
