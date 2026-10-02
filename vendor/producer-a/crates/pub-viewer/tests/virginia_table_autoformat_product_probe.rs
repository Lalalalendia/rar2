use pub_viewer::{open_pub_bundle, viewer_geometry_environment_v0_1};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{env, fs, path::PathBuf};

const EXPECTED_SHA256: &str =
    "88f57d800aeec808798ea487b9d4ab85dc85c02cd190b987a332709b81018506";

#[derive(Debug, Serialize)]
struct PageReceipt {
    viewer_page_index: u32,
    table_count: usize,
    cell_count: usize,
    painted_cell_count: usize,
    unpainted_cell_count: usize,
}

#[derive(Debug, Serialize)]
struct Receipt {
    schema: &'static str,
    source_sha256: String,
    pages: Vec<PageReceipt>,
}

#[test]
#[ignore = "requires exact public Virginia Remplacante fixture"]
fn exact_virginia_table_autoformat_product_paint_census() {
    let fixture = PathBuf::from(
        env::var_os("CHAPTERA_VIRGINIA_TABLE_PRODUCT_FIXTURE")
            .expect("CHAPTERA_VIRGINIA_TABLE_PRODUCT_FIXTURE"),
    );
    let output = PathBuf::from(
        env::var_os("CHAPTERA_VIRGINIA_TABLE_PRODUCT_RECEIPT")
            .expect("CHAPTERA_VIRGINIA_TABLE_PRODUCT_RECEIPT"),
    );

    let bytes = fs::read(&fixture).expect("read exact Virginia Remplacante PUB");
    let actual_sha = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    assert_eq!(actual_sha, EXPECTED_SHA256, "exact Virginia source identity");

    let bundle = open_pub_bundle(&bytes, viewer_geometry_environment_v0_1())
        .expect("open exact Virginia through shared Viewer bundle");
    assert_eq!(
        bundle.geometry.document.pages.len(),
        25,
        "bounded Virginia family profile must expose 25 customer pages"
    );

    let mut pages = Vec::new();
    for viewer_page_index in [21_u32, 22_u32, 23_u32] {
        let layout_page = bundle
            .geometry
            .document
            .pages
            .get((viewer_page_index - 1) as usize)
            .expect("selected Viewer page exists");
        let parent = layout_page.id.into_canonical();

        let mut table_count = 0_usize;
        let mut cell_count = 0_usize;
        let mut painted_cell_count = 0_usize;

        for table in bundle
            .resolved_graph
            .nodes
            .values()
            .filter(|node| node.header.parent_id == parent)
            .filter_map(|node| node.payload.table.as_ref())
        {
            table_count += 1;
            cell_count += table.cells.len();
            painted_cell_count += table.cells.iter().filter(|cell| cell.paint.is_some()).count();
        }

        pages.push(PageReceipt {
            viewer_page_index,
            table_count,
            cell_count,
            painted_cell_count,
            unpainted_cell_count: cell_count - painted_cell_count,
        });
    }

    let expected = [(21_u32, 36_usize, 16_usize), (22, 110, 100), (23, 42, 9)];
    for (page_index, expected_cells, expected_painted) in expected {
        let page = pages
            .iter()
            .find(|page| page.viewer_page_index == page_index)
            .expect("selected page receipt");
        assert_eq!(page.cell_count, expected_cells, "exact TABLE cell cohort");
        assert_eq!(
            page.painted_cell_count, expected_painted,
            "product bridge must admit exactly the source-backed native T840 carrier cohort"
        );
        assert_eq!(
            page.unpainted_cell_count,
            expected_cells - expected_painted,
            "absent native carriers must remain unpainted"
        );
    }

    let p22 = pages
        .iter()
        .find(|page| page.viewer_page_index == 22)
        .expect("p22 receipt");
    assert_eq!(p22.table_count, 3, "p22 exact TABLE cohort");

    let receipt = Receipt {
        schema: "chaptera.virginia-table-autoformat-product-paint.v1",
        source_sha256: actual_sha,
        pages,
    };
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).expect("create product paint receipt directory");
    }
    fs::write(
        &output,
        serde_json::to_vec_pretty(&receipt).expect("serialize product paint receipt"),
    )
    .expect("write product paint receipt");

    println!(
        "VIRGINIA_TABLE_PRODUCT_PAINT p21={}/36 p22={}/110 p23={}/42",
        receipt.pages[0].painted_cell_count,
        receipt.pages[1].painted_cell_count,
        receipt.pages[2].painted_cell_count,
    );
}
