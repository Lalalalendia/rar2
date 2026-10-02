use pub_viewer::{open_pub_bundle, viewer_geometry_environment_v0_1};
use sha2::{Digest, Sha256};
use std::{env, fs, path::PathBuf};

const EXPECTED_SHA256: &str = "88f57d800aeec808798ea487b9d4ab85dc85c02cd190b987a332709b81018506";

fn page_table_paint_counts(
    bundle: &pub_viewer::ViewerOpenBundle,
    viewer_page_index: usize,
) -> (usize, usize, usize) {
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
        painted_cell_count += table
            .cells
            .iter()
            .filter(|cell| cell.paint.is_some())
            .count();
    }

    (table_count, cell_count, painted_cell_count)
}

#[test]
#[ignore = "requires exact public Virginia Remplacante fixture"]
fn exact_virginia_native_autoformat_cell_paint_acceptance() {
    let fixture = PathBuf::from(
        env::var_os("CHAPTERA_VIRGINIA_TABLE_NATIVE_PAINT_FIXTURE")
            .expect("CHAPTERA_VIRGINIA_TABLE_NATIVE_PAINT_FIXTURE"),
    );
    let bytes = fs::read(&fixture).expect("read exact Virginia Remplacante PUB");
    let actual_sha = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    assert_eq!(
        actual_sha, EXPECTED_SHA256,
        "exact Virginia source identity"
    );

    let bundle = open_pub_bundle(&bytes, viewer_geometry_environment_v0_1())
        .expect("open exact Virginia through shared Viewer bundle");
    assert_eq!(
        bundle.geometry.document.pages.len(),
        25,
        "bounded Virginia family profile must expose 25 customer pages"
    );

    let p21 = page_table_paint_counts(&bundle, 21);
    let p22 = page_table_paint_counts(&bundle, 22);
    let p23 = page_table_paint_counts(&bundle, 23);

    assert_eq!(
        (p21.1, p21.2),
        (36, 16),
        "p21 exact source-backed TABLE cell/paint count"
    );
    assert_eq!(
        (p22.1, p22.2),
        (110, 100),
        "p22 exact source-backed TABLE cell/paint count"
    );
    assert_eq!(
        (p23.1, p23.2),
        (42, 9),
        "p23 exact source-backed TABLE cell/paint count"
    );

    println!(
        "VIRGINIA_TABLE_NATIVE_AUTOFORMAT_ACCEPTANCE p21=tables:{} paint:{}/{} p22=tables:{} paint:{}/{} p23=tables:{} paint:{}/{}",
        p21.0, p21.2, p21.1, p22.0, p22.2, p22.1, p23.0, p23.2, p23.1
    );
}
