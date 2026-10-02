use std::{collections::BTreeMap, env, fs, path::PathBuf};

use pub_viewer::{open_pub_bundle, viewer_geometry_environment_v0_1};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const EXPECTED_REMPLACANTE_SHA256: &str =
    "88f57d800aeec808798ea487b9d4ab85dc85c02cd190b987a332709b81018506";
const TARGET_VIEWER_PAGES: [u32; 3] = [21, 22, 23];

fn bump(map: &mut BTreeMap<String, usize>, key: impl Into<String>) {
    *map.entry(key.into()).or_default() += 1;
}

fn page_table_census(bundle: &pub_viewer::ViewerOpenBundle, one_based_page: u32) -> Value {
    let page = bundle
        .geometry
        .document
        .pages
        .iter()
        .find(|page| page.index == one_based_page)
        .unwrap_or_else(|| panic!("Viewer page {one_based_page} must exist"));
    let page_canonical = page.id.into_canonical();

    let mut table_nodes = 0_usize;
    let mut provenance = BTreeMap::<String, usize>::new();
    let mut simple_table = BTreeMap::<String, usize>::new();
    let mut layout_metrics = BTreeMap::<String, usize>::new();
    let mut topology = BTreeMap::<String, usize>::new();
    let mut total_rows = 0_u64;
    let mut total_columns = 0_u64;
    let mut total_cells = 0_usize;
    let mut coordinate_complete_cells = 0_usize;
    let mut bounded_cells = 0_usize;
    let mut painted_cells = 0_usize;
    let mut spanning_cells = 0_usize;
    let mut tables_with_all_cell_bounds = 0_usize;
    let mut tables_with_no_cell_bounds = 0_usize;
    let mut tables_with_partial_cell_bounds = 0_usize;

    for node in bundle
        .resolved_graph
        .nodes
        .values()
        .filter(|node| node.header.parent_id == page_canonical)
    {
        let Some(table) = node.payload.table.as_ref() else {
            continue;
        };
        table_nodes += 1;

        let grouped = node.header.source_refs.iter().any(|source| {
            source
                .object_key
                .as_deref()
                .is_some_and(|key| key.starts_with("escher/group-ancestor/"))
        });
        bump(&mut provenance, if grouped { "grouped" } else { "direct" });

        bump(
            &mut simple_table,
            if table.simple_table.is_some() {
                "present"
            } else {
                "absent"
            },
        );
        bump(
            &mut layout_metrics,
            if table.layout_metrics.is_some() {
                "present"
            } else {
                "absent"
            },
        );

        total_rows += u64::from(table.rows);
        total_columns += u64::from(table.columns);
        total_cells += table.cells.len();

        let mut table_bounded = 0_usize;
        let mut table_coordinate_complete = 0_usize;
        let mut table_spanning = 0_usize;
        let mut table_painted = 0_usize;

        for cell in &table.cells {
            if let Some(coordinates) = cell.coordinates {
                table_coordinate_complete += 1;
                if coordinates.end_row > coordinates.start_row
                    || coordinates.end_column > coordinates.start_column
                {
                    table_spanning += 1;
                }
            }
            table_bounded += usize::from(cell.bounds.is_some());
            table_painted += usize::from(cell.paint.is_some());
        }

        coordinate_complete_cells += table_coordinate_complete;
        bounded_cells += table_bounded;
        spanning_cells += table_spanning;
        painted_cells += table_painted;

        if table_bounded == 0 {
            tables_with_no_cell_bounds += 1;
        } else if table_bounded == table.cells.len() {
            tables_with_all_cell_bounds += 1;
        } else {
            tables_with_partial_cell_bounds += 1;
        }

        let topology_class = if table.cells.is_empty() {
            "empty_cells"
        } else if table_coordinate_complete != table.cells.len() {
            "coordinates_incomplete"
        } else if table_spanning > 0 {
            "spanning_or_merged"
        } else if table.simple_table.is_some() {
            "simple_unmerged"
        } else {
            "complete_non_simple"
        };
        bump(&mut topology, topology_class);
    }

    let materialization_class = if table_nodes == 0 {
        "no_tables"
    } else if bounded_cells == total_cells && total_cells > 0 {
        "cell_rectangles_complete"
    } else if bounded_cells == 0 {
        "cell_rectangles_absent"
    } else {
        "cell_rectangles_partial"
    };

    json!({
        "viewer_page": one_based_page,
        "table_nodes": table_nodes,
        "provenance": provenance,
        "simple_table": simple_table,
        "layout_metrics": layout_metrics,
        "topology": topology,
        "total_rows": total_rows,
        "total_columns": total_columns,
        "total_cells": total_cells,
        "coordinate_complete_cells": coordinate_complete_cells,
        "bounded_cells": bounded_cells,
        "spanning_cells": spanning_cells,
        "painted_cells": painted_cells,
        "tables_with_all_cell_bounds": tables_with_all_cell_bounds,
        "tables_with_no_cell_bounds": tables_with_no_cell_bounds,
        "tables_with_partial_cell_bounds": tables_with_partial_cell_bounds,
        "materialization_class": materialization_class,
    })
}

#[test]
#[ignore = "requires exact public Virginia Remplacante fixture"]
fn exact_virginia_p22_table_layout_census() {
    let fixture = PathBuf::from(
        env::var("CHAPTERA_VIRGINIA_TABLE_LAYOUT_PUB")
            .expect("CHAPTERA_VIRGINIA_TABLE_LAYOUT_PUB must name the exact public fixture"),
    );
    let receipt_path = PathBuf::from(
        env::var("CHAPTERA_VIRGINIA_TABLE_LAYOUT_RECEIPT")
            .expect("CHAPTERA_VIRGINIA_TABLE_LAYOUT_RECEIPT must name the sanitized receipt"),
    );

    let bytes = fs::read(&fixture).expect("read exact public Virginia Remplacante fixture");
    let actual_sha256 = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    assert_eq!(
        actual_sha256, EXPECTED_REMPLACANTE_SHA256,
        "exact Virginia Remplacante source identity drift"
    );

    let bundle = open_pub_bundle(&bytes, viewer_geometry_environment_v0_1())
        .expect("open exact Virginia Remplacante through shared Viewer bundle");
    assert!(
        bundle.geometry.document.pages.len() >= 23,
        "p21-p23 table census requires at least 23 admitted Viewer pages"
    );

    let pages = TARGET_VIEWER_PAGES
        .iter()
        .map(|page| page_table_census(&bundle, *page))
        .collect::<Vec<_>>();

    let receipt = json!({
        "schema": "chaptera.viewer-virginia-table-layout-probe.v1",
        "target_viewer_page": 22,
        "control_viewer_pages": [21, 23],
        "pages": pages,
        "claims": {
            "exact_public_source_identity_checked": true,
            "publisher_pdf_used_for_semantics": false,
            "cell_text_emitted": false,
            "object_ids_emitted": false,
            "coordinates_emitted": false,
            "raw_property_values_emitted": false,
        },
    });

    if let Some(parent) = receipt_path.parent() {
        fs::create_dir_all(parent).expect("create Virginia table-layout receipt directory");
    }
    fs::write(
        &receipt_path,
        serde_json::to_vec_pretty(&receipt).expect("serialize Virginia table-layout receipt"),
    )
    .expect("write Virginia table-layout receipt");

    println!(
        "VIRGINIA_TABLE_LAYOUT_PROBE {}",
        serde_json::to_string(&receipt).expect("serialize Virginia table-layout receipt for log")
    );
}
