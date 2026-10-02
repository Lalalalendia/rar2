use pub_model::NodeKind;
use pub_viewer::{open_pub_bundle, viewer_geometry_environment_v0_1};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, env, fs, path::PathBuf};

const EXPECTED_SHA256: &str =
    "88f57d800aeec808798ea487b9d4ab85dc85c02cd190b987a332709b81018506";

fn bump(map: &mut BTreeMap<String, usize>, key: impl Into<String>) {
    *map.entry(key.into()).or_default() += 1;
}

#[derive(Debug, Default, Serialize)]
struct PageReceipt {
    viewer_page_index: u32,
    table_count: usize,
    table_cell_count: usize,
    ordinary_shape_count: usize,
    exact_unique_cell_match_count: usize,
    exact_ambiguous_cell_match_count: usize,
    exact_absent_cell_match_count: usize,
    unique_matches_with_effective_paint: usize,
    unique_match_shape_type_histogram: BTreeMap<String, usize>,
}

#[derive(Debug, Serialize)]
struct Receipt {
    schema: &'static str,
    source_sha256: String,
    pages: Vec<PageReceipt>,
    guardrails: Vec<&'static str>,
}

#[test]
#[ignore = "requires exact public Virginia Remplacante fixture"]
fn exact_virginia_table_materialized_sibling_probe() {
    let fixture = PathBuf::from(
        env::var_os("CHAPTERA_VIRGINIA_TABLE_SIBLING_FIXTURE")
            .expect("CHAPTERA_VIRGINIA_TABLE_SIBLING_FIXTURE"),
    );
    let output = PathBuf::from(
        env::var_os("CHAPTERA_VIRGINIA_TABLE_SIBLING_OUT")
            .expect("CHAPTERA_VIRGINIA_TABLE_SIBLING_OUT"),
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
        let page = bundle
            .geometry
            .document
            .pages
            .get((viewer_page_index - 1) as usize)
            .expect("selected Viewer page exists");
        let page_id = page.id.into_canonical();

        let ordinary_shapes = bundle
            .resolved_graph
            .nodes
            .values()
            .filter(|node| {
                node.header.parent_id == page_id
                    && node.kind == NodeKind::Shape
                    && node.payload.table.is_none()
            })
            .collect::<Vec<_>>();

        let mut receipt = PageReceipt {
            viewer_page_index,
            ordinary_shape_count: ordinary_shapes.len(),
            ..PageReceipt::default()
        };

        for table_node in bundle.resolved_graph.nodes.values().filter(|node| {
            node.header.parent_id == page_id && node.payload.table.is_some()
        }) {
            let table = table_node.payload.table.as_ref().expect("filtered TABLE");
            receipt.table_count += 1;
            receipt.table_cell_count += table.cells.len();

            for cell in &table.cells {
                let Some(cell_bounds) = cell.bounds else {
                    receipt.exact_absent_cell_match_count += 1;
                    continue;
                };

                let matches = ordinary_shapes
                    .iter()
                    .copied()
                    .filter(|shape| shape.header.bounds == cell_bounds)
                    .collect::<Vec<_>>();

                match matches.as_slice() {
                    [shape] => {
                        receipt.exact_unique_cell_match_count += 1;
                        receipt.unique_matches_with_effective_paint +=
                            usize::from(shape.payload.effective_paint.is_some());
                        bump(
                            &mut receipt.unique_match_shape_type_histogram,
                            shape
                                .payload
                                .officeart_shape_type
                                .map(|value| format!("0x{value:04x}"))
                                .unwrap_or_else(|| "absent".to_owned()),
                        );
                    }
                    [] => receipt.exact_absent_cell_match_count += 1,
                    _ => receipt.exact_ambiguous_cell_match_count += 1,
                }
            }
        }

        assert_eq!(
            receipt.exact_unique_cell_match_count
                + receipt.exact_ambiguous_cell_match_count
                + receipt.exact_absent_cell_match_count,
            receipt.table_cell_count,
            "every selected TABLE cell must be classified"
        );
        pages.push(receipt);
    }

    let p22 = pages
        .iter()
        .find(|page| page.viewer_page_index == 22)
        .expect("p22 receipt");
    assert_eq!(p22.table_count, 3, "p22 exact TABLE cohort");
    assert_eq!(p22.table_cell_count, 110, "p22 exact TABLE cell cohort");

    let receipt = Receipt {
        schema: "chaptera.virginia-table-materialized-sibling-probe.v1",
        source_sha256: actual_sha,
        pages,
        guardrails: vec![
            "TABLE cell bounds and ordinary Shape bounds come from the existing shared resolved graph; no second ClientAnchor decoder is introduced.",
            "Only exact page-local rectangle equality is admitted; no proximity, row-major, nearest-neighbour or PDF-derived matching is used.",
            "The probe emits aggregate match classes and OfficeArt shape-type classes only; no coordinates, object ids, text, filenames, property values or raw bytes are emitted.",
            "A positive match is evidence of an already-materialized sibling carrier, not by itself authorization to copy arbitrary Shape paint into TABLE cells.",
        ],
    };

    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).expect("create sibling-probe receipt directory");
    }
    fs::write(
        &output,
        serde_json::to_vec_pretty(&receipt).expect("serialize sibling-probe receipt"),
    )
    .expect("write sibling-probe receipt");

    println!(
        "VIRGINIA_TABLE_MATERIALIZED_SIBLING p21={}/{}/{} p22={}/{}/{} p22_painted={} p23={}/{}/{}",
        receipt.pages[0].exact_unique_cell_match_count,
        receipt.pages[0].exact_ambiguous_cell_match_count,
        receipt.pages[0].exact_absent_cell_match_count,
        receipt.pages[1].exact_unique_cell_match_count,
        receipt.pages[1].exact_ambiguous_cell_match_count,
        receipt.pages[1].exact_absent_cell_match_count,
        receipt.pages[1].unique_matches_with_effective_paint,
        receipt.pages[2].exact_unique_cell_match_count,
        receipt.pages[2].exact_ambiguous_cell_match_count,
        receipt.pages[2].exact_absent_cell_match_count,
    );
}
