use std::{collections::BTreeSet, env, fs, path::PathBuf};

use chaptera_server::reader_scene_v1::from_viewer_geometry;
use pub_viewer::{open_pub_bundle, viewer_geometry_environment_v0_1};
use serde::Serialize;

const ROUND_RECTANGLE: u16 = 0x0002;

fn canonical_string<T: Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .expect("serialize canonical id")
        .as_str()
        .expect("canonical id serializes as string")
        .to_owned()
}

#[test]
#[ignore = "requires exact public Virginia Devinettes fixture"]
fn exact_devinettes_roundrect_line_projection_probe() {
    let fixture = PathBuf::from(
        env::var("CHAPTERA_DEVINETTES_ROUNDRECT_PUB")
            .expect("CHAPTERA_DEVINETTES_ROUNDRECT_PUB"),
    );
    let expected_sha = env::var("CHAPTERA_DEVINETTES_ROUNDRECT_SHA256")
        .expect("CHAPTERA_DEVINETTES_ROUNDRECT_SHA256");

    let bytes = fs::read(&fixture).expect("read exact Devinettes fixture");
    let bundle = open_pub_bundle(&bytes, viewer_geometry_environment_v0_1())
        .expect("open exact Devinettes through Viewer bundle");
    assert_eq!(
        bundle.geometry.document.source.source_hash.to_string(),
        expected_sha,
        "exact Devinettes source identity drift"
    );

    let roundrect_ids = bundle
        .resolved_graph
        .nodes
        .values()
        .filter(|node| node.payload.officeart_shape_type == Some(ROUND_RECTANGLE))
        .map(|node| node.header.id)
        .collect::<BTreeSet<_>>();
    let roundrect_id_strings = roundrect_ids
        .iter()
        .map(canonical_string)
        .collect::<BTreeSet<_>>();

    let source_complete_visible_line = bundle
        .resolved_graph
        .nodes
        .values()
        .filter(|node| roundrect_ids.contains(&node.header.id))
        .filter(|node| {
            node.payload
                .effective_paint
                .as_ref()
                .is_some_and(|paint| {
                    paint.line.color_rgb.is_some()
                        && paint
                            .line
                            .width_emu
                            .as_ref()
                            .is_some_and(|width| width.value > 0)
                        && paint
                            .line
                            .visible
                            .as_ref()
                            .is_some_and(|visible| visible.value)
                })
        })
        .count();

    let viewer_scene_survivors = bundle
        .geometry
        .scene
        .nodes
        .iter()
        .filter(|node| roundrect_ids.contains(&node.origin))
        .count();
    let viewer_line_paints = bundle
        .geometry
        .paints
        .iter()
        .filter(|paint| roundrect_ids.contains(&paint.node_id) && paint.solid_line.is_some())
        .count();

    let scene = from_viewer_geometry(
        "probe:devinettes".to_owned(),
        expected_sha,
        "probe:roundrect-line".to_owned(),
        &bundle.geometry,
        &bundle.source_page_paint_orders,
    )
    .expect("project exact Devinettes through ReaderScene");

    let reader_survivors = scene
        .nodes
        .iter()
        .filter(|node| roundrect_id_strings.contains(&node.node_id))
        .count();
    let reader_line_nodes = scene
        .nodes
        .iter()
        .filter(|node| roundrect_id_strings.contains(&node.node_id))
        .filter(|node| node.paint.as_ref().and_then(|paint| paint.line.as_ref()).is_some())
        .count();

    println!(
        "DEVINETTES_ROUNDRECT_LINE source={} source_complete_visible={} viewer_scene={} viewer_line={} reader_scene={} reader_line={}",
        roundrect_ids.len(),
        source_complete_visible_line,
        viewer_scene_survivors,
        viewer_line_paints,
        reader_survivors,
        reader_line_nodes,
    );

    assert_eq!(roundrect_ids.len(), 24, "exact RoundRectangle cohort drift");
    assert_eq!(
        viewer_scene_survivors,
        roundrect_ids.len(),
        "every RoundRectangle must survive Viewer scene"
    );
    assert_eq!(
        reader_survivors,
        roundrect_ids.len(),
        "every RoundRectangle must survive Reader scene"
    );
}
