use pub_viewer::{open_pub_bundle, viewer_geometry_environment_v0_1};
use serde::Serialize;
use std::{collections::BTreeSet, env, fs, path::PathBuf};

#[derive(Debug, Serialize)]
struct PageReceipt {
    page: u32,
    scene_nodes: usize,
    image_bound_nodes: usize,
    admitted_recolor_placements: usize,
}

#[derive(Debug, Serialize)]
struct FixtureReceipt {
    fixture: &'static str,
    pages: Vec<PageReceipt>,
}

fn census(fixture_name: &'static str, path: PathBuf) -> FixtureReceipt {
    let bytes = fs::read(&path).expect("read exact fixture");
    let bundle =
        open_pub_bundle(&bytes, viewer_geometry_environment_v0_1()).expect("open Viewer bundle");

    let image_bound_nodes = bundle
        .geometry
        .images
        .iter()
        .flat_map(|image| image.node_ids.iter().copied())
        .collect::<BTreeSet<_>>();
    let viewer_recolor_nodes = bundle
        .geometry
        .images
        .iter()
        .flat_map(|image| image.placements.iter())
        .filter(|placement| placement.recolor.is_some())
        .map(|placement| placement.node_id)
        .collect::<BTreeSet<_>>();

    let mut pages = Vec::new();
    for page in &bundle.geometry.document.pages {
        let page_origin = page.id.into_canonical();
        let scene_nodes = bundle
            .geometry
            .scene
            .nodes
            .iter()
            .filter(|node| node.parent_origin == page_origin)
            .map(|node| node.origin)
            .collect::<BTreeSet<_>>();

        let page_image_bound_nodes = scene_nodes
            .iter()
            .filter(|node_id| image_bound_nodes.contains(node_id))
            .count();
        let admitted_recolor_placements = scene_nodes
            .iter()
            .filter(|node_id| viewer_recolor_nodes.contains(node_id))
            .count();

        println!(
            "REFERENCE_RECOLOR_CENSUS fixture={} page={} scene={} image_bound={} admitted_recolor_placements={}",
            fixture_name,
            page.index,
            scene_nodes.len(),
            page_image_bound_nodes,
            admitted_recolor_placements,
        );

        pages.push(PageReceipt {
            page: page.index,
            scene_nodes: scene_nodes.len(),
            image_bound_nodes: page_image_bound_nodes,
            admitted_recolor_placements,
        });
    }

    FixtureReceipt {
        fixture: fixture_name,
        pages,
    }
}

#[test]
#[ignore = "requires exact public reference fixtures"]
fn exact_reference_recolor_admission_census() {
    let out = PathBuf::from(env::var_os("CHAPTERA_RECOLOR_CENSUS_OUT").expect("output path"));
    let fixtures = [
        (
            "CarltonMarch2026",
            PathBuf::from(env::var_os("CHAPTERA_RECOLOR_CARLTON").expect("Carlton fixture")),
        ),
        (
            "VirginiaDevinettes2021",
            PathBuf::from(env::var_os("CHAPTERA_RECOLOR_DEVINETTES").expect("Devinettes fixture")),
        ),
        (
            "VirginiaRemplacanteZoneA2015",
            PathBuf::from(env::var_os("CHAPTERA_RECOLOR_REMPLACANTE").expect("Remplacante fixture")),
        ),
    ];

    let receipts = fixtures
        .into_iter()
        .map(|(name, path)| census(name, path))
        .collect::<Vec<_>>();

    fs::create_dir_all(out.parent().expect("output parent")).expect("create output dir");
    fs::write(
        &out,
        serde_json::to_vec_pretty(&receipts).expect("serialize census"),
    )
    .expect("write census");
}


#[test]
fn recolor_census_receipt_json_shape_is_stable() {
    let receipt = FixtureReceipt {
        fixture: "Synthetic",
        pages: vec![PageReceipt {
            page: 2,
            scene_nodes: 7,
            image_bound_nodes: 3,
            admitted_recolor_placements: 1,
        }],
    };

    let value = serde_json::to_value(receipt).expect("serialize synthetic receipt");
    assert_eq!(value["fixture"], "Synthetic");
    assert_eq!(value["pages"][0]["page"], 2);
    assert_eq!(value["pages"][0]["scene_nodes"], 7);
    assert_eq!(value["pages"][0]["image_bound_nodes"], 3);
    assert_eq!(value["pages"][0]["admitted_recolor_placements"], 1);
}
