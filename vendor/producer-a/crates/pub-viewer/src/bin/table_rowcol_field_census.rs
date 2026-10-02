use pub_contents::{
    ContentsCursor, RawContentsBlockBody, parse_0x2c_header, parse_confirmed_0x2c_chunk,
    parse_confirmed_0x2c_trailer_root, parse_confirmed_block, parse_confirmed_chunk_reference,
};
use pub_core::StreamPath;
use pub_viewer::{open_pub_bundle, viewer_geometry_environment_v0_1};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, env, fs, path::PathBuf};

const EXPECTED_SHA256: &str =
    "88f57d800aeec808798ea487b9d4ab85dc85c02cd190b987a332709b81018506";
const CONTENTS_STREAM: &str = "/Contents";
const RAW_TYPE_TABLE: u16 = 0x10;
const NUM_ROWS: u16 = 0x66;
const NUM_COLUMNS: u16 = 0x67;
const TABLE_WIDTH: u16 = 0x68;
const TABLE_HEIGHT: u16 = 0x69;
const ROWCOL_ARRAY: u16 = 0x6D;
const TRACK_END: u16 = 0x01;
const TRACK_SIZE: u16 = 0x02;

#[derive(Debug, Clone)]
struct TableContext {
    viewer_page: u32,
    rows: u32,
    columns: u32,
    owner_width: i64,
    owner_height: i64,
}

#[derive(Debug, Serialize)]
struct FieldValue {
    id: u16,
    wire_type: u8,
    value_u32: Option<u32>,
}

#[derive(Debug, Serialize)]
struct TrackItem {
    index: usize,
    axis: &'static str,
    axis_index: usize,
    fields: Vec<FieldValue>,
    cumulative_end: Option<u32>,
    logical_size: Option<u32>,
    physical_increment: Option<u32>,
    increment_minus_size: Option<i64>,
}

#[derive(Debug, Serialize)]
struct TableReceipt {
    viewer_page: u32,
    table_seq_num: u32,
    rows: u32,
    columns: u32,
    owner_width_emu: i64,
    owner_height_emu: i64,
    declared_width_emu: Option<u32>,
    declared_height_emu: Option<u32>,
    column_size_sum_emu: Option<u64>,
    row_size_sum_emu: Option<u64>,
    column_final_end_emu: Option<u32>,
    row_final_end_emu: Option<u32>,
    column_final_end_matches_owner: Option<bool>,
    row_final_end_matches_owner: Option<bool>,
    column_final_end_matches_declared: Option<bool>,
    row_final_end_matches_declared: Option<bool>,
    row_size_sum_matches_declared: Option<bool>,
    row_size_sum_matches_owner: Option<bool>,
    row_final_end_minus_size_sum_emu: Option<i64>,
    owner_minus_row_size_sum_emu: Option<i64>,
    owner_minus_row_final_end_emu: Option<i64>,
    all_tracks_have_unique_end_and_size: bool,
    tracks: Vec<TrackItem>,
}

#[derive(Debug, Default, Serialize)]
struct PageSummary {
    viewer_page: u32,
    table_count: usize,
    unique_end_size_tables: usize,
    row_end_matches_owner_tables: usize,
    row_end_matches_declared_tables: usize,
    row_size_matches_owner_tables: usize,
    row_size_matches_declared_tables: usize,
}

#[derive(Debug, Default, Serialize)]
struct Totals {
    table_count: usize,
    unique_end_size_tables: usize,
    row_end_matches_owner_tables: usize,
    row_end_matches_declared_tables: usize,
    row_size_matches_owner_tables: usize,
    row_size_matches_declared_tables: usize,
}

#[derive(Debug, Serialize)]
struct Receipt {
    schema: &'static str,
    source_sha256: String,
    pages: Vec<PageSummary>,
    totals: Totals,
    tables: Vec<TableReceipt>,
    claims: Claims,
}

#[derive(Debug, Serialize)]
struct Claims {
    pdf_geometry_used: bool,
    proximity_geometry_used: bool,
    modern_rowcol_field_01_semantics_predeclared: bool,
    comparison: &'static str,
}

fn unique_u32_field(fields: &[pub_contents::RawContentsBlock], id: u16) -> Option<u32> {
    let mut values = fields.iter().filter_map(|field| {
        if field.id != id {
            return None;
        }
        match field.body {
            RawContentsBlockBody::U32 { value, .. } => Some(value),
            _ => None,
        }
    });
    let first = values.next()?;
    values.next().is_none().then_some(first)
}

fn unique_reference_u16(
    values: &[pub_contents::ObservedU16Field],
) -> Option<u16> {
    let [only] = values else {
        return None;
    };
    Some(only.value)
}

fn unique_reference_u32(
    values: &[pub_contents::ObservedU32Field],
) -> Option<u32> {
    let [only] = values else {
        return None;
    };
    Some(only.value)
}

fn sum(values: &[Option<u32>]) -> Option<u64> {
    values.iter().try_fold(0_u64, |total, value| {
        total.checked_add(u64::from((*value)?))
    })
}

fn parse_track_items(
    contents: &[u8],
    table_fields: &[pub_contents::RawContentsBlock],
    columns: usize,
) -> anyhow::Result<Vec<TrackItem>> {
    let arrays = table_fields
        .iter()
        .filter(|field| field.id == ROWCOL_ARRAY)
        .collect::<Vec<_>>();
    anyhow::ensure!(arrays.len() == 1, "TABLE rowcol array count != 1");
    let RawContentsBlockBody::Container { content_source, .. } = &arrays[0].body else {
        anyhow::bail!("TABLE rowcol array is not a container");
    };

    let start = usize::try_from(content_source.offset)?;
    let len = usize::try_from(content_source.len)?;
    let mut cursor = ContentsCursor::bounded(
        content_source.stream.clone(),
        contents,
        start,
        len,
    )?;

    let mut raw_items = Vec::<Vec<pub_contents::RawContentsBlock>>::new();
    while cursor.remaining() > 0 {
        let item = parse_confirmed_block(&mut cursor)?;
        anyhow::ensure!(item.id == 0, "TABLE rowcol item id != 0");
        let RawContentsBlockBody::Container {
            content_source: item_source,
            ..
        } = item.body
        else {
            anyhow::bail!("TABLE rowcol item is not a container");
        };

        let item_start = usize::try_from(item_source.offset)?;
        let item_len = usize::try_from(item_source.len)?;
        let mut item_cursor = ContentsCursor::bounded(
            item_source.stream.clone(),
            contents,
            item_start,
            item_len,
        )?;
        let mut fields = Vec::new();
        while item_cursor.remaining() > 0 {
            fields.push(parse_confirmed_block(&mut item_cursor)?);
        }
        raw_items.push(fields);
    }

    let mut out = Vec::with_capacity(raw_items.len());
    let mut previous_column_end = 0_u32;
    let mut previous_row_end = 0_u32;

    for (index, fields) in raw_items.into_iter().enumerate() {
        let axis = if index < columns { "column" } else { "row" };
        let axis_index = if index < columns { index } else { index - columns };
        let cumulative_end = unique_u32_field(&fields, TRACK_END);
        let logical_size = unique_u32_field(&fields, TRACK_SIZE);
        let previous = if axis == "column" {
            previous_column_end
        } else {
            previous_row_end
        };
        let physical_increment = cumulative_end.and_then(|end| end.checked_sub(previous));
        if let Some(end) = cumulative_end {
            if axis == "column" {
                previous_column_end = end;
            } else {
                previous_row_end = end;
            }
        }
        let increment_minus_size = physical_increment
            .zip(logical_size)
            .map(|(increment, size)| i64::from(increment) - i64::from(size));

        let rendered_fields = fields
            .iter()
            .map(|field| FieldValue {
                id: field.id,
                wire_type: field.block_type,
                value_u32: match field.body {
                    RawContentsBlockBody::U32 { value, .. } => Some(value),
                    _ => None,
                },
            })
            .collect();

        out.push(TrackItem {
            index,
            axis,
            axis_index,
            fields: rendered_fields,
            cumulative_end,
            logical_size,
            physical_increment,
            increment_minus_size,
        });
    }

    Ok(out)
}

fn main() -> anyhow::Result<()> {
    let fixture = PathBuf::from(env::args_os().nth(1).ok_or_else(|| anyhow::anyhow!("fixture path"))?);
    let output = PathBuf::from(env::args_os().nth(2).ok_or_else(|| anyhow::anyhow!("output path"))?);

    let bytes = fs::read(&fixture)?;
    let sha = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    anyhow::ensure!(sha == EXPECTED_SHA256, "exact Virginia SHA mismatch");

    let bundle = open_pub_bundle(&bytes, viewer_geometry_environment_v0_1())?;
    let mut selected = BTreeMap::<u32, TableContext>::new();
    for viewer_page in [6_u32, 7, 9, 10, 11, 21, 22, 23] {
        let page = bundle
            .geometry
            .document
            .pages
            .get((viewer_page - 1) as usize)
            .ok_or_else(|| anyhow::anyhow!("selected page missing"))?;
        let parent = page.id.into_canonical();
        for node in bundle
            .resolved_graph
            .nodes
            .values()
            .filter(|node| node.header.parent_id == parent && node.payload.table.is_some())
        {
            let table = node.payload.table.as_ref().unwrap();
            selected.insert(
                node.payload.contents_seq_num,
                TableContext {
                    viewer_page,
                    rows: table.rows,
                    columns: table.columns,
                    owner_width: node.header.bounds.width.get(),
                    owner_height: node.header.bounds.height.get(),
                },
            );
        }
    }
    anyhow::ensure!(selected.len() == 22, "selected TABLE count != 22");

    let contents = pub_cfb::read_stream_path(&fixture, CONTENTS_STREAM)?;
    let stream = StreamPath(CONTENTS_STREAM.into());
    let header = parse_0x2c_header(stream.clone(), &contents)?;
    let trailer = parse_confirmed_0x2c_trailer_root(&contents, &header)?;

    let mut tables = Vec::new();
    for seq_num in 0..trailer.directory.slots.len() {
        let Some(reference) = parse_confirmed_chunk_reference(
            &contents,
            &trailer.directory,
            seq_num,
        )? else {
            continue;
        };
        if unique_reference_u16(&reference.raw_types) != Some(RAW_TYPE_TABLE) {
            continue;
        }
        let seq_u32 = u32::try_from(seq_num)?;
        let Some(context) = selected.get(&seq_u32) else {
            continue;
        };
        let offset = unique_reference_u32(&reference.chunk_offsets)
            .ok_or_else(|| anyhow::anyhow!("TABLE chunk offset ambiguous"))?;
        let chunk = parse_confirmed_0x2c_chunk(stream.clone(), &contents, offset)?;

        let rows = unique_u32_field(&chunk.fields, NUM_ROWS)
            .ok_or_else(|| anyhow::anyhow!("TABLE rows missing"))?;
        let columns = unique_u32_field(&chunk.fields, NUM_COLUMNS)
            .ok_or_else(|| anyhow::anyhow!("TABLE columns missing"))?;
        anyhow::ensure!(rows == context.rows && columns == context.columns);

        let declared_width = unique_u32_field(&chunk.fields, TABLE_WIDTH);
        let declared_height = unique_u32_field(&chunk.fields, TABLE_HEIGHT);
        let tracks = parse_track_items(&contents, &chunk.fields, usize::try_from(columns)?)?;
        anyhow::ensure!(
            tracks.len() == usize::try_from(rows + columns)?,
            "track count mismatch"
        );

        let column_tracks = &tracks[..usize::try_from(columns)?];
        let row_tracks = &tracks[usize::try_from(columns)?..];
        let column_sizes = column_tracks.iter().map(|track| track.logical_size).collect::<Vec<_>>();
        let row_sizes = row_tracks.iter().map(|track| track.logical_size).collect::<Vec<_>>();
        let column_size_sum = sum(&column_sizes);
        let row_size_sum = sum(&row_sizes);
        let column_final_end = column_tracks.last().and_then(|track| track.cumulative_end);
        let row_final_end = row_tracks.last().and_then(|track| track.cumulative_end);
        let all_tracks_have_unique_end_and_size = tracks
            .iter()
            .all(|track| track.cumulative_end.is_some() && track.logical_size.is_some());

        let owner_width_u32 = u32::try_from(context.owner_width).ok();
        let owner_height_u32 = u32::try_from(context.owner_height).ok();

        tables.push(TableReceipt {
            viewer_page: context.viewer_page,
            table_seq_num: seq_u32,
            rows,
            columns,
            owner_width_emu: context.owner_width,
            owner_height_emu: context.owner_height,
            declared_width_emu: declared_width,
            declared_height_emu: declared_height,
            column_size_sum_emu: column_size_sum,
            row_size_sum_emu: row_size_sum,
            column_final_end_emu: column_final_end,
            row_final_end_emu: row_final_end,
            column_final_end_matches_owner: column_final_end
                .zip(owner_width_u32)
                .map(|(end, owner)| end == owner),
            row_final_end_matches_owner: row_final_end
                .zip(owner_height_u32)
                .map(|(end, owner)| end == owner),
            column_final_end_matches_declared: column_final_end
                .zip(declared_width)
                .map(|(end, declared)| end == declared),
            row_final_end_matches_declared: row_final_end
                .zip(declared_height)
                .map(|(end, declared)| end == declared),
            row_size_sum_matches_declared: row_size_sum
                .zip(declared_height.map(u64::from))
                .map(|(sum, declared)| sum == declared),
            row_size_sum_matches_owner: row_size_sum
                .zip(u64::try_from(context.owner_height).ok())
                .map(|(sum, owner)| sum == owner),
            row_final_end_minus_size_sum_emu: row_final_end
                .zip(row_size_sum)
                .and_then(|(end, sum)| i64::try_from(i128::from(end) - i128::from(sum)).ok()),
            owner_minus_row_size_sum_emu: row_size_sum
                .and_then(|sum| i64::try_from(i128::from(context.owner_height) - i128::from(sum)).ok()),
            owner_minus_row_final_end_emu: row_final_end
                .and_then(|end| i64::try_from(i128::from(context.owner_height) - i128::from(end)).ok()),
            all_tracks_have_unique_end_and_size,
            tracks,
        });
    }

    tables.sort_by_key(|table| (table.viewer_page, table.table_seq_num));
    anyhow::ensure!(tables.len() == 22, "raw selected TABLE count != 22");

    let mut pages = Vec::new();
    let mut totals = Totals::default();
    for viewer_page in [6_u32, 7, 9, 10, 11, 21, 22, 23] {
        let page_tables = tables.iter().filter(|table| table.viewer_page == viewer_page).collect::<Vec<_>>();
        let summary = PageSummary {
            viewer_page,
            table_count: page_tables.len(),
            unique_end_size_tables: page_tables
                .iter()
                .filter(|table| table.all_tracks_have_unique_end_and_size)
                .count(),
            row_end_matches_owner_tables: page_tables
                .iter()
                .filter(|table| table.row_final_end_matches_owner == Some(true))
                .count(),
            row_end_matches_declared_tables: page_tables
                .iter()
                .filter(|table| table.row_final_end_matches_declared == Some(true))
                .count(),
            row_size_matches_owner_tables: page_tables
                .iter()
                .filter(|table| table.row_size_sum_matches_owner == Some(true))
                .count(),
            row_size_matches_declared_tables: page_tables
                .iter()
                .filter(|table| table.row_size_sum_matches_declared == Some(true))
                .count(),
        };
        totals.table_count += summary.table_count;
        totals.unique_end_size_tables += summary.unique_end_size_tables;
        totals.row_end_matches_owner_tables += summary.row_end_matches_owner_tables;
        totals.row_end_matches_declared_tables += summary.row_end_matches_declared_tables;
        totals.row_size_matches_owner_tables += summary.row_size_matches_owner_tables;
        totals.row_size_matches_declared_tables += summary.row_size_matches_declared_tables;
        pages.push(summary);
    }

    let receipt = Receipt {
        schema: "chaptera.virginia-table-rowcol-field-census.v1",
        source_sha256: sha,
        pages,
        totals,
        tables,
        claims: Claims {
            pdf_geometry_used: false,
            proximity_geometry_used: false,
            modern_rowcol_field_01_semantics_predeclared: false,
            comparison: "raw mature TABLE 0x6D item fields vs 0x68/0x69 and exact OfficeArt owner bounds",
        },
    };

    fs::create_dir_all(output.parent().ok_or_else(|| anyhow::anyhow!("output parent"))?)?;
    fs::write(&output, serde_json::to_vec_pretty(&receipt)?)?;
    println!("{}", serde_json::to_string(&receipt.totals)?);
    Ok(())
}
