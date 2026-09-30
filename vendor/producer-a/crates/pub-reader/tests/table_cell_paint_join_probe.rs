use pub_core::StreamPath;
use pub_model::Sha256Digest;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::io::Cursor;
use std::path::PathBuf;

const FILL_COLOR_PROPERTY: u16 = 0x0181;
const FILL_BOOLEANS_PROPERTY: u16 = 0x01BF;

fn source_hash(bytes: &[u8]) -> Sha256Digest {
    let digest = Sha256::digest(bytes);
    let mut raw = [0_u8; 32];
    raw.copy_from_slice(&digest);
    Sha256Digest::from_bytes(raw)
}

fn project_axis(
    value: i64,
    source_start: i64,
    source_end: i64,
    target_start: i64,
    target_end: i64,
) -> Option<i64> {
    let source_len = i128::from(source_end) - i128::from(source_start);
    let target_len = i128::from(target_end) - i128::from(target_start);
    if source_len <= 0 || target_len <= 0 {
        return None;
    }
    let projected = i128::from(target_start)
        + (i128::from(value) - i128::from(source_start)) * target_len / source_len;
    i64::try_from(projected).ok()
}

fn projected_child_bounds(
    child: &pub_escher::OfficeArtCoordinateRect,
    group: &pub_escher::OfficeArtCoordinateRect,
    target: pub_model::RectEmu,
) -> Option<[i64; 4]> {
    let target_left = target.x.get();
    let target_top = target.y.get();
    let target_right = target_left.checked_add(target.width.get())?;
    let target_bottom = target_top.checked_add(target.height.get())?;

    Some([
        project_axis(
            i64::from(child.x_left),
            i64::from(group.x_left),
            i64::from(group.x_right),
            target_left,
            target_right,
        )?,
        project_axis(
            i64::from(child.y_top),
            i64::from(group.y_top),
            i64::from(group.y_bottom),
            target_top,
            target_bottom,
        )?,
        project_axis(
            i64::from(child.x_right),
            i64::from(group.x_left),
            i64::from(group.x_right),
            target_left,
            target_right,
        )?,
        project_axis(
            i64::from(child.y_bottom),
            i64::from(group.y_top),
            i64::from(group.y_bottom),
            target_top,
            target_bottom,
        )?,
    ])
}

fn rect_array(rect: pub_model::RectEmu) -> [i64; 4] {
    [
        rect.x.get(),
        rect.y.get(),
        rect.x.get() + rect.width.get(),
        rect.y.get() + rect.height.get(),
    ]
}

fn color_class(raw: u32) -> &'static str {
    match (raw >> 24) as u8 {
        0x00 => "direct_rgb",
        0x08 => "scheme",
        0x10 => "system_or_extended",
        _ => "other_flagged",
    }
}

#[test]
fn exact_table_cell_officeart_geometry_join_probe() {
    let fixture = PathBuf::from(
        std::env::var_os("CHAPTERA_TABLE_CELL_PAINT_FIXTURE")
            .expect("CHAPTERA_TABLE_CELL_PAINT_FIXTURE"),
    );
    let bytes = fs::read(&fixture).expect("read exact TABLE fixture");
    let build = pub_reader::build_mature_0x2c_source_graph(
        Cursor::new(bytes.as_slice()),
        source_hash(&bytes),
    )
    .expect("mature SourceGraph");

    let escher = pub_cfb::read_stream_reader(
        Cursor::new(bytes.as_slice()),
        pub_reader::ESCHER_STREAM_PATH,
    )
    .expect("read EscherStm");
    let inventory = pub_escher::inspect_sp_containers(
        StreamPath(pub_reader::ESCHER_STREAM_PATH.into()),
        &escher,
    )
    .expect("inspect SpContainers");

    let mut by_contents_seq = BTreeMap::<u32, Vec<usize>>::new();
    for (shape_index, shape) in inventory.shapes.iter().enumerate() {
        let Some(client_data) = shape.client_data.as_ref() else {
            continue;
        };
        for field in client_data
            .fields
            .iter()
            .filter(|field| field.id == pub_escher::PUBLISHER_FIELD_SHAPE_ID)
        {
            by_contents_seq
                .entry(field.value)
                .or_default()
                .push(shape_index);
        }
    }

    let mut table_count = 0_usize;
    let mut table_cell_count = 0_usize;
    let mut owner_unique_count = 0_usize;
    let mut direct_child_count = 0_usize;
    let mut exact_match_count = 0_usize;
    let mut ambiguous_match_count = 0_usize;
    let mut unmatched_count = 0_usize;
    let mut exact_matches_with_fill_color = 0_usize;
    let mut exact_matches_with_fill_booleans = 0_usize;
    let mut fill_color_histogram = BTreeMap::<String, usize>::new();
    let mut fill_boolean_histogram = BTreeMap::<String, usize>::new();
    let mut cells = Vec::new();

    for node in build.graph.nodes.values() {
        let Some(table) = node.payload.table.as_ref() else {
            continue;
        };
        if table.cells.is_empty() || table.cells.iter().any(|cell| cell.bounds.is_none()) {
            continue;
        }
        table_count += 1;
        table_cell_count += table.cells.len();

        let owner_matches = by_contents_seq
            .get(&node.payload.contents_seq_num)
            .cloned()
            .unwrap_or_default();
        if owner_matches.len() != 1 {
            unmatched_count += table.cells.len();
            continue;
        }
        owner_unique_count += 1;

        let owner = &inventory.shapes[owner_matches[0]];
        let Some(group_rect) = owner.fspgr.as_ref() else {
            unmatched_count += table.cells.len();
            continue;
        };

        let children = inventory
            .shapes
            .iter()
            .filter(|shape| shape.parent_group_shape_source.as_ref() == Some(&owner.source))
            .filter_map(|shape| {
                let child_anchor = shape.child_anchor.as_ref()?;
                let projected =
                    projected_child_bounds(child_anchor, group_rect, node.header.bounds)?;
                let fill_colors = shape
                    .fopts
                    .iter()
                    .flat_map(|record| record.properties.iter())
                    .filter(|property| {
                        property.property_id() == FILL_COLOR_PROPERTY
                            && !property.f_bid()
                            && !property.f_complex()
                    })
                    .map(|property| property.op)
                    .collect::<Vec<_>>();
                let fill_booleans = shape
                    .fopts
                    .iter()
                    .flat_map(|record| record.properties.iter())
                    .filter(|property| {
                        property.property_id() == FILL_BOOLEANS_PROPERTY
                            && !property.f_bid()
                            && !property.f_complex()
                    })
                    .map(|property| property.op)
                    .collect::<Vec<_>>();
                Some((projected, fill_colors, fill_booleans))
            })
            .collect::<Vec<_>>();
        direct_child_count += children.len();

        for cell in &table.cells {
            let bounds = rect_array(cell.bounds.expect("prechecked bounds"));
            let matches = children
                .iter()
                .filter(|(projected, _, _)| *projected == bounds)
                .collect::<Vec<_>>();

            match matches.as_slice() {
                [( _, fill_colors, fill_booleans)] => {
                    exact_match_count += 1;
                    exact_matches_with_fill_color += usize::from(!fill_colors.is_empty());
                    exact_matches_with_fill_booleans += usize::from(!fill_booleans.is_empty());

                    for raw in fill_colors.iter().copied() {
                        *fill_color_histogram
                            .entry(format!("{}:0x{raw:08X}", color_class(raw)))
                            .or_default() += 1;
                    }
                    for raw in fill_booleans.iter().copied() {
                        *fill_boolean_histogram
                            .entry(format!("0x{raw:08X}"))
                            .or_default() += 1;
                    }

                    let coordinates = cell.coordinates.expect("bounded TABLE cell coordinates");
                    cells.push(serde_json::json!({
                        "row": coordinates.start_row,
                        "column": coordinates.start_column,
                        "row_span": coordinates.end_row - coordinates.start_row + 1,
                        "column_span": coordinates.end_column - coordinates.start_column + 1,
                        "fill_color_raw": fill_colors
                            .iter()
                            .map(|raw| format!("0x{raw:08X}"))
                            .collect::<Vec<_>>(),
                        "fill_color_class": fill_colors
                            .iter()
                            .map(|raw| color_class(*raw))
                            .collect::<Vec<_>>(),
                        "fill_booleans_raw": fill_booleans
                            .iter()
                            .map(|raw| format!("0x{raw:08X}"))
                            .collect::<Vec<_>>(),
                    }));
                }
                [] => unmatched_count += 1,
                _ => ambiguous_match_count += 1,
            }
        }
    }

    cells.sort_by_key(|cell| {
        (
            cell["row"].as_u64().unwrap_or(u64::MAX),
            cell["column"].as_u64().unwrap_or(u64::MAX),
        )
    });

    let receipt = serde_json::json!({
        "schema": "chaptera.pub-table-cell-officeart-join-probe.v2",
        "source_sha256": Sha256::digest(&bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>(),
        "table_count": table_count,
        "table_cell_count": table_cell_count,
        "owner_unique_count": owner_unique_count,
        "direct_child_count": direct_child_count,
        "exact_match_count": exact_match_count,
        "ambiguous_match_count": ambiguous_match_count,
        "unmatched_count": unmatched_count,
        "exact_matches_with_fill_color": exact_matches_with_fill_color,
        "exact_matches_with_fill_booleans": exact_matches_with_fill_booleans,
        "fill_color_histogram": fill_color_histogram,
        "fill_boolean_histogram": fill_boolean_histogram,
        "cells": cells,
        "guardrails": [
            "Cells are joined only by exact authoritative page-space geometry; no row-major/order guess is used.",
            "Raw child OfficeArt fill values remain observations; no generic border or table-style semantics are inferred.",
            "Receipt emits no document text, object ids, source offsets, filenames, or raw bytes."
        ]
    });

    assert!(table_count > 0, "fixture exposes at least one bounded TABLE");
    assert_eq!(
        exact_match_count + ambiguous_match_count + unmatched_count,
        table_cell_count,
        "every bounded cell must be classified"
    );

    if let Some(path) = std::env::var_os("CHAPTERA_TABLE_CELL_PAINT_RECEIPT") {
        fs::write(
            path,
            serde_json::to_vec_pretty(&receipt).expect("serialize TABLE cell join receipt"),
        )
        .expect("write TABLE cell join receipt");
    }

    println!(
        "TABLE_CELL_PAINT_JOIN tables={} cells={} owner_unique={} children={} exact={} ambiguous={} unmatched={} fill_color={} fill_booleans={}",
        table_count,
        table_cell_count,
        owner_unique_count,
        direct_child_count,
        exact_match_count,
        ambiguous_match_count,
        unmatched_count,
        exact_matches_with_fill_color,
        exact_matches_with_fill_booleans,
    );
}
