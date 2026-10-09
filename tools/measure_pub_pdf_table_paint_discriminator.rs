#![recursion_limit = "256"]

use anyhow::{Context, Result, bail};
use pub_model::Affine2D;
use pub_reader::PubTableBorderAxis;
use pub_viewer::{open_pub_bundle, viewer_geometry_environment_v0_1};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::{env, fs, path::PathBuf};

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn main() -> Result<()> {
    let mut args = env::args_os().skip(1);
    let source = PathBuf::from(
        args.next()
            .context("usage: table-paint-discriminator SOURCE.pub OUTPUT.json")?,
    );
    let output = PathBuf::from(
        args.next()
            .context("usage: table-paint-discriminator SOURCE.pub OUTPUT.json")?,
    );
    if args.next().is_some() {
        bail!("table-paint-discriminator accepts exactly SOURCE.pub OUTPUT.json");
    }

    let bytes = fs::read(&source).with_context(|| format!("read {}", source.display()))?;
    let source_sha256 = sha256_hex(&bytes);
    let bundle = open_pub_bundle(&bytes, viewer_geometry_environment_v0_1())
        .context("open PUB bundle")?;
    let visual = &bundle.geometry;

    let mut rows = Vec::new();
    for table in &visual.tables {
        let source_node = bundle
            .resolved_graph
            .nodes
            .get(&table.node_id)
            .context("Viewer TABLE node missing from resolved graph")?;
        let source_table = source_node
            .payload
            .table
            .as_ref()
            .context("Viewer TABLE node missing source TABLE payload")?;
        let owner = visual.scene.nodes.iter().find(|node| node.origin == table.node_id);

        let merged_cell_count = table
            .cells
            .iter()
            .filter(|cell| cell.row_span > 1 || cell.column_span > 1)
            .count();
        let cell_bounds_count = table.cells.iter().filter(|cell| cell.bounds.is_some()).count();
        let fill_true_count = table
            .cells
            .iter()
            .filter(|cell| cell.fill_visible == Some(true))
            .count();
        let fill_false_count = table
            .cells
            .iter()
            .filter(|cell| cell.fill_visible == Some(false))
            .count();
        let fill_unknown_count = table
            .cells
            .len()
            .saturating_sub(fill_true_count + fill_false_count);
        let visible_fill_rgb_count = table
            .cells
            .iter()
            .filter(|cell| cell.fill_visible == Some(true) && cell.fill_rgb.is_some())
            .count();
        let visible_fill_white_count = table
            .cells
            .iter()
            .filter(|cell| {
                cell.fill_visible == Some(true) && cell.fill_rgb == Some([255, 255, 255])
            })
            .count();
        let visible_fill_nonwhite_count =
            visible_fill_rgb_count.saturating_sub(visible_fill_white_count);
        let visible_fill_unique_color_count = table
            .cells
            .iter()
            .filter(|cell| cell.fill_visible == Some(true))
            .filter_map(|cell| cell.fill_rgb)
            .collect::<BTreeSet<_>>()
            .len();
        let nonempty_text_cell_count = table
            .cells
            .iter()
            .filter(|cell| !cell.text.is_empty())
            .count();

        let horizontal_border_count = table
            .borders
            .iter()
            .filter(|border| border.y1_emu == border.y2_emu && border.x1_emu < border.x2_emu)
            .count();
        let vertical_border_count = table
            .borders
            .iter()
            .filter(|border| border.x1_emu == border.x2_emu && border.y1_emu < border.y2_emu)
            .count();
        let invalid_border_count = table
            .borders
            .len()
            .saturating_sub(horizontal_border_count + vertical_border_count);
        let border_widths = table
            .borders
            .iter()
            .map(|border| border.width_emu)
            .collect::<BTreeSet<_>>();

        let source_merged_cell_count = source_table
            .cells
            .iter()
            .filter_map(|cell| cell.coordinates)
            .filter(|c| c.start_row != c.end_row || c.start_column != c.end_column)
            .count();
        let source_cell_bounds_count = source_table
            .cells
            .iter()
            .filter(|cell| cell.bounds.is_some())
            .count();
        let source_cell_paint_count = source_table
            .cells
            .iter()
            .filter(|cell| cell.paint.is_some())
            .count();
        let source_fill_true_count = source_table
            .cells
            .iter()
            .filter(|cell| cell.paint.as_ref().is_some_and(|paint| paint.fill_visible))
            .count();
        let source_fill_false_count = source_table
            .cells
            .iter()
            .filter(|cell| cell.paint.as_ref().is_some_and(|paint| !paint.fill_visible))
            .count();
        let source_plain_cell_fill_count = source_table
            .cells
            .iter()
            .filter(|cell| {
                cell.paint.as_ref().is_some_and(|paint| paint.source_refs.iter().any(|source_ref| {
                    source_ref.path.as_deref() == Some("SpContainer/FOPT/table-cell-fill")
                }))
            })
            .count();
        let source_autoformat_cell_fill_count = source_table
            .cells
            .iter()
            .filter(|cell| {
                cell.paint.as_ref().is_some_and(|paint| paint.source_refs.iter().any(|source_ref| {
                    source_ref.path.as_deref() == Some("SpContainer/FOPT/table-autoformat-cell-fill")
                }))
            })
            .count();
        let source_cell_fill_scheme_ref_count = source_table
            .cells
            .iter()
            .filter(|cell| {
                cell.paint.as_ref().is_some_and(|paint| paint.source_refs.iter().any(|source_ref| {
                    source_ref.path.as_deref() == Some("OplSccm/current-color-scheme")
                }))
            })
            .count();
        let source_cell_fill_dgg_default_ref_count = source_table
            .cells
            .iter()
            .filter(|cell| {
                cell.paint.as_ref().is_some_and(|paint| paint.source_refs.iter().any(|source_ref| {
                    source_ref.path.as_deref() == Some("DggContainer/FOPT-defaults")
                }))
            })
            .count();
        let source_border_scheme_ref_count = source_table
            .border_segments
            .iter()
            .filter(|segment| segment.source_refs.iter().any(|source_ref| {
                source_ref.path.as_deref() == Some("OplSccm/current-color-scheme")
            }))
            .count();
        let source_horizontal_border_count = source_table
            .border_segments
            .iter()
            .filter(|segment| segment.axis == PubTableBorderAxis::Horizontal)
            .count();
        let source_vertical_border_count = source_table
            .border_segments
            .iter()
            .filter(|segment| segment.axis == PubTableBorderAxis::Vertical)
            .count();

        let (owner_present, owner_transform_identity, cell_union_matches_owner, visible_fill_area_ppm) =
            if let Some(owner) = owner {
                let all_bounds = table.cells.iter().all(|cell| cell.bounds.is_some());
                let union = if all_bounds && !table.cells.is_empty() {
                    let min_x = table.cells.iter().filter_map(|cell| cell.bounds).map(|b| b.x.get()).min();
                    let min_y = table.cells.iter().filter_map(|cell| cell.bounds).map(|b| b.y.get()).min();
                    let max_x = table.cells.iter().filter_map(|cell| cell.bounds)
                        .map(|b| b.x.get().saturating_add(b.width.get())).max();
                    let max_y = table.cells.iter().filter_map(|cell| cell.bounds)
                        .map(|b| b.y.get().saturating_add(b.height.get())).max();
                    match (min_x, min_y, max_x, max_y) {
                        (Some(x1), Some(y1), Some(x2), Some(y2)) => {
                            x1 == owner.bounds.x.get()
                                && y1 == owner.bounds.y.get()
                                && x2 == owner.bounds.x.get().saturating_add(owner.bounds.width.get())
                                && y2 == owner.bounds.y.get().saturating_add(owner.bounds.height.get())
                        }
                        _ => false,
                    }
                } else {
                    false
                };
                let owner_area = i128::from(owner.bounds.width.get().max(0))
                    * i128::from(owner.bounds.height.get().max(0));
                let fill_area = table
                    .cells
                    .iter()
                    .filter(|cell| cell.fill_visible == Some(true))
                    .filter_map(|cell| cell.bounds)
                    .map(|b| i128::from(b.width.get().max(0)) * i128::from(b.height.get().max(0)))
                    .sum::<i128>();
                let ppm = if owner_area > 0 {
                    Some(((fill_area.saturating_mul(1_000_000)) / owner_area).clamp(0, 1_000_000) as i64)
                } else {
                    None
                };
                (true, owner.transform == Affine2D::identity(), union, ppm)
            } else {
                (false, false, false, None)
            };

        rows.push(json!({
            "rows": table.rows,
            "columns": table.columns,
            "cell_count": table.cells.len(),
            "merged_cell_count": merged_cell_count,
            "cell_bounds_count": cell_bounds_count,
            "fill_true_count": fill_true_count,
            "fill_false_count": fill_false_count,
            "fill_unknown_count": fill_unknown_count,
            "visible_fill_rgb_count": visible_fill_rgb_count,
            "visible_fill_white_count": visible_fill_white_count,
            "visible_fill_nonwhite_count": visible_fill_nonwhite_count,
            "visible_fill_unique_color_count": visible_fill_unique_color_count,
            "nonempty_text_cell_count": nonempty_text_cell_count,
            "border_count": table.borders.len(),
            "horizontal_border_count": horizontal_border_count,
            "vertical_border_count": vertical_border_count,
            "invalid_border_count": invalid_border_count,
            "border_width_distinct_count": border_widths.len(),
            "border_width_min_emu": border_widths.iter().next().copied(),
            "border_width_max_emu": border_widths.iter().next_back().copied(),
            "owner_present": owner_present,
            "owner_transform_identity": owner_transform_identity,
            "cell_union_matches_owner": cell_union_matches_owner,
            "visible_fill_area_ppm_of_owner": visible_fill_area_ppm,
            "source": {
                "rows": source_table.rows,
                "columns": source_table.columns,
                "cell_count": source_table.cells.len(),
                "merged_cell_count": source_merged_cell_count,
                "cell_bounds_count": source_cell_bounds_count,
                "cell_paint_count": source_cell_paint_count,
                "fill_true_count": source_fill_true_count,
                "fill_false_count": source_fill_false_count,
                "plain_cell_fill_count": source_plain_cell_fill_count,
                "autoformat_cell_fill_count": source_autoformat_cell_fill_count,
                "cell_fill_scheme_ref_count": source_cell_fill_scheme_ref_count,
                "cell_fill_dgg_default_ref_count": source_cell_fill_dgg_default_ref_count,
                "border_scheme_ref_count": source_border_scheme_ref_count,
                "simple_table": source_table.simple_table.is_some(),
                "story_id_present": source_table.story_id.is_some(),
                "cells_seq_num_present": source_table.cells_seq_num.is_some(),
                "tcd_story_ordinal_present": source_table.tcd_story_ordinal.is_some(),
                "layout_relation_present": source_table.layout_relation.is_some(),
                "uniform_vertical_alignment_present": source_table
                    .layout_relation
                    .as_ref()
                    .and_then(|relation| relation.uniform_cell_vertical_alignment.as_ref())
                    .is_some(),
                "uniform_text_inset_present": source_table.uniform_cell_text_inset.is_some(),
                "layout_metrics_present": source_table.layout_metrics.is_some(),
                "border_count": source_table.border_segments.len(),
                "horizontal_border_count": source_horizontal_border_count,
                "vertical_border_count": source_vertical_border_count,
            }
        }));
    }

    let payload = json!({
        "schema": "chaptera.pub-pdf-table-paint-discriminator.v2",
        "source_sha256": source_sha256,
        "table_count": rows.len(),
        "tables": rows,
        "source_text_emitted": false,
        "source_ids_emitted": false,
    });
    fs::write(
        &output,
        serde_json::to_vec_pretty(&payload).context("serialize TABLE discriminator")?,
    )
    .with_context(|| format!("write {}", output.display()))?;
    Ok(())
}
