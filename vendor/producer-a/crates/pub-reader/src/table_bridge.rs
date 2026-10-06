use super::*;
use pub_contents::{
    CONTENTS_RAW_TYPE_CELLS, ContentsCursor, MatureCellCoordinates, parse_confirmed_block,
    parse_confirmed_mature_cells,
};
use pub_model::{
    RectEmu, SimpleRectangularTable, SimpleTableCell, Story, TableCellAddress, TableCellId,
};
use pub_quill::{
    QuillMcldChunk, QuillStoryCatalog, bounded_mcld_table_metrics,
    bounded_mcld_table_uniform_text_inset,
};

pub const RAW_TYPE_TABLE: u16 = 0x10;
pub const TABLE_NUM_ROWS_ID: u16 = 0x66;
pub const TABLE_NUM_COLUMNS_ID: u16 = 0x67;
pub const TABLE_CELLS_SEQ_NUM_ID: u16 = 0x6B;
pub const TABLE_WIDTH_ID: u16 = 0x68;
pub const TABLE_HEIGHT_ID: u16 = 0x69;
pub const TABLE_ROWCOL_ARRAY_ID: u16 = 0x6D;
pub const TABLE_ROWCOL_PHYSICAL_END_ID: u16 = 0x01;
pub const TABLE_ROWCOL_SIZE_ID: u16 = 0x02;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PubTableCellSource {
    pub id: TableCellId,
    pub stored_record_index: u32,
    pub coordinates: Option<PubTableCellCoordinates>,
    pub utf16_start: u32,
    pub utf16_end: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bounds: Option<RectEmu>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paint: Option<PubTableCellPaintSource>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_refs: Vec<SourceRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PubTableCellPaintSource {
    /// Resolved sRGB from one bounded source-backed OfficeArt cell-paint carrier.
    pub solid_fill_rgb: [u8; 3],
    /// Resolved effective fill visibility from that same admitted carrier.
    pub fill_visible: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_refs: Vec<SourceRef>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PubTableBorderAxis {
    Horizontal,
    Vertical,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PubTableBorderSegmentSource {
    pub axis: PubTableBorderAxis,
    pub row_start: u32,
    pub column_start: u32,
    pub row_end: u32,
    pub column_end: u32,
    pub rgb: [u8; 3],
    pub width_emu: i64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_refs: Vec<SourceRef>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PubTableCellCoordinates {
    pub start_row: u32,
    pub end_row: u32,
    pub start_column: u32,
    pub end_column: u32,
}

impl From<MatureCellCoordinates> for PubTableCellCoordinates {
    fn from(value: MatureCellCoordinates) -> Self {
        Self {
            start_row: value.start_row,
            end_row: value.end_row,
            start_column: value.start_column,
            end_column: value.end_column,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PubMaterializedTableCell {
    pub id: TableCellId,
    pub address: TableCellAddress,
    pub row_span: u32,
    pub column_span: u32,
    /// Story-global Unicode-scalar bounds for the materialized cell text.
    ///
    /// These are derived only after the TCD/CELLS UTF-16 boundaries and
    /// Publisher cell separators have been validated.
    pub story_scalar_start: u32,
    pub story_scalar_end: u32,
    pub text: String,
    pub bounds: Option<RectEmu>,
    pub fill_rgb: Option<[u8; 3]>,
    pub fill_visible: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PubTableTextError {
    NotSimpleRectangular,
    StoryMismatch {
        expected: Option<StoryId>,
        found: StoryId,
    },
    MissingSourceCell {
        id: TableCellId,
    },
    MissingCoordinates {
        id: TableCellId,
    },
    InvalidCoordinates {
        id: TableCellId,
    },
    OverlappingCells {
        id: TableCellId,
        other_id: TableCellId,
    },
    GridCoverageOverflow,
    IncompleteGridCoverage {
        expected: u64,
        covered: u64,
    },
    InvalidRange {
        id: TableCellId,
        start: u32,
        end: u32,
        story_len_utf16: usize,
    },
    MissingLeadingCellSeparator {
        id: TableCellId,
        start: u32,
    },
    InvalidUtf16 {
        id: TableCellId,
    },
    ScalarRangeOverflow {
        id: TableCellId,
    },
}

pub fn materialize_bounded_table_cells(
    table: &PubTableSource,
    story: &Story,
) -> Result<Vec<PubMaterializedTableCell>, PubTableTextError> {
    if table.story_id != Some(story.id) {
        return Err(PubTableTextError::StoryMismatch {
            expected: table.story_id,
            found: story.id,
        });
    }

    let expected = u64::from(table.rows)
        .checked_mul(u64::from(table.columns))
        .ok_or(PubTableTextError::GridCoverageOverflow)?;
    let mut covered = 0_u64;
    let mut semantic_cells = table.cells.iter().collect::<Vec<_>>();
    semantic_cells.sort_by_key(|cell| {
        let coordinates = cell.coordinates.unwrap_or(PubTableCellCoordinates {
            start_row: u32::MAX,
            end_row: u32::MAX,
            start_column: u32::MAX,
            end_column: u32::MAX,
        });
        (
            coordinates.start_row,
            coordinates.start_column,
            coordinates.end_row,
            coordinates.end_column,
            cell.id,
        )
    });

    for (index, cell) in semantic_cells.iter().enumerate() {
        let coordinates = cell
            .coordinates
            .ok_or(PubTableTextError::MissingCoordinates { id: cell.id })?;
        if coordinates.start_row > coordinates.end_row
            || coordinates.start_column > coordinates.end_column
            || coordinates.end_row >= table.rows
            || coordinates.end_column >= table.columns
        {
            return Err(PubTableTextError::InvalidCoordinates { id: cell.id });
        }

        let row_span = u64::from(coordinates.end_row - coordinates.start_row + 1);
        let column_span = u64::from(coordinates.end_column - coordinates.start_column + 1);
        covered = covered
            .checked_add(
                row_span
                    .checked_mul(column_span)
                    .ok_or(PubTableTextError::GridCoverageOverflow)?,
            )
            .ok_or(PubTableTextError::GridCoverageOverflow)?;

        for other in &semantic_cells[..index] {
            let other_coordinates = other
                .coordinates
                .ok_or(PubTableTextError::MissingCoordinates { id: other.id })?;
            let rows_overlap = coordinates.start_row <= other_coordinates.end_row
                && other_coordinates.start_row <= coordinates.end_row;
            let columns_overlap = coordinates.start_column <= other_coordinates.end_column
                && other_coordinates.start_column <= coordinates.end_column;
            if rows_overlap && columns_overlap {
                return Err(PubTableTextError::OverlappingCells {
                    id: cell.id,
                    other_id: other.id,
                });
            }
        }
    }

    if covered != expected {
        return Err(PubTableTextError::IncompleteGridCoverage { expected, covered });
    }

    let story_utf16 = story.text.encode_utf16().collect::<Vec<_>>();
    semantic_cells
        .into_iter()
        .map(|source| {
            let coordinates = source
                .coordinates
                .ok_or(PubTableTextError::MissingCoordinates { id: source.id })?;
            let start = usize::try_from(source.utf16_start).map_err(|_| {
                PubTableTextError::InvalidRange {
                    id: source.id,
                    start: source.utf16_start,
                    end: source.utf16_end,
                    story_len_utf16: story_utf16.len(),
                }
            })?;
            let end =
                usize::try_from(source.utf16_end).map_err(|_| PubTableTextError::InvalidRange {
                    id: source.id,
                    start: source.utf16_start,
                    end: source.utf16_end,
                    story_len_utf16: story_utf16.len(),
                })?;
            if start > end || end > story_utf16.len() {
                return Err(PubTableTextError::InvalidRange {
                    id: source.id,
                    start: source.utf16_start,
                    end: source.utf16_end,
                    story_len_utf16: story_utf16.len(),
                });
            }

            let mut cell_start = start;
            let mut cell_end = end;
            if start > 0 {
                if story_utf16.get(start) != Some(&0x000D) {
                    return Err(PubTableTextError::MissingLeadingCellSeparator {
                        id: source.id,
                        start: source.utf16_start,
                    });
                }
                cell_start += 1;
            }
            if end == story_utf16.len()
                && cell_end > cell_start
                && story_utf16.get(cell_end - 1) == Some(&0x000D)
            {
                cell_end -= 1;
            }

            let prefix = String::from_utf16(&story_utf16[..cell_start])
                .map_err(|_| PubTableTextError::InvalidUtf16 { id: source.id })?;
            let text = String::from_utf16(&story_utf16[cell_start..cell_end])
                .map_err(|_| PubTableTextError::InvalidUtf16 { id: source.id })?;
            let story_scalar_start = u32::try_from(prefix.chars().count())
                .map_err(|_| PubTableTextError::ScalarRangeOverflow { id: source.id })?;
            let cell_scalar_len = u32::try_from(text.chars().count())
                .map_err(|_| PubTableTextError::ScalarRangeOverflow { id: source.id })?;
            let story_scalar_end = story_scalar_start
                .checked_add(cell_scalar_len)
                .ok_or(PubTableTextError::ScalarRangeOverflow { id: source.id })?;

            Ok(PubMaterializedTableCell {
                id: source.id,
                address: TableCellAddress {
                    row: coordinates.start_row,
                    column: coordinates.start_column,
                },
                row_span: coordinates.end_row - coordinates.start_row + 1,
                column_span: coordinates.end_column - coordinates.start_column + 1,
                story_scalar_start,
                story_scalar_end,
                text,
                bounds: source.bounds,
                fill_rgb: source.paint.as_ref().map(|paint| paint.solid_fill_rgb),
                fill_visible: source.paint.as_ref().map(|paint| paint.fill_visible),
            })
        })
        .collect()
}

pub fn materialize_bounded_simple_table_cells(
    table: &PubTableSource,
    story: &Story,
) -> Result<Vec<PubMaterializedTableCell>, PubTableTextError> {
    if table.simple_table.is_none() {
        return Err(PubTableTextError::NotSimpleRectangular);
    }
    materialize_bounded_table_cells(table, story)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PubTableLayoutRelationSource {
    /// Exact Story-catalog key selecting the source layout/MCLD record.
    ///
    /// This relation remains valid independently of whether a narrower
    /// geometry-metric consumer can admit that record.
    pub story_layout_key: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_refs: Vec<SourceRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PubTableUniformTextInsetSource {
    /// Symmetric record-level TABLE text inset admitted independently of
    /// uniform row/column geometry metrics.
    pub inset_emu: LengthEmu,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_refs: Vec<SourceRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PubTableLayoutMetricsSource {
    pub story_layout_key: u32,
    pub cell_width: LengthEmu,
    pub row_pitch: LengthEmu,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_refs: Vec<SourceRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PubTableStoryOwnershipSource {
    pub text_id: u32,
    pub story_id: Option<StoryId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_refs: Vec<SourceRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PubTableSource {
    pub text_id: u32,
    pub story_id: Option<StoryId>,
    pub rows: u32,
    pub columns: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cells_seq_num: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tcd_story_ordinal: Option<u16>,
    pub cells: Vec<PubTableCellSource>,
    /// Present only for a complete, unmerged, unambiguous rectangular grid.
    pub simple_table: Option<SimpleRectangularTable<TableCellId>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layout_relation: Option<PubTableLayoutRelationSource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uniform_cell_text_inset: Option<PubTableUniformTextInsetSource>,
    pub layout_metrics: Option<PubTableLayoutMetricsSource>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub border_segments: Vec<PubTableBorderSegmentSource>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_refs: Vec<SourceRef>,
}

pub(crate) struct TableBridgeContext<'a> {
    pub source: &'a SourceDescriptor,
    pub contents_stream: &'a StreamPath,
    pub contents: &'a [u8],
    pub references: &'a BTreeMap<u32, Contents0x2cChunkReference>,
    pub quill_catalog: &'a QuillStoryCatalog,
    pub story_by_syid: &'a BTreeMap<u32, StoryId>,
    pub story_layout_keys: &'a BTreeMap<u32, (u32, RawSpan)>,
    pub mcld: Option<&'a QuillMcldChunk>,
    pub table_bounds: &'a RectEmu,
    pub officeart_owner_shape: &'a pub_escher::SpContainerObservation,
    pub officeart_inventory: &'a SpContainerInventory,
    pub color_scheme: Option<&'a MatureColorScheme>,
    pub dgg_defaults: Option<&'a pub_escher::DggDefaultOptionsObservation>,
}

fn rect_edges(rect: RectEmu) -> Option<[i128; 4]> {
    let left = i128::from(rect.x.get());
    let top = i128::from(rect.y.get());
    let right = left.checked_add(i128::from(rect.width.get()))?;
    let bottom = top.checked_add(i128::from(rect.height.get()))?;
    Some([left, top, right, bottom])
}

fn populate_bounded_table_cell_fill(
    context: &TableBridgeContext<'_>,
    table_seq_num: u32,
    cells: &mut [PubTableCellSource],
) -> Result<usize> {
    let owner = context.officeart_owner_shape;
    let group_coords = owner
        .fspgr
        .as_ref()
        .context("plain TABLE OfficeArt owner has no FSPGR")?;
    let group_rect = coordinate_rect_i128(group_coords)?;
    let target_rect =
        rect_edges(*context.table_bounds).context("plain TABLE owner bounds overflow")?;

    let children = context
        .officeart_inventory
        .shapes
        .iter()
        .filter(|shape| shape.parent_group_shape_source.as_ref() == Some(&owner.source))
        .filter_map(|shape| {
            let anchor = shape.child_anchor.as_ref()?;
            let child_rect = coordinate_rect_i128(anchor).ok()?;
            let projected = project_rect_trunc(child_rect, group_rect, target_rect).ok()?;
            Some((shape, projected))
        })
        .collect::<Vec<_>>();

    let mut admitted = 0_usize;
    for cell in cells {
        let Some(bounds) = cell.bounds.and_then(rect_edges) else {
            continue;
        };
        let matches = children
            .iter()
            .filter(|(_, projected)| *projected == bounds)
            .collect::<Vec<_>>();
        let [(shape, _)] = matches.as_slice() else {
            continue;
        };

        // T595/RUN522 authority is intentionally narrower than ordinary Shape
        // paint: exact cell geometry join + literal solid fill color + explicit
        // visibility are all required. Missing/ambiguous state remains absent.
        if unique_explicit_officeart_scalar(shape, OFFICE_ART_FILL_TYPE)
            .is_some_and(|fill_type| fill_type != 0)
        {
            continue;
        }
        let Some(color) = unique_explicit_officeart_scalar(shape, OFFICE_ART_FILL_COLOR)
            .and_then(direct_officeart_rgb)
        else {
            continue;
        };
        let Some(visible) = unique_explicit_officeart_scalar(shape, OFFICE_ART_FILL_BOOLEANS)
            .and_then(|value| {
                (value & FILL_USE_FILLED_BIT != 0).then_some(value & FILL_FILLED_BIT != 0)
            })
        else {
            continue;
        };

        cell.paint = Some(PubTableCellPaintSource {
            solid_fill_rgb: color,
            fill_visible: visible,
            source_refs: vec![source_ref(
                context.source,
                &shape.source,
                Some(format!(
                    "contents/0x2c/seq/{table_seq_num}/cell/stored/{}",
                    cell.stored_record_index
                )),
                Some("SpContainer/FOPT/table-cell-fill".into()),
                SourceRole::Projection,
                AuthorityClass::Authoritative,
                ReadConfidence::Exact,
            )],
        });
        admitted += 1;
    }

    Ok(admitted)
}

const TABLE_AUTOFORMAT_OWNER_REF_ID: u16 = 0x6802;
const TABLE_AUTOFORMAT_CELL_ORDINAL_ID: u16 = 0x2003;
const TABLE_AUTOFORMAT_RECTANGLE_SHAPE_TYPE: u16 = 0x0001;
const TABLE_AUTOFORMAT_SEGMENT_ORIENTATION_ID: u16 = 0x2001;
const TABLE_AUTOFORMAT_ROW_START_ID: u16 = 0x2004;
const TABLE_AUTOFORMAT_COLUMN_START_ID: u16 = 0x2005;
const TABLE_AUTOFORMAT_ROW_END_ID: u16 = 0x2006;
const TABLE_AUTOFORMAT_COLUMN_END_ID: u16 = 0x2007;

fn unique_anchor_scalar(anchor: &pub_escher::PublisherFieldRecord, field_id: u16) -> Option<u32> {
    let mut fields = anchor.fields.iter().filter(|field| field.id == field_id);
    let first = fields.next()?.value;
    if fields.next().is_some() {
        return None;
    }
    Some(first)
}

fn has_any_client_data_identity(shape: &pub_escher::SpContainerObservation) -> bool {
    shape.client_data.as_ref().is_some_and(|record| {
        record
            .fields
            .iter()
            .any(|field| field.id == PUBLISHER_FIELD_SHAPE_ID)
    })
}

fn native_autoformat_cell_ordinal_from_parts(
    shape_type: Option<u16>,
    has_client_data_identity: bool,
    anchor: Option<&pub_escher::PublisherFieldRecord>,
    table_seq_num: u32,
    cell_count: usize,
) -> Option<u32> {
    if shape_type != Some(TABLE_AUTOFORMAT_RECTANGLE_SHAPE_TYPE) || has_client_data_identity {
        return None;
    }

    let anchor = anchor?;
    if unique_anchor_scalar(anchor, TABLE_AUTOFORMAT_OWNER_REF_ID)? != table_seq_num {
        return None;
    }

    let ordinal = match anchor.fields.as_slice() {
        [only] if only.id == TABLE_AUTOFORMAT_OWNER_REF_ID => 0,
        [first, second]
            if (first.id == TABLE_AUTOFORMAT_OWNER_REF_ID
                && second.id == TABLE_AUTOFORMAT_CELL_ORDINAL_ID)
                || (first.id == TABLE_AUTOFORMAT_CELL_ORDINAL_ID
                    && second.id == TABLE_AUTOFORMAT_OWNER_REF_ID) =>
        {
            unique_anchor_scalar(anchor, TABLE_AUTOFORMAT_CELL_ORDINAL_ID)?
        }
        _ => return None,
    };

    let ordinal_index = usize::try_from(ordinal).ok()?;
    (ordinal_index < cell_count).then_some(ordinal)
}

fn native_autoformat_cell_ordinal(
    shape: &pub_escher::SpContainerObservation,
    table_seq_num: u32,
    cell_count: usize,
) -> Option<u32> {
    native_autoformat_cell_ordinal_from_parts(
        shape.fsp.as_ref().map(|fsp| fsp.shape_type),
        has_any_client_data_identity(shape),
        shape.client_anchor.as_ref(),
        table_seq_num,
        cell_count,
    )
}

fn unique_anchor_scalar_or_zero(
    anchor: &pub_escher::PublisherFieldRecord,
    field_id: u16,
) -> Option<u32> {
    let mut fields = anchor.fields.iter().filter(|field| field.id == field_id);
    let Some(first) = fields.next() else {
        return Some(0);
    };
    fields.next().is_none().then_some(first.value)
}

fn is_bounded_native_autoformat_border_carrier(
    shape: &pub_escher::SpContainerObservation,
    table_seq_num: u32,
    rows: u32,
    columns: u32,
) -> bool {
    if shape.fsp.as_ref().map(|fsp| fsp.shape_type) != Some(TABLE_AUTOFORMAT_RECTANGLE_SHAPE_TYPE)
        || has_any_client_data_identity(shape)
    {
        return false;
    }
    let Some(anchor) = shape.client_anchor.as_ref() else {
        return false;
    };
    if unique_anchor_scalar(anchor, TABLE_AUTOFORMAT_OWNER_REF_ID) != Some(table_seq_num)
        || anchor
            .fields
            .iter()
            .any(|field| field.id == TABLE_AUTOFORMAT_CELL_ORDINAL_ID)
    {
        return false;
    }

    let allowed = [
        TABLE_AUTOFORMAT_OWNER_REF_ID,
        TABLE_AUTOFORMAT_SEGMENT_ORIENTATION_ID,
        TABLE_AUTOFORMAT_ROW_START_ID,
        TABLE_AUTOFORMAT_COLUMN_START_ID,
        TABLE_AUTOFORMAT_ROW_END_ID,
        TABLE_AUTOFORMAT_COLUMN_END_ID,
    ];
    if anchor
        .fields
        .iter()
        .any(|field| !allowed.contains(&field.id))
        || !anchor.fields.iter().any(|field| {
            matches!(
                field.id,
                TABLE_AUTOFORMAT_SEGMENT_ORIENTATION_ID
                    | TABLE_AUTOFORMAT_ROW_START_ID
                    | TABLE_AUTOFORMAT_COLUMN_START_ID
                    | TABLE_AUTOFORMAT_ROW_END_ID
                    | TABLE_AUTOFORMAT_COLUMN_END_ID
            )
        })
    {
        return false;
    }

    let Some(orientation) = unique_anchor_scalar(anchor, TABLE_AUTOFORMAT_SEGMENT_ORIENTATION_ID)
    else {
        return false;
    };
    let Some(row_start) = unique_anchor_scalar_or_zero(anchor, TABLE_AUTOFORMAT_ROW_START_ID)
    else {
        return false;
    };
    let Some(column_start) = unique_anchor_scalar_or_zero(anchor, TABLE_AUTOFORMAT_COLUMN_START_ID)
    else {
        return false;
    };
    let Some(row_end) = unique_anchor_scalar_or_zero(anchor, TABLE_AUTOFORMAT_ROW_END_ID) else {
        return false;
    };
    let Some(column_end) = unique_anchor_scalar_or_zero(anchor, TABLE_AUTOFORMAT_COLUMN_END_ID)
    else {
        return false;
    };
    if row_start > rows || row_end > rows || column_start > columns || column_end > columns {
        return false;
    }

    matches!(
        orientation,
        1 if row_start == row_end && column_start < column_end
    ) || matches!(
        orientation,
        2 if column_start == column_end && row_start < row_end
    )
}

fn complete_merged_table_grid(rows: u32, columns: u32, cells: &[PubTableCellSource]) -> bool {
    let Some(slot_count_u64) = u64::from(rows).checked_mul(u64::from(columns)) else {
        return false;
    };
    let Ok(slot_count) = usize::try_from(slot_count_u64) else {
        return false;
    };
    if slot_count == 0 || cells.is_empty() {
        return false;
    }

    let mut covered = vec![false; slot_count];
    let mut stored_ordinals = vec![false; cells.len()];
    let mut has_span = false;

    for cell in cells {
        let Some(coordinates) = cell.coordinates else {
            return false;
        };
        if cell.bounds.is_none()
            || coordinates.start_row > coordinates.end_row
            || coordinates.start_column > coordinates.end_column
            || coordinates.end_row >= rows
            || coordinates.end_column >= columns
        {
            return false;
        }

        let Ok(stored_index) = usize::try_from(cell.stored_record_index) else {
            return false;
        };
        if stored_index >= stored_ordinals.len() || stored_ordinals[stored_index] {
            return false;
        }
        stored_ordinals[stored_index] = true;

        has_span |= coordinates.start_row != coordinates.end_row
            || coordinates.start_column != coordinates.end_column;

        for row in coordinates.start_row..=coordinates.end_row {
            for column in coordinates.start_column..=coordinates.end_column {
                let Some(slot_u64) = u64::from(row)
                    .checked_mul(u64::from(columns))
                    .and_then(|value| value.checked_add(u64::from(column)))
                else {
                    return false;
                };
                let Ok(slot) = usize::try_from(slot_u64) else {
                    return false;
                };
                if slot >= covered.len() || covered[slot] {
                    return false;
                }
                covered[slot] = true;
            }
        }
    }

    has_span
        && covered.into_iter().all(|value| value)
        && stored_ordinals.into_iter().all(|value| value)
}

fn complete_merged_native_autoformat_carrier_cohort(
    context: &TableBridgeContext<'_>,
    table_seq_num: u32,
    rows: u32,
    columns: u32,
    cells: &[PubTableCellSource],
) -> bool {
    if !complete_merged_table_grid(rows, columns, cells) {
        return false;
    }

    let mut ordinal_counts = vec![0_u8; cells.len()];
    for shape in &context.officeart_inventory.shapes {
        let Some(anchor) = shape.client_anchor.as_ref() else {
            continue;
        };
        if !anchor
            .fields
            .iter()
            .any(|field| field.id == TABLE_AUTOFORMAT_OWNER_REF_ID && field.value == table_seq_num)
        {
            continue;
        }
        if unique_anchor_scalar(anchor, TABLE_AUTOFORMAT_OWNER_REF_ID) != Some(table_seq_num) {
            return false;
        }

        if let Some(ordinal) = native_autoformat_cell_ordinal(shape, table_seq_num, cells.len()) {
            let Ok(index) = usize::try_from(ordinal) else {
                return false;
            };
            let Some(count) = ordinal_counts.get_mut(index) else {
                return false;
            };
            *count = count.saturating_add(1);
            if *count != 1 {
                return false;
            }
            continue;
        }

        if is_bounded_native_autoformat_border_carrier(shape, table_seq_num, rows, columns) {
            continue;
        }

        return false;
    }

    ordinal_counts.into_iter().all(|count| count == 1)
}

fn populate_native_autoformat_table_borders(
    context: &TableBridgeContext<'_>,
    table_seq_num: u32,
    rows: u32,
    columns: u32,
) -> Vec<PubTableBorderSegmentSource> {
    let mut candidates = 0_usize;
    let mut by_key = BTreeMap::<
        (PubTableBorderAxis, u32, u32, u32, u32),
        Vec<&pub_escher::SpContainerObservation>,
    >::new();

    for shape in &context.officeart_inventory.shapes {
        if shape.fsp.as_ref().map(|fsp| fsp.shape_type)
            != Some(TABLE_AUTOFORMAT_RECTANGLE_SHAPE_TYPE)
            || has_any_client_data_identity(shape)
        {
            continue;
        }
        let Some(anchor) = shape.client_anchor.as_ref() else {
            continue;
        };
        if unique_anchor_scalar(anchor, TABLE_AUTOFORMAT_OWNER_REF_ID) != Some(table_seq_num)
            || anchor
                .fields
                .iter()
                .any(|field| field.id == TABLE_AUTOFORMAT_CELL_ORDINAL_ID)
            || !anchor.fields.iter().any(|field| {
                matches!(
                    field.id,
                    TABLE_AUTOFORMAT_SEGMENT_ORIENTATION_ID
                        | TABLE_AUTOFORMAT_ROW_START_ID
                        | TABLE_AUTOFORMAT_COLUMN_START_ID
                        | TABLE_AUTOFORMAT_ROW_END_ID
                        | TABLE_AUTOFORMAT_COLUMN_END_ID
                )
            })
        {
            continue;
        }
        candidates += 1;

        let allowed = [
            TABLE_AUTOFORMAT_OWNER_REF_ID,
            TABLE_AUTOFORMAT_SEGMENT_ORIENTATION_ID,
            TABLE_AUTOFORMAT_ROW_START_ID,
            TABLE_AUTOFORMAT_COLUMN_START_ID,
            TABLE_AUTOFORMAT_ROW_END_ID,
            TABLE_AUTOFORMAT_COLUMN_END_ID,
        ];
        if anchor
            .fields
            .iter()
            .any(|field| !allowed.contains(&field.id))
        {
            return Vec::new();
        }

        let Some(orientation) =
            unique_anchor_scalar(anchor, TABLE_AUTOFORMAT_SEGMENT_ORIENTATION_ID)
        else {
            return Vec::new();
        };
        let Some(row_start) = unique_anchor_scalar_or_zero(anchor, TABLE_AUTOFORMAT_ROW_START_ID)
        else {
            return Vec::new();
        };
        let Some(column_start) =
            unique_anchor_scalar_or_zero(anchor, TABLE_AUTOFORMAT_COLUMN_START_ID)
        else {
            return Vec::new();
        };
        let Some(row_end) = unique_anchor_scalar_or_zero(anchor, TABLE_AUTOFORMAT_ROW_END_ID)
        else {
            return Vec::new();
        };
        let Some(column_end) = unique_anchor_scalar_or_zero(anchor, TABLE_AUTOFORMAT_COLUMN_END_ID)
        else {
            return Vec::new();
        };
        if row_start > rows || row_end > rows || column_start > columns || column_end > columns {
            return Vec::new();
        }
        let axis = match orientation {
            1 if row_start == row_end && column_start < column_end => {
                PubTableBorderAxis::Horizontal
            }
            2 if column_start == column_end && row_start < row_end => PubTableBorderAxis::Vertical,
            _ => return Vec::new(),
        };
        by_key
            .entry((axis, row_start, column_start, row_end, column_end))
            .or_default()
            .push(shape);
    }

    if candidates == 0 {
        return Vec::new();
    }

    let mut out = Vec::with_capacity(candidates);
    for ((axis, row_start, column_start, row_end, column_end), shapes) in by_key {
        let [shape] = shapes.as_slice() else {
            return Vec::new();
        };
        let Some(fill_color_raw) = unique_explicit_officeart_scalar(shape, OFFICE_ART_FILL_COLOR)
        else {
            return Vec::new();
        };
        let Some(rgb) = bounded_officeart_rgb(fill_color_raw, context.color_scheme) else {
            return Vec::new();
        };
        let Some(width_emu) = unique_explicit_officeart_scalar(shape, OFFICE_ART_LINE_WIDTH)
            .and_then(|value| (value > 0 && value <= 0x0132_F540).then_some(i64::from(value)))
        else {
            return Vec::new();
        };

        let mut source_refs = vec![source_ref(
            context.source,
            &shape.source,
            Some(contents_object_key(table_seq_num)),
            Some("SpContainer/FOPT/table-autoformat-border-segment".into()),
            SourceRole::Projection,
            AuthorityClass::Authoritative,
            ReadConfidence::Exact,
        )];
        if (fill_color_raw >> 24) as u8 == 0x08 {
            let Some(scheme) = context.color_scheme else {
                return Vec::new();
            };
            source_refs.push(source_ref(
                context.source,
                &scheme.source,
                None,
                Some("OplSccm/current-color-scheme".into()),
                SourceRole::Projection,
                AuthorityClass::Authoritative,
                ReadConfidence::Exact,
            ));
        }

        out.push(PubTableBorderSegmentSource {
            axis,
            row_start,
            column_start,
            row_end,
            column_end,
            rgb,
            width_emu,
            source_refs,
        });
    }

    if out.len() == candidates {
        out
    } else {
        Vec::new()
    }
}

fn populate_native_autoformat_table_cell_fill(
    context: &TableBridgeContext<'_>,
    table_seq_num: u32,
    cells: &mut [PubTableCellSource],
) -> usize {
    let mut by_ordinal = BTreeMap::<u32, Vec<&pub_escher::SpContainerObservation>>::new();

    for shape in &context.officeart_inventory.shapes {
        let Some(ordinal) = native_autoformat_cell_ordinal(shape, table_seq_num, cells.len())
        else {
            continue;
        };
        by_ordinal.entry(ordinal).or_default().push(shape);
    }

    let mut admitted = 0_usize;
    for cell in cells.iter_mut().filter(|cell| cell.paint.is_none()) {
        let ordinal = cell.stored_record_index;
        let Some([shape]) = by_ordinal.get(&ordinal).map(Vec::as_slice) else {
            continue;
        };

        let Some(paint) = resolve_bounded_effective_officeart_paint(
            shape,
            context.dgg_defaults,
            context.color_scheme,
            admits_normative_2d_paint_defaults(shape),
        ) else {
            continue;
        };
        if paint.fill.solid.as_ref().map(|value| value.value) != Some(true) {
            continue;
        }
        let Some(color) = paint.fill.color_rgb.as_ref().map(|value| value.value) else {
            continue;
        };
        let Some(visible) = paint.fill.visible.as_ref().map(|value| value.value) else {
            continue;
        };

        let mut source_refs = vec![source_ref(
            context.source,
            &shape.source,
            Some(format!(
                "contents/0x2c/seq/{table_seq_num}/cell/stored/{}",
                cell.stored_record_index
            )),
            Some("SpContainer/FOPT/table-autoformat-cell-fill".into()),
            SourceRole::Projection,
            AuthorityClass::Authoritative,
            ReadConfidence::Exact,
        )];
        if paint_context_uses_officeart_scheme_color(shape, context.dgg_defaults) {
            if let Some(scheme) = context.color_scheme {
                source_refs.push(source_ref(
                    context.source,
                    &scheme.source,
                    None,
                    Some("OplSccm/current-color-scheme".into()),
                    SourceRole::Projection,
                    AuthorityClass::Authoritative,
                    ReadConfidence::Exact,
                ));
            }
        }
        if effective_paint_has_dgg_authority(&paint) {
            if let Some(dgg) = context.dgg_defaults {
                source_refs.push(source_ref(
                    context.source,
                    &dgg.source,
                    Some("escher/dgg/default-options".into()),
                    Some("DggContainer/FOPT-defaults".into()),
                    SourceRole::Projection,
                    AuthorityClass::Authoritative,
                    ReadConfidence::Exact,
                ));
            }
        }

        cell.paint = Some(PubTableCellPaintSource {
            solid_fill_rgb: color,
            fill_visible: visible,
            source_refs,
        });
        admitted += 1;
    }

    admitted
}

fn populate_exact_table_cell_bounds(
    context: &TableBridgeContext<'_>,
    table_seq_num: u32,
    table_chunk: &Contents0x2cChunk,
    tail_scalars: &TableTailScalars,
    rows: u32,
    columns: u32,
    cells: &mut [PubTableCellSource],
) -> Result<bool> {
    let mut arrays = table_chunk
        .fields
        .iter()
        .filter(|field| field.id == TABLE_ROWCOL_ARRAY_ID);
    let Some(array) = arrays.next() else {
        return Ok(false);
    };
    if arrays.next().is_some() {
        bail!("TABLE has duplicate row/column arrays");
    }
    let RawContentsBlockBody::Container { content_source, .. } = &array.body else {
        bail!("TABLE row/column array is not a container");
    };

    let start = usize::try_from(content_source.offset)
        .map_err(|_| anyhow!("TABLE row/column array offset does not fit usize"))?;
    let len = usize::try_from(content_source.len)
        .map_err(|_| anyhow!("TABLE row/column array length does not fit usize"))?;
    let mut cursor =
        ContentsCursor::bounded(content_source.stream.clone(), context.contents, start, len)?;
    let mut sizes = Vec::new();
    let mut physical_ends = Vec::new();

    while cursor.remaining() > 0 {
        let item = parse_confirmed_block(&mut cursor)?;
        if item.id != 0 {
            bail!(
                "TABLE row/column array item has nonzero id 0x{:02X}",
                item.id
            );
        }
        let RawContentsBlockBody::Container {
            content_source: item_source,
            ..
        } = &item.body
        else {
            bail!("TABLE row/column array item is not a container");
        };
        let item_start = usize::try_from(item_source.offset)
            .map_err(|_| anyhow!("TABLE row/column item offset does not fit usize"))?;
        let item_len = usize::try_from(item_source.len)
            .map_err(|_| anyhow!("TABLE row/column item length does not fit usize"))?;
        let mut item_cursor = ContentsCursor::bounded(
            item_source.stream.clone(),
            context.contents,
            item_start,
            item_len,
        )?;
        let mut size = None;
        let mut physical_end = None;
        while item_cursor.remaining() > 0 {
            let field = parse_confirmed_block(&mut item_cursor)?;
            if field.id != TABLE_ROWCOL_SIZE_ID && field.id != TABLE_ROWCOL_PHYSICAL_END_ID {
                continue;
            }
            let RawContentsBlockBody::U32 { value, .. } = field.body else {
                bail!("TABLE row/column geometry field is not u32");
            };
            if field.id == TABLE_ROWCOL_SIZE_ID {
                if size.replace(value).is_some() {
                    bail!("TABLE row/column item has duplicate size");
                }
            } else if physical_end.replace(value).is_some() {
                bail!("TABLE row/column item has duplicate physical end");
            }
        }
        let size = size.context("TABLE row/column item has no size")?;
        let physical_end =
            physical_end.context("TABLE row/column item has no physical cumulative end")?;
        if size == 0 || physical_end == 0 {
            bail!("TABLE row/column size or physical end is zero");
        }
        sizes.push(size);
        physical_ends.push(physical_end);
    }

    let expected = usize::try_from(columns)
        .ok()
        .and_then(|columns| {
            usize::try_from(rows)
                .ok()
                .and_then(|rows| columns.checked_add(rows))
        })
        .context("TABLE row/column count overflows usize")?;
    if sizes.len() != expected || physical_ends.len() != expected {
        bail!(
            "TABLE row/column array count differs from columns+rows {}",
            expected
        );
    }

    let split = usize::try_from(columns).context("TABLE column count does not fit usize")?;
    let (column_widths, row_heights) = sizes.split_at(split);
    let (column_ends, row_ends) = physical_ends.split_at(split);
    let declared_width = unique_table_scalar(table_chunk, tail_scalars, TABLE_WIDTH_ID)?
        .map(|(value, _)| value)
        .context("TABLE width is missing")?;
    let declared_height = unique_table_scalar(table_chunk, tail_scalars, TABLE_HEIGHT_ID)?
        .map(|(value, _)| value)
        .context("TABLE height is missing")?;

    let sum = |values: &[u32]| -> Result<u64> {
        values.iter().try_fold(0_u64, |total, value| {
            total
                .checked_add(u64::from(*value))
                .context("TABLE track extent sum overflow")
        })
    };
    let width_sum = sum(column_widths)?;
    let height_sum = sum(row_heights)?;
    if width_sum != u64::from(declared_width) || height_sum != u64::from(declared_height) {
        bail!(
            "TABLE logical track sums {}x{} differ from declared {}x{}",
            width_sum,
            height_sum,
            declared_width,
            declared_height
        );
    }

    let owner_width = u64::try_from(context.table_bounds.width.get())
        .map_err(|_| anyhow!("TABLE owner width is negative"))?;
    let owner_height = u64::try_from(context.table_bounds.height.get())
        .map_err(|_| anyhow!("TABLE owner height is negative"))?;

    let exact_boundaries = |ends: &[u32], owner_extent: u64, axis: &str| -> Result<Vec<u64>> {
        let mut out = Vec::with_capacity(ends.len() + 1);
        out.push(0);
        let mut previous = 0_u64;
        for end in ends {
            let end = u64::from(*end);
            if end <= previous {
                bail!("TABLE {axis} physical cumulative ends are not strictly increasing");
            }
            if end > owner_extent {
                bail!("TABLE {axis} physical cumulative end exceeds owner extent");
            }
            out.push(end);
            previous = end;
        }
        if previous != owner_extent {
            bail!(
                "TABLE {axis} physical terminal end {} differs from owner extent {}",
                previous,
                owner_extent
            );
        }
        Ok(out)
    };

    let column_prefix = exact_boundaries(column_ends, owner_width, "column")?;
    let row_prefix = exact_boundaries(row_ends, owner_height, "row")?;

    for cell in cells {
        let coordinates = cell
            .coordinates
            .context("TABLE cell coordinates are unavailable for exact track geometry")?;
        if coordinates.start_row > coordinates.end_row
            || coordinates.start_column > coordinates.end_column
            || coordinates.end_row >= rows
            || coordinates.end_column >= columns
        {
            bail!("TABLE cell coordinates exceed bounded table grid");
        }

        let start_column =
            usize::try_from(coordinates.start_column).context("TABLE column index overflow")?;
        let end_column = usize::try_from(coordinates.end_column)
            .context("TABLE column index overflow")?
            .checked_add(1)
            .context("TABLE column end overflow")?;
        let start_row =
            usize::try_from(coordinates.start_row).context("TABLE row index overflow")?;
        let end_row = usize::try_from(coordinates.end_row)
            .context("TABLE row index overflow")?
            .checked_add(1)
            .context("TABLE row end overflow")?;

        let x_offset = i64::try_from(column_prefix[start_column])
            .context("TABLE x offset does not fit i64")?;
        let y_offset =
            i64::try_from(row_prefix[start_row]).context("TABLE y offset does not fit i64")?;
        let width = i64::try_from(column_prefix[end_column] - column_prefix[start_column])
            .context("TABLE cell width does not fit i64")?;
        let height = i64::try_from(row_prefix[end_row] - row_prefix[start_row])
            .context("TABLE cell height does not fit i64")?;
        let x = context
            .table_bounds
            .x
            .get()
            .checked_add(x_offset)
            .context("TABLE cell x overflow")?;
        let y = context
            .table_bounds
            .y
            .get()
            .checked_add(y_offset)
            .context("TABLE cell y overflow")?;

        cell.bounds = Some(RectEmu::new(
            LengthEmu::new(x),
            LengthEmu::new(y),
            LengthEmu::new(width),
            LengthEmu::new(height),
        ));
        cell.source_refs.push(source_ref(
            context.source,
            &array.source,
            Some(contents_object_key(table_seq_num)),
            Some("TABLE/rowcol_array/physical_end".into()),
            SourceRole::Projection,
            AuthorityClass::Authoritative,
            ReadConfidence::Exact,
        ));
    }

    Ok(true)
}

pub(crate) fn build_table_story_ownership_source(
    context: &TableBridgeContext<'_>,
    table_seq_num: u32,
    table_chunk: &Contents0x2cChunk,
) -> Result<Option<PubTableStoryOwnershipSource>> {
    let tail_scalars = scan_table_tail_scalars(context, table_chunk)?;
    let Some((text_id, text_id_source)) =
        unique_table_scalar(table_chunk, &tail_scalars, FIELD_STORY_ID)?
    else {
        return Ok(None);
    };

    Ok(Some(PubTableStoryOwnershipSource {
        text_id,
        story_id: context.story_by_syid.get(&text_id).copied(),
        source_refs: vec![source_ref(
            context.source,
            &text_id_source,
            Some(contents_object_key(table_seq_num)),
            Some("TABLE/textId".into()),
            SourceRole::Relation,
            AuthorityClass::Authoritative,
            ReadConfidence::Exact,
        )],
    }))
}

pub(crate) fn build_table_source(
    context: &TableBridgeContext<'_>,
    table_seq_num: u32,
    table_chunk: &Contents0x2cChunk,
    diagnostics: &mut Vec<PubBridgeDiagnostic>,
) -> Result<Option<PubTableSource>> {
    let tail_scalars = scan_table_tail_scalars(context, table_chunk)?;
    let Some((text_id, text_id_source)) =
        unique_table_scalar(table_chunk, &tail_scalars, FIELD_STORY_ID)?
    else {
        diagnostics.push(PubBridgeDiagnostic::TableMissingRequiredField {
            seq_num: table_seq_num,
            field_id: FIELD_STORY_ID,
        });
        return Ok(None);
    };
    let Some((rows, rows_source)) =
        unique_table_scalar(table_chunk, &tail_scalars, TABLE_NUM_ROWS_ID)?
    else {
        diagnostics.push(PubBridgeDiagnostic::TableMissingRequiredField {
            seq_num: table_seq_num,
            field_id: TABLE_NUM_ROWS_ID,
        });
        return Ok(None);
    };
    let Some((columns, columns_source)) =
        unique_table_scalar(table_chunk, &tail_scalars, TABLE_NUM_COLUMNS_ID)?
    else {
        diagnostics.push(PubBridgeDiagnostic::TableMissingRequiredField {
            seq_num: table_seq_num,
            field_id: TABLE_NUM_COLUMNS_ID,
        });
        return Ok(None);
    };
    let Some((cells_seq_num, cells_seq_source)) =
        unique_table_scalar(table_chunk, &tail_scalars, TABLE_CELLS_SEQ_NUM_ID)?
    else {
        diagnostics.push(PubBridgeDiagnostic::TableMissingRequiredField {
            seq_num: table_seq_num,
            field_id: TABLE_CELLS_SEQ_NUM_ID,
        });
        return Ok(None);
    };

    let story_id = context.story_by_syid.get(&text_id).copied();
    if story_id.is_none() {
        diagnostics.push(PubBridgeDiagnostic::MissingQuillStory {
            seq_num: table_seq_num,
            text_id,
        });
    }

    let mut tcd_matches = context
        .quill_catalog
        .tcd
        .iter()
        .filter(|tcd| tcd.story_syid.value.0 == text_id);
    let Some(tcd) = tcd_matches.next() else {
        diagnostics.push(PubBridgeDiagnostic::TableMissingTcd {
            seq_num: table_seq_num,
            text_id,
        });
        return Ok(None);
    };
    if tcd_matches.next().is_some() {
        diagnostics.push(PubBridgeDiagnostic::TableAmbiguousTcd {
            seq_num: table_seq_num,
            text_id,
        });
        return Ok(None);
    }

    let Some(cells_reference) = context.references.get(&cells_seq_num) else {
        diagnostics.push(PubBridgeDiagnostic::TableMissingCellsObject {
            seq_num: table_seq_num,
            cells_seq_num,
        });
        return Ok(None);
    };
    if single_raw_type(cells_reference) != Some(CONTENTS_RAW_TYPE_CELLS) {
        diagnostics.push(PubBridgeDiagnostic::TableCellsWrongRawType {
            seq_num: table_seq_num,
            cells_seq_num,
            raw_type: single_raw_type(cells_reference),
        });
        return Ok(None);
    }
    if single_parent_seq(cells_reference) != Some(table_seq_num) {
        diagnostics.push(PubBridgeDiagnostic::TableCellsWrongParent {
            seq_num: table_seq_num,
            cells_seq_num,
            parent_seq_num: single_parent_seq(cells_reference),
        });
    }

    let cells_chunk = chunk_for_reference(
        context.contents_stream.clone(),
        context.contents,
        cells_reference,
    )
    .context("parse TABLE-owned CELLS chunk")?;
    let cells = parse_confirmed_mature_cells(context.contents, &cells_chunk)
        .context("parse TABLE-owned mature CELLS")?;

    if cells.records.len() != tcd.cell_end_offsets_utf16.len() {
        diagnostics.push(PubBridgeDiagnostic::TableCellCountMismatch {
            seq_num: table_seq_num,
            cells_records: cells.records.len(),
            tcd_boundaries: tcd.cell_end_offsets_utf16.len(),
        });
        return Ok(None);
    }

    let mut joined_cells = Vec::with_capacity(cells.records.len());
    let mut previous_end = 0_u32;
    let mut monotonic = true;

    for (record, end) in cells.records.iter().zip(&tcd.cell_end_offsets_utf16) {
        if end.value < previous_end || end.value > tcd.story_utf16_code_units.value {
            monotonic = false;
            diagnostics.push(PubBridgeDiagnostic::TableCellTextRangeInvalid {
                seq_num: table_seq_num,
                stored_record_index: record.record_index,
                previous_end,
                end: end.value,
                story_len: tcd.story_utf16_code_units.value,
            });
        }

        let coordinates = record.effective_coordinates().map(Into::into);
        if coordinates.is_none() {
            diagnostics.push(PubBridgeDiagnostic::TableCellCoordinatesAmbiguous {
                seq_num: table_seq_num,
                stored_record_index: record.record_index,
            });
        }

        let id = TableCellId::from_canonical(derive_pub_id(
            &context.source.source_hash,
            &format!(
                "contents/0x2c/seq/{table_seq_num}/cells/stored/{}",
                record.record_index
            ),
            "cdm.table_cell",
        )?);

        joined_cells.push(PubTableCellSource {
            id,
            stored_record_index: record.record_index,
            coordinates,
            utf16_start: previous_end,
            utf16_end: end.value,
            bounds: None,
            paint: None,
            source_refs: vec![
                source_ref(
                    context.source,
                    &record.source,
                    Some(contents_object_key(cells_seq_num)),
                    Some(format!("CELLS/record/{}", record.record_index)),
                    SourceRole::Relation,
                    AuthorityClass::Authoritative,
                    ReadConfidence::Exact,
                ),
                source_ref(
                    context.source,
                    &end.source,
                    Some(quill_story_object_key(text_id)),
                    Some(format!("TCD/cell_end/{}", record.record_index)),
                    SourceRole::Relation,
                    AuthorityClass::Authoritative,
                    ReadConfidence::Exact,
                ),
            ],
        });
        previous_end = end.value;
    }

    if monotonic && previous_end != tcd.story_utf16_code_units.value {
        diagnostics.push(PubBridgeDiagnostic::TableStoryLengthMismatch {
            seq_num: table_seq_num,
            tcd_last_end: previous_end,
            story_len: tcd.story_utf16_code_units.value,
        });
    }

    if let Err(error) = populate_exact_table_cell_bounds(
        context,
        table_seq_num,
        table_chunk,
        &tail_scalars,
        rows,
        columns,
        &mut joined_cells,
    ) {
        diagnostics.push(PubBridgeDiagnostic::TableLayoutMetricsUnavailable {
            seq_num: table_seq_num,
            text_id,
            layout_key: None,
            reason: format!("TABLE row/column track geometry unavailable: {error}"),
        });
    }

    let simple_table = build_simple_table(rows, columns, &joined_cells);
    let merged_autoformat_cohort = simple_table.is_none()
        && complete_merged_native_autoformat_carrier_cohort(
            context,
            table_seq_num,
            rows,
            columns,
            &joined_cells,
        );
    let border_segments = if simple_table.is_some() {
        let _ = populate_bounded_table_cell_fill(context, table_seq_num, &mut joined_cells);
        let _ =
            populate_native_autoformat_table_cell_fill(context, table_seq_num, &mut joined_cells);
        populate_native_autoformat_table_borders(context, table_seq_num, rows, columns)
    } else if merged_autoformat_cohort {
        let mut candidate_cells = joined_cells.clone();
        let admitted = populate_native_autoformat_table_cell_fill(
            context,
            table_seq_num,
            &mut candidate_cells,
        );
        if admitted == candidate_cells.len() {
            joined_cells = candidate_cells;
            populate_native_autoformat_table_borders(context, table_seq_num, rows, columns)
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };
    let layout_relation = build_table_layout_relation(context, text_id);
    let uniform_cell_text_inset = layout_relation
        .as_ref()
        .and_then(|relation| build_table_uniform_text_inset(context, text_id, relation));
    let layout_metrics = build_table_layout_metrics(context, table_seq_num, text_id, diagnostics);

    Ok(Some(PubTableSource {
        text_id,
        story_id,
        rows,
        columns,
        cells_seq_num: Some(cells_seq_num),
        tcd_story_ordinal: Some(tcd.story_ordinal.value),
        cells: joined_cells,
        simple_table,
        layout_relation,
        uniform_cell_text_inset,
        layout_metrics,
        border_segments,
        source_refs: vec![
            source_ref(
                context.source,
                &text_id_source,
                Some(contents_object_key(table_seq_num)),
                Some("TABLE/textId".into()),
                SourceRole::Relation,
                AuthorityClass::Authoritative,
                ReadConfidence::Exact,
            ),
            source_ref(
                context.source,
                &rows_source,
                Some(contents_object_key(table_seq_num)),
                Some("TABLE/rows".into()),
                SourceRole::Semantic,
                AuthorityClass::Authoritative,
                ReadConfidence::Exact,
            ),
            source_ref(
                context.source,
                &columns_source,
                Some(contents_object_key(table_seq_num)),
                Some("TABLE/columns".into()),
                SourceRole::Semantic,
                AuthorityClass::Authoritative,
                ReadConfidence::Exact,
            ),
            source_ref(
                context.source,
                &cells_seq_source,
                Some(contents_object_key(table_seq_num)),
                Some("TABLE/cellsSeqNum".into()),
                SourceRole::Relation,
                AuthorityClass::Authoritative,
                ReadConfidence::Exact,
            ),
            source_ref(
                context.source,
                &tcd.source,
                Some(quill_story_object_key(text_id)),
                Some("TCD".into()),
                SourceRole::Relation,
                AuthorityClass::Authoritative,
                ReadConfidence::Exact,
            ),
            source_ref(
                context.source,
                &cells.source,
                Some(contents_object_key(cells_seq_num)),
                Some("CELLS".into()),
                SourceRole::Relation,
                AuthorityClass::Authoritative,
                ReadConfidence::Exact,
            ),
        ],
    }))
}

fn build_table_layout_relation(
    context: &TableBridgeContext<'_>,
    text_id: u32,
) -> Option<PubTableLayoutRelationSource> {
    let (layout_key, layout_key_source) = context.story_layout_keys.get(&text_id)?;
    let mcld = context.mcld?;
    let record = mcld
        .records
        .iter()
        .find(|record| record.record_id == *layout_key)?;

    Some(PubTableLayoutRelationSource {
        story_layout_key: *layout_key,
        source_refs: vec![
            source_ref(
                context.source,
                layout_key_source,
                Some(format!("contents/0x65/story/{text_id}")),
                Some("story/layout_key".into()),
                SourceRole::Relation,
                AuthorityClass::Authoritative,
                ReadConfidence::Exact,
            ),
            source_ref(
                context.source,
                &record.source,
                Some(quill_story_object_key(text_id)),
                Some("MCLD/record".into()),
                SourceRole::Relation,
                AuthorityClass::Authoritative,
                ReadConfidence::Exact,
            ),
        ],
    })
}

fn build_table_uniform_text_inset(
    context: &TableBridgeContext<'_>,
    text_id: u32,
    relation: &PubTableLayoutRelationSource,
) -> Option<PubTableUniformTextInsetSource> {
    let mcld = context.mcld?;
    let inset =
        bounded_mcld_table_uniform_text_inset(mcld, relation.story_layout_key).ok()?;

    let mut source_refs = relation.source_refs.clone();
    source_refs.extend(inset.sources.iter().map(|source| {
        source_ref(
            context.source,
            source,
            Some(quill_story_object_key(text_id)),
            Some("MCLD/table/child/fields06-09-unanimous".into()),
            SourceRole::Projection,
            AuthorityClass::Authoritative,
            ReadConfidence::Exact,
        )
    }));

    Some(PubTableUniformTextInsetSource {
        inset_emu: LengthEmu::new(i64::from(inset.inset_emu)),
        source_refs,
    })
}

fn build_table_layout_metrics(
    context: &TableBridgeContext<'_>,
    table_seq_num: u32,
    text_id: u32,
    diagnostics: &mut Vec<PubBridgeDiagnostic>,
) -> Option<PubTableLayoutMetricsSource> {
    let Some((layout_key, layout_key_source)) = context.story_layout_keys.get(&text_id) else {
        diagnostics.push(PubBridgeDiagnostic::TableLayoutMetricsUnavailable {
            seq_num: table_seq_num,
            text_id,
            layout_key: None,
            reason: "story catalog has no unique layout key".into(),
        });
        return None;
    };

    let Some(mcld) = context.mcld else {
        diagnostics.push(PubBridgeDiagnostic::TableLayoutMetricsUnavailable {
            seq_num: table_seq_num,
            text_id,
            layout_key: Some(*layout_key),
            reason: "usable bounded Quill MCLD is unavailable".into(),
        });
        return None;
    };

    let metrics = match bounded_mcld_table_metrics(mcld, *layout_key) {
        Ok(metrics) => metrics,
        Err(error) => {
            diagnostics.push(PubBridgeDiagnostic::TableLayoutMetricsUnavailable {
                seq_num: table_seq_num,
                text_id,
                layout_key: Some(*layout_key),
                reason: error.to_string(),
            });
            return None;
        }
    };

    let mut source_refs = vec![source_ref(
        context.source,
        layout_key_source,
        Some(format!("contents/0x65/story/{text_id}")),
        Some("story/layout_key".into()),
        SourceRole::Relation,
        AuthorityClass::Authoritative,
        ReadConfidence::Exact,
    )];

    source_refs.extend(metrics.cell_width_emu.sources.iter().map(|source| {
        source_ref(
            context.source,
            source,
            Some(quill_story_object_key(text_id)),
            Some("MCLD/table/child/field04".into()),
            SourceRole::Projection,
            AuthorityClass::Authoritative,
            ReadConfidence::Exact,
        )
    }));
    source_refs.extend(metrics.row_pitch_emu.sources.iter().map(|source| {
        source_ref(
            context.source,
            source,
            Some(quill_story_object_key(text_id)),
            Some("MCLD/table/child/field05".into()),
            SourceRole::Projection,
            AuthorityClass::Authoritative,
            ReadConfidence::Exact,
        )
    }));

    Some(PubTableLayoutMetricsSource {
        story_layout_key: *layout_key,
        cell_width: LengthEmu::new(i64::from(metrics.cell_width_emu.value)),
        row_pitch: LengthEmu::new(i64::from(metrics.row_pitch_emu.value)),
        source_refs,
    })
}

fn build_simple_table(
    rows: u32,
    columns: u32,
    cells: &[PubTableCellSource],
) -> Option<SimpleRectangularTable<TableCellId>> {
    let simple_cells = cells
        .iter()
        .map(|cell| {
            let coordinates = cell.coordinates?;
            if coordinates.start_row != coordinates.end_row
                || coordinates.start_column != coordinates.end_column
            {
                return None;
            }
            Some(SimpleTableCell {
                id: cell.id,
                address: TableCellAddress {
                    row: coordinates.start_row,
                    column: coordinates.start_column,
                },
            })
        })
        .collect::<Option<Vec<_>>>()?;

    SimpleRectangularTable::new(rows, columns, simple_cells).ok()
}

type TableTailScalars = BTreeMap<u16, Vec<(u32, RawSpan)>>;

fn unique_table_scalar(
    chunk: &Contents0x2cChunk,
    tail_scalars: &TableTailScalars,
    id: u16,
) -> Result<Option<(u32, RawSpan)>> {
    let mut values = Vec::new();

    for field in chunk.fields.iter().filter(|field| field.id == id) {
        match &field.body {
            RawContentsBlockBody::U16 {
                value,
                value_source,
            } => values.push((u32::from(*value), value_source.clone())),
            RawContentsBlockBody::U32 {
                value,
                value_source,
            } => values.push((*value, value_source.clone())),
            _ => bail!(
                "TABLE field 0x{id:02X} at {} is not a confirmed integer/reference body",
                field.source.offset
            ),
        }
    }

    if let Some(tail_values) = tail_scalars.get(&id) {
        values.extend(tail_values.iter().cloned());
    }

    match values.as_slice() {
        [] => Ok(None),
        [value] => Ok(Some(value.clone())),
        _ => bail!("duplicate TABLE scalar field 0x{id:02X}"),
    }
}

/// Scans only the opaque suffix of a mature TABLE chunk using the physical
/// block-length grammar independently implemented by libmspub's
/// parseBlock(..., true). The scanner keeps only 2/4-byte scalar observations;
/// variable-length blocks are skipped as opaque payloads.
///
/// This does not promote the rest of the tail to semantic state.
fn scan_table_tail_scalars(
    context: &TableBridgeContext<'_>,
    chunk: &Contents0x2cChunk,
) -> Result<TableTailScalars> {
    let Some(tail) = chunk.unsupported_tail.as_ref() else {
        return Ok(BTreeMap::new());
    };

    let start = usize::try_from(tail.offset)
        .map_err(|_| anyhow!("TABLE opaque tail offset does not fit usize"))?;
    let len = usize::try_from(tail.len)
        .map_err(|_| anyhow!("TABLE opaque tail length does not fit usize"))?;
    let end = start
        .checked_add(len)
        .filter(|end| *end <= context.contents.len())
        .ok_or_else(|| anyhow!("TABLE opaque tail is outside Contents stream"))?;

    let bytes = context.contents;
    let mut position = start;
    let mut scalars = BTreeMap::<u16, Vec<(u32, RawSpan)>>::new();

    while position < end {
        if end - position < 2 {
            bail!("truncated TABLE tail block header at {position}");
        }

        let raw_tag = [bytes[position], bytes[position + 1]];
        let (id, wire_type) = pub_contents::decode_packed_field_tag(raw_tag);
        position += 2;

        match wire_type {
            0x00 | 0x08 | 0x78 => {}
            0x10 | 0x18 => {
                if end - position < 2 {
                    bail!("truncated TABLE tail u16 field 0x{id:02X} at {position}");
                }
                let value = u16::from_le_bytes([bytes[position], bytes[position + 1]]);
                scalars.entry(id).or_default().push((
                    u32::from(value),
                    RawSpan {
                        stream: tail.stream.clone(),
                        offset: position as u64,
                        len: 2,
                    },
                ));
                position += 2;
            }
            0x20 | 0x58 | 0x68 | 0x70 | 0xB8 => {
                if end - position < 4 {
                    bail!("truncated TABLE tail u32 field 0x{id:02X} at {position}");
                }
                let value = u32::from_le_bytes([
                    bytes[position],
                    bytes[position + 1],
                    bytes[position + 2],
                    bytes[position + 3],
                ]);
                scalars.entry(id).or_default().push((
                    value,
                    RawSpan {
                        stream: tail.stream.clone(),
                        offset: position as u64,
                        len: 4,
                    },
                ));
                position += 4;
            }
            0x28 => {
                position = checked_skip(position, 8, end, id, wire_type)?;
            }
            0x38 => {
                position = checked_skip(position, 16, end, id, wire_type)?;
            }
            0x48 => {
                position = checked_skip(position, 24, end, id, wire_type)?;
            }
            0x80 | 0x88 | 0x90 | 0x98 | 0xA0 | 0xC0 => {
                if end - position < 4 {
                    bail!("truncated TABLE tail variable field 0x{id:02X} length at {position}");
                }
                let declared_length = u32::from_le_bytes([
                    bytes[position],
                    bytes[position + 1],
                    bytes[position + 2],
                    bytes[position + 3],
                ]);
                if declared_length < 4 {
                    bail!("invalid TABLE tail variable field 0x{id:02X} length {declared_length}");
                }
                position = checked_skip(
                    position,
                    usize::try_from(declared_length)
                        .map_err(|_| anyhow!("TABLE tail declared length does not fit usize"))?,
                    end,
                    id,
                    wire_type,
                )?;
            }
            other => {
                bail!(
                    "unsupported TABLE tail wire type 0x{other:02X} for field 0x{id:02X} at {}",
                    position - 2
                );
            }
        }
    }

    Ok(scalars)
}

fn checked_skip(
    position: usize,
    length: usize,
    end: usize,
    id: u16,
    wire_type: u8,
) -> Result<usize> {
    position
        .checked_add(length)
        .filter(|next| *next <= end)
        .ok_or_else(|| {
            anyhow!("TABLE tail field 0x{id:02X}/type 0x{wire_type:02X} exceeds bounded tail")
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table_cell_id(byte: u8) -> TableCellId {
        TableCellId::from_canonical(CanonicalId::from_bytes([byte; 16]))
    }

    fn test_span() -> RawSpan {
        RawSpan {
            stream: StreamPath("test".into()),
            offset: 0,
            len: 0,
        }
    }

    fn publisher_fields(fields: &[(u16, u32)]) -> pub_escher::PublisherFieldRecord {
        pub_escher::PublisherFieldRecord {
            duplicated_length: 0,
            duplicated_length_source: test_span(),
            fields: fields
                .iter()
                .map(|(id, value)| pub_escher::PublisherField {
                    id: *id,
                    value: *value,
                    source: test_span(),
                })
                .collect(),
            trailing_source: None,
        }
    }

    #[test]
    fn native_autoformat_anchor_accepts_exact_base_and_ordinal_forms() {
        let base = publisher_fields(&[(TABLE_AUTOFORMAT_OWNER_REF_ID, 77)]);
        assert_eq!(
            native_autoformat_cell_ordinal_from_parts(
                Some(TABLE_AUTOFORMAT_RECTANGLE_SHAPE_TYPE),
                false,
                Some(&base),
                77,
                4,
            ),
            Some(0)
        );

        for fields in [
            vec![
                (TABLE_AUTOFORMAT_OWNER_REF_ID, 77),
                (TABLE_AUTOFORMAT_CELL_ORDINAL_ID, 3),
            ],
            vec![
                (TABLE_AUTOFORMAT_CELL_ORDINAL_ID, 3),
                (TABLE_AUTOFORMAT_OWNER_REF_ID, 77),
            ],
        ] {
            let ordinal = publisher_fields(&fields);
            assert_eq!(
                native_autoformat_cell_ordinal_from_parts(
                    Some(TABLE_AUTOFORMAT_RECTANGLE_SHAPE_TYPE),
                    false,
                    Some(&ordinal),
                    77,
                    4,
                ),
                Some(3)
            );
        }
    }

    #[test]
    fn native_autoformat_anchor_rejects_unproven_or_ambiguous_forms() {
        let base = publisher_fields(&[(TABLE_AUTOFORMAT_OWNER_REF_ID, 77)]);
        assert_eq!(
            native_autoformat_cell_ordinal_from_parts(Some(0x0002), false, Some(&base), 77, 4,),
            None,
            "non-rectangle carriers stay unsupported"
        );
        assert_eq!(
            native_autoformat_cell_ordinal_from_parts(
                Some(TABLE_AUTOFORMAT_RECTANGLE_SHAPE_TYPE),
                true,
                Some(&base),
                77,
                4,
            ),
            None,
            "carriers with an ordinary ClientData identity stay unsupported"
        );

        for anchor in [
            publisher_fields(&[(TABLE_AUTOFORMAT_OWNER_REF_ID, 78)]),
            publisher_fields(&[
                (TABLE_AUTOFORMAT_OWNER_REF_ID, 77),
                (TABLE_AUTOFORMAT_CELL_ORDINAL_ID, 4),
            ]),
            publisher_fields(&[
                (TABLE_AUTOFORMAT_OWNER_REF_ID, 77),
                (TABLE_AUTOFORMAT_OWNER_REF_ID, 77),
            ]),
            publisher_fields(&[
                (TABLE_AUTOFORMAT_OWNER_REF_ID, 77),
                (TABLE_AUTOFORMAT_CELL_ORDINAL_ID, 1),
                (0x1234, 9),
            ]),
        ] {
            assert_eq!(
                native_autoformat_cell_ordinal_from_parts(
                    Some(TABLE_AUTOFORMAT_RECTANGLE_SHAPE_TYPE),
                    false,
                    Some(&anchor),
                    77,
                    4,
                ),
                None
            );
        }
    }

    #[test]
    fn table_autoformat_border_color_accepts_bounded_scheme_color() {
        let source = test_span();
        let scheme = MatureColorScheme {
            source: source.clone(),
            declared_count: 2,
            declared_count_source: source.clone(),
            slots: vec![
                pub_contents::MatureColorSchemeSlot {
                    ordinal: 0,
                    rgb: Some([0, 0, 0]),
                    source: source.clone(),
                    rgb_source: None,
                },
                pub_contents::MatureColorSchemeSlot {
                    ordinal: 1,
                    rgb: Some([12, 34, 56]),
                    source: source.clone(),
                    rgb_source: Some(source.clone()),
                },
            ],
            name: None,
            name_source: None,
        };

        assert_eq!(
            bounded_officeart_rgb(0x0800_0001, Some(&scheme)),
            Some([12, 34, 56])
        );
        assert_eq!(bounded_officeart_rgb(0x0800_0001, None), None);
        assert_eq!(bounded_officeart_rgb(0x0100_0001, Some(&scheme)), None);
    }

    #[test]
    fn merged_table_grid_requires_exact_non_overlapping_spanning_coverage() {
        let bounds = Some(RectEmu::new(
            LengthEmu::new(0),
            LengthEmu::new(0),
            LengthEmu::new(100),
            LengthEmu::new(100),
        ));
        let cell = |byte, stored_record_index, start_row, end_row, start_column, end_column| {
            PubTableCellSource {
                id: table_cell_id(byte),
                stored_record_index,
                coordinates: Some(PubTableCellCoordinates {
                    start_row,
                    end_row,
                    start_column,
                    end_column,
                }),
                utf16_start: 0,
                utf16_end: 0,
                bounds,
                paint: None,
                source_refs: Vec::new(),
            }
        };

        let complete = vec![
            cell(1, 0, 0, 0, 0, 1),
            cell(2, 1, 1, 1, 0, 0),
            cell(3, 2, 1, 1, 1, 1),
        ];
        assert!(complete_merged_table_grid(2, 2, &complete));

        let mut missing = complete.clone();
        missing.pop();
        assert!(!complete_merged_table_grid(2, 2, &missing));

        let overlapping = vec![
            cell(1, 0, 0, 0, 0, 1),
            cell(2, 1, 0, 1, 0, 0),
            cell(3, 2, 1, 1, 1, 1),
        ];
        assert!(!complete_merged_table_grid(2, 2, &overlapping));
    }

    #[test]
    fn simple_view_preserves_stored_ids_but_uses_visual_coordinates() {
        let cells = vec![
            PubTableCellSource {
                id: table_cell_id(2),
                stored_record_index: 2,
                coordinates: Some(PubTableCellCoordinates {
                    start_row: 1,
                    end_row: 1,
                    start_column: 0,
                    end_column: 0,
                }),
                utf16_start: 0,
                utf16_end: 1,
                bounds: None,
                paint: None,
                source_refs: Vec::new(),
            },
            PubTableCellSource {
                id: table_cell_id(0),
                stored_record_index: 0,
                coordinates: Some(PubTableCellCoordinates {
                    start_row: 0,
                    end_row: 0,
                    start_column: 0,
                    end_column: 0,
                }),
                utf16_start: 1,
                utf16_end: 2,
                bounds: None,
                paint: None,
                source_refs: Vec::new(),
            },
            PubTableCellSource {
                id: table_cell_id(3),
                stored_record_index: 3,
                coordinates: Some(PubTableCellCoordinates {
                    start_row: 1,
                    end_row: 1,
                    start_column: 1,
                    end_column: 1,
                }),
                utf16_start: 2,
                utf16_end: 3,
                bounds: None,
                paint: None,
                source_refs: Vec::new(),
            },
            PubTableCellSource {
                id: table_cell_id(1),
                stored_record_index: 1,
                coordinates: Some(PubTableCellCoordinates {
                    start_row: 0,
                    end_row: 0,
                    start_column: 1,
                    end_column: 1,
                }),
                utf16_start: 3,
                utf16_end: 4,
                bounds: None,
                paint: None,
                source_refs: Vec::new(),
            },
        ];

        let simple = build_simple_table(2, 2, &cells).expect("complete unmerged grid");

        assert_eq!(
            simple.cells.iter().map(|cell| cell.id).collect::<Vec<_>>(),
            vec![
                table_cell_id(2),
                table_cell_id(0),
                table_cell_id(3),
                table_cell_id(1),
            ],
            "promotion must not reorder stored record identity"
        );
        assert!(!simple.cells_are_row_major());
    }

    #[test]
    fn materialized_cell_preserves_exact_source_bounds() {
        let story_id = StoryId::from_canonical(CanonicalId::from_bytes([9; 16]));
        let cell_id = table_cell_id(4);
        let bounds = RectEmu::new(
            LengthEmu::new(100),
            LengthEmu::new(200),
            LengthEmu::new(300),
            LengthEmu::new(400),
        );
        let source_cell = PubTableCellSource {
            id: cell_id,
            stored_record_index: 0,
            coordinates: Some(PubTableCellCoordinates {
                start_row: 0,
                end_row: 0,
                start_column: 0,
                end_column: 0,
            }),
            utf16_start: 0,
            utf16_end: 1,
            bounds: Some(bounds),
            paint: None,
            source_refs: Vec::new(),
        };
        let simple_table = SimpleRectangularTable::new(
            1,
            1,
            vec![SimpleTableCell {
                id: cell_id,
                address: TableCellAddress { row: 0, column: 0 },
            }],
        )
        .expect("one-cell table");
        let table = PubTableSource {
            text_id: 1,
            story_id: Some(story_id),
            rows: 1,
            columns: 1,
            cells_seq_num: None,
            tcd_story_ordinal: None,
            cells: vec![source_cell],
            simple_table: Some(simple_table),
            layout_relation: None,
            uniform_cell_text_inset: None,
            layout_metrics: None,
            border_segments: Vec::new(),
            source_refs: Vec::new(),
        };
        let story = Story {
            id: story_id,
            text: "A".into(),
            paragraphs: Vec::new(),
            runs: Vec::new(),
            fields: Vec::new(),
            hyperlinks: Vec::new(),
            source_refs: Vec::new(),
        };

        let cells =
            materialize_bounded_simple_table_cells(&table, &story).expect("bounded table cell");
        assert_eq!(cells.len(), 1);
        assert_eq!(cells[0].id, cell_id);
        assert_eq!(cells[0].text, "A");
        assert_eq!(cells[0].bounds, Some(bounds));
    }

    #[test]
    fn spanning_cell_materializes_without_flattening() {
        let story_id = StoryId::from_canonical(CanonicalId::from_bytes([8; 16]));
        let cell_id = table_cell_id(5);
        let bounds = RectEmu::new(
            LengthEmu::new(10),
            LengthEmu::new(20),
            LengthEmu::new(600),
            LengthEmu::new(100),
        );
        let table = PubTableSource {
            text_id: 2,
            story_id: Some(story_id),
            rows: 1,
            columns: 2,
            cells_seq_num: None,
            tcd_story_ordinal: None,
            cells: vec![PubTableCellSource {
                id: cell_id,
                stored_record_index: 0,
                coordinates: Some(PubTableCellCoordinates {
                    start_row: 0,
                    end_row: 0,
                    start_column: 0,
                    end_column: 1,
                }),
                utf16_start: 0,
                utf16_end: 6,
                bounds: Some(bounds),
                paint: None,
                source_refs: Vec::new(),
            }],
            simple_table: None,
            layout_relation: None,
            uniform_cell_text_inset: None,
            layout_metrics: None,
            border_segments: Vec::new(),
            source_refs: Vec::new(),
        };
        let story = Story {
            id: story_id,
            text: "Header".into(),
            paragraphs: Vec::new(),
            runs: Vec::new(),
            fields: Vec::new(),
            hyperlinks: Vec::new(),
            source_refs: Vec::new(),
        };

        let cells = materialize_bounded_table_cells(&table, &story)
            .expect("one cell spanning two columns must remain one semantic cell");

        assert_eq!(cells.len(), 1);
        assert_eq!(cells[0].id, cell_id);
        assert_eq!(cells[0].address, TableCellAddress { row: 0, column: 0 });
        assert_eq!(cells[0].row_span, 1);
        assert_eq!(cells[0].column_span, 2);
        assert_eq!(cells[0].text, "Header");
        assert_eq!(cells[0].bounds, Some(bounds));
    }

    #[test]
    fn spanning_cell_is_not_flattened_to_simple_subset() {
        let cells = vec![PubTableCellSource {
            id: table_cell_id(0),
            stored_record_index: 0,
            coordinates: Some(PubTableCellCoordinates {
                start_row: 0,
                end_row: 0,
                start_column: 0,
                end_column: 1,
            }),
            utf16_start: 0,
            utf16_end: 1,
            bounds: None,
            paint: None,
            source_refs: Vec::new(),
        }];

        assert!(build_simple_table(1, 2, &cells).is_none());
    }
}
