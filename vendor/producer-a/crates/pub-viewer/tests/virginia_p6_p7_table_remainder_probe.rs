use pub_core::StreamPath;
use pub_escher::{
    PUBLISHER_FIELD_SHAPE_ID, PUBLISHER_FIELD_XE, PublisherFieldRecord,
    SpContainerObservation, inspect_sp_containers,
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
const RECTANGLE: u16 = 0x0001;

fn unique_field(record: &PublisherFieldRecord, id: u16) -> Option<u32> {
    let mut matches = record.fields.iter().filter(|field| field.id == id);
    let first = matches.next()?.value;
    if matches.next().is_some() {
        return None;
    }
    Some(first)
}

fn grounded(shape: &SpContainerObservation, grounded_seq_nums: &BTreeSet<u32>) -> bool {
    shape
        .client_data
        .as_ref()
        .and_then(|record| unique_field(record, PUBLISHER_FIELD_SHAPE_ID))
        .is_some_and(|seq| grounded_seq_nums.contains(&seq))
}

fn table_owner(shape: &SpContainerObservation) -> Option<u32> {
    unique_field(shape.client_anchor.as_ref()?, TABLE_OWNER_REF)
}

fn exact_t840_ordinal(shape: &SpContainerObservation, table_seq: u32) -> Option<u32> {
    if shape.fsp.as_ref()?.shape_type != RECTANGLE {
        return None;
    }
    let anchor = shape.client_anchor.as_ref()?;
    if unique_field(anchor, TABLE_OWNER_REF)? != table_seq {
        return None;
    }
    match anchor.fields.as_slice() {
        [only] if only.id == TABLE_OWNER_REF => Some(0),
        [a, b]
            if (a.id == TABLE_OWNER_REF && b.id == CELL_ORDINAL)
                || (a.id == CELL_ORDINAL && b.id == TABLE_OWNER_REF) =>
        {
            unique_field(anchor, CELL_ORDINAL)
        }
        _ => None,
    }
}

fn ordinal_hint(shape: &SpContainerObservation, table_seq: u32) -> Option<u32> {
    let anchor = shape.client_anchor.as_ref()?;
    if unique_field(anchor, TABLE_OWNER_REF)? != table_seq {
        return None;
    }
    if let Some(ordinal) = unique_field(anchor, CELL_ORDINAL) {
        return Some(ordinal);
    }
    if anchor.fields.len() == 1 && anchor.fields[0].id == TABLE_OWNER_REF {
        return Some(0);
    }
    None
}

fn anchor_signature(record: &PublisherFieldRecord) -> String {
    record
        .fields
        .iter()
        .map(|field| format!("0x{:04x}", field.id))
        .collect::<Vec<_>>()
        .join(",")
}

fn fopt_signature(shape: &SpContainerObservation) -> String {
    let ids = shape
        .fopts
        .iter()
        .flat_map(|record| record.properties.iter())
        .map(|property| property.property_id())
        .collect::<BTreeSet<_>>();
    if ids.is_empty() {
        "absent".to_owned()
    } else {
        ids.into_iter()
            .map(|id| format!("0x{id:04x}"))
            .collect::<Vec<_>>()
            .join(",")
    }
}

fn paint_class(shape: &SpContainerObservation) -> &'static str {
    let ids = shape
        .fopts
        .iter()
        .flat_map(|record| record.properties.iter())
        .map(|property| property.property_id())
        .collect::<BTreeSet<_>>();
    let fill = ids.iter().any(|id| (0x0180..=0x01bf).contains(id));
    let line = ids.iter().any(|id| (0x01c0..=0x01ff).contains(id));
    match (fill, line) {
        (true, true) => "fill_and_line_family",
        (true, false) => "fill_family_only",
        (false, true) => "line_family_only",
        (false, false) => "no_fill_or_line_family",
    }
}

fn bump(map: &mut BTreeMap<String, usize>, key: impl Into<String>) {
    *map.entry(key.into()).or_default() += 1;
}

#[derive(Debug, Default, Serialize)]
struct PageReceipt {
    viewer_page_index: u32,
    table_count: usize,
    cell_count: usize,
    painted_cell_count: usize,
    unpainted_cell_count: usize,
    linked_ungrounded_carrier_count: usize,
    exact_t840_unique_ordinal_count: usize,
    unsupported_linked_carrier_count: usize,
    unpainted_with_exact_carrier_count: usize,
    unpainted_with_unique_unsupported_ordinal_carrier_count: usize,
    unpainted_with_ambiguous_unsupported_ordinal_carrier_count: usize,
    unpainted_with_no_ordinal_carrier_count: usize,
    unsupported_anchor_signature_histogram: BTreeMap<String, usize>,
    unsupported_shape_type_histogram: BTreeMap<String, usize>,
    unsupported_fopt_signature_histogram: BTreeMap<String, usize>,
    unsupported_paint_class_histogram: BTreeMap<String, usize>,
}

#[derive(Debug, Serialize)]
struct Receipt {
    schema: &'static str,
    source_sha256: String,
    pages: Vec<PageReceipt>,
    guardrails: Vec<&'static str>,
}

#[test]
#[ignore = "requires exact public Virginia Remplacante fixture"]
fn exact_virginia_p6_p7_table_remainder_probe() {
    let fixture = PathBuf::from(
        env::var_os("CHAPTERA_VIRGINIA_TABLE_REMAINDER_FIXTURE")
            .expect("CHAPTERA_VIRGINIA_TABLE_REMAINDER_FIXTURE"),
    );
    let output = PathBuf::from(
        env::var_os("CHAPTERA_VIRGINIA_TABLE_REMAINDER_OUT")
            .expect("CHAPTERA_VIRGINIA_TABLE_REMAINDER_OUT"),
    );

    let bytes = fs::read(&fixture).expect("read exact Virginia Remplacante PUB");
    let actual_sha = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    assert_eq!(actual_sha, EXPECTED_SHA256, "exact Virginia source identity");

    let bundle = open_pub_bundle(&bytes, viewer_geometry_environment_v0_1())
        .expect("open exact Virginia through shared Viewer bundle");

    let escher = pub_cfb::read_stream_reader(
        Cursor::new(bytes.as_slice()),
        pub_reader::ESCHER_STREAM_PATH,
    )
    .expect("read exact Escher stream");
    let inventory = inspect_sp_containers(
        StreamPath(pub_reader::ESCHER_STREAM_PATH.into()),
        &escher,
    )
    .expect("inspect OfficeArt SpContainers");

    let grounded_seq_nums = bundle
        .resolved_graph
        .nodes
        .values()
        .map(|node| node.payload.contents_seq_num)
        .collect::<BTreeSet<_>>();

    let mut pages = Vec::new();
    for viewer_page_index in [6_u32, 7_u32, 21_u32, 22_u32, 23_u32] {
        let layout_page = bundle
            .geometry
            .document
            .pages
            .get((viewer_page_index - 1) as usize)
            .expect("selected Viewer page exists");
        let parent = layout_page.id.into_canonical();
        let mut page = PageReceipt {
            viewer_page_index,
            ..PageReceipt::default()
        };

        for table_node in bundle
            .resolved_graph
            .nodes
            .values()
            .filter(|node| node.header.parent_id == parent && node.payload.table.is_some())
        {
            let table = table_node.payload.table.as_ref().expect("filtered TABLE");
            let table_seq = table_node.payload.contents_seq_num;
            page.table_count += 1;
            page.cell_count += table.cells.len();
            page.painted_cell_count += table.cells.iter().filter(|cell| cell.paint.is_some()).count();
            page.unpainted_cell_count += table.cells.iter().filter(|cell| cell.paint.is_none()).count();

            let linked = inventory
                .shapes
                .iter()
                .filter(|shape| table_owner(shape) == Some(table_seq))
                .filter(|shape| !grounded(shape, &grounded_seq_nums))
                .collect::<Vec<_>>();
            page.linked_ungrounded_carrier_count += linked.len();

            let mut exact_by_ordinal = BTreeMap::<u32, usize>::new();
            let mut unsupported_by_ordinal = BTreeMap::<u32, usize>::new();

            for shape in linked {
                if let Some(ordinal) = exact_t840_ordinal(shape, table_seq) {
                    *exact_by_ordinal.entry(ordinal).or_default() += 1;
                    continue;
                }

                page.unsupported_linked_carrier_count += 1;
                if let Some(anchor) = shape.client_anchor.as_ref() {
                    bump(
                        &mut page.unsupported_anchor_signature_histogram,
                        anchor_signature(anchor),
                    );
                }
                bump(
                    &mut page.unsupported_shape_type_histogram,
                    shape
                        .fsp
                        .as_ref()
                        .map(|fsp| format!("0x{:04x}", fsp.shape_type))
                        .unwrap_or_else(|| "absent".to_owned()),
                );
                bump(
                    &mut page.unsupported_fopt_signature_histogram,
                    fopt_signature(shape),
                );
                bump(
                    &mut page.unsupported_paint_class_histogram,
                    paint_class(shape),
                );
                if let Some(ordinal) = ordinal_hint(shape, table_seq) {
                    *unsupported_by_ordinal.entry(ordinal).or_default() += 1;
                }
            }

            page.exact_t840_unique_ordinal_count += exact_by_ordinal
                .values()
                .filter(|count| **count == 1)
                .count();

            for cell in table.cells.iter().filter(|cell| cell.paint.is_none()) {
                let ordinal = cell.stored_record_index;
                if exact_by_ordinal.get(&ordinal) == Some(&1) {
                    page.unpainted_with_exact_carrier_count += 1;
                    continue;
                }
                match unsupported_by_ordinal.get(&ordinal).copied().unwrap_or(0) {
                    0 => page.unpainted_with_no_ordinal_carrier_count += 1,
                    1 => page.unpainted_with_unique_unsupported_ordinal_carrier_count += 1,
                    _ => page.unpainted_with_ambiguous_unsupported_ordinal_carrier_count += 1,
                }
            }
        }

        assert_eq!(
            page.painted_cell_count + page.unpainted_cell_count,
            page.cell_count,
            "every selected TABLE cell classified"
        );
        pages.push(page);
    }

    let receipt = Receipt {
        schema: "chaptera.virginia-p6-p7-table-remainder.v1",
        source_sha256: actual_sha,
        pages,
        guardrails: vec![
            "The exact T840 carrier law is treated as fixed authority and is not widened by this probe.",
            "Unsupported TABLE-linked carriers are classified only by field-ID shape, OfficeArt shape type, FOPT property-ID presence, and coarse fill/line family presence.",
            "No property values, colors, coordinates, object identities, text, offsets, filenames, PDF-derived semantics, or raw bytes are emitted.",
            "An unsupported ordinal-bearing carrier is only a discriminator candidate; it receives no product semantics from this receipt.",
        ],
    };

    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).expect("create remainder receipt directory");
    }
    fs::write(
        &output,
        serde_json::to_vec_pretty(&receipt).expect("serialize remainder receipt"),
    )
    .expect("write remainder receipt");

    for page in &receipt.pages {
        println!(
            "VIRGINIA_TABLE_REMAINDER p{} paint={}/{} unsupported={} no_ordinal={} unique_unsupported={} ambiguous_unsupported={} exact_but_unpainted={}",
            page.viewer_page_index,
            page.painted_cell_count,
            page.cell_count,
            page.unsupported_linked_carrier_count,
            page.unpainted_with_no_ordinal_carrier_count,
            page.unpainted_with_unique_unsupported_ordinal_carrier_count,
            page.unpainted_with_ambiguous_unsupported_ordinal_carrier_count,
            page.unpainted_with_exact_carrier_count,
        );
    }
}
