use crate::rebuild_authored_table_story_v1;
use pub_model::{
    EffectiveTableCellV1, EffectiveTableGridV1, EffectiveTableTrackV1, LengthEmu, NodeId, RectEmu,
    SourceRef, StoryId, TableCellAddress, TableCellId, TableColumnId, TableRowId,
};
use pub_reader::PubTableCellPaintSource;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const TABLE_ROWCOL_HISTORY_V1: &str = "chaptera.table-rowcol-history.v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableCellContentSnapshotV1 {
    pub cell_id: TableCellId,
    pub text: String,
    pub stored_record_index: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bounds: Option<RectEmu>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paint: Option<PubTableCellPaintSource>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_refs: Vec<SourceRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableStructureSnapshotV1 {
    pub grid: EffectiveTableGridV1,
    pub bounds: RectEmu,
    pub story_id: StoryId,
    pub story_text: String,
    pub cells: Vec<TableCellContentSnapshotV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TableRowColMutationV1 {
    InsertRow {
        index: u32,
        row_id: TableRowId,
        cell_ids: Vec<TableCellId>,
        extent: LengthEmu,
    },
    DeleteRow {
        row_id: TableRowId,
    },
    InsertColumn {
        index: u32,
        column_id: TableColumnId,
        cell_ids: Vec<TableCellId>,
        extent: LengthEmu,
    },
    DeleteColumn {
        column_id: TableColumnId,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableRowColHistoryV1 {
    pub protocol_version: String,
    pub table_id: NodeId,
    pub story_id: StoryId,
    pub mutation: TableRowColMutationV1,
    pub before: TableStructureSnapshotV1,
    pub after: TableStructureSnapshotV1,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TableRowColHistoryErrorV1 {
    WrongProtocol,
    TableIdentityMismatch,
    StoryIdentityMismatch,
    InvalidGrid,
    InvalidBounds,
    CellContentMismatch,
    InsertIndexOutOfBounds,
    WrongNewCellCount,
    InsertedIdentityInvalid,
    InsertedIdentityCollision,
    UnknownTrack,
    UnknownTrackExtent,
    CannotDeleteLastTrack,
    InvalidExtent,
    BoundsOverflow,
    CountOverflow,
    StoryBuildFailed,
    StaleSnapshot,
    NonCanonicalAfterState,
}

fn is_editor_created_uuid_v7(bytes: &[u8; 16]) -> bool {
    (bytes[6] & 0xf0) == 0x70 && (bytes[8] & 0xc0) == 0x80
}

fn validate_bounds(bounds: RectEmu) -> Result<(), TableRowColHistoryErrorV1> {
    if bounds.width.get() <= 0
        || bounds.height.get() <= 0
        || bounds.right().is_none()
        || bounds.bottom().is_none()
    {
        return Err(TableRowColHistoryErrorV1::InvalidBounds);
    }
    Ok(())
}

fn cell_text_map(
    snapshot: &TableStructureSnapshotV1,
) -> Result<BTreeMap<TableCellId, String>, TableRowColHistoryErrorV1> {
    let mut by_id = BTreeMap::new();
    for cell in &snapshot.cells {
        if by_id.insert(cell.cell_id, cell.text.clone()).is_some() {
            return Err(TableRowColHistoryErrorV1::CellContentMismatch);
        }
    }
    if by_id.len() != snapshot.grid.cells.len()
        || snapshot.grid.cells.iter().any(|cell| {
            cell.story_id != Some(snapshot.story_id)
                || cell.utf16_start.is_none()
                || cell.utf16_end.is_none()
                || !by_id.contains_key(&cell.id)
        })
    {
        return Err(TableRowColHistoryErrorV1::CellContentMismatch);
    }
    Ok(by_id)
}

pub fn validate_table_structure_snapshot_v1(
    snapshot: &TableStructureSnapshotV1,
) -> Result<(), TableRowColHistoryErrorV1> {
    snapshot
        .grid
        .validate()
        .map_err(|_| TableRowColHistoryErrorV1::InvalidGrid)?;
    validate_bounds(snapshot.bounds)?;
    let _ = cell_text_map(snapshot)?;
    Ok(())
}

fn all_identity_bytes(snapshot: &TableStructureSnapshotV1) -> BTreeSet<[u8; 16]> {
    snapshot
        .grid
        .rows
        .iter()
        .map(|track| *track.id.as_canonical().as_bytes())
        .chain(
            snapshot
                .grid
                .columns
                .iter()
                .map(|track| *track.id.as_canonical().as_bytes()),
        )
        .chain(
            snapshot
                .grid
                .cells
                .iter()
                .map(|cell| *cell.id.as_canonical().as_bytes()),
        )
        .collect()
}

fn validate_new_identities(
    before: &TableStructureSnapshotV1,
    track_id: [u8; 16],
    cell_ids: impl Iterator<Item = [u8; 16]>,
) -> Result<(), TableRowColHistoryErrorV1> {
    if !is_editor_created_uuid_v7(&track_id) {
        return Err(TableRowColHistoryErrorV1::InsertedIdentityInvalid);
    }
    let mut occupied = all_identity_bytes(before);
    if !occupied.insert(track_id) {
        return Err(TableRowColHistoryErrorV1::InsertedIdentityCollision);
    }
    for cell_id in cell_ids {
        if !is_editor_created_uuid_v7(&cell_id) {
            return Err(TableRowColHistoryErrorV1::InsertedIdentityInvalid);
        }
        if !occupied.insert(cell_id) {
            return Err(TableRowColHistoryErrorV1::InsertedIdentityCollision);
        }
    }
    Ok(())
}

fn checked_track_count(count: usize) -> Result<u32, TableRowColHistoryErrorV1> {
    u32::try_from(count).map_err(|_| TableRowColHistoryErrorV1::CountOverflow)
}

fn reindex_rows(
    rows: &mut [EffectiveTableTrackV1<TableRowId>],
) -> Result<(), TableRowColHistoryErrorV1> {
    for (index, row) in rows.iter_mut().enumerate() {
        row.index = checked_track_count(index)?;
    }
    Ok(())
}

fn reindex_columns(
    columns: &mut [EffectiveTableTrackV1<TableColumnId>],
) -> Result<(), TableRowColHistoryErrorV1> {
    for (index, column) in columns.iter_mut().enumerate() {
        column.index = checked_track_count(index)?;
    }
    Ok(())
}

fn grow_bounds(
    bounds: RectEmu,
    row_axis: bool,
    extent: LengthEmu,
) -> Result<RectEmu, TableRowColHistoryErrorV1> {
    if extent.get() <= 0 {
        return Err(TableRowColHistoryErrorV1::InvalidExtent);
    }
    let width = if row_axis {
        bounds.width.get()
    } else {
        bounds
            .width
            .get()
            .checked_add(extent.get())
            .ok_or(TableRowColHistoryErrorV1::BoundsOverflow)?
    };
    let height = if row_axis {
        bounds
            .height
            .get()
            .checked_add(extent.get())
            .ok_or(TableRowColHistoryErrorV1::BoundsOverflow)?
    } else {
        bounds.height.get()
    };
    let after = RectEmu::new(
        bounds.x,
        bounds.y,
        LengthEmu::new(width),
        LengthEmu::new(height),
    );
    validate_bounds(after)?;
    Ok(after)
}

fn shrink_bounds(
    bounds: RectEmu,
    row_axis: bool,
    extent: LengthEmu,
) -> Result<RectEmu, TableRowColHistoryErrorV1> {
    if extent.get() <= 0 {
        return Err(TableRowColHistoryErrorV1::InvalidExtent);
    }
    let width = if row_axis {
        bounds.width.get()
    } else {
        bounds
            .width
            .get()
            .checked_sub(extent.get())
            .ok_or(TableRowColHistoryErrorV1::BoundsOverflow)?
    };
    let height = if row_axis {
        bounds
            .height
            .get()
            .checked_sub(extent.get())
            .ok_or(TableRowColHistoryErrorV1::BoundsOverflow)?
    } else {
        bounds.height.get()
    };
    let after = RectEmu::new(
        bounds.x,
        bounds.y,
        LengthEmu::new(width),
        LengthEmu::new(height),
    );
    validate_bounds(after)?;
    Ok(after)
}

fn canonicalize_story(
    snapshot: &mut TableStructureSnapshotV1,
) -> Result<(), TableRowColHistoryErrorV1> {
    let by_id = cell_text_map(snapshot)?;
    let mut ordered = snapshot.grid.cells.iter().collect::<Vec<_>>();
    ordered.sort_by_key(|cell| (cell.address.row, cell.address.column, cell.id));
    let ordered_cells = ordered
        .iter()
        .map(|cell| {
            Ok((
                cell.id,
                by_id
                    .get(&cell.id)
                    .ok_or(TableRowColHistoryErrorV1::CellContentMismatch)?
                    .clone(),
            ))
        })
        .collect::<Result<Vec<_>, TableRowColHistoryErrorV1>>()?;

    let (story_text, ranges) = rebuild_authored_table_story_v1(&ordered_cells)
        .map_err(|_| TableRowColHistoryErrorV1::StoryBuildFailed)?;
    snapshot.story_text = story_text;

    for cell in &mut snapshot.grid.cells {
        let (start, end) = ranges
            .get(&cell.id)
            .copied()
            .ok_or(TableRowColHistoryErrorV1::CellContentMismatch)?;
        cell.story_id = Some(snapshot.story_id);
        cell.utf16_start = Some(start);
        cell.utf16_end = Some(end);
    }
    snapshot.cells.sort_by_key(|content| {
        snapshot
            .grid
            .cells
            .iter()
            .find(|cell| cell.id == content.cell_id)
            .map(|cell| (cell.address.row, cell.address.column, cell.id))
            .unwrap_or((u32::MAX, u32::MAX, content.cell_id))
    });
    snapshot
        .grid
        .validate()
        .map_err(|_| TableRowColHistoryErrorV1::InvalidGrid)
}

fn insert_row(
    before: &TableStructureSnapshotV1,
    index: u32,
    row_id: TableRowId,
    cell_ids: &[TableCellId],
    extent: LengthEmu,
) -> Result<TableStructureSnapshotV1, TableRowColHistoryErrorV1> {
    let index_usize =
        usize::try_from(index).map_err(|_| TableRowColHistoryErrorV1::InsertIndexOutOfBounds)?;
    if index_usize > before.grid.rows.len() {
        return Err(TableRowColHistoryErrorV1::InsertIndexOutOfBounds);
    }
    if cell_ids.len() != before.grid.columns.len() {
        return Err(TableRowColHistoryErrorV1::WrongNewCellCount);
    }
    validate_new_identities(
        before,
        *row_id.as_canonical().as_bytes(),
        cell_ids.iter().map(|id| *id.as_canonical().as_bytes()),
    )?;

    let mut after = before.clone();
    after.grid.rows.insert(
        index_usize,
        EffectiveTableTrackV1 {
            id: row_id,
            index,
            extent: Some(extent),
        },
    );
    reindex_rows(&mut after.grid.rows)?;
    for cell in &mut after.grid.cells {
        if cell.address.row >= index {
            cell.address.row = cell
                .address
                .row
                .checked_add(1)
                .ok_or(TableRowColHistoryErrorV1::CountOverflow)?;
        }
    }
    for (column, (column_track, cell_id)) in after
        .grid
        .columns
        .iter()
        .zip(cell_ids.iter().copied())
        .enumerate()
    {
        after.grid.cells.push(EffectiveTableCellV1 {
            id: cell_id,
            row_id,
            column_id: column_track.id,
            address: TableCellAddress {
                row: index,
                column: checked_track_count(column)?,
            },
            row_span: 1,
            column_span: 1,
            story_id: Some(after.story_id),
            utf16_start: Some(0),
            utf16_end: Some(0),
        });
        after.cells.push(TableCellContentSnapshotV1 {
            cell_id,
            text: String::new(),
        });
    }
    after.bounds = grow_bounds(before.bounds, true, extent)?;
    canonicalize_story(&mut after)?;
    Ok(after)
}

fn insert_column(
    before: &TableStructureSnapshotV1,
    index: u32,
    column_id: TableColumnId,
    cell_ids: &[TableCellId],
    extent: LengthEmu,
) -> Result<TableStructureSnapshotV1, TableRowColHistoryErrorV1> {
    let index_usize =
        usize::try_from(index).map_err(|_| TableRowColHistoryErrorV1::InsertIndexOutOfBounds)?;
    if index_usize > before.grid.columns.len() {
        return Err(TableRowColHistoryErrorV1::InsertIndexOutOfBounds);
    }
    if cell_ids.len() != before.grid.rows.len() {
        return Err(TableRowColHistoryErrorV1::WrongNewCellCount);
    }
    validate_new_identities(
        before,
        *column_id.as_canonical().as_bytes(),
        cell_ids.iter().map(|id| *id.as_canonical().as_bytes()),
    )?;

    let mut after = before.clone();
    after.grid.columns.insert(
        index_usize,
        EffectiveTableTrackV1 {
            id: column_id,
            index,
            extent: Some(extent),
        },
    );
    reindex_columns(&mut after.grid.columns)?;
    for cell in &mut after.grid.cells {
        if cell.address.column >= index {
            cell.address.column = cell
                .address
                .column
                .checked_add(1)
                .ok_or(TableRowColHistoryErrorV1::CountOverflow)?;
        }
    }
    for (row, (row_track, cell_id)) in after
        .grid
        .rows
        .iter()
        .zip(cell_ids.iter().copied())
        .enumerate()
    {
        after.grid.cells.push(EffectiveTableCellV1 {
            id: cell_id,
            row_id: row_track.id,
            column_id,
            address: TableCellAddress {
                row: checked_track_count(row)?,
                column: index,
            },
            row_span: 1,
            column_span: 1,
            story_id: Some(after.story_id),
            utf16_start: Some(0),
            utf16_end: Some(0),
        });
        after.cells.push(TableCellContentSnapshotV1 {
            cell_id,
            text: String::new(),
        });
    }
    after.bounds = grow_bounds(before.bounds, false, extent)?;
    canonicalize_story(&mut after)?;
    Ok(after)
}

fn delete_row(
    before: &TableStructureSnapshotV1,
    row_id: TableRowId,
) -> Result<TableStructureSnapshotV1, TableRowColHistoryErrorV1> {
    if before.grid.rows.len() <= 1 {
        return Err(TableRowColHistoryErrorV1::CannotDeleteLastTrack);
    }
    let row_index = before
        .grid
        .rows
        .iter()
        .position(|row| row.id == row_id)
        .ok_or(TableRowColHistoryErrorV1::UnknownTrack)?;
    let extent = before.grid.rows[row_index]
        .extent
        .ok_or(TableRowColHistoryErrorV1::UnknownTrackExtent)?;

    let removed_ids = before
        .grid
        .cells
        .iter()
        .filter(|cell| cell.row_id == row_id)
        .map(|cell| cell.id)
        .collect::<BTreeSet<_>>();
    let row_index_u32 = checked_track_count(row_index)?;
    let mut after = before.clone();
    after.grid.rows.remove(row_index);
    reindex_rows(&mut after.grid.rows)?;
    after.grid.cells.retain(|cell| cell.row_id != row_id);
    for cell in &mut after.grid.cells {
        if cell.address.row > row_index_u32 {
            cell.address.row -= 1;
        }
    }
    after
        .cells
        .retain(|content| !removed_ids.contains(&content.cell_id));
    after.bounds = shrink_bounds(before.bounds, true, extent)?;
    canonicalize_story(&mut after)?;
    Ok(after)
}

fn delete_column(
    before: &TableStructureSnapshotV1,
    column_id: TableColumnId,
) -> Result<TableStructureSnapshotV1, TableRowColHistoryErrorV1> {
    if before.grid.columns.len() <= 1 {
        return Err(TableRowColHistoryErrorV1::CannotDeleteLastTrack);
    }
    let column_index = before
        .grid
        .columns
        .iter()
        .position(|column| column.id == column_id)
        .ok_or(TableRowColHistoryErrorV1::UnknownTrack)?;
    let extent = before.grid.columns[column_index]
        .extent
        .ok_or(TableRowColHistoryErrorV1::UnknownTrackExtent)?;

    let removed_ids = before
        .grid
        .cells
        .iter()
        .filter(|cell| cell.column_id == column_id)
        .map(|cell| cell.id)
        .collect::<BTreeSet<_>>();
    let column_index_u32 = checked_track_count(column_index)?;
    let mut after = before.clone();
    after.grid.columns.remove(column_index);
    reindex_columns(&mut after.grid.columns)?;
    after.grid.cells.retain(|cell| cell.column_id != column_id);
    for cell in &mut after.grid.cells {
        if cell.address.column > column_index_u32 {
            cell.address.column -= 1;
        }
    }
    after
        .cells
        .retain(|content| !removed_ids.contains(&content.cell_id));
    after.bounds = shrink_bounds(before.bounds, false, extent)?;
    canonicalize_story(&mut after)?;
    Ok(after)
}

pub fn plan_table_rowcol_mutation_v1(
    before: &TableStructureSnapshotV1,
    mutation: &TableRowColMutationV1,
) -> Result<TableStructureSnapshotV1, TableRowColHistoryErrorV1> {
    validate_table_structure_snapshot_v1(before)?;
    match mutation {
        TableRowColMutationV1::InsertRow {
            index,
            row_id,
            cell_ids,
            extent,
        } => insert_row(before, *index, *row_id, cell_ids, *extent),
        TableRowColMutationV1::DeleteRow { row_id } => delete_row(before, *row_id),
        TableRowColMutationV1::InsertColumn {
            index,
            column_id,
            cell_ids,
            extent,
        } => insert_column(before, *index, *column_id, cell_ids, *extent),
        TableRowColMutationV1::DeleteColumn { column_id } => delete_column(before, *column_id),
    }
}

pub fn canonical_table_rowcol_history_v1(
    before: &TableStructureSnapshotV1,
    mutation: TableRowColMutationV1,
) -> Result<TableRowColHistoryV1, TableRowColHistoryErrorV1> {
    let after = plan_table_rowcol_mutation_v1(before, &mutation)?;
    Ok(TableRowColHistoryV1 {
        protocol_version: TABLE_ROWCOL_HISTORY_V1.into(),
        table_id: before.grid.table_id,
        story_id: before.story_id,
        mutation,
        before: before.clone(),
        after,
    })
}

fn validate_history_header(
    current: &TableStructureSnapshotV1,
    history: &TableRowColHistoryV1,
) -> Result<(), TableRowColHistoryErrorV1> {
    if history.protocol_version != TABLE_ROWCOL_HISTORY_V1 {
        return Err(TableRowColHistoryErrorV1::WrongProtocol);
    }
    if history.table_id != current.grid.table_id
        || history.before.grid.table_id != history.table_id
        || history.after.grid.table_id != history.table_id
    {
        return Err(TableRowColHistoryErrorV1::TableIdentityMismatch);
    }
    if history.story_id != current.story_id
        || history.before.story_id != history.story_id
        || history.after.story_id != history.story_id
    {
        return Err(TableRowColHistoryErrorV1::StoryIdentityMismatch);
    }
    Ok(())
}

pub fn apply_table_rowcol_history_forward_v1(
    current: &TableStructureSnapshotV1,
    history: &TableRowColHistoryV1,
) -> Result<TableStructureSnapshotV1, TableRowColHistoryErrorV1> {
    validate_history_header(current, history)?;
    if current != &history.before {
        return Err(TableRowColHistoryErrorV1::StaleSnapshot);
    }
    let canonical_after = plan_table_rowcol_mutation_v1(&history.before, &history.mutation)?;
    if canonical_after != history.after {
        return Err(TableRowColHistoryErrorV1::NonCanonicalAfterState);
    }
    Ok(canonical_after)
}

pub fn apply_table_rowcol_history_inverse_v1(
    current: &TableStructureSnapshotV1,
    history: &TableRowColHistoryV1,
) -> Result<TableStructureSnapshotV1, TableRowColHistoryErrorV1> {
    validate_history_header(current, history)?;
    let canonical_after = plan_table_rowcol_mutation_v1(&history.before, &history.mutation)?;
    if canonical_after != history.after {
        return Err(TableRowColHistoryErrorV1::NonCanonicalAfterState);
    }
    if current != &history.after {
        return Err(TableRowColHistoryErrorV1::StaleSnapshot);
    }
    validate_table_structure_snapshot_v1(&history.before)?;
    Ok(history.before.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pub_model::EFFECTIVE_TABLE_GRID_V1;

    fn id<T: serde::de::DeserializeOwned>(value: &str) -> T {
        serde_json::from_str(&format!("\"{value}\"")).expect("typed id")
    }

    fn row(value: &str) -> TableRowId {
        id(value)
    }

    fn column(value: &str) -> TableColumnId {
        id(value)
    }

    fn cell(value: &str) -> TableCellId {
        id(value)
    }

    fn base_snapshot() -> TableStructureSnapshotV1 {
        let table_id: NodeId = id("01890f47-4000-7abc-8def-0123456789ab");
        let story_id: StoryId = id("01890f47-4001-7abc-8def-0123456789ab");
        let rows = vec![
            EffectiveTableTrackV1 {
                id: row("01890f47-4010-7abc-8def-0123456789ab"),
                index: 0,
                extent: Some(LengthEmu::new(200)),
            },
            EffectiveTableTrackV1 {
                id: row("01890f47-4011-7abc-8def-0123456789ab"),
                index: 1,
                extent: Some(LengthEmu::new(250)),
            },
        ];
        let columns = vec![
            EffectiveTableTrackV1 {
                id: column("01890f47-4020-7abc-8def-0123456789ab"),
                index: 0,
                extent: Some(LengthEmu::new(300)),
            },
            EffectiveTableTrackV1 {
                id: column("01890f47-4021-7abc-8def-0123456789ab"),
                index: 1,
                extent: Some(LengthEmu::new(350)),
            },
        ];
        let ids = [
            cell("01890f47-4030-7abc-8def-0123456789ab"),
            cell("01890f47-4031-7abc-8def-0123456789ab"),
            cell("01890f47-4032-7abc-8def-0123456789ab"),
            cell("01890f47-4033-7abc-8def-0123456789ab"),
        ];
        let mut grid = EffectiveTableGridV1 {
            version: EFFECTIVE_TABLE_GRID_V1.into(),
            table_id,
            rows,
            columns,
            cells: vec![
                EffectiveTableCellV1 {
                    id: ids[0],
                    row_id: row("01890f47-4010-7abc-8def-0123456789ab"),
                    column_id: column("01890f47-4020-7abc-8def-0123456789ab"),
                    address: TableCellAddress { row: 0, column: 0 },
                    row_span: 1,
                    column_span: 1,
                    story_id: Some(story_id),
                    utf16_start: Some(0),
                    utf16_end: Some(1),
                },
                EffectiveTableCellV1 {
                    id: ids[1],
                    row_id: row("01890f47-4010-7abc-8def-0123456789ab"),
                    column_id: column("01890f47-4021-7abc-8def-0123456789ab"),
                    address: TableCellAddress { row: 0, column: 1 },
                    row_span: 1,
                    column_span: 1,
                    story_id: Some(story_id),
                    utf16_start: Some(1),
                    utf16_end: Some(3),
                },
                EffectiveTableCellV1 {
                    id: ids[2],
                    row_id: row("01890f47-4011-7abc-8def-0123456789ab"),
                    column_id: column("01890f47-4020-7abc-8def-0123456789ab"),
                    address: TableCellAddress { row: 1, column: 0 },
                    row_span: 1,
                    column_span: 1,
                    story_id: Some(story_id),
                    utf16_start: Some(3),
                    utf16_end: Some(5),
                },
                EffectiveTableCellV1 {
                    id: ids[3],
                    row_id: row("01890f47-4011-7abc-8def-0123456789ab"),
                    column_id: column("01890f47-4021-7abc-8def-0123456789ab"),
                    address: TableCellAddress { row: 1, column: 1 },
                    row_span: 1,
                    column_span: 1,
                    story_id: Some(story_id),
                    utf16_start: Some(5),
                    utf16_end: Some(8),
                },
            ],
        };
        let cells = vec![
            TableCellContentSnapshotV1 {
                cell_id: ids[0],
                text: "A".into(),
            },
            TableCellContentSnapshotV1 {
                cell_id: ids[1],
                text: "B".into(),
            },
            TableCellContentSnapshotV1 {
                cell_id: ids[2],
                text: "C".into(),
            },
            TableCellContentSnapshotV1 {
                cell_id: ids[3],
                text: "D".into(),
            },
        ];
        let ordered = cells
            .iter()
            .map(|content| (content.cell_id, content.text.clone()))
            .collect::<Vec<_>>();
        let (story_text, ranges) =
            rebuild_authored_table_story_v1(&ordered).expect("canonical story");
        for entry in &mut grid.cells {
            let (start, end) = ranges[&entry.id];
            entry.utf16_start = Some(start);
            entry.utf16_end = Some(end);
        }
        TableStructureSnapshotV1 {
            grid,
            bounds: RectEmu::new(
                LengthEmu::new(100),
                LengthEmu::new(200),
                LengthEmu::new(650),
                LengthEmu::new(450),
            ),
            story_id,
            story_text,
            cells,
        }
    }

    #[test]
    fn insert_row_preserves_survivor_identity_and_adds_explicit_extent() {
        let before = base_snapshot();
        let old_second_row = before.grid.rows[1].id;
        let old_bottom_left = before.grid.cells[2].id;
        let mutation = TableRowColMutationV1::InsertRow {
            index: 1,
            row_id: row("01890f47-5000-7abc-8def-0123456789ab"),
            cell_ids: vec![
                cell("01890f47-5001-7abc-8def-0123456789ab"),
                cell("01890f47-5002-7abc-8def-0123456789ab"),
            ],
            extent: LengthEmu::new(225),
        };
        let history =
            canonical_table_rowcol_history_v1(&before, mutation).expect("insert row history");
        let after = apply_table_rowcol_history_forward_v1(&before, &history).expect("forward");

        assert_eq!(after.grid.rows.len(), 3);
        assert_eq!(after.grid.rows[1].extent, Some(LengthEmu::new(225)));
        assert_eq!(after.grid.rows[2].id, old_second_row);
        assert_eq!(
            after
                .grid
                .cells
                .iter()
                .find(|entry| entry.id == old_bottom_left)
                .expect("survivor")
                .address
                .row,
            2
        );
        assert_eq!(after.bounds.height, LengthEmu::new(675));
        assert_eq!(
            after
                .cells
                .iter()
                .find(|entry| entry.cell_id == old_bottom_left)
                .expect("surviving content")
                .text,
            "C"
        );
        assert_eq!(
            apply_table_rowcol_history_inverse_v1(&after, &history).expect("undo"),
            before
        );
    }

    #[test]
    fn insert_column_preserves_survivor_identity_and_text() {
        let before = base_snapshot();
        let old_right = before.grid.columns[1].id;
        let old_top_right = before.grid.cells[1].id;
        let mutation = TableRowColMutationV1::InsertColumn {
            index: 1,
            column_id: column("01890f47-5100-7abc-8def-0123456789ab"),
            cell_ids: vec![
                cell("01890f47-5101-7abc-8def-0123456789ab"),
                cell("01890f47-5102-7abc-8def-0123456789ab"),
            ],
            extent: LengthEmu::new(325),
        };
        let after = plan_table_rowcol_mutation_v1(&before, &mutation).expect("insert column plan");

        assert_eq!(after.grid.columns.len(), 3);
        assert_eq!(after.grid.columns[2].id, old_right);
        assert_eq!(after.bounds.width, LengthEmu::new(975));
        assert_eq!(
            after
                .grid
                .cells
                .iter()
                .find(|entry| entry.id == old_top_right)
                .expect("survivor")
                .address
                .column,
            2
        );
        assert_eq!(
            after
                .cells
                .iter()
                .find(|entry| entry.cell_id == old_top_right)
                .expect("surviving content")
                .text,
            "B"
        );
    }

    #[test]
    fn delete_row_and_undo_restore_exact_removed_ids_and_text() {
        let before = base_snapshot();
        let removed_row = before.grid.rows[0].id;
        let removed_cell = before.grid.cells[0].id;
        let history = canonical_table_rowcol_history_v1(
            &before,
            TableRowColMutationV1::DeleteRow {
                row_id: removed_row,
            },
        )
        .expect("delete row history");
        assert!(
            !history
                .after
                .grid
                .rows
                .iter()
                .any(|row| row.id == removed_row)
        );
        assert!(
            !history
                .after
                .cells
                .iter()
                .any(|cell| cell.cell_id == removed_cell)
        );

        let restored =
            apply_table_rowcol_history_inverse_v1(&history.after, &history).expect("undo");
        assert_eq!(restored, before);
    }

    #[test]
    fn delete_unknown_extent_fails_closed() {
        let mut before = base_snapshot();
        let target = before.grid.rows[0].id;
        before.grid.rows[0].extent = None;
        assert_eq!(
            plan_table_rowcol_mutation_v1(
                &before,
                &TableRowColMutationV1::DeleteRow { row_id: target }
            ),
            Err(TableRowColHistoryErrorV1::UnknownTrackExtent)
        );
    }

    #[test]
    fn inserted_ids_must_be_fresh_uuidv7() {
        let before = base_snapshot();
        let existing = before.grid.cells[0].id;
        assert_eq!(
            plan_table_rowcol_mutation_v1(
                &before,
                &TableRowColMutationV1::InsertRow {
                    index: 1,
                    row_id: row("01890f47-5000-7abc-8def-0123456789ab"),
                    cell_ids: vec![existing, cell("01890f47-5002-7abc-8def-0123456789ab"),],
                    extent: LengthEmu::new(225),
                }
            ),
            Err(TableRowColHistoryErrorV1::InsertedIdentityCollision)
        );
    }

    #[test]
    fn tampered_after_snapshot_is_rejected() {
        let before = base_snapshot();
        let mut history = canonical_table_rowcol_history_v1(
            &before,
            TableRowColMutationV1::DeleteColumn {
                column_id: before.grid.columns[0].id,
            },
        )
        .expect("delete column history");
        history.after.bounds.width = LengthEmu::new(history.after.bounds.width.get() + 1);
        assert_eq!(
            apply_table_rowcol_history_forward_v1(&before, &history),
            Err(TableRowColHistoryErrorV1::NonCanonicalAfterState)
        );
    }
}
