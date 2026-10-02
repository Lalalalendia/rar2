use pub_core::StreamPath;
use pub_escher::{
    PUBLISHER_FIELD_SHAPE_ID, PublisherFieldRecord, SpContainerObservation, inspect_sp_containers,
};
use pub_viewer::{open_pub_bundle, viewer_geometry_environment_v0_1};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    io::Cursor,
    path::PathBuf,
};

const EXPECTED_SHA256: &str =
    "88f57d800aeec808798ea487b9d4ab85dc85c02cd190b987a332709b81018506";
const TABLE_OWNER_REF: u16 = 0x6802;
const CELL_ORDINAL: u16 = 0x2003;
const SEGMENT_ORIENTATION: u16 = 0x2001;
const ROW_START: u16 = 0x2004;
const COLUMN_START: u16 = 0x2005;
const ROW_END: u16 = 0x2006;
const COLUMN_END: u16 = 0x2007;
const RECTANGLE: u16 = 0x0001;
const FILL_COLOR: u16 = 0x0181;

fn unique_field(record: &PublisherFieldRecord, id: u16) -> Option<u32> {
    let mut matches = record.fields.iter().filter(|field| field.id == id);
    let first = matches.next()?.value;
    matches.next().is_none().then_some(first)
}

fn unique_or_zero(record: &PublisherFieldRecord, id: u16) -> Option<u32> {
    let mut matches = record.fields.iter().filter(|field| field.id == id);
    let Some(first) = matches.next() else {
        return Some(0);
    };
    matches.next().is_none().then_some(first.value)
}

fn unique_fopt_scalar(shape: &SpContainerObservation, property_id: u16) -> Option<u32> {
    let mut matches = shape
        .fopts
        .iter()
        .flat_map(|record| record.properties.iter())
        .filter(|property| property.property_id() == property_id)
        .filter(|property| !property.f_bid() && !property.f_complex());
    let first = matches.next()?.op;
    matches.next().is_none().then_some(first)
}

fn grounded(shape: &SpContainerObservation, grounded: &BTreeSet<u32>) -> bool {
    shape
        .client_data
        .as_ref()
        .and_then(|record| unique_field(record, PUBLISHER_FIELD_SHAPE_ID))
        .is_some_and(|seq| grounded.contains(&seq))
}

fn owner_ref(shape: &SpContainerObservation) -> Option<u32> {
    unique_field(shape.client_anchor.as_ref()?, TABLE_OWNER_REF)
}

fn is_candidate(shape: &SpContainerObservation, table_seq: u32) -> bool {
    if shape.fsp.as_ref().map(|fsp| fsp.shape_type) != Some(RECTANGLE) {
        return false;
    }
    let Some(anchor) = shape.client_anchor.as_ref() else {
        return false;
    };
    if unique_field(anchor, TABLE_OWNER_REF) != Some(table_seq) {
        return false;
    }
    if anchor.fields.iter().any(|field| field.id == CELL_ORDINAL) {
        return false;
    }
    anchor.fields.iter().any(|field| {
        matches!(
            field.id,
            SEGMENT_ORIENTATION | ROW_START | COLUMN_START | ROW_END | COLUMN_END
        )
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
enum Axis {
    Horizontal,
    Vertical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct SegmentKey {
    axis: Axis,
    row_start: u32,
    column_start: u32,
    row_end: u32,
    column_end: u32,
}

#[derive(Debug, Serialize)]
struct SegmentReceipt {
    axis: Axis,
    row_start: u32,
    column_start: u32,
    row_end: u32,
    column_end: u32,
    unit_segment: bool,
    fill_color_class: &'static str,
}

#[derive(Debug, Default, Serialize)]
struct TableReceipt {
    rows: u32,
    columns: u32,
    candidate_count: usize,
    decoded_count: usize,
    rejected_count: usize,
    horizontal_count: usize,
    vertical_count: usize,
    unit_segment_count: usize,
    multi_cell_segment_count: usize,
    duplicate_semantic_segment_count: usize,
    rejected_reasons: BTreeMap<String, usize>,
    fill_color_classes: BTreeMap<String, usize>,
    segments: Vec<SegmentReceipt>,
}

#[derive(Debug, Default, Serialize)]
struct PageReceipt {
    viewer_page: u32,
    table_count: usize,
    candidate_count: usize,
    decoded_count: usize,
    rejected_count: usize,
    horizontal_count: usize,
    vertical_count: usize,
    unit_segment_count: usize,
    multi_cell_segment_count: usize,
    duplicate_semantic_segment_count: usize,
    tables: Vec<TableReceipt>,
}

#[derive(Debug, Serialize)]
struct Receipt {
    schema: &'static str,
    source_sha256: String,
    native_law: NativeLaw,
    pages: Vec<PageReceipt>,
    totals: Totals,
    claims: Claims,
}

#[derive(Debug, Serialize)]
struct NativeLaw {
    table_owner_field: &'static str,
    orientation_field: &'static str,
    horizontal_value: u32,
    vertical_value: u32,
    row_start_field: &'static str,
    row_end_field: &'static str,
    column_start_field: &'static str,
    column_end_field: &'static str,
    omitted_boundary_value: u32,
    color_property: &'static str,
}

#[derive(Debug, Default, Serialize)]
struct Totals {
    table_count: usize,
    candidate_count: usize,
    decoded_count: usize,
    rejected_count: usize,
    duplicate_semantic_segment_count: usize,
}

#[derive(Debug, Serialize)]
struct Claims {
    pdf_used_as_semantic_authority: bool,
    geometry_proximity_used: bool,
    page_or_hash_product_rule_used: bool,
    native_side_role_authority: &'static str,
}

fn bump(map: &mut BTreeMap<String, usize>, key: impl Into<String>) {
    *map.entry(key.into()).or_default() += 1;
}

fn color_class(shape: &SpContainerObservation) -> &'static str {
    match unique_fopt_scalar(shape, FILL_COLOR) {
        None => "missing_or_ambiguous",
        Some(value) if (value >> 24) as u8 == 0x08 => "scheme_ref",
        Some(value) if (value >> 24) == 0 => "direct_or_literal",
        Some(_) => "other_colorref",
    }
}

fn decode_segment(
    shape: &SpContainerObservation,
    rows: u32,
    columns: u32,
) -> Result<SegmentKey, &'static str> {
    let anchor = shape.client_anchor.as_ref().ok_or("anchor_absent")?;

    let allowed = [
        TABLE_OWNER_REF,
        SEGMENT_ORIENTATION,
        ROW_START,
        COLUMN_START,
        ROW_END,
        COLUMN_END,
    ];
    if anchor
        .fields
        .iter()
        .any(|field| !allowed.contains(&field.id))
    {
        return Err("unexpected_anchor_field");
    }

    let orientation = unique_field(anchor, SEGMENT_ORIENTATION)
        .ok_or("orientation_missing_or_ambiguous")?;
    let row_start = unique_or_zero(anchor, ROW_START).ok_or("row_start_ambiguous")?;
    let column_start =
        unique_or_zero(anchor, COLUMN_START).ok_or("column_start_ambiguous")?;
    let row_end = unique_or_zero(anchor, ROW_END).ok_or("row_end_ambiguous")?;
    let column_end = unique_or_zero(anchor, COLUMN_END).ok_or("column_end_ambiguous")?;

    if row_start > rows || row_end > rows || column_start > columns || column_end > columns {
        return Err("boundary_out_of_grid");
    }

    match orientation {
        1 if row_start == row_end && column_start < column_end => Ok(SegmentKey {
            axis: Axis::Horizontal,
            row_start,
            column_start,
            row_end,
            column_end,
        }),
        2 if column_start == column_end && row_start < row_end => Ok(SegmentKey {
            axis: Axis::Vertical,
            row_start,
            column_start,
            row_end,
            column_end,
        }),
        1 => Err("horizontal_shape_invalid"),
        2 => Err("vertical_shape_invalid"),
        _ => Err("orientation_unknown"),
    }
}

#[test]
#[ignore = "requires exact public Virginia Remplacante fixture"]
fn exact_virginia_table_border_segment_probe() {
    let fixture = PathBuf::from(
        env::var_os("CHAPTERA_VIRGINIA_TABLE_BORDER_FIXTURE")
            .expect("CHAPTERA_VIRGINIA_TABLE_BORDER_FIXTURE"),
    );
    let output = PathBuf::from(
        env::var_os("CHAPTERA_VIRGINIA_TABLE_BORDER_OUT")
            .expect("CHAPTERA_VIRGINIA_TABLE_BORDER_OUT"),
    );

    let bytes = fs::read(&fixture).expect("read exact Virginia PUB");
    let sha = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    assert_eq!(sha, EXPECTED_SHA256, "exact Virginia source identity");

    let bundle = open_pub_bundle(&bytes, viewer_geometry_environment_v0_1())
        .expect("open exact Virginia through Viewer");
    let escher = pub_cfb::read_stream_reader(
        Cursor::new(bytes.as_slice()),
        pub_reader::ESCHER_STREAM_PATH,
    )
    .expect("read Escher stream");
    let inventory = inspect_sp_containers(
        StreamPath(pub_reader::ESCHER_STREAM_PATH.into()),
        &escher,
    )
    .expect("inspect OfficeArt SpContainers");

    let grounded_ids = bundle
        .resolved_graph
        .nodes
        .values()
        .map(|node| node.payload.contents_seq_num)
        .collect::<BTreeSet<_>>();

    let mut pages = Vec::new();
    let mut totals = Totals::default();

    for viewer_page in [6_u32, 7, 9, 10, 11, 21, 22, 23] {
        let page = bundle
            .geometry
            .document
            .pages
            .get((viewer_page - 1) as usize)
            .expect("selected page exists");
        let parent = page.id.into_canonical();
        let mut page_receipt = PageReceipt {
            viewer_page,
            ..PageReceipt::default()
        };

        for node in bundle
            .resolved_graph
            .nodes
            .values()
            .filter(|node| node.header.parent_id == parent && node.payload.table.is_some())
        {
            let table = node.payload.table.as_ref().expect("filtered table");
            let table_seq = node.payload.contents_seq_num;
            let mut table_receipt = TableReceipt {
                rows: table.rows,
                columns: table.columns,
                ..TableReceipt::default()
            };
            let mut semantic_counts = BTreeMap::<SegmentKey, usize>::new();

            for shape in inventory
                .shapes
                .iter()
                .filter(|shape| owner_ref(shape) == Some(table_seq))
                .filter(|shape| !grounded(shape, &grounded_ids))
                .filter(|shape| is_candidate(shape, table_seq))
            {
                table_receipt.candidate_count += 1;
                let class = color_class(shape);
                bump(&mut table_receipt.fill_color_classes, class);

                match decode_segment(shape, table.rows, table.columns) {
                    Ok(segment) => {
                        table_receipt.decoded_count += 1;
                        match segment.axis {
                            Axis::Horizontal => table_receipt.horizontal_count += 1,
                            Axis::Vertical => table_receipt.vertical_count += 1,
                        }
                        let span = match segment.axis {
                            Axis::Horizontal => segment.column_end - segment.column_start,
                            Axis::Vertical => segment.row_end - segment.row_start,
                        };
                        let unit = span == 1;
                        if unit {
                            table_receipt.unit_segment_count += 1;
                        } else {
                            table_receipt.multi_cell_segment_count += 1;
                        }
                        *semantic_counts.entry(segment).or_default() += 1;
                        table_receipt.segments.push(SegmentReceipt {
                            axis: segment.axis,
                            row_start: segment.row_start,
                            column_start: segment.column_start,
                            row_end: segment.row_end,
                            column_end: segment.column_end,
                            unit_segment: unit,
                            fill_color_class: class,
                        });
                    }
                    Err(reason) => {
                        table_receipt.rejected_count += 1;
                        bump(&mut table_receipt.rejected_reasons, reason);
                    }
                }
            }

            table_receipt.duplicate_semantic_segment_count = semantic_counts
                .values()
                .map(|count| count.saturating_sub(1))
                .sum();

            page_receipt.table_count += 1;
            page_receipt.candidate_count += table_receipt.candidate_count;
            page_receipt.decoded_count += table_receipt.decoded_count;
            page_receipt.rejected_count += table_receipt.rejected_count;
            page_receipt.horizontal_count += table_receipt.horizontal_count;
            page_receipt.vertical_count += table_receipt.vertical_count;
            page_receipt.unit_segment_count += table_receipt.unit_segment_count;
            page_receipt.multi_cell_segment_count += table_receipt.multi_cell_segment_count;
            page_receipt.duplicate_semantic_segment_count +=
                table_receipt.duplicate_semantic_segment_count;
            page_receipt.tables.push(table_receipt);
        }

        totals.table_count += page_receipt.table_count;
        totals.candidate_count += page_receipt.candidate_count;
        totals.decoded_count += page_receipt.decoded_count;
        totals.rejected_count += page_receipt.rejected_count;
        totals.duplicate_semantic_segment_count += page_receipt.duplicate_semantic_segment_count;
        pages.push(page_receipt);
    }

    let receipt = Receipt {
        schema: "chaptera.virginia-table-border-segment-probe.v1",
        source_sha256: sha,
        native_law: NativeLaw {
            table_owner_field: "0x6802",
            orientation_field: "0x2001",
            horizontal_value: 1,
            vertical_value: 2,
            row_start_field: "0x2004",
            row_end_field: "0x2006",
            column_start_field: "0x2005",
            column_end_field: "0x2007",
            omitted_boundary_value: 0,
            color_property: "fillColor/0x0181",
        },
        pages,
        totals,
        claims: Claims {
            pdf_used_as_semantic_authority: false,
            geometry_proximity_used: false,
            page_or_hash_product_rule_used: false,
            native_side_role_authority:
                "#740 standalone-v3 Publisher 16.0/build12527 run 20261002-225541",
        },
    };

    fs::create_dir_all(output.parent().expect("receipt parent")).expect("create receipt dir");
    fs::write(
        &output,
        serde_json::to_vec_pretty(&receipt).expect("serialize receipt"),
    )
    .expect("write receipt");

    assert_eq!(
        receipt.totals.rejected_count, 0,
        "native-proven TABLE border segment grammar must decode all selected candidates"
    );
    assert_eq!(
        receipt.totals.duplicate_semantic_segment_count, 0,
        "decoded border segments must be unique within each table"
    );
}
