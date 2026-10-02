use std::{collections::BTreeSet, env, fs, path::PathBuf};

use pub_viewer::{open_pub_bundle, viewer_geometry_environment_v0_1};
use serde::Serialize;
use sha2::{Digest, Sha256};

const EXPECTED_SHA256: &str =
    "88f57d800aeec808798ea487b9d4ab85dc85c02cd190b987a332709b81018506";

#[derive(Debug, Serialize)]
struct Receipt {
    schema: &'static str,
    viewer_page: u32,
    source_image_count: usize,
    source_image_with_slot_count: usize,
    scene_survival_count: usize,
    viewer_embedded_image_coverage_count: usize,
    viewer_image_resource_count: usize,
    claims: Vec<&'static str>,
}

#[test]
#[ignore = "requires exact public Virginia Remplacante fixture"]
fn exact_virginia_p24_image_materialization_probe() {
    let fixture = PathBuf::from(
        env::var("CHAPTERA_VIRGINIA_P24_IMAGE_FIXTURE")
            .expect("CHAPTERA_VIRGINIA_P24_IMAGE_FIXTURE"),
    );
    let output = PathBuf::from(
        env::var("CHAPTERA_VIRGINIA_P24_IMAGE_OUT")
            .expect("CHAPTERA_VIRGINIA_P24_IMAGE_OUT"),
    );

    let bytes = fs::read(&fixture).expect("read exact Virginia source");
    let actual_sha = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    assert_eq!(actual_sha, EXPECTED_SHA256, "exact Virginia source identity");

    let bundle = open_pub_bundle(&bytes, viewer_geometry_environment_v0_1())
        .expect("open exact Virginia Viewer bundle");
    let page = bundle
        .geometry
        .document
        .pages
        .get(23)
        .expect("Viewer p24 exists");
    let parent = page.id.into_canonical();

    let source_images = bundle
        .resolved_graph
        .nodes
        .values()
        .filter(|node| node.header.parent_id == parent && node.payload.image_slot.is_some())
        .map(|node| node.header.id)
        .collect::<BTreeSet<_>>();

    let scene_origins = bundle
        .geometry
        .scene
        .nodes
        .iter()
        .map(|node| node.origin)
        .collect::<BTreeSet<_>>();
    let scene_survival_count = source_images
        .iter()
        .filter(|node_id| scene_origins.contains(node_id))
        .count();

    let viewer_image_nodes = bundle
        .geometry
        .images
        .iter()
        .flat_map(|image| image.node_ids.iter().copied())
        .collect::<BTreeSet<_>>();
    let viewer_embedded_image_coverage_count = source_images
        .iter()
        .filter(|node_id| viewer_image_nodes.contains(node_id))
        .count();

    let receipt = Receipt {
        schema: "chaptera.virginia-p24-image-materialization-probe.v1",
        viewer_page: 24,
        source_image_count: source_images.len(),
        source_image_with_slot_count: source_images.len(),
        scene_survival_count,
        viewer_embedded_image_coverage_count,
        viewer_image_resource_count: bundle.geometry.images.len(),
        claims: vec![
            "Counts only; source node identities and image-slot ordinals are not emitted.",
            "Publisher PDF is not used as image identity or materialization authority.",
            "This probe does not infer that every p24 source image is the Stage-L occluder unless the source-image count is exactly one.",
        ],
    };

    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).expect("create p24 image receipt directory");
    }
    fs::write(
        &output,
        serde_json::to_vec_pretty(&receipt).expect("serialize p24 image receipt"),
    )
    .expect("write p24 image receipt");

    println!(
        "VIRGINIA_P24_IMAGE source={} scene={} viewer_image_covered={} resources={}",
        receipt.source_image_count,
        receipt.scene_survival_count,
        receipt.viewer_embedded_image_coverage_count,
        receipt.viewer_image_resource_count,
    );
}
