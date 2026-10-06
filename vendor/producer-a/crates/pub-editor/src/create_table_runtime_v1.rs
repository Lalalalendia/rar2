use crate::create_shape_runtime_v1::MAX_SAFE_EMU_V1;
use pub_model::{
    Affine2D, EFFECTIVE_TABLE_GRID_V1, EffectiveTableCellV1, EffectiveTableGridV1,
    EffectiveTableTrackV1, LengthEmu, Node, NodeHeader, NodeId, NodeKind, PageId, RectEmu,
    SimpleRectangularTable, SimpleTableCell, Story, StoryId, TableCellAddress, TableCellId,
    TableColumnId, TableRowId,
};
use pub_reader::{
    PubExplicitShapePaintSource, PubResolvedGraph, PubResolvedNodePayload, PubTableCellCoordinates,
    PubTableCellSource, PubTableLayoutMetricsSource, PubTableSource, PubTableStoryOwnershipSource,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const AUTHORED_TABLE_SENTINEL_TEXT_ID_V1: u32 = 0;
pub const AUTHORED_TABLE_SENTINEL_CONTENTS_SEQ_NUM_V1: u32 = 0;

pub type AuthoredTableStoryRangesV1 = BTreeMap<TableCellId, (u32, u32)>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateTableRuntimeV1 {
    pub node_id: NodeId,
    pub story_id: StoryId,
    pub page_id: PageId,
    pub bounds: RectEmu,
    pub row_ids: Vec<TableRowId>,
    pub column_ids: Vec<TableColumnId>,
    pub cell_ids: Vec<TableCellId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateTablePlanV1 {
    pub node: Node<PubResolvedNodePayload>,
    pub story: Story,
    pub grid: EffectiveTableGridV1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreateTableRuntimeValidationError {
    NodeIdNotUuidV7,
    StoryIdNotUuidV7,
    TrackIdNotUuidV7,
    CellIdNotUuidV7,
    DuplicateIdentity,
    EmptyRows,
    EmptyColumns,
    CellCountOverflow,
    WrongCellCount,
    InvalidBounds,
    NonUniformBounds,
    RangeOverflow,
    InvalidGrid,
    PageMissing,
    NodeIdCollision,
    StoryIdCollision,
    StaleGraphState,
}

fn is_editor_created_uuid_v7(bytes: &[u8; 16]) -> bool {
    (bytes[6] & 0xf0) == 0x70 && (bytes[8] & 0xc0) == 0x80
}

fn checked_uuid_v7<T>(id: &T, bytes: impl Fn(&T) -> &[u8; 16]) -> bool {
    is_editor_created_uuid_v7(bytes(id))
}

fn authored_table_story_v1(
    ordered_cells: &[(TableCellId, String)],
) -> Result<(String, AuthoredTableStoryRangesV1), CreateTableRuntimeValidationError> {
    let mut utf16 = Vec::<u16>::new();
    let mut ranges = BTreeMap::new();

    for (index, (cell_id, text)) in ordered_cells.iter().enumerate() {
        let start = if index == 0 {
            0
        } else {
            // The ordinary source-table law stores the leading separator inside
            // each non-first cell range. If the first authored cell is empty,
            // its zero-length range leaves offset 0 unavailable for that law.
            // Reserve one unowned CR once so every later empty cell still has a
            // positive range start and materializes as empty.
            if utf16.is_empty() {
                utf16.push(0x000D);
            }
            let start = u32::try_from(utf16.len())
                .map_err(|_| CreateTableRuntimeValidationError::RangeOverflow)?;
            utf16.push(0x000D);
            start
        };

        utf16.extend(text.encode_utf16());
        if index + 1 == ordered_cells.len() {
            utf16.push(0x000D);
        }
        let end = u32::try_from(utf16.len())
            .map_err(|_| CreateTableRuntimeValidationError::RangeOverflow)?;
        ranges.insert(*cell_id, (start, end));
    }

    let text =
        String::from_utf16(&utf16).map_err(|_| CreateTableRuntimeValidationError::RangeOverflow)?;
    Ok((text, ranges))
}

pub fn rebuild_authored_table_story_v1(
    ordered_cells: &[(TableCellId, String)],
) -> Result<(String, AuthoredTableStoryRangesV1), CreateTableRuntimeValidationError> {
    authored_table_story_v1(ordered_cells)
}

pub fn validate_create_table_runtime_v1(
    table: &CreateTableRuntimeV1,
) -> Result<(), CreateTableRuntimeValidationError> {
    if !checked_uuid_v7(&table.node_id, |id| id.as_canonical().as_bytes()) {
        return Err(CreateTableRuntimeValidationError::NodeIdNotUuidV7);
    }
    if !checked_uuid_v7(&table.story_id, |id| id.as_canonical().as_bytes()) {
        return Err(CreateTableRuntimeValidationError::StoryIdNotUuidV7);
    }
    if table.row_ids.is_empty() {
        return Err(CreateTableRuntimeValidationError::EmptyRows);
    }
    if table.column_ids.is_empty() {
        return Err(CreateTableRuntimeValidationError::EmptyColumns);
    }
    if table
        .row_ids
        .iter()
        .any(|id| !is_editor_created_uuid_v7(id.as_canonical().as_bytes()))
        || table
            .column_ids
            .iter()
            .any(|id| !is_editor_created_uuid_v7(id.as_canonical().as_bytes()))
    {
        return Err(CreateTableRuntimeValidationError::TrackIdNotUuidV7);
    }
    if table
        .cell_ids
        .iter()
        .any(|id| !is_editor_created_uuid_v7(id.as_canonical().as_bytes()))
    {
        return Err(CreateTableRuntimeValidationError::CellIdNotUuidV7);
    }

    let expected = table
        .row_ids
        .len()
        .checked_mul(table.column_ids.len())
        .ok_or(CreateTableRuntimeValidationError::CellCountOverflow)?;
    if expected != table.cell_ids.len() {
        return Err(CreateTableRuntimeValidationError::WrongCellCount);
    }

    let mut identities = BTreeSet::<[u8; 16]>::new();
    for id in std::iter::once(*table.node_id.as_canonical().as_bytes())
        .chain(std::iter::once(*table.story_id.as_canonical().as_bytes()))
        .chain(table.row_ids.iter().map(|id| *id.as_canonical().as_bytes()))
        .chain(
            table
                .column_ids
                .iter()
                .map(|id| *id.as_canonical().as_bytes()),
        )
        .chain(
            table
                .cell_ids
                .iter()
                .map(|id| *id.as_canonical().as_bytes()),
        )
    {
        if !identities.insert(id) {
            return Err(CreateTableRuntimeValidationError::DuplicateIdentity);
        }
    }

    let bounds = table.bounds;
    for value in [bounds.x.get(), bounds.y.get()] {
        if !(-MAX_SAFE_EMU_V1..=MAX_SAFE_EMU_V1).contains(&value) {
            return Err(CreateTableRuntimeValidationError::InvalidBounds);
        }
    }
    for value in [bounds.width.get(), bounds.height.get()] {
        if value <= 0 || value > MAX_SAFE_EMU_V1 {
            return Err(CreateTableRuntimeValidationError::InvalidBounds);
        }
    }
    let Some(right) = bounds.right() else {
        return Err(CreateTableRuntimeValidationError::InvalidBounds);
    };
    let Some(bottom) = bounds.bottom() else {
        return Err(CreateTableRuntimeValidationError::InvalidBounds);
    };
    if !(-MAX_SAFE_EMU_V1..=MAX_SAFE_EMU_V1).contains(&right.get())
        || !(-MAX_SAFE_EMU_V1..=MAX_SAFE_EMU_V1).contains(&bottom.get())
    {
        return Err(CreateTableRuntimeValidationError::InvalidBounds);
    }

    let rows = i64::try_from(table.row_ids.len())
        .map_err(|_| CreateTableRuntimeValidationError::InvalidBounds)?;
    let columns = i64::try_from(table.column_ids.len())
        .map_err(|_| CreateTableRuntimeValidationError::InvalidBounds)?;
    if bounds.height.get() % rows != 0 || bounds.width.get() % columns != 0 {
        return Err(CreateTableRuntimeValidationError::NonUniformBounds);
    }
    if bounds.height.get() / rows <= 0 || bounds.width.get() / columns <= 0 {
        return Err(CreateTableRuntimeValidationError::InvalidBounds);
    }

    Ok(())
}

pub fn build_create_table_plan_v1(
    table: &CreateTableRuntimeV1,
) -> Result<CreateTablePlanV1, CreateTableRuntimeValidationError> {
    validate_create_table_runtime_v1(table)?;

    let rows = u32::try_from(table.row_ids.len())
        .map_err(|_| CreateTableRuntimeValidationError::InvalidBounds)?;
    let columns = u32::try_from(table.column_ids.len())
        .map_err(|_| CreateTableRuntimeValidationError::InvalidBounds)?;
    let row_extent = LengthEmu::new(table.bounds.height.get() / i64::from(rows));
    let column_extent = LengthEmu::new(table.bounds.width.get() / i64::from(columns));

    let ordered_cells = table
        .cell_ids
        .iter()
        .map(|id| (*id, String::new()))
        .collect::<Vec<_>>();
    let (story_text, ranges) = authored_table_story_v1(&ordered_cells)?;

    let row_tracks = table
        .row_ids
        .iter()
        .enumerate()
        .map(|(index, id)| EffectiveTableTrackV1 {
            id: *id,
            index: u32::try_from(index).expect("validated row count fits u32"),
            extent: Some(row_extent),
        })
        .collect::<Vec<_>>();
    let column_tracks = table
        .column_ids
        .iter()
        .enumerate()
        .map(|(index, id)| EffectiveTableTrackV1 {
            id: *id,
            index: u32::try_from(index).expect("validated column count fits u32"),
            extent: Some(column_extent),
        })
        .collect::<Vec<_>>();

    let mut source_cells = Vec::with_capacity(table.cell_ids.len());
    let mut simple_cells = Vec::with_capacity(table.cell_ids.len());
    let mut effective_cells = Vec::with_capacity(table.cell_ids.len());

    for (index, cell_id) in table.cell_ids.iter().copied().enumerate() {
        let row =
            u32::try_from(index / table.column_ids.len()).expect("validated cell row fits u32");
        let column =
            u32::try_from(index % table.column_ids.len()).expect("validated cell column fits u32");
        let address = TableCellAddress { row, column };
        let row_id = row_tracks[usize::try_from(row).expect("row u32 fits usize")].id;
        let column_id = column_tracks[usize::try_from(column).expect("column u32 fits usize")].id;
        let (utf16_start, utf16_end) = ranges[&cell_id];

        let x = table
            .bounds
            .x
            .get()
            .checked_add(i64::from(column).saturating_mul(column_extent.get()))
            .ok_or(CreateTableRuntimeValidationError::InvalidBounds)?;
        let y = table
            .bounds
            .y
            .get()
            .checked_add(i64::from(row).saturating_mul(row_extent.get()))
            .ok_or(CreateTableRuntimeValidationError::InvalidBounds)?;
        let cell_bounds = RectEmu::new(
            LengthEmu::new(x),
            LengthEmu::new(y),
            column_extent,
            row_extent,
        );

        source_cells.push(PubTableCellSource {
            id: cell_id,
            stored_record_index: u32::try_from(index)
                .map_err(|_| CreateTableRuntimeValidationError::CellCountOverflow)?,
            coordinates: Some(PubTableCellCoordinates {
                start_row: row,
                end_row: row,
                start_column: column,
                end_column: column,
            }),
            utf16_start,
            utf16_end,
            bounds: Some(cell_bounds),
            paint: None,
            source_refs: Vec::new(),
        });
        simple_cells.push(SimpleTableCell {
            id: cell_id,
            address,
        });
        effective_cells.push(EffectiveTableCellV1 {
            id: cell_id,
            row_id,
            column_id,
            address,
            row_span: 1,
            column_span: 1,
            story_id: Some(table.story_id),
            utf16_start: Some(utf16_start),
            utf16_end: Some(utf16_end),
        });
    }

    let simple_table = SimpleRectangularTable::new(rows, columns, simple_cells)
        .map_err(|_| CreateTableRuntimeValidationError::InvalidGrid)?;
    let grid = EffectiveTableGridV1 {
        version: EFFECTIVE_TABLE_GRID_V1.into(),
        table_id: table.node_id,
        rows: row_tracks,
        columns: column_tracks,
        cells: effective_cells,
    };
    grid.validate()
        .map_err(|_| CreateTableRuntimeValidationError::InvalidGrid)?;

    let story = Story {
        id: table.story_id,
        text: story_text,
        paragraphs: Vec::new(),
        runs: Vec::new(),
        fields: Vec::new(),
        hyperlinks: Vec::new(),
        source_refs: Vec::new(),
    };
    let node = Node {
        kind: NodeKind::Table,
        header: NodeHeader {
            id: table.node_id,
            parent_id: table.page_id.into_canonical(),
            bounds: table.bounds,
            transform: Affine2D::identity(),
            source_refs: Vec::new(),
            extensions: Vec::new(),
        },
        payload: PubResolvedNodePayload {
            contents_seq_num: AUTHORED_TABLE_SENTINEL_CONTENTS_SEQ_NUM_V1,
            officeart_shape_type: None,
            officeart_spid: None,
            image_slot: None,
            legacy_ole: None,
            explicit_image_crop: None,
            explicit_image_cardinal_rotation_degrees: None,
            explicit_paint: PubExplicitShapePaintSource::default(),
            effective_paint: None,
            story_frame: None,
            text_frame_inset: None,
            table_story: Some(PubTableStoryOwnershipSource {
                text_id: AUTHORED_TABLE_SENTINEL_TEXT_ID_V1,
                story_id: Some(table.story_id),
                source_refs: Vec::new(),
            }),
            table: Some(PubTableSource {
                text_id: AUTHORED_TABLE_SENTINEL_TEXT_ID_V1,
                story_id: Some(table.story_id),
                rows,
                columns,
                cells_seq_num: None,
                tcd_story_ordinal: None,
                cells: source_cells,
                simple_table: Some(simple_table),
                layout_metrics: Some(PubTableLayoutMetricsSource {
                    story_layout_key: 0,
                    cell_width: column_extent,
                    row_pitch: row_extent,
                    source_refs: Vec::new(),
                }),
                border_segments: Vec::new(),
                source_refs: Vec::new(),
            }),
        },
    };

    Ok(CreateTablePlanV1 { node, story, grid })
}

pub fn apply_create_table_forward_v1(
    graph: &mut PubResolvedGraph,
    table: &CreateTableRuntimeV1,
) -> Result<(), CreateTableRuntimeValidationError> {
    if !graph.pages.contains_key(&table.page_id) {
        return Err(CreateTableRuntimeValidationError::PageMissing);
    }
    if graph.nodes.contains_key(&table.node_id)
        || graph
            .pages
            .values()
            .any(|page| page.children.contains(&table.node_id))
    {
        return Err(CreateTableRuntimeValidationError::NodeIdCollision);
    }
    if graph.stories.contains_key(&table.story_id) {
        return Err(CreateTableRuntimeValidationError::StoryIdCollision);
    }

    let plan = build_create_table_plan_v1(table)?;
    graph
        .pages
        .get_mut(&table.page_id)
        .expect("CreateTable page was validated")
        .children
        .push(table.node_id);
    graph.stories.insert(table.story_id, plan.story);
    graph.nodes.insert(table.node_id, plan.node);
    Ok(())
}

pub fn apply_create_table_inverse_v1(
    graph: &mut PubResolvedGraph,
    table: &CreateTableRuntimeV1,
) -> Result<(), CreateTableRuntimeValidationError> {
    let plan = build_create_table_plan_v1(table)?;
    let exact_child_count = graph
        .pages
        .get(&table.page_id)
        .map(|page| page.children.iter().filter(|id| **id == table.node_id).count())
        .unwrap_or(0);
    if graph.nodes.get(&table.node_id) != Some(&plan.node)
        || graph.stories.get(&table.story_id) != Some(&plan.story)
        || exact_child_count != 1
    {
        return Err(CreateTableRuntimeValidationError::StaleGraphState);
    }

    graph.nodes.remove(&table.node_id);
    graph.stories.remove(&table.story_id);
    graph
        .pages
        .get_mut(&table.page_id)
        .expect("CreateTable inverse validated page")
        .children
        .retain(|id| *id != table.node_id);
    Ok(())
}
