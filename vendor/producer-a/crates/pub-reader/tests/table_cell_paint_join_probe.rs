use pub_core::StreamPath;
use pub_model::Sha256Digest;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::io::Cursor;
use std::path::PathBuf;

const FILL_COLOR_PROPERTY: u16 = 0x0181;

fn source_hash(bytes: &[u8]) -> Sha256Digest {
    let digest = Sha256::digest(bytes);
    let mut raw = [0_u8; 32];
    raw.copy_from_slice(&digest);
    Sha256Digest::from_bytes(raw)
}

fn project_axis(value: i64, source_start: i64, source_end: i64, target_start: i64, target_end: i64) -> Option<i64> {
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

#[test]
fn sample_table_cell_officeart_geometry_join_probe() {
    let fixture = PathBuf::from(
        std::env::var_os("CHAPTERA_TABLE_CELL_PAINT_FIXTURE")
            .expect("CHAPTERA_TABLE_CELL_PAINT_FIXTURE"),
    );
    let bytes = fs::read(&fixture).expect("read pinned Sample.pub");
    let build = pub_reader::build_mature_0x2c_source_graph(Cursor::new(bytes.as_slice()), source_hash(&bytes))
        .expect("Sample.pub mature SourceGraph");

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
            by_contents_seq.entry(field.value).or_default().push(shape_index);
        }
    }

    let mut tables = Vec::new();

    for node in build.graph.nodes.values() {
        let Some(table) = node.payload.table.as_ref() else {
            continue;
        };
        let Some(simple) = table.simple_table.as_ref() else {
            continue;
        };
        if simple.cells.len() != table.cells.len()
            || table.cells.iter().any(|cell| cell.bounds.is_none())
        {
            continue;
        }

        let owner_matches = by_contents_seq
            .get(&node.payload.contents_seq_num)
            .cloned()
            .unwrap_or_default();
        if owner_matches.len() != 1 {
            tables.push(serde_json::json!({
                "table_seq_num": node.payload.contents_seq_num,
                "cell_count": table.cells.len(),
                "owner_match_count": owner_matches.len(),
                "disposition": "owner_not_unique"
            }));
            continue;
        }

        let owner = &inventory.shapes[owner_matches[0]];
        let Some(group_rect) = owner.fspgr.as_ref() else {
            tables.push(serde_json::json!({
                "table_seq_num": node.payload.contents_seq_num,
                "cell_count": table.cells.len(),
                "owner_match_count": 1,
                "direct_child_count": 0,
                "disposition": "owner_has_no_fspgr"
            }));
            continue;
        };

        let children = inventory
            .shapes
            .iter()
            .filter(|shape| shape.parent_group_shape_source.as_ref() == Some(&owner.source))
            .filter_map(|shape| {
                let child_anchor = shape.child_anchor.as_ref()?;
                let projected = projected_child_bounds(child_anchor, group_rect, node.header.bounds)?;
                let fill_values = shape
                    .fopts
                    .iter()
                    .flat_map(|record| record.properties.iter())
                    .filter(|property| {
                        property.property_id() == FILL_COLOR_PROPERTY && !property.f_complex()
                    })
                    .map(|property| property.op)
                    .collect::<Vec<_>>();
                Some((shape, projected, fill_values))
            })
            .collect::<Vec<_>>();

        let mut cells = Vec::new();
        let mut exact_match_count = 0_usize;
        let mut ambiguous_match_count = 0_usize;
        let mut unmatched_count = 0_usize;
        let mut exact_matches_with_fill_0181 = 0_usize;

        for cell in &table.cells {
            let bounds = rect_array(cell.bounds.expect("prechecked bounds"));
            let matches = children
                .iter()
                .filter(|(_, projected, _)| *projected == bounds)
                .collect::<Vec<_>>();

            match matches.as_slice() {
                [(shape, projected, fill_values)] => {
                    exact_match_count += 1;
                    exact_matches_with_fill_0181 += usize::from(!fill_values.is_empty());
                    cells.push(serde_json::json!({
                        "stored_record_index": cell.stored_record_index,
                        "coordinates": cell.coordinates,
                        "cell_bounds": bounds,
                        "projected_child_bounds": projected,
                        "child_shape_source": &shape.source,
                        "fill_0181_raw_values": fill_values,
                        "disposition": "exact_geometry_match"
                    }));
                }
                [] => {
                    unmatched_count += 1;
                    let nearest = children
                        .iter()
                        .map(|(shape, projected, fill_values)| {
                            let delta = [
                                projected[0] - bounds[0],
                                projected[1] - bounds[1],
                                projected[2] - bounds[2],
                                projected[3] - bounds[3],
                            ];
                            let score = delta.iter().map(|value| value.unsigned_abs()).sum::<u64>();
                            (score, shape, projected, fill_values, delta)
                        })
                        .min_by_key(|(score, _, _, _, _)| *score);
                    cells.push(serde_json::json!({
                        "stored_record_index": cell.stored_record_index,
                        "coordinates": cell.coordinates,
                        "cell_bounds": bounds,
                        "nearest_child": nearest.map(|(score, shape, projected, fill_values, delta)| serde_json::json!({
                            "score": score,
                            "delta": delta,
                            "projected_child_bounds": projected,
                            "child_shape_source": &shape.source,
                            "fill_0181_raw_values": fill_values
                        })),
                        "disposition": "no_exact_geometry_match"
                    }));
                }
                many => {
                    ambiguous_match_count += 1;
                    cells.push(serde_json::json!({
                        "stored_record_index": cell.stored_record_index,
                        "coordinates": cell.coordinates,
                        "cell_bounds": bounds,
                        "match_count": many.len(),
                        "disposition": "ambiguous_exact_geometry_match"
                    }));
                }
            }
        }

        tables.push(serde_json::json!({
            "table_seq_num": node.payload.contents_seq_num,
            "cell_count": table.cells.len(),
            "owner_match_count": 1,
            "direct_child_count": children.len(),
            "exact_match_count": exact_match_count,
            "ambiguous_match_count": ambiguous_match_count,
            "unmatched_count": unmatched_count,
            "exact_matches_with_fill_0181": exact_matches_with_fill_0181,
            "projected_children": children.iter().map(|(shape, projected, fill_values)| serde_json::json!({
                "projected_bounds": projected,
                "child_shape_source": &shape.source,
                "fill_0181_raw_values": fill_values
            })).collect::<Vec<_>>(),
            "disposition": if exact_match_count == table.cells.len() {
                "complete_exact_geometry_join"
            } else {
                "incomplete_geometry_join"
            },
            "cells": cells
        }));
    }

    assert!(!tables.is_empty(), "Sample.pub exposes at least one bounded simple table");
    let receipt = serde_json::json!({
        "schema": "chaptera.pub-table-cell-officeart-join-probe.v1",
        "source_sha256": Sha256::digest(&bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>(),
        "table_count": tables.len(),
        "tables": tables
    });

    if let Some(path) = std::env::var_os("CHAPTERA_TABLE_CELL_PAINT_RECEIPT") {
        fs::write(
            path,
            serde_json::to_vec_pretty(&receipt).expect("serialize table-cell join receipt"),
        )
        .expect("write table-cell join receipt");
    }

    println!("{}", serde_json::to_string(&receipt).expect("receipt json"));
}
