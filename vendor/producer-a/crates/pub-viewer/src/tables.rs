use crate::{ViewerDiagnostic, ViewerDiagnosticSeverity, ViewerTextVerticalAlignment};
use pub_layout::{
    BoundedLayoutProjection, BoundedUniformTableMetrics, resolve_bounded_uniform_table_cells,
};
use pub_model::{NodeId, RectEmu, StoryId, TableCellAddress, TableCellId};
use pub_reader::{
    PubResolvedGraph, PubTextFrameVerticalAlignment, materialize_bounded_table_cells,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewerTable {
    pub node_id: NodeId,
    pub story_id: StoryId,
    pub rows: u32,
    pub columns: u32,
    pub cells: Vec<ViewerTableCell>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uniform_cell_text_inset_emu: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uniform_cell_vertical_alignment: Option<ViewerTextVerticalAlignment>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub borders: Vec<ViewerTableBorderSegment>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewerTableBorderSegment {
    pub x1_emu: i64,
    pub y1_emu: i64,
    pub x2_emu: i64,
    pub y2_emu: i64,
    pub rgb: [u8; 3],
    pub width_emu: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewerTableCell {
    pub id: TableCellId,
    pub address: TableCellAddress,
    #[serde(
        default = "default_table_span",
        skip_serializing_if = "table_span_is_one"
    )]
    pub row_span: u32,
    #[serde(
        default = "default_table_span",
        skip_serializing_if = "table_span_is_one"
    )]
    pub column_span: u32,
    /// Story-global Unicode-scalar bounds for this exact cell text.
    ///
    /// Missing remains explicit for non-mature/legacy producers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub story_scalar_start: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub story_scalar_end: Option<u32>,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bounds: Option<RectEmu>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill_rgb: Option<[u8; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill_visible: Option<bool>,
}

pub(super) fn unique_table_column_boundary(
    cells: &[ViewerTableCell],
    column: u32,
    columns: u32,
) -> Option<i64> {
    if columns == 0 || column > columns {
        return None;
    }

    let mut boundary = None;
    for cell in cells {
        let end = cell.address.column.checked_add(cell.column_span)?;
        let starts_here = cell.address.column == column;
        let ends_here = end == column;
        if !starts_here && !ends_here {
            continue;
        }
        let bounds = cell.bounds?;
        let candidate = if starts_here {
            bounds.x.get()
        } else {
            bounds.x.get().checked_add(bounds.width.get())?
        };
        match boundary {
            None => boundary = Some(candidate),
            Some(existing) if existing == candidate => {}
            Some(_) => return None,
        }
    }
    boundary
}

fn unique_table_row_boundary(cells: &[ViewerTableCell], row: u32, rows: u32) -> Option<i64> {
    if rows == 0 || row > rows {
        return None;
    }

    let mut boundary = None;
    for cell in cells {
        let end = cell.address.row.checked_add(cell.row_span)?;
        let starts_here = cell.address.row == row;
        let ends_here = end == row;
        if !starts_here && !ends_here {
            continue;
        }
        let bounds = cell.bounds?;
        let candidate = if starts_here {
            bounds.y.get()
        } else {
            bounds.y.get().checked_add(bounds.height.get())?
        };
        match boundary {
            None => boundary = Some(candidate),
            Some(existing) if existing == candidate => {}
            Some(_) => return None,
        }
    }
    boundary
}

fn viewer_table_border_segments(
    source: &pub_reader::PubTableSource,
    cells: &[ViewerTableCell],
) -> Vec<ViewerTableBorderSegment> {
    source
        .border_segments
        .iter()
        .filter_map(|segment| match segment.axis {
            pub_reader::PubTableBorderAxis::Horizontal => {
                let y = unique_table_row_boundary(cells, segment.row_start, source.rows)?;
                let x1 = unique_table_column_boundary(cells, segment.column_start, source.columns)?;
                let x2 = unique_table_column_boundary(cells, segment.column_end, source.columns)?;
                (x1 < x2).then_some(ViewerTableBorderSegment {
                    x1_emu: x1,
                    y1_emu: y,
                    x2_emu: x2,
                    y2_emu: y,
                    rgb: segment.rgb,
                    width_emu: segment.width_emu,
                })
            }
            pub_reader::PubTableBorderAxis::Vertical => {
                let x = unique_table_column_boundary(cells, segment.column_start, source.columns)?;
                let y1 = unique_table_row_boundary(cells, segment.row_start, source.rows)?;
                let y2 = unique_table_row_boundary(cells, segment.row_end, source.rows)?;
                (y1 < y2).then_some(ViewerTableBorderSegment {
                    x1_emu: x,
                    y1_emu: y1,
                    x2_emu: x,
                    y2_emu: y2,
                    rgb: segment.rgb,
                    width_emu: segment.width_emu,
                })
            }
        })
        .collect()
}

fn default_table_span() -> u32 {
    1
}

fn table_span_is_one(value: &u32) -> bool {
    *value == 1
}

pub(super) fn viewer_tables_from_resolved(
    graph: &PubResolvedGraph,
    projection: &BoundedLayoutProjection,
) -> (Vec<ViewerTable>, Vec<ViewerDiagnostic>) {
    let mut tables = Vec::new();
    let mut diagnostics = Vec::new();
    let visible_node_ids = projection
        .node_geometry
        .iter()
        .map(|geometry| geometry.origin)
        .collect::<BTreeSet<_>>();

    for node in graph.nodes.values() {
        let node_id = node.header.id;
        if !visible_node_ids.contains(&node_id) {
            continue;
        }
        let Some(source) = node.payload.table.as_ref() else {
            continue;
        };
        let Some(story_id) = source.story_id else {
            continue;
        };
        let Some(story) = graph.stories.get(&story_id) else {
            continue;
        };

        let materialized = match materialize_bounded_table_cells(source, story) {
            Ok(cells) => cells,
            Err(error) => {
                diagnostics.push(ViewerDiagnostic {
                    code: "viewer.table.cell_text_unavailable".to_owned(),
                    severity: ViewerDiagnosticSeverity::FidelityWarning,
                    message: format!(
                        "Bounded table cells could not be materialized safely ({error:?})."
                    ),
                });
                continue;
            }
        };

        let needs_fallback_geometry = materialized.iter().any(|cell| cell.bounds.is_none());
        let projected = projection
            .tables
            .iter()
            .find(|table| table.origin == node_id);
        let resolved_bounds = if needs_fallback_geometry {
            source
                .layout_metrics
                .as_ref()
                .zip(projected)
                .and_then(|(metrics, projected)| {
                    let mut table_projection = projection.clone();
                    table_projection
                        .tables
                        .retain(|table| table.origin == projected.origin);
                    table_projection
                        .node_geometry
                        .retain(|geometry| geometry.origin == projected.origin);
                    table_projection.diagnostics.clear();

                    resolve_bounded_uniform_table_cells(
                        &table_projection,
                        &[BoundedUniformTableMetrics {
                            table_origin: projected.origin,
                            cell_width: metrics.cell_width,
                            row_pitch: metrics.row_pitch,
                        }],
                    )
                    .ok()
                })
        } else {
            None
        };

        if needs_fallback_geometry && source.layout_metrics.is_some() && resolved_bounds.is_none() {
            diagnostics.push(ViewerDiagnostic {
                code: "viewer.table.cell_geometry_unavailable".to_owned(),
                severity: ViewerDiagnosticSeverity::FidelityWarning,
                message: "Exact table track geometry was unavailable and the bounded fallback cell-geometry resolver rejected this table; semantic cells remain available.".to_owned(),
            });
        }

        let cells: Vec<ViewerTableCell> = materialized
            .into_iter()
            .map(|cell| ViewerTableCell {
                id: cell.id,
                address: cell.address,
                row_span: cell.row_span,
                column_span: cell.column_span,
                story_scalar_start: Some(cell.story_scalar_start),
                story_scalar_end: Some(cell.story_scalar_end),
                text: cell.text,
                bounds: cell.bounds.or_else(|| {
                    resolved_bounds.as_ref().and_then(|resolved| {
                        resolved
                            .cells
                            .iter()
                            .find(|candidate| candidate.origin == cell.id)
                            .map(|candidate| candidate.bounds)
                    })
                }),
                fill_rgb: cell.fill_rgb,
                fill_visible: cell.fill_visible,
            })
            .collect();

        let borders = viewer_table_border_segments(source, &cells);
        tables.push(ViewerTable {
            node_id,
            story_id,
            rows: source.rows,
            columns: source.columns,
            cells,
            uniform_cell_text_inset_emu: source
                .uniform_cell_text_inset
                .as_ref()
                .map(|inset| inset.inset_emu.get()),
            uniform_cell_vertical_alignment: source
                .layout_relation
                .as_ref()
                .and_then(|relation| relation.uniform_cell_vertical_alignment.as_ref())
                .map(|alignment| match alignment.alignment {
                    PubTextFrameVerticalAlignment::Top => ViewerTextVerticalAlignment::Top,
                    PubTextFrameVerticalAlignment::Center => ViewerTextVerticalAlignment::Center,
                    PubTextFrameVerticalAlignment::Bottom => ViewerTextVerticalAlignment::Bottom,
                }),
            borders,
        });
    }

    tables.sort_by_key(|table| table.node_id);
    (tables, diagnostics)
}
