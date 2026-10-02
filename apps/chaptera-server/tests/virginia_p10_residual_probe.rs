use std::{collections::BTreeMap, env, fs, path::PathBuf};

use chaptera_server::reader_scene_v1::{ReaderSceneV1, from_viewer_geometry};
use pub_viewer::{ViewerOpenBundle, open_pub_bundle, viewer_geometry_environment_v0_1};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const EXPECTED_SHA256: &str =
    "88f57d800aeec808798ea487b9d4ab85dc85c02cd190b987a332709b81018506";
const PAGES: [u32; 3] = [9, 10, 11];

fn bump(map: &mut BTreeMap<String, usize>, key: impl Into<String>) {
    *map.entry(key.into()).or_default() += 1;
}

fn source_page_census(bundle: &ViewerOpenBundle, one_based_page: u32) -> Value {
    let page = bundle
        .geometry
        .document
        .pages
        .iter()
        .find(|page| page.index == one_based_page)
        .expect("selected Viewer page exists");
    let parent = page.id.into_canonical();
    let nodes = bundle
        .resolved_graph
        .nodes
        .values()
        .filter(|node| node.header.parent_id == parent)
        .collect::<Vec<_>>();

    let source_order = bundle
        .source_page_paint_orders
        .iter()
        .find(|order| order.page_id == page.id);

    let mut family_histogram = BTreeMap::<String, usize>::new();
    let mut provenance_histogram = BTreeMap::<String, usize>::new();
    let mut fill_histogram = BTreeMap::<String, usize>::new();
    let mut line_histogram = BTreeMap::<String, usize>::new();
    let mut positive_bounds = 0usize;
    let mut source_order_covered = 0usize;

    for node in &nodes {
        let family = if node.payload.table.is_some() {
            "table"
        } else if node.payload.image_slot.is_some() {
            "image"
        } else if node.payload.story_frame.is_some() {
            "story"
        } else {
            "other"
        };
        bump(&mut family_histogram, family);

        let grouped = node.header.source_refs.iter().any(|source| {
            source
                .object_key
                .as_deref()
                .is_some_and(|key| key.starts_with("escher/group-ancestor/"))
        });
        bump(
            &mut provenance_histogram,
            if grouped { "grouped" } else { "direct" },
        );

        positive_bounds += usize::from(
            node.header.bounds.width.get() > 0 && node.header.bounds.height.get() > 0,
        );
        source_order_covered += usize::from(
            source_order.is_some_and(|order| order.node_ids.contains(&node.header.id)),
        );

        let Some(paint) = node.payload.effective_paint.as_ref() else {
            bump(&mut fill_histogram, "paint_absent");
            bump(&mut line_histogram, "paint_absent");
            continue;
        };

        let fill = &paint.fill;
        let fill_state = match (
            fill.solid.as_ref(),
            fill.color_rgb.as_ref(),
            fill.visible.as_ref(),
        ) {
            (Some(solid), Some(_), Some(visible)) if solid.value && visible.value => {
                "complete_visible_solid"
            }
            (Some(solid), Some(_), Some(visible)) if solid.value && !visible.value => {
                "complete_hidden_solid"
            }
            (Some(solid), _, _) if !solid.value => "non_solid",
            _ if fill.solid.is_some() || fill.color_rgb.is_some() || fill.visible.is_some() => {
                "incomplete"
            }
            _ => "absent",
        };
        bump(&mut fill_histogram, fill_state);

        let line = &paint.line;
        let line_state = match (
            line.color_rgb.as_ref(),
            line.width_emu.as_ref(),
            line.visible.as_ref(),
        ) {
            (Some(_), Some(width), Some(visible)) if width.value > 0 && visible.value => {
                "complete_visible"
            }
            (Some(_), Some(width), Some(visible)) if width.value > 0 && !visible.value => {
                "complete_hidden"
            }
            _ if line.color_rgb.is_some() || line.width_emu.is_some() || line.visible.is_some() => {
                "incomplete"
            }
            _ => "absent",
        };
        bump(&mut line_histogram, line_state);
    }

    let tables = bundle
        .geometry
        .tables
        .iter()
        .filter(|table| nodes.iter().any(|node| node.header.id == table.node_id))
        .collect::<Vec<_>>();
    let table_cell_count = tables.iter().map(|table| table.cells.len()).sum::<usize>();
    let table_painted_cell_count = tables
        .iter()
        .flat_map(|table| table.cells.iter())
        .filter(|cell| cell.fill_rgb.is_some() && cell.fill_visible == Some(true))
        .count();

    let images = bundle
        .geometry
        .images
        .iter()
        .filter(|image| {
            image
                .node_ids
                .iter()
                .any(|id| nodes.iter().any(|node| node.header.id == *id))
        })
        .collect::<Vec<_>>();

    json!({
        "node_count": nodes.len(),
        "family_histogram": family_histogram,
        "provenance_histogram": provenance_histogram,
        "positive_bounds": positive_bounds,
        "source_order_covered": source_order_covered,
        "source_order_count": source_order.map_or(0, |order| order.node_ids.len()),
        "fill_histogram": fill_histogram,
        "line_histogram": line_histogram,
        "viewer_table_count": tables.len(),
        "viewer_table_cell_count": table_cell_count,
        "viewer_table_painted_cell_count": table_painted_cell_count,
        "viewer_embedded_image_resource_count": images.len(),
    })
}

fn reader_page_census(scene: &ReaderSceneV1, one_based_page: u32) -> Value {
    let page = scene
        .pages
        .iter()
        .find(|page| page.order + 1 == one_based_page)
        .expect("selected Reader page exists");
    let nodes = scene
        .nodes
        .iter()
        .filter(|node| node.page_id == page.page_id)
        .collect::<Vec<_>>();

    let mut kind_histogram = BTreeMap::<String, usize>::new();
    let mut paint_histogram = BTreeMap::<String, usize>::new();
    let mut text_layout_histogram = BTreeMap::<String, usize>::new();
    let mut positive_bounds = 0usize;
    let mut picture_resource_bound = 0usize;
    let mut text_nodes = 0usize;
    let mut table_cell_count = 0usize;
    let mut table_painted_cell_count = 0usize;

    for node in &nodes {
        bump(&mut kind_histogram, node.kind);
        positive_bounds += usize::from(node.bounds.width > 0 && node.bounds.height > 0);

        let paint_state = match node.paint.as_ref() {
            Some(paint) if paint.fill_rgb.is_some() && paint.line.is_some() => "fill_and_line",
            Some(paint) if paint.fill_rgb.is_some() => "fill_only",
            Some(paint) if paint.line.is_some() => "line_only",
            Some(_) => "empty",
            None => "absent",
        };
        bump(&mut paint_histogram, paint_state);

        if node.kind == "picture_frame" && node.resource_id.is_some() {
            picture_resource_bound += 1;
        }

        if node.text.is_some() {
            text_nodes += 1;
            bump(
                &mut text_layout_histogram,
                node.text_layout
                    .as_ref()
                    .map(|layout| layout.disposition)
                    .unwrap_or("text_without_layout"),
            );
        }

        if let Some(table) = node.table.as_ref() {
            table_cell_count += table.cells.len();
            table_painted_cell_count += table
                .cells
                .iter()
                .filter(|cell| cell.fill_rgb.is_some() && cell.fill_visible == Some(true))
                .count();
        }
    }

    let resource_availability_histogram = scene
        .resources
        .iter()
        .filter(|resource| {
            nodes.iter().any(|node| {
                node.resource_id
                    .as_deref()
                    .is_some_and(|resource_id| resource_id == resource.resource_id)
            })
        })
        .fold(BTreeMap::<String, usize>::new(), |mut acc, resource| {
            bump(&mut acc, resource.availability);
            acc
        });

    let node_ids = nodes
        .iter()
        .map(|node| node.node_id.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    let mut diagnostic_histogram = BTreeMap::<String, usize>::new();
    for diagnostic in &scene.diagnostics {
        if diagnostic
            .origin_id
            .as_deref()
            .is_some_and(|origin| node_ids.contains(origin))
        {
            bump(&mut diagnostic_histogram, diagnostic.code.as_str());
        }
    }

    json!({
        "node_count": nodes.len(),
        "kind_histogram": kind_histogram,
        "positive_bounds": positive_bounds,
        "paint_histogram": paint_histogram,
        "picture_resource_bound": picture_resource_bound,
        "resource_availability_histogram": resource_availability_histogram,
        "text_nodes": text_nodes,
        "text_layout_histogram": text_layout_histogram,
        "table_cell_count": table_cell_count,
        "table_painted_cell_count": table_painted_cell_count,
        "diagnostic_histogram": diagnostic_histogram,
    })
}

#[test]
#[ignore = "requires exact public Virginia Remplacante fixture"]
fn exact_virginia_p10_product_boundary_census() {
    let fixture = PathBuf::from(
        env::var_os("CHAPTERA_VIRGINIA_P10_FIXTURE").expect("CHAPTERA_VIRGINIA_P10_FIXTURE"),
    );
    let output = PathBuf::from(
        env::var_os("CHAPTERA_VIRGINIA_P10_OUT").expect("CHAPTERA_VIRGINIA_P10_OUT"),
    );

    let bytes = fs::read(&fixture).expect("read exact Virginia source");
    let actual_sha = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    assert_eq!(actual_sha, EXPECTED_SHA256, "exact Virginia source identity");

    let bundle = open_pub_bundle(&bytes, viewer_geometry_environment_v0_1())
        .expect("open exact Virginia Viewer bundle");
    let scene = from_viewer_geometry(
        "probe:virginia-remplacante".to_owned(),
        actual_sha.clone(),
        "probe:p10-residual".to_owned(),
        &bundle.geometry,
        &bundle.source_page_paint_orders,
    )
    .expect("project exact Virginia through ReaderScene");

    let pages = PAGES
        .iter()
        .map(|page| {
            json!({
                "viewer_page": page,
                "source_viewer": source_page_census(&bundle, *page),
                "reader": reader_page_census(&scene, *page),
            })
        })
        .collect::<Vec<_>>();

    let receipt = json!({
        "schema": "chaptera.virginia-p10-product-boundary.v1",
        "target_viewer_page": 10,
        "control_viewer_pages": [9, 11],
        "source_sha256": actual_sha,
        "pages": pages,
        "claims": {
            "publisher_pdf_used_for_semantics": false,
            "story_text_emitted": false,
            "node_ids_emitted": false,
            "coordinates_emitted": false,
            "colors_emitted": false,
            "raw_property_values_emitted": false,
        }
    });

    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).expect("create receipt directory");
    }
    fs::write(
        &output,
        serde_json::to_vec_pretty(&receipt).expect("serialize p10 receipt"),
    )
    .expect("write p10 receipt");

    println!(
        "VIRGINIA_P10_PRODUCT_BOUNDARY {}",
        serde_json::to_string(&receipt).expect("serialize p10 log receipt")
    );
}
