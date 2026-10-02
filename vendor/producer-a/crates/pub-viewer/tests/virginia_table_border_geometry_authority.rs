use pub_model::{AuthorityClass, ReadConfidence, RectEmu};
use pub_viewer::{open_pub_bundle, viewer_geometry_environment_v0_1};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, env, fs, path::PathBuf};

const EXPECTED_SHA256: &str =
    "88f57d800aeec808798ea487b9d4ab85dc85c02cd190b987a332709b81018506";

fn right(rect: RectEmu) -> Option<i64> {
    rect.x.get().checked_add(rect.width.get())
}

fn bottom(rect: RectEmu) -> Option<i64> {
    rect.y.get().checked_add(rect.height.get())
}

fn exact_track_ref(cell: &pub_reader::PubTableCellSource) -> bool {
    cell.source_refs.iter().any(|reference| {
        reference.path.as_deref() == Some("TABLE/rowcol_array")
            && reference.authority == AuthorityClass::Authoritative
            && reference.confidence == Some(ReadConfidence::Exact)
    })
}

fn grouped_projection(node: &pub_model::Node<pub_reader::PubResolvedNodePayload>) -> bool {
    node.header.source_refs.iter().any(|reference| {
        reference.path.as_deref() == Some("SpgrContainer/SpContainer")
    })
}

#[derive(Debug, Serialize)]
struct TableReceipt {
    viewer_page: u32,
    table_seq_num: u32,
    rows: u32,
    columns: u32,
    simple_table: bool,
    grouped_projection: bool,
    source_cell_count: usize,
    source_bounds_count: usize,
    exact_track_cell_count: usize,
    viewer_bounds_count: usize,
    layout_metrics_present: bool,
    geometry_class: String,
    grid_starts_at_owner_origin: Option<bool>,
    grid_ends_at_owner_right: Option<bool>,
    grid_ends_at_owner_bottom: Option<bool>,
    owner_minus_grid_right_emu: Option<i64>,
    owner_minus_grid_bottom_emu: Option<i64>,
    owner_width_emu: i64,
    owner_height_emu: i64,
    grid_width_emu: Option<i64>,
    grid_height_emu: Option<i64>,
}

#[derive(Debug, Default, Serialize)]
struct PageReceipt {
    viewer_page: u32,
    table_count: usize,
    exact_track_edge_match_count: usize,
    exact_track_extent_mismatch_count: usize,
    fallback_mcld_count: usize,
    incomplete_geometry_count: usize,
    grouped_table_count: usize,
}

#[derive(Debug, Default, Serialize)]
struct Totals {
    table_count: usize,
    exact_track_edge_match_count: usize,
    exact_track_extent_mismatch_count: usize,
    fallback_mcld_count: usize,
    incomplete_geometry_count: usize,
    grouped_table_count: usize,
}

#[derive(Debug, Serialize)]
struct Receipt {
    schema: &'static str,
    source_sha256: String,
    pages: Vec<PageReceipt>,
    totals: Totals,
    tables: Vec<TableReceipt>,
    claims: Claims,
}

#[derive(Debug, Serialize)]
struct Claims {
    pdf_geometry_used: bool,
    geometry_proximity_used: bool,
    page_or_hash_product_rule_used: bool,
    exact_track_authority_path: &'static str,
}

#[test]
#[ignore = "requires exact public Virginia Remplacante fixture"]
fn exact_virginia_table_border_geometry_authority_census() {
    let fixture = PathBuf::from(
        env::var_os("CHAPTERA_VIRGINIA_TABLE_GEOMETRY_FIXTURE")
            .expect("CHAPTERA_VIRGINIA_TABLE_GEOMETRY_FIXTURE"),
    );
    let output = PathBuf::from(
        env::var_os("CHAPTERA_VIRGINIA_TABLE_GEOMETRY_OUT")
            .expect("CHAPTERA_VIRGINIA_TABLE_GEOMETRY_OUT"),
    );

    let bytes = fs::read(&fixture).expect("read exact Virginia PUB");
    let sha = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    assert_eq!(sha, EXPECTED_SHA256, "exact Virginia source identity");

    let bundle =
        open_pub_bundle(&bytes, viewer_geometry_environment_v0_1()).expect("open Viewer bundle");

    let expected_tables = BTreeMap::from([
        (6_u32, 12_usize),
        (7, 1),
        (9, 1),
        (10, 1),
        (11, 1),
        (21, 2),
        (22, 3),
        (23, 1),
    ]);

    let mut pages = Vec::new();
    let mut tables = Vec::new();
    let mut totals = Totals::default();

    for (viewer_page, expected_table_count) in expected_tables {
        let page = bundle
            .geometry
            .document
            .pages
            .get((viewer_page - 1) as usize)
            .expect("selected page exists");
        let parent = page.id.into_canonical();
        let page_nodes = bundle
            .resolved_graph
            .nodes
            .values()
            .filter(|node| node.header.parent_id == parent && node.payload.table.is_some())
            .collect::<Vec<_>>();

        assert_eq!(
            page_nodes.len(),
            expected_table_count,
            "selected Virginia page TABLE count must remain pinned"
        );

        let mut page_receipt = PageReceipt {
            viewer_page,
            table_count: page_nodes.len(),
            ..PageReceipt::default()
        };

        for node in page_nodes {
            let source = node.payload.table.as_ref().expect("filtered TABLE");
            let viewer = bundle
                .geometry
                .tables
                .iter()
                .find(|table| table.node_id == node.header.id)
                .expect("Viewer TABLE for resolved source TABLE");

            let source_cell_count = source.cells.len();
            let source_bounds_count = source.cells.iter().filter(|cell| cell.bounds.is_some()).count();
            let exact_track_cell_count = source.cells.iter().filter(|cell| exact_track_ref(cell)).count();
            let viewer_bounds_count = viewer.cells.iter().filter(|cell| cell.bounds.is_some()).count();
            let grouped = grouped_projection(node);

            let exact_grid_rect = if source_bounds_count == source_cell_count && source_cell_count > 0 {
                let left = source.cells.iter().filter_map(|cell| cell.bounds.map(|b| b.x.get())).min();
                let top = source.cells.iter().filter_map(|cell| cell.bounds.map(|b| b.y.get())).min();
                let right_edge = source.cells.iter().filter_map(|cell| cell.bounds.and_then(right)).max();
                let bottom_edge = source.cells.iter().filter_map(|cell| cell.bounds.and_then(bottom)).max();
                match (left, top, right_edge, bottom_edge) {
                    (Some(left), Some(top), Some(right_edge), Some(bottom_edge))
                        if right_edge >= left && bottom_edge >= top =>
                    {
                        Some((left, top, right_edge, bottom_edge))
                    }
                    _ => None,
                }
            } else {
                None
            };

            let owner = node.header.bounds;
            let owner_right = right(owner).expect("owner right");
            let owner_bottom = bottom(owner).expect("owner bottom");

            let (
                grid_starts_at_owner_origin,
                grid_ends_at_owner_right,
                grid_ends_at_owner_bottom,
                owner_minus_grid_right_emu,
                owner_minus_grid_bottom_emu,
                grid_width_emu,
                grid_height_emu,
            ) = match exact_grid_rect {
                Some((left, top, grid_right, grid_bottom)) => (
                    Some(left == owner.x.get() && top == owner.y.get()),
                    Some(grid_right == owner_right),
                    Some(grid_bottom == owner_bottom),
                    owner_right.checked_sub(grid_right),
                    owner_bottom.checked_sub(grid_bottom),
                    grid_right.checked_sub(left),
                    grid_bottom.checked_sub(top),
                ),
                None => (None, None, None, None, None, None, None),
            };

            let geometry_class = if exact_track_cell_count == source_cell_count
                && source_bounds_count == source_cell_count
                && source_cell_count > 0
            {
                if grid_starts_at_owner_origin == Some(true)
                    && grid_ends_at_owner_right == Some(true)
                    && grid_ends_at_owner_bottom == Some(true)
                {
                    page_receipt.exact_track_edge_match_count += 1;
                    totals.exact_track_edge_match_count += 1;
                    "exact_track_owner_edge_match"
                } else {
                    page_receipt.exact_track_extent_mismatch_count += 1;
                    totals.exact_track_extent_mismatch_count += 1;
                    "exact_track_owner_extent_mismatch"
                }
            } else if source_bounds_count < source_cell_count
                && source.layout_metrics.is_some()
                && viewer_bounds_count == source_cell_count
            {
                page_receipt.fallback_mcld_count += 1;
                totals.fallback_mcld_count += 1;
                "mcld_uniform_fallback"
            } else {
                page_receipt.incomplete_geometry_count += 1;
                totals.incomplete_geometry_count += 1;
                "incomplete_or_mixed"
            };

            if grouped {
                page_receipt.grouped_table_count += 1;
                totals.grouped_table_count += 1;
            }

            tables.push(TableReceipt {
                viewer_page,
                table_seq_num: node.payload.contents_seq_num,
                rows: source.rows,
                columns: source.columns,
                simple_table: source.simple_table.is_some(),
                grouped_projection: grouped,
                source_cell_count,
                source_bounds_count,
                exact_track_cell_count,
                viewer_bounds_count,
                layout_metrics_present: source.layout_metrics.is_some(),
                geometry_class: geometry_class.to_owned(),
                grid_starts_at_owner_origin,
                grid_ends_at_owner_right,
                grid_ends_at_owner_bottom,
                owner_minus_grid_right_emu,
                owner_minus_grid_bottom_emu,
                owner_width_emu: owner.width.get(),
                owner_height_emu: owner.height.get(),
                grid_width_emu,
                grid_height_emu,
            });
        }

        totals.table_count += page_receipt.table_count;
        pages.push(page_receipt);
    }

    assert_eq!(totals.table_count, 22, "expected #772 TABLE cohort");

    let receipt = Receipt {
        schema: "chaptera.virginia-table-border-geometry-authority.v1",
        source_sha256: sha,
        pages,
        totals,
        tables,
        claims: Claims {
            pdf_geometry_used: false,
            geometry_proximity_used: false,
            page_or_hash_product_rule_used: false,
            exact_track_authority_path: "TABLE/rowcol_array + exact authoritative SourceRef",
        },
    };

    fs::create_dir_all(output.parent().expect("receipt parent")).expect("create receipt dir");
    fs::write(
        &output,
        serde_json::to_vec_pretty(&receipt).expect("serialize receipt"),
    )
    .expect("write receipt");

    println!(
        "TABLE_BORDER_GEOMETRY tables={} exact_edge={} exact_mismatch={} fallback_mcld={} incomplete={} grouped={}",
        receipt.totals.table_count,
        receipt.totals.exact_track_edge_match_count,
        receipt.totals.exact_track_extent_mismatch_count,
        receipt.totals.fallback_mcld_count,
        receipt.totals.incomplete_geometry_count,
        receipt.totals.grouped_table_count
    );
}
