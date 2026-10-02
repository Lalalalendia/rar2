use pub_model::{CanonicalId, Sha256Digest};
use pub_reader::{
    PubBridgeDiagnostic, PubTableTextError, build_mature_0x2c_source_graph,
    materialize_bounded_simple_table_cells, materialize_bounded_table_cells,
};
use pub_viewer::{open_pub_bundle, viewer_geometry_environment_v0_1};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    io::Cursor,
    path::PathBuf,
};

fn source_hash(bytes: &[u8]) -> Sha256Digest {
    let digest = Sha256::digest(bytes);
    let mut raw = [0_u8; 32];
    raw.copy_from_slice(&digest);
    Sha256Digest::from_bytes(raw)
}

fn bump(map: &mut BTreeMap<String, usize>, key: impl Into<String>) {
    *map.entry(key.into()).or_default() += 1;
}

fn table_error_class(error: &PubTableTextError) -> &'static str {
    match error {
        PubTableTextError::NotSimpleRectangular => "not_simple_rectangular",
        PubTableTextError::StoryMismatch { .. } => "story_mismatch",
        PubTableTextError::MissingSourceCell { .. } => "missing_source_cell",
        PubTableTextError::MissingCoordinates { .. } => "missing_coordinates",
        PubTableTextError::InvalidCoordinates { .. } => "invalid_coordinates",
        PubTableTextError::OverlappingCells { .. } => "overlapping_cells",
        PubTableTextError::GridCoverageOverflow => "grid_coverage_overflow",
        PubTableTextError::IncompleteGridCoverage { .. } => "incomplete_grid_coverage",
        PubTableTextError::InvalidRange { .. } => "invalid_text_range",
        PubTableTextError::MissingLeadingCellSeparator { .. } => "missing_cell_separator",
        PubTableTextError::InvalidUtf16 { .. } => "invalid_utf16",
    }
}

fn layout_reason_class(reason: &str) -> &'static str {
    if reason == "story catalog has no unique layout key" {
        "story_layout_key_missing"
    } else if reason == "usable bounded Quill MCLD is unavailable" {
        "mcld_unavailable"
    } else if reason.contains("row/column array count") {
        "rowcol_array_count_mismatch"
    } else if reason.contains("duplicate row/column arrays") {
        "rowcol_array_duplicate"
    } else if reason.contains("row/column array is not a container") {
        "rowcol_array_not_container"
    } else if reason.contains("row/column item has no size") {
        "rowcol_item_size_missing"
    } else if reason.contains("row/column size is zero") {
        "rowcol_item_size_zero"
    } else if reason.contains("TABLE width is missing") {
        "table_width_missing"
    } else if reason.contains("TABLE height is missing") {
        "table_height_missing"
    } else if reason.contains("track sums") {
        "track_sum_mismatch"
    } else if reason.contains("exceed owner bounds") {
        "track_extent_exceeds_owner"
    } else if reason.contains("cell coordinates") {
        "cell_coordinates_unavailable"
    } else if reason.contains("MCLD") || reason.contains("mcld") {
        "mcld_metrics_rejected"
    } else if reason.contains("row/column track geometry unavailable") {
        "rowcol_track_geometry_other"
    } else {
        "other_bounded_layout_reason"
    }
}

#[derive(Debug, Default, Serialize)]
struct PageReceipt {
    viewer_page_index: u32,
    table_count: usize,
    direct_table_count: usize,
    grouped_table_count: usize,
    semantic_grid_histogram: BTreeMap<String, usize>,
    coordinates_complete_table_count: usize,
    simple_unmerged_table_count: usize,
    merged_cell_count: usize,
    stable_cell_identity_count: usize,
    exact_cell_bounds_count: usize,
    exact_track_geometry_complete_table_count: usize,
    mcld_fallback_metrics_table_count: usize,
    no_geometry_metrics_table_count: usize,
    per_cell_paint_count: usize,
    general_materialize_outcome: BTreeMap<String, usize>,
    simple_materialize_outcome: BTreeMap<String, usize>,
    materialized_cell_count: usize,
    viewer_table_count: usize,
    viewer_cell_count: usize,
    viewer_cell_bounds_count: usize,
    viewer_cell_paint_count: usize,
    source_layout_reason_histogram: BTreeMap<String, usize>,
}

#[derive(Debug, Serialize)]
struct Receipt {
    schema: &'static str,
    source_sha256: String,
    selected_viewer_page_count: usize,
    pages: Vec<PageReceipt>,
    guardrails: Vec<&'static str>,
}

#[test]
#[ignore = "requires exact public Virginia Remplacante fixture and receipt path"]
fn exact_virginia_p22_table_layout_probe() {
    let fixture = env::var_os("CHAPTERA_VIRGINIA_TABLE_LAYOUT_FIXTURE")
        .map(PathBuf::from)
        .expect("CHAPTERA_VIRGINIA_TABLE_LAYOUT_FIXTURE");
    let output = env::var_os("CHAPTERA_VIRGINIA_TABLE_LAYOUT_OUT")
        .map(PathBuf::from)
        .expect("CHAPTERA_VIRGINIA_TABLE_LAYOUT_OUT");
    let expected_sha = env::var("CHAPTERA_VIRGINIA_TABLE_LAYOUT_SHA256")
        .expect("CHAPTERA_VIRGINIA_TABLE_LAYOUT_SHA256");

    let bytes = fs::read(&fixture).expect("read exact Virginia Remplacante PUB");
    let actual_sha = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    assert_eq!(actual_sha, expected_sha, "exact Virginia source identity");

    let source = build_mature_0x2c_source_graph(
        Cursor::new(bytes.as_slice()),
        source_hash(&bytes),
    )
    .expect("build exact Virginia mature source graph");
    let bundle = open_pub_bundle(&bytes, viewer_geometry_environment_v0_1())
        .expect("open exact Virginia through Viewer bundle");

    assert_eq!(
        bundle.geometry.document.pages.len(),
        25,
        "bounded Virginia family profile must expose 25 customer pages"
    );

    let target_indexes = [21_u32, 22_u32, 23_u32];
    let mut pages = Vec::new();

    for viewer_page_index in target_indexes {
        let page = bundle
            .geometry
            .document
            .pages
            .get((viewer_page_index - 1) as usize)
            .expect("selected Viewer page exists");
        let parent = page.id.into_canonical();
        let mut receipt = PageReceipt {
            viewer_page_index,
            ..PageReceipt::default()
        };
        let mut table_seq_nums = BTreeSet::new();

        for node in bundle
            .resolved_graph
            .nodes
            .values()
            .filter(|node| node.header.parent_id == parent)
        {
            let Some(table) = node.payload.table.as_ref() else {
                continue;
            };
            receipt.table_count += 1;
            table_seq_nums.insert(node.payload.contents_seq_num);

            let grouped = node.header.source_refs.iter().any(|source| {
                source
                    .object_key
                    .as_deref()
                    .is_some_and(|key| key.starts_with("escher/group-ancestor/"))
            });
            if grouped {
                receipt.grouped_table_count += 1;
            } else {
                receipt.direct_table_count += 1;
            }

            bump(
                &mut receipt.semantic_grid_histogram,
                format!("{}x{}", table.rows, table.columns),
            );
            receipt.stable_cell_identity_count += table.cells.len();

            let coordinates_complete = table.cells.iter().all(|cell| cell.coordinates.is_some());
            receipt.coordinates_complete_table_count += usize::from(coordinates_complete);
            receipt.simple_unmerged_table_count += usize::from(table.simple_table.is_some());

            for cell in &table.cells {
                if let Some(coordinates) = cell.coordinates {
                    if coordinates.start_row != coordinates.end_row
                        || coordinates.start_column != coordinates.end_column
                    {
                        receipt.merged_cell_count += 1;
                    }
                }
                receipt.exact_cell_bounds_count += usize::from(cell.bounds.is_some());
                receipt.per_cell_paint_count += usize::from(cell.paint.is_some());
            }

            let all_exact_bounds =
                !table.cells.is_empty() && table.cells.iter().all(|cell| cell.bounds.is_some());
            receipt.exact_track_geometry_complete_table_count += usize::from(all_exact_bounds);
            receipt.mcld_fallback_metrics_table_count += usize::from(table.layout_metrics.is_some());
            receipt.no_geometry_metrics_table_count +=
                usize::from(!all_exact_bounds && table.layout_metrics.is_none());

            let Some(story_id) = table.story_id else {
                bump(&mut receipt.general_materialize_outcome, "story_id_missing");
                bump(&mut receipt.simple_materialize_outcome, "story_id_missing");
                continue;
            };
            let Some(story) = bundle.resolved_graph.stories.get(&story_id) else {
                bump(&mut receipt.general_materialize_outcome, "story_missing");
                bump(&mut receipt.simple_materialize_outcome, "story_missing");
                continue;
            };

            match materialize_bounded_table_cells(table, story) {
                Ok(cells) => {
                    bump(&mut receipt.general_materialize_outcome, "admitted");
                    receipt.materialized_cell_count += cells.len();
                }
                Err(error) => bump(
                    &mut receipt.general_materialize_outcome,
                    table_error_class(&error),
                ),
            }
            match materialize_bounded_simple_table_cells(table, story) {
                Ok(_) => bump(&mut receipt.simple_materialize_outcome, "admitted"),
                Err(error) => bump(
                    &mut receipt.simple_materialize_outcome,
                    table_error_class(&error),
                ),
            }

            if let Some(viewer_table) = bundle
                .geometry
                .tables
                .iter()
                .find(|viewer_table| viewer_table.node_id == node.header.id)
            {
                receipt.viewer_table_count += 1;
                receipt.viewer_cell_count += viewer_table.cells.len();
                receipt.viewer_cell_bounds_count += viewer_table
                    .cells
                    .iter()
                    .filter(|cell| cell.bounds.is_some())
                    .count();
                receipt.viewer_cell_paint_count += viewer_table
                    .cells
                    .iter()
                    .filter(|cell| cell.fill_rgb.is_some() && cell.fill_visible == Some(true))
                    .count();
            }
        }

        for diagnostic in &source.diagnostics {
            if let PubBridgeDiagnostic::TableLayoutMetricsUnavailable {
                seq_num, reason, ..
            } = diagnostic
            {
                if table_seq_nums.contains(seq_num) {
                    bump(
                        &mut receipt.source_layout_reason_histogram,
                        layout_reason_class(reason),
                    );
                }
            }
        }

        pages.push(receipt);
    }

    let p22 = pages
        .iter()
        .find(|page| page.viewer_page_index == 22)
        .expect("p22 receipt");
    assert!(p22.table_count > 0, "Viewer p22 must contain TABLE nodes");

    let receipt = Receipt {
        schema: "chaptera.virginia-p22-table-layout-probe.v1",
        source_sha256: actual_sha,
        selected_viewer_page_count: bundle.geometry.document.pages.len(),
        pages,
        guardrails: vec![
            "Viewer p21/p22/p23 are selected through the existing product page projection; no raw PAGE ordinal is used as product authority.",
            "Exact cell bounds are counted only when already source-backed by the current TABLE row/column track bridge.",
            "MCLD fallback availability is reported separately from exact TABLE track geometry.",
            "No equal-grid geometry, PDF-derived coordinates, source text, object ids, SPIDs, offsets, filenames, or raw property values are emitted.",
            "Per-cell paint counts reuse the existing bounded TABLE-cell paint bridge and do not substitute ordinary Shape paint.",
        ],
    };

    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).expect("create table layout receipt directory");
    }
    fs::write(
        &output,
        serde_json::to_vec_pretty(&receipt).expect("serialize table layout receipt"),
    )
    .expect("write table layout receipt");

    println!(
        "VIRGINIA_P22_TABLE_LAYOUT p21_tables={} p22_tables={} p23_tables={} p22_exact_bounds={} p22_mcld={} p22_viewer_bounds={}",
        receipt.pages[0].table_count,
        receipt.pages[1].table_count,
        receipt.pages[2].table_count,
        receipt.pages[1].exact_cell_bounds_count,
        receipt.pages[1].mcld_fallback_metrics_table_count,
        receipt.pages[1].viewer_cell_bounds_count,
    );
}
