use pub_viewer::{open_pub_bundle, viewer_geometry_environment_v0_1};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{env, fs, path::PathBuf};

const EXPECTED_SHA256: &str =
    "88f57d800aeec808798ea487b9d4ab85dc85c02cd190b987a332709b81018506";

#[derive(Debug, Clone, Serialize)]
struct PageReceipt {
    viewer_page_index: usize,
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

fn page_receipt(bundle: &pub_viewer::ViewerOpenBundle, viewer_page_index: usize) -> PageReceipt {
    let page = bundle
        .geometry
        .document
        .pages
        .get(viewer_page_index - 1)
        .expect("selected Viewer page exists");
    let parent = page.id.into_canonical();

    let mut table_count = 0_usize;
    let mut cell_count = 0_usize;
    let mut painted_cell_count = 0_usize;

    for node in bundle
        .resolved_graph
        .nodes
        .values()
        .filter(|node| node.header.parent_id == parent)
    {
        let Some(table) = node.payload.table.as_ref() else {
            continue;
        };
        table_count += 1;
        cell_count += table.cells.len();
        painted_cell_count += table.cells.iter().filter(|cell| cell.paint.is_some()).count();
    }

    PageReceipt {
        viewer_page_index,
        table_count,
        cell_count,
        painted_cell_count,
        unpainted_cell_count: cell_count - painted_cell_count,
    }
}

#[test]
#[ignore = "requires exact public Virginia Remplacante fixture"]
fn exact_virginia_native_autoformat_product_acceptance() {
    let fixture = PathBuf::from(
        env::var_os("CHAPTERA_VIRGINIA_TABLE_NATIVE_PAINT_FIXTURE")
            .expect("CHAPTERA_VIRGINIA_TABLE_NATIVE_PAINT_FIXTURE"),
    );
    let output = PathBuf::from(
        env::var_os("CHAPTERA_VIRGINIA_TABLE_NATIVE_PAINT_RECEIPT")
            .expect("CHAPTERA_VIRGINIA_TABLE_NATIVE_PAINT_RECEIPT"),
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

    let pages = [6_usize, 7, 21, 22, 23]
        .into_iter()
        .map(|page| page_receipt(&bundle, page))
        .collect::<Vec<_>>();

    let p6 = pages.iter().find(|p| p.viewer_page_index == 6).unwrap();
    let p7 = pages.iter().find(|p| p.viewer_page_index == 7).unwrap();
    let p21 = pages.iter().find(|p| p.viewer_page_index == 21).unwrap();
    let p22 = pages.iter().find(|p| p.viewer_page_index == 22).unwrap();
    let p23 = pages.iter().find(|p| p.viewer_page_index == 23).unwrap();

    assert_eq!((p6.table_count, p6.cell_count), (12, 588), "p6 exact TABLE/cell control");
    assert_eq!((p7.table_count, p7.cell_count), (1, 192), "p7 exact TABLE/cell control");

    assert_eq!((p21.cell_count, p21.painted_cell_count), (36, 16), "p21 exact cell/paint count");
    assert_eq!(
        (p22.table_count, p22.cell_count, p22.painted_cell_count),
        (3, 110, 100),
        "p22 exact TABLE/cell/paint count"
    );
    assert_eq!((p23.cell_count, p23.painted_cell_count), (42, 9), "p23 exact cell/paint count");

    let receipt = Receipt {
        schema: "chaptera.virginia-table-native-autoformat-product-acceptance.v1",
        source_sha256: actual_sha,
        pages: pages.clone(),
    };
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).expect("create receipt directory");
    }
    fs::write(
        &output,
        serde_json::to_vec_pretty(&receipt).expect("serialize receipt"),
    )
    .expect("write receipt");

    println!(
        "VIRGINIA_TABLE_NATIVE_AUTOFORMAT_PRODUCT p6={}/{} p7={}/{} p21={}/{} p22={}/{} p23={}/{}",
        p6.painted_cell_count, p6.cell_count,
        p7.painted_cell_count, p7.cell_count,
        p21.painted_cell_count, p21.cell_count,
        p22.painted_cell_count, p22.cell_count,
        p23.painted_cell_count, p23.cell_count,
    );
}
