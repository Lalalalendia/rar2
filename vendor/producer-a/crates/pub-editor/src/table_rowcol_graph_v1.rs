use crate::{
    TableCellContentSnapshotV1, TableRowColHistoryErrorV1, TableStructureSnapshotV1,
    validate_table_structure_snapshot_v1,
};
use pub_model::{EffectiveTableGridV1, NodeId, RectEmu, SimpleRectangularTable, SimpleTableCell};
use pub_reader::{
    PubResolvedGraph, PubTableCellCoordinates, PubTableCellSource,
    materialize_bounded_simple_table_cells,
};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TableRowColGraphErrorV1 {
    MissingTable,
    MissingStory,
    StoryMismatch,
    NotSimpleRectangular,
    UnsupportedBorderSegments,
    GridMismatch,
    MaterializationFailed,
    InvalidSnapshot(TableRowColHistoryErrorV1),
    ApplyMismatch,
}

impl From<TableRowColHistoryErrorV1> for TableRowColGraphErrorV1 {
    fn from(value: TableRowColHistoryErrorV1) -> Self {
        Self::InvalidSnapshot(value)
    }
}

fn topology_matches_grid(table: &pub_reader::PubTableSource, grid: &EffectiveTableGridV1) -> bool {
    let Some(simple) = table.simple_table.as_ref() else {
        return false;
    };
    if simple.rows != u32::try_from(grid.rows.len()).unwrap_or(u32::MAX)
        || simple.columns != u32::try_from(grid.columns.len()).unwrap_or(u32::MAX)
        || simple.cells.len() != grid.cells.len()
    {
        return false;
    }
    simple.cells.iter().all(|simple_cell| {
        grid.cells
            .iter()
            .any(|cell| cell.id == simple_cell.id && cell.address == simple_cell.address)
    })
}

pub fn table_structure_snapshot_from_graph_v1(
    graph: &PubResolvedGraph,
    table_id: NodeId,
    grid: EffectiveTableGridV1,
    bounds: RectEmu,
) -> Result<TableStructureSnapshotV1, TableRowColGraphErrorV1> {
    if grid.table_id != table_id {
        return Err(TableRowColGraphErrorV1::GridMismatch);
    }
    let node = graph
        .nodes
        .get(&table_id)
        .ok_or(TableRowColGraphErrorV1::MissingTable)?;
    let table = node
        .payload
        .table
        .as_ref()
        .ok_or(TableRowColGraphErrorV1::MissingTable)?;
    if !table.border_segments.is_empty() {
        return Err(TableRowColGraphErrorV1::UnsupportedBorderSegments);
    }
    if !topology_matches_grid(table, &grid) {
        return Err(TableRowColGraphErrorV1::GridMismatch);
    }
    let story_id = table
        .story_id
        .ok_or(TableRowColGraphErrorV1::MissingStory)?;
    let story = graph
        .stories
        .get(&story_id)
        .ok_or(TableRowColGraphErrorV1::MissingStory)?;
    if table.story_id != Some(story.id) {
        return Err(TableRowColGraphErrorV1::StoryMismatch);
    }

    let materialized = materialize_bounded_simple_table_cells(table, story)
        .map_err(|_| TableRowColGraphErrorV1::MaterializationFailed)?;
    let source_by_id = table
        .cells
        .iter()
        .map(|cell| (cell.id, cell))
        .collect::<BTreeMap<_, _>>();

    let mut cells = materialized
        .into_iter()
        .map(|cell| {
            if cell.row_span != 1 || cell.column_span != 1 {
                return Err(TableRowColGraphErrorV1::NotSimpleRectangular);
            }
            let source = source_by_id
                .get(&cell.id)
                .copied()
                .ok_or(TableRowColGraphErrorV1::GridMismatch)?;
            Ok(TableCellContentSnapshotV1 {
                cell_id: cell.id,
                text: cell.text,
                stored_record_index: source.stored_record_index,
                bounds: source.bounds,
                paint: source.paint.clone(),
                source_refs: source.source_refs.clone(),
            })
        })
        .collect::<Result<Vec<_>, TableRowColGraphErrorV1>>()?;

    cells.sort_by_key(|content| {
        grid.cells
            .iter()
            .find(|cell| cell.id == content.cell_id)
            .map(|cell| (cell.address.row, cell.address.column, cell.id))
            .unwrap_or((u32::MAX, u32::MAX, content.cell_id))
    });

    let snapshot = TableStructureSnapshotV1 {
        grid,
        bounds,
        story_id,
        story_text: story.text.clone(),
        cells,
    };
    validate_table_structure_snapshot_v1(&snapshot)?;
    Ok(snapshot)
}

fn source_cells_from_snapshot(
    snapshot: &TableStructureSnapshotV1,
) -> Result<Vec<PubTableCellSource>, TableRowColGraphErrorV1> {
    let preservation = snapshot
        .cells
        .iter()
        .map(|cell| (cell.cell_id, cell))
        .collect::<BTreeMap<_, _>>();
    let mut cells = Vec::with_capacity(snapshot.grid.cells.len());
    for semantic in &snapshot.grid.cells {
        let preserved = preservation
            .get(&semantic.id)
            .copied()
            .ok_or(TableRowColGraphErrorV1::GridMismatch)?;
        let utf16_start = semantic
            .utf16_start
            .ok_or(TableRowColGraphErrorV1::GridMismatch)?;
        let utf16_end = semantic
            .utf16_end
            .ok_or(TableRowColGraphErrorV1::GridMismatch)?;
        cells.push(PubTableCellSource {
            id: semantic.id,
            stored_record_index: preserved.stored_record_index,
            coordinates: Some(PubTableCellCoordinates {
                start_row: semantic.address.row,
                end_row: semantic.address.row,
                start_column: semantic.address.column,
                end_column: semantic.address.column,
            }),
            utf16_start,
            utf16_end,
            bounds: preserved.bounds,
            paint: preserved.paint.clone(),
            source_refs: preserved.source_refs.clone(),
        });
    }
    cells.sort_by_key(|cell| cell.stored_record_index);
    Ok(cells)
}

pub fn apply_table_structure_snapshot_to_graph_v1(
    graph: &mut PubResolvedGraph,
    snapshot: &TableStructureSnapshotV1,
) -> Result<(), TableRowColGraphErrorV1> {
    validate_table_structure_snapshot_v1(snapshot)?;

    let mut candidate = graph.clone();
    let node = candidate
        .nodes
        .get_mut(&snapshot.grid.table_id)
        .ok_or(TableRowColGraphErrorV1::MissingTable)?;
    let table = node
        .payload
        .table
        .as_mut()
        .ok_or(TableRowColGraphErrorV1::MissingTable)?;
    if !table.border_segments.is_empty() {
        return Err(TableRowColGraphErrorV1::UnsupportedBorderSegments);
    }
    if table.story_id != Some(snapshot.story_id) {
        return Err(TableRowColGraphErrorV1::StoryMismatch);
    }

    let rows = u32::try_from(snapshot.grid.rows.len())
        .map_err(|_| TableRowColGraphErrorV1::GridMismatch)?;
    let columns = u32::try_from(snapshot.grid.columns.len())
        .map_err(|_| TableRowColGraphErrorV1::GridMismatch)?;
    let mut simple_cells = snapshot
        .grid
        .cells
        .iter()
        .map(|cell| SimpleTableCell {
            id: cell.id,
            address: cell.address,
        })
        .collect::<Vec<_>>();
    simple_cells.sort_by_key(|cell| (cell.address.row, cell.address.column, cell.id));
    let simple_table = SimpleRectangularTable::new(rows, columns, simple_cells)
        .map_err(|_| TableRowColGraphErrorV1::NotSimpleRectangular)?;

    table.rows = rows;
    table.columns = columns;
    table.simple_table = Some(simple_table);
    table.cells = source_cells_from_snapshot(snapshot)?;

    candidate
        .stories
        .get_mut(&snapshot.story_id)
        .ok_or(TableRowColGraphErrorV1::MissingStory)?
        .text
        .clone_from(&snapshot.story_text);

    let round_trip = table_structure_snapshot_from_graph_v1(
        &candidate,
        snapshot.grid.table_id,
        snapshot.grid.clone(),
        snapshot.bounds,
    )?;
    if round_trip != *snapshot {
        return Err(TableRowColGraphErrorV1::ApplyMismatch);
    }

    *graph = candidate;
    Ok(())
}
