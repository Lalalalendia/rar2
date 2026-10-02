use std::{collections::BTreeSet, env, fs, path::PathBuf};

use chaptera_server::reader_scene_v1::from_viewer_geometry;
use pub_editor::RectEmu;
use pub_viewer::{open_pub_bundle, viewer_geometry_environment_v0_1};
use serde::Serialize;
use sha2::{Digest, Sha256};

const EXPECTED_REMPLACANTE_SHA256: &str =
    "88f57d800aeec808798ea487b9d4ab85dc85c02cd190b987a332709b81018506";

#[derive(Debug, Serialize)]
struct Receipt {
    schema: &'static str,
    viewer_page: u32,
    later_overlapping_image_count: usize,
    overlapping_story_count: usize,
    reader_overlapping_story_node_count: usize,
    reader_story_before_image_count: usize,
    reader_source_back_to_front: bool,
    reader_picture_node_count: usize,
    reader_resource_bound_node_count: usize,
    reader_positive_bounds_count: usize,
    reader_source_window_count: usize,
    reader_valid_source_window_count: usize,
    reader_bound_resource_descriptor_count: usize,
    reader_bound_resource_inline_count: usize,
    claims: Claims,
}

#[derive(Debug, Serialize)]
struct Claims {
    exact_source_identity_checked: bool,
    node_ids_emitted: bool,
    resource_ids_emitted: bool,
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

fn serialized_string<T: Serialize>(value: &T) -> String {
    let value = serde_json::to_value(value).expect("serialize canonical id");
    value
        .as_str()
        .expect("canonical id must serialize as string")
        .to_owned()
}

#[test]
#[ignore = "requires exact public Virginia Remplacante fixture and receipt path"]
fn exact_virginia_p24_later_image_reader_binding_probe() {
    let fixture = PathBuf::from(
        env::var("CHAPTERA_VIRGINIA_RESTORED_FILL_FIXTURE")
            .expect("CHAPTERA_VIRGINIA_RESTORED_FILL_FIXTURE"),
    );
    let output = PathBuf::from(
        env::var("CHAPTERA_VIRGINIA_OCCLUSION_READER_OUT")
            .expect("CHAPTERA_VIRGINIA_OCCLUSION_READER_OUT"),
    );

    let bytes = fs::read(&fixture).expect("read exact Virginia Remplacante PUB");
    let actual_sha = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    assert_eq!(
        actual_sha, EXPECTED_REMPLACANTE_SHA256,
        "exact Virginia source identity drift"
    );

    let bundle = open_pub_bundle(&bytes, viewer_geometry_environment_v0_1())
        .expect("open exact Virginia Remplacante through Viewer bundle");

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
    let mut later_images = BTreeSet::new();
    let mut overlapping_stories = BTreeSet::new();

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
            overlapping_stories.insert(node_id);
        }
    }

    assert_eq!(
        later_images.len(),
        1,
        "Stage-M Reader binding probe expects the unique later p24 image"
    );

    let later_image_ids = later_images
        .iter()
        .map(serialized_string)
        .collect::<BTreeSet<_>>();
    let overlapping_story_ids = overlapping_stories
        .iter()
        .map(serialized_string)
        .collect::<BTreeSet<_>>();

    let scene = from_viewer_geometry(
        "probe:virginia-remplacante".to_owned(),
        actual_sha,
        "probe:p24-occluder".to_owned(),
        &bundle.geometry,
        &bundle.source_page_paint_orders,
    )
    .expect("project exact Virginia Remplacante through ReaderScene boundary");

    let reader_nodes = scene
        .nodes
        .iter()
        .filter(|node| later_image_ids.contains(&node.node_id))
        .collect::<Vec<_>>();

    let later_image_scene_positions = scene
        .nodes
        .iter()
        .enumerate()
        .filter(|(_, node)| later_image_ids.contains(&node.node_id))
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    let reader_overlapping_story_node_count = scene
        .nodes
        .iter()
        .filter(|node| overlapping_story_ids.contains(&node.node_id))
        .count();
    let reader_story_before_image_count = later_image_scene_positions
        .first()
        .map(|image_position| {
            scene
                .nodes
                .iter()
                .take(*image_position)
                .filter(|node| overlapping_story_ids.contains(&node.node_id))
                .count()
        })
        .unwrap_or(0);

    let reader_picture_node_count = reader_nodes
        .iter()
        .filter(|node| node.kind == "picture_frame")
        .count();
    let reader_resource_bound_node_count = reader_nodes
        .iter()
        .filter(|node| node.resource_id.is_some())
        .count();
    let reader_positive_bounds_count = reader_nodes
        .iter()
        .filter(|node| node.bounds.width > 0 && node.bounds.height > 0)
        .count();
    let reader_source_window_count = reader_nodes
        .iter()
        .filter(|node| node.image_source_window.is_some())
        .count();
    let reader_valid_source_window_count = reader_nodes
        .iter()
        .filter(|node| {
            node.image_source_window.as_ref().is_some_and(|window| {
                window.right_q16 > window.left_q16 && window.bottom_q16 > window.top_q16
            })
        })
        .count();

    let bound_resource_ids = reader_nodes
        .iter()
        .filter_map(|node| node.resource_id.as_deref())
        .collect::<BTreeSet<_>>();
    let reader_bound_resource_descriptor_count = scene
        .resources
        .iter()
        .filter(|resource| bound_resource_ids.contains(resource.resource_id.as_str()))
        .count();
    let reader_bound_resource_inline_count = scene
        .resources
        .iter()
        .filter(|resource| {
            bound_resource_ids.contains(resource.resource_id.as_str())
                && resource.inline_data_url.is_some()
        })
        .count();

    let receipt = Receipt {
        schema: "chaptera.virginia-p24-occluder-reader-binding.v1",
        viewer_page,
        later_overlapping_image_count: later_images.len(),
        overlapping_story_count: overlapping_stories.len(),
        reader_overlapping_story_node_count,
        reader_story_before_image_count,
        reader_source_back_to_front: scene.stacking_fidelity == "source_back_to_front",
        reader_picture_node_count,
        reader_resource_bound_node_count,
        reader_positive_bounds_count,
        reader_source_window_count,
        reader_valid_source_window_count,
        reader_bound_resource_descriptor_count,
        reader_bound_resource_inline_count,
        claims: Claims {
            exact_source_identity_checked: true,
            node_ids_emitted: false,
            resource_ids_emitted: false,
            coordinates_emitted: false,
            colors_emitted: false,
            text_emitted: false,
            pdf_used_as_semantic_authority: false,
        },
    };

    assert!(
        receipt.reader_source_back_to_front,
        "ReaderScene must retain the bounded source back-to-front stacking claim"
    );
    assert_eq!(
        receipt.reader_picture_node_count, 1,
        "later p24 image must survive as one Reader picture_frame"
    );
    assert_eq!(
        receipt.reader_overlapping_story_node_count, receipt.overlapping_story_count,
        "all source-overlapped Story nodes must survive into Reader Scene"
    );
    assert_eq!(
        receipt.reader_story_before_image_count, receipt.overlapping_story_count,
        "Reader Scene must preserve later-image ordering over the overlapped Story nodes"
    );
    assert_eq!(
        receipt.reader_resource_bound_node_count, 1,
        "later p24 image must retain one Reader resource binding"
    );
    assert_eq!(
        receipt.reader_positive_bounds_count, 1,
        "later p24 image must retain positive Reader bounds"
    );
    assert_eq!(
        receipt.reader_valid_source_window_count, receipt.reader_source_window_count,
        "any later p24 image source window must remain geometrically valid"
    );
    assert_eq!(
        receipt.reader_bound_resource_descriptor_count, 1,
        "later p24 image resource must exist in Reader resource descriptors"
    );

    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).expect("create Reader occlusion receipt directory");
    }
    fs::write(
        &output,
        serde_json::to_vec_pretty(&receipt).expect("serialize Reader occlusion receipt"),
    )
    .expect("write Reader occlusion receipt");

    println!(
        "VIRGINIA_P24_OCCLUDER_READER_BINDING later_images={} overlapping_stories={} reader_stories={} stories_before_image={} source_order={} picture_nodes={} resource_bound={} positive_bounds={} source_window={} valid_source_window={} descriptors={} inline={}",
        receipt.later_overlapping_image_count,
        receipt.overlapping_story_count,
        receipt.reader_overlapping_story_node_count,
        receipt.reader_story_before_image_count,
        receipt.reader_source_back_to_front,
        receipt.reader_picture_node_count,
        receipt.reader_resource_bound_node_count,
        receipt.reader_positive_bounds_count,
        receipt.reader_source_window_count,
        receipt.reader_valid_source_window_count,
        receipt.reader_bound_resource_descriptor_count,
        receipt.reader_bound_resource_inline_count,
    );
}
