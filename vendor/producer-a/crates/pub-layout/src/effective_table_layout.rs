use crate::{
    BoundedLayoutProjection, BoundedResolvedTableCells, ProjectionSeverity, ResolveBlocked,
    ResolvedTableCell, TableCellOriginMapping,
};
use pub_model::{EffectiveTableGridV1, LengthEmu, NodeId};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BoundedEffectiveTableResolveError {
    ProjectionBlocked(ResolveBlocked),
    DuplicateGrid {
        table_origin: NodeId,
    },
    GridForUnknownTable {
        table_origin: NodeId,
    },
    MissingGrid {
        table_origin: NodeId,
    },
    MissingTableGeometry {
        table_origin: NodeId,
    },
    InvalidGrid {
        table_origin: NodeId,
    },
    TopologyMismatch {
        table_origin: NodeId,
    },
    UnknownRowExtent {
        table_origin: NodeId,
        index: u32,
    },
    UnknownColumnExtent {
        table_origin: NodeId,
        index: u32,
    },
    NonPositiveRowExtent {
        table_origin: NodeId,
        index: u32,
        value: i64,
    },
    NonPositiveColumnExtent {
        table_origin: NodeId,
        index: u32,
        value: i64,
    },
    MetricOverflow {
        table_origin: NodeId,
    },
    MetricsExceedTableBounds {
        table_origin: NodeId,
        required_width: i64,
        required_height: i64,
        available_width: i64,
        available_height: i64,
    },
}

impl fmt::Display for BoundedEffectiveTableResolveError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ProjectionBlocked(error) => write!(
                formatter,
                "projection blocked table resolution with {} errors",
                error.projection_errors.len()
            ),
            Self::DuplicateGrid { table_origin } => {
                write!(
                    formatter,
                    "duplicate effective grid for table {}",
                    table_origin.as_canonical()
                )
            }
            Self::GridForUnknownTable { table_origin } => write!(
                formatter,
                "effective grid supplied for unknown table {}",
                table_origin.as_canonical()
            ),
            Self::MissingGrid { table_origin } => write!(
                formatter,
                "missing effective grid for table {}",
                table_origin.as_canonical()
            ),
            Self::MissingTableGeometry { table_origin } => write!(
                formatter,
                "table {} has no authored node geometry",
                table_origin.as_canonical()
            ),
            Self::InvalidGrid { table_origin } => write!(
                formatter,
                "effective grid for table {} is invalid",
                table_origin.as_canonical()
            ),
            Self::TopologyMismatch { table_origin } => write!(
                formatter,
                "effective grid topology does not match projected table {}",
                table_origin.as_canonical()
            ),
            Self::UnknownRowExtent {
                table_origin,
                index,
            } => write!(
                formatter,
                "table {} row {} has unknown effective extent",
                table_origin.as_canonical(),
                index
            ),
            Self::UnknownColumnExtent {
                table_origin,
                index,
            } => write!(
                formatter,
                "table {} column {} has unknown effective extent",
                table_origin.as_canonical(),
                index
            ),
            Self::NonPositiveRowExtent {
                table_origin,
                index,
                value,
            } => write!(
                formatter,
                "table {} row {} has non-positive effective extent {} EMU",
                table_origin.as_canonical(),
                index,
                value
            ),
            Self::NonPositiveColumnExtent {
                table_origin,
                index,
                value,
            } => write!(
                formatter,
                "table {} column {} has non-positive effective extent {} EMU",
                table_origin.as_canonical(),
                index,
                value
            ),
            Self::MetricOverflow { table_origin } => write!(
                formatter,
                "table {} effective track arithmetic overflowed",
                table_origin.as_canonical()
            ),
            Self::MetricsExceedTableBounds {
                table_origin,
                required_width,
                required_height,
                available_width,
                available_height,
            } => write!(
                formatter,
                "table {} effective tracks require {}x{} EMU but owner bounds provide {}x{} EMU",
                table_origin.as_canonical(),
                required_width,
                required_height,
                available_width,
                available_height
            ),
        }
    }
}

impl std::error::Error for BoundedEffectiveTableResolveError {}

fn checked_offsets(
    table_origin: NodeId,
    extents: &[(u32, Option<LengthEmu>)],
    row_axis: bool,
) -> Result<(Vec<i64>, i64), BoundedEffectiveTableResolveError> {
    let mut offsets = Vec::with_capacity(extents.len());
    let mut total = 0i64;
    for (index, extent) in extents {
        offsets.push(total);
        let extent = match extent {
            Some(extent) => *extent,
            None if row_axis => {
                return Err(BoundedEffectiveTableResolveError::UnknownRowExtent {
                    table_origin,
                    index: *index,
                });
            }
            None => {
                return Err(BoundedEffectiveTableResolveError::UnknownColumnExtent {
                    table_origin,
                    index: *index,
                });
            }
        };
        if extent.get() <= 0 {
            return if row_axis {
                Err(BoundedEffectiveTableResolveError::NonPositiveRowExtent {
                    table_origin,
                    index: *index,
                    value: extent.get(),
                })
            } else {
                Err(BoundedEffectiveTableResolveError::NonPositiveColumnExtent {
                    table_origin,
                    index: *index,
                    value: extent.get(),
                })
            };
        }
        total = total
            .checked_add(extent.get())
            .ok_or(BoundedEffectiveTableResolveError::MetricOverflow { table_origin })?;
    }
    Ok((offsets, total))
}

pub fn resolve_bounded_effective_table_cells(
    projection: &BoundedLayoutProjection,
    grids: &[EffectiveTableGridV1],
) -> Result<BoundedResolvedTableCells, BoundedEffectiveTableResolveError> {
    let projection_errors = projection
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.severity == ProjectionSeverity::Error)
        .cloned()
        .collect::<Vec<_>>();
    if !projection_errors.is_empty() {
        return Err(BoundedEffectiveTableResolveError::ProjectionBlocked(
            ResolveBlocked { projection_errors },
        ));
    }

    let table_ids = projection
        .tables
        .iter()
        .map(|table| table.origin)
        .collect::<BTreeSet<_>>();
    let mut grid_map = BTreeMap::new();
    for grid in grids {
        if !table_ids.contains(&grid.table_id) {
            return Err(BoundedEffectiveTableResolveError::GridForUnknownTable {
                table_origin: grid.table_id,
            });
        }
        if grid_map.insert(grid.table_id, grid).is_some() {
            return Err(BoundedEffectiveTableResolveError::DuplicateGrid {
                table_origin: grid.table_id,
            });
        }
    }

    let geometry = projection
        .node_geometry
        .iter()
        .map(|node| (node.origin, node))
        .collect::<BTreeMap<_, _>>();

    let mut cells = Vec::new();
    let mut origin_mapping = Vec::new();

    for table in &projection.tables {
        let grid = grid_map
            .get(&table.origin)
            .copied()
            .ok_or(BoundedEffectiveTableResolveError::MissingGrid {
                table_origin: table.origin,
            })?;
        grid.validate()
            .map_err(|_| BoundedEffectiveTableResolveError::InvalidGrid {
                table_origin: table.origin,
            })?;

        if grid.rows.len() != usize::try_from(table.rows).unwrap_or(usize::MAX)
            || grid.columns.len() != usize::try_from(table.columns).unwrap_or(usize::MAX)
            || grid.cells.len() != table.cells.len()
            || table.cells.iter().any(|projected| {
                !grid.cells.iter().any(|cell| {
                    cell.id == projected.origin && cell.address == projected.address
                })
            })
        {
            return Err(BoundedEffectiveTableResolveError::TopologyMismatch {
                table_origin: table.origin,
            });
        }

        let owner = geometry
            .get(&table.origin)
            .copied()
            .ok_or(BoundedEffectiveTableResolveError::MissingTableGeometry {
                table_origin: table.origin,
            })?;

        let row_extents = grid
            .rows
            .iter()
            .map(|track| (track.index, track.extent))
            .collect::<Vec<_>>();
        let column_extents = grid
            .columns
            .iter()
            .map(|track| (track.index, track.extent))
            .collect::<Vec<_>>();
        let (row_offsets, required_height) = checked_offsets(table.origin, &row_extents, true)?;
        let (column_offsets, required_width) =
            checked_offsets(table.origin, &column_extents, false)?;

        if required_width > owner.bounds.width.get() || required_height > owner.bounds.height.get() {
            return Err(BoundedEffectiveTableResolveError::MetricsExceedTableBounds {
                table_origin: table.origin,
                required_width,
                required_height,
                available_width: owner.bounds.width.get(),
                available_height: owner.bounds.height.get(),
            });
        }

        for projected in &table.cells {
            let row = usize::try_from(projected.address.row).map_err(|_| {
                BoundedEffectiveTableResolveError::TopologyMismatch {
                    table_origin: table.origin,
                }
            })?;
            let column = usize::try_from(projected.address.column).map_err(|_| {
                BoundedEffectiveTableResolveError::TopologyMismatch {
                    table_origin: table.origin,
                }
            })?;
            let row_extent = row_extents
                .get(row)
                .and_then(|(_, extent)| *extent)
                .ok_or(BoundedEffectiveTableResolveError::TopologyMismatch {
                    table_origin: table.origin,
                })?;
            let column_extent = column_extents
                .get(column)
                .and_then(|(_, extent)| *extent)
                .ok_or(BoundedEffectiveTableResolveError::TopologyMismatch {
                    table_origin: table.origin,
                })?;
            let x = owner
                .bounds
                .x
                .get()
                .checked_add(
                    *column_offsets
                        .get(column)
                        .ok_or(BoundedEffectiveTableResolveError::TopologyMismatch {
                            table_origin: table.origin,
                        })?,
                )
                .ok_or(BoundedEffectiveTableResolveError::MetricOverflow {
                    table_origin: table.origin,
                })?;
            let y = owner
                .bounds
                .y
                .get()
                .checked_add(
                    *row_offsets
                        .get(row)
                        .ok_or(BoundedEffectiveTableResolveError::TopologyMismatch {
                            table_origin: table.origin,
                        })?,
                )
                .ok_or(BoundedEffectiveTableResolveError::MetricOverflow {
                    table_origin: table.origin,
                })?;

            cells.push(ResolvedTableCell {
                origin: projected.origin,
                table_origin: table.origin,
                address: projected.address,
                bounds: pub_model::RectEmu::new(
                    LengthEmu::new(x),
                    LengthEmu::new(y),
                    column_extent,
                    row_extent,
                ),
                transform: owner.transform.clone(),
            });
            origin_mapping.push(TableCellOriginMapping {
                cell_origin: projected.origin,
                table_origin: table.origin,
            });
        }
    }

    cells.sort_by_key(|cell| {
        (
            cell.table_origin,
            cell.address.row,
            cell.address.column,
            cell.origin,
        )
    });
    origin_mapping.sort_by_key(|mapping| (mapping.table_origin, mapping.cell_origin));

    Ok(BoundedResolvedTableCells {
        cells,
        origin_mapping,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        BoundedAuthoringSlice, BoundedNodeGeometryInput, bounded_table_input_from_effective_grid,
        project_bounded,
    };
    use pub_model::{
        Affine2D, CanonicalId, DocumentId, EFFECTIVE_TABLE_GRID_V1, EffectiveTableCellV1,
        EffectiveTableTrackV1, Page, PageId, Size2D, TableCellAddress, TableCellId, TableColumnId,
        TableRowId,
    };

    fn canonical(byte: u8) -> CanonicalId {
        CanonicalId::from_bytes([byte; 16])
    }

    fn page_id() -> PageId {
        PageId::from_canonical(canonical(1))
    }

    fn table_id() -> NodeId {
        NodeId::from_canonical(canonical(2))
    }

    fn row_id(byte: u8) -> TableRowId {
        TableRowId::from_canonical(canonical(byte))
    }

    fn column_id(byte: u8) -> TableColumnId {
        TableColumnId::from_canonical(canonical(byte))
    }

    fn cell_id(byte: u8) -> TableCellId {
        TableCellId::from_canonical(canonical(byte))
    }

    fn grid() -> EffectiveTableGridV1 {
        let rows = vec![
            EffectiveTableTrackV1 {
                id: row_id(10),
                index: 0,
                extent: Some(LengthEmu::new(200)),
            },
            EffectiveTableTrackV1 {
                id: row_id(11),
                index: 1,
                extent: Some(LengthEmu::new(250)),
            },
        ];
        let columns = vec![
            EffectiveTableTrackV1 {
                id: column_id(20),
                index: 0,
                extent: Some(LengthEmu::new(300)),
            },
            EffectiveTableTrackV1 {
                id: column_id(21),
                index: 1,
                extent: Some(LengthEmu::new(350)),
            },
        ];
        EffectiveTableGridV1 {
            version: EFFECTIVE_TABLE_GRID_V1.into(),
            table_id: table_id(),
            rows: rows.clone(),
            columns: columns.clone(),
            cells: vec![
                EffectiveTableCellV1 {
                    id: cell_id(30),
                    row_id: rows[0].id,
                    column_id: columns[0].id,
                    address: TableCellAddress { row: 0, column: 0 },
                    row_span: 1,
                    column_span: 1,
                    story_id: None,
                    utf16_start: None,
                    utf16_end: None,
                },
                EffectiveTableCellV1 {
                    id: cell_id(31),
                    row_id: rows[0].id,
                    column_id: columns[1].id,
                    address: TableCellAddress { row: 0, column: 1 },
                    row_span: 1,
                    column_span: 1,
                    story_id: None,
                    utf16_start: None,
                    utf16_end: None,
                },
                EffectiveTableCellV1 {
                    id: cell_id(32),
                    row_id: rows[1].id,
                    column_id: columns[0].id,
                    address: TableCellAddress { row: 1, column: 0 },
                    row_span: 1,
                    column_span: 1,
                    story_id: None,
                    utf16_start: None,
                    utf16_end: None,
                },
                EffectiveTableCellV1 {
                    id: cell_id(33),
                    row_id: rows[1].id,
                    column_id: columns[1].id,
                    address: TableCellAddress { row: 1, column: 1 },
                    row_span: 1,
                    column_span: 1,
                    story_id: None,
                    utf16_start: None,
                    utf16_end: None,
                },
            ],
        }
    }

    fn projection(grid: &EffectiveTableGridV1) -> BoundedLayoutProjection {
        let table = bounded_table_input_from_effective_grid(grid).expect("valid table input");
        project_bounded(BoundedAuthoringSlice {
            pages: vec![Page {
                id: page_id(),
                size: Size2D::new(LengthEmu::new(10_000), LengthEmu::new(10_000)),
                bleed: None,
                margins: None,
                children: vec![table_id()],
                extensions: Vec::new(),
            }],
            node_geometry: vec![BoundedNodeGeometryInput {
                node_id: table_id(),
                parent_origin: page_id().into_canonical(),
                bounds: pub_model::RectEmu::new(
                    LengthEmu::new(100),
                    LengthEmu::new(200),
                    LengthEmu::new(650),
                    LengthEmu::new(450),
                ),
                transform: Affine2D::identity(),
            }],
            stories: Vec::new(),
            story_frames: Vec::new(),
            tables: vec![table],
            guides: Vec::new(),
            unknown_layout_state: Vec::new(),
        })
    }

    #[test]
    fn nonuniform_effective_tracks_resolve_exact_cell_rectangles() {
        let grid = grid();
        let resolved =
            resolve_bounded_effective_table_cells(&projection(&grid), &[grid]).expect("resolve");

        assert_eq!(resolved.cells.len(), 4);
        assert_eq!(
            resolved.cells[0].bounds,
            pub_model::RectEmu::new(
                LengthEmu::new(100),
                LengthEmu::new(200),
                LengthEmu::new(300),
                LengthEmu::new(200),
            )
        );
        assert_eq!(
            resolved.cells[3].bounds,
            pub_model::RectEmu::new(
                LengthEmu::new(400),
                LengthEmu::new(400),
                LengthEmu::new(350),
                LengthEmu::new(250),
            )
        );
    }

    #[test]
    fn unknown_track_extent_fails_closed() {
        let mut grid = grid();
        grid.columns[1].extent = None;
        assert!(matches!(
            resolve_bounded_effective_table_cells(&projection(&grid), &[grid]),
            Err(BoundedEffectiveTableResolveError::UnknownColumnExtent {
                table_origin,
                index: 1,
            }) if table_origin == table_id()
        ));
    }

    #[test]
    fn topology_mismatch_fails_closed() {
        let grid = grid();
        let projection = projection(&grid);
        let mut mismatched = grid.clone();
        mismatched.cells.swap(0, 1);
        assert!(matches!(
            resolve_bounded_effective_table_cells(&projection, &[mismatched]),
            Err(BoundedEffectiveTableResolveError::InvalidGrid { .. })
                | Err(BoundedEffectiveTableResolveError::TopologyMismatch { .. })
        ));
    }

    #[test]
    fn track_metrics_may_not_exceed_owner_bounds() {
        let grid = grid();
        let mut projection = projection(&grid);
        projection.node_geometry[0].bounds = pub_model::RectEmu::new(
            LengthEmu::new(100),
            LengthEmu::new(200),
            LengthEmu::new(649),
            LengthEmu::new(450),
        );
        assert!(matches!(
            resolve_bounded_effective_table_cells(&projection, &[grid]),
            Err(BoundedEffectiveTableResolveError::MetricsExceedTableBounds { .. })
        ));
    }
}
