use pub_core::{RawSpan, StreamPath};
use pub_escher::{
    PUBLISHER_FIELD_SHAPE_ID, PublisherFieldRecord, SpContainerObservation, inspect_sp_containers,
};
use pub_model::SourceRef;
use pub_viewer::{open_pub_bundle, viewer_geometry_environment_v0_1};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{env, fs, io::Cursor, path::PathBuf};

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
const LINE_WIDTH: u16 = 0x01CB;

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

fn has_client_data_identity(shape: &SpContainerObservation) -> bool {
    shape.client_data.as_ref().is_some_and(|record| {
        record
            .fields
            .iter()
            .any(|field| field.id == PUBLISHER_FIELD_SHAPE_ID)
    })
}

fn owner_ref(shape: &SpContainerObservation) -> Option<u32> {
    unique_field(shape.client_anchor.as_ref()?, TABLE_OWNER_REF)
}

fn admitted_border_carrier(
    shape: &SpContainerObservation,
    table_seq: u32,
    rows: u32,
    columns: u32,
) -> bool {
    if shape.fsp.as_ref().map(|fsp| fsp.shape_type) != Some(RECTANGLE)
        || has_client_data_identity(shape)
    {
        return false;
    }
    let Some(anchor) = shape.client_anchor.as_ref() else {
        return false;
    };
    if unique_field(anchor, TABLE_OWNER_REF) != Some(table_seq)
        || anchor.fields.iter().any(|field| field.id == CELL_ORDINAL)
    {
        return false;
    }

    let allowed = [
        TABLE_OWNER_REF,
        SEGMENT_ORIENTATION,
        ROW_START,
        COLUMN_START,
        ROW_END,
        COLUMN_END,
    ];
    if anchor.fields.iter().any(|field| !allowed.contains(&field.id)) {
        return false;
    }

    let Some(orientation) = unique_field(anchor, SEGMENT_ORIENTATION) else {
        return false;
    };
    let Some(row_start) = unique_or_zero(anchor, ROW_START) else {
        return false;
    };
    let Some(column_start) = unique_or_zero(anchor, COLUMN_START) else {
        return false;
    };
    let Some(row_end) = unique_or_zero(anchor, ROW_END) else {
        return false;
    };
    let Some(column_end) = unique_or_zero(anchor, COLUMN_END) else {
        return false;
    };

    if row_start > rows || row_end > rows || column_start > columns || column_end > columns {
        return false;
    }
    let valid_segment = match orientation {
        1 => row_start == row_end && column_start < column_end,
        2 => column_start == column_end && row_start < row_end,
        _ => false,
    };
    if !valid_segment {
        return false;
    }

    let direct_color = unique_fopt_scalar(shape, FILL_COLOR)
        .is_some_and(|value| (value >> 24) == 0);
    let explicit_width = unique_fopt_scalar(shape, LINE_WIDTH)
        .is_some_and(|value| value > 0 && value <= 0x0132_F540);
    direct_color && explicit_width
}

fn source_ref_matches_span(reference: &SourceRef, span: &RawSpan) -> bool {
    reference.carrier.as_str() == span.stream.0.as_str()
        && reference.byte_range.is_some_and(|range| {
            range.offset == span.offset && range.length == span.len
        })
}

#[derive(Debug, Serialize)]
struct CarrierReceipt {
    viewer_page: u32,
    table_seq_num: u32,
    shape_offset: u64,
    shape_len: u64,
    generic_node_count: usize,
    generic_ref_count: usize,
    table_source_ref_count: usize,
    table_cell_source_ref_count: usize,
    table_cell_paint_ref_count: usize,
}

#[derive(Debug, Default, Serialize)]
struct PageReceipt {
    viewer_page: u32,
    candidate_count: usize,
    any_overlap_count: usize,
    generic_node_overlap_count: usize,
    table_source_overlap_count: usize,
    table_cell_source_overlap_count: usize,
    table_cell_paint_overlap_count: usize,
}

#[derive(Debug, Default, Serialize)]
struct Totals {
    candidate_count: usize,
    any_overlap_count: usize,
    generic_node_overlap_count: usize,
    table_source_overlap_count: usize,
    table_cell_source_overlap_count: usize,
    table_cell_paint_overlap_count: usize,
}

#[derive(Debug, Serialize)]
struct Receipt {
    schema: &'static str,
    source_sha256: String,
    pages: Vec<PageReceipt>,
    totals: Totals,
    carriers: Vec<CarrierReceipt>,
    claims: Claims,
}

#[derive(Debug, Serialize)]
struct Claims {
    geometry_proximity_used: bool,
    pdf_used_as_semantic_authority: bool,
    page_or_hash_product_rule_used: bool,
    overlap_identity: &'static str,
}

#[test]
#[ignore = "requires exact public Virginia Remplacante fixture"]
fn exact_virginia_table_border_composition_overlap_probe() {
    let fixture = PathBuf::from(
        env::var_os("CHAPTERA_VIRGINIA_TABLE_BORDER_FIXTURE")
            .expect("CHAPTERA_VIRGINIA_TABLE_BORDER_FIXTURE"),
    );
    let output = PathBuf::from(
        env::var_os("CHAPTERA_VIRGINIA_TABLE_BORDER_COMPOSITION_OUT")
            .expect("CHAPTERA_VIRGINIA_TABLE_BORDER_COMPOSITION_OUT"),
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

    let mut pages = Vec::new();
    let mut carriers = Vec::new();
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

        for table_node in bundle
            .resolved_graph
            .nodes
            .values()
            .filter(|node| node.header.parent_id == parent && node.payload.table.is_some())
        {
            let table = table_node.payload.table.as_ref().expect("filtered table");
            let table_seq = table_node.payload.contents_seq_num;

            for shape in inventory
                .shapes
                .iter()
                .filter(|shape| owner_ref(shape) == Some(table_seq))
                .filter(|shape| admitted_border_carrier(shape, table_seq, table.rows, table.columns))
            {
                let mut generic_node_count = 0_usize;
                let mut generic_ref_count = 0_usize;
                let mut table_source_ref_count = 0_usize;
                let mut table_cell_source_ref_count = 0_usize;
                let mut table_cell_paint_ref_count = 0_usize;

                for node in bundle.resolved_graph.nodes.values() {
                    let matching_refs = node
                        .header
                        .source_refs
                        .iter()
                        .filter(|reference| source_ref_matches_span(reference, &shape.source))
                        .count();
                    if matching_refs > 0 {
                        generic_node_count += 1;
                        generic_ref_count += matching_refs;
                    }

                    if let Some(source_table) = node.payload.table.as_ref() {
                        table_source_ref_count += source_table
                            .source_refs
                            .iter()
                            .filter(|reference| source_ref_matches_span(reference, &shape.source))
                            .count();
                        for cell in &source_table.cells {
                            table_cell_source_ref_count += cell
                                .source_refs
                                .iter()
                                .filter(|reference| source_ref_matches_span(reference, &shape.source))
                                .count();
                            if let Some(paint) = cell.paint.as_ref() {
                                table_cell_paint_ref_count += paint
                                    .source_refs
                                    .iter()
                                    .filter(|reference| {
                                        source_ref_matches_span(reference, &shape.source)
                                    })
                                    .count();
                            }
                        }
                    }
                }

                let any_overlap = generic_node_count > 0
                    || table_source_ref_count > 0
                    || table_cell_source_ref_count > 0
                    || table_cell_paint_ref_count > 0;

                page_receipt.candidate_count += 1;
                totals.candidate_count += 1;
                if any_overlap {
                    page_receipt.any_overlap_count += 1;
                    totals.any_overlap_count += 1;
                }
                if generic_node_count > 0 {
                    page_receipt.generic_node_overlap_count += 1;
                    totals.generic_node_overlap_count += 1;
                }
                if table_source_ref_count > 0 {
                    page_receipt.table_source_overlap_count += 1;
                    totals.table_source_overlap_count += 1;
                }
                if table_cell_source_ref_count > 0 {
                    page_receipt.table_cell_source_overlap_count += 1;
                    totals.table_cell_source_overlap_count += 1;
                }
                if table_cell_paint_ref_count > 0 {
                    page_receipt.table_cell_paint_overlap_count += 1;
                    totals.table_cell_paint_overlap_count += 1;
                }

                carriers.push(CarrierReceipt {
                    viewer_page,
                    table_seq_num: table_seq,
                    shape_offset: shape.source.offset,
                    shape_len: shape.source.len,
                    generic_node_count,
                    generic_ref_count,
                    table_source_ref_count,
                    table_cell_source_ref_count,
                    table_cell_paint_ref_count,
                });
            }
        }

        pages.push(page_receipt);
    }

    let receipt = Receipt {
        schema: "chaptera.virginia-table-border-composition-overlap.v1",
        source_sha256: sha,
        pages,
        totals,
        carriers,
        claims: Claims {
            geometry_proximity_used: false,
            pdf_used_as_semantic_authority: false,
            page_or_hash_product_rule_used: false,
            overlap_identity: "exact source carrier + byte range equality only",
        },
    };

    fs::create_dir_all(output.parent().expect("receipt parent")).expect("create receipt dir");
    fs::write(
        &output,
        serde_json::to_vec_pretty(&receipt).expect("serialize receipt"),
    )
    .expect("write receipt");

    assert_eq!(
        receipt.totals.candidate_count, 978,
        "composition probe must cover the already-proven 978 Virginia TABLE border carriers"
    );
    assert_eq!(
        receipt.totals.any_overlap_count, 0,
        "native-proven TABLE border carriers must not already be owned by an existing graph/table paint path"
    );
}
