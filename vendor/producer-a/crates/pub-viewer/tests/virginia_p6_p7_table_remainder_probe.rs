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
const RECTANGLE: u16 = 0x0001;

fn unique_field(record: &PublisherFieldRecord, id: u16) -> Option<u32> {
    let mut matches = record.fields.iter().filter(|field| field.id == id);
    let first = matches.next()?.value;
    matches.next().is_none().then_some(first)
}

fn client_data_grounded(shape: &SpContainerObservation, grounded: &BTreeSet<u32>) -> bool {
    shape
        .client_data
        .as_ref()
        .and_then(|record| unique_field(record, PUBLISHER_FIELD_SHAPE_ID))
        .is_some_and(|seq| grounded.contains(&seq))
}

fn table_owner_ref(shape: &SpContainerObservation) -> Option<u32> {
    unique_field(shape.client_anchor.as_ref()?, TABLE_OWNER_REF)
}

fn native_t840_ordinal(shape: &SpContainerObservation, table_seq: u32) -> Option<u32> {
    if shape.fsp.as_ref()?.shape_type != RECTANGLE {
        return None;
    }
    let anchor = shape.client_anchor.as_ref()?;
    if unique_field(anchor, TABLE_OWNER_REF)? != table_seq {
        return None;
    }
    match anchor.fields.as_slice() {
        [only] if only.id == TABLE_OWNER_REF => Some(0),
        [first, second]
            if (first.id == TABLE_OWNER_REF && second.id == PUBLISHER_FIELD_XE)
                || (first.id == PUBLISHER_FIELD_XE && second.id == TABLE_OWNER_REF) =>
        {
            unique_field(anchor, PUBLISHER_FIELD_XE)
        }
        _ => None,
    }
}

fn anchor_signature(shape: &SpContainerObservation) -> String {
    let Some(anchor) = shape.client_anchor.as_ref() else {
        return "absent".into();
    };
    anchor
        .fields
        .iter()
        .map(|field| format!("0x{:04x}", field.id))
        .collect::<Vec<_>>()
        .join(",")
}

fn paint_family(shape: &SpContainerObservation) -> &'static str {
    let ids = shape
        .fopts
        .iter()
        .flat_map(|record| record.properties.iter())
        .map(|property| property.property_id())
        .collect::<BTreeSet<_>>();
    let has_fill = ids.iter().any(|id| matches!(*id, 0x0180..=0x01bf));
    let has_line = ids.iter().any(|id| matches!(*id, 0x01c0..=0x01ff));
    match (has_fill, has_line) {
        (true, true) => "fill+line",
        (true, false) => "fill-only",
        (false, true) => "line-only",
        (false, false) => "none",
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
    exact_native_unique_cells: usize,
    exact_native_ambiguous_cells: usize,
    exact_native_absent_cells: usize,
    unpainted_with_exact_native_carrier: usize,
    unpainted_with_unsupported_mapped_carrier: usize,
    unpainted_with_no_mapped_carrier: usize,
    unsupported_mapped_ambiguous_cells: usize,
    unsupported_unmapped_carrier_count: usize,
    unsupported_anchor_signatures: BTreeMap<String, usize>,
    unsupported_shape_types: BTreeMap<String, usize>,
    unsupported_paint_families: BTreeMap<String, usize>,
}

#[derive(Debug, Serialize)]
struct Receipt {
    schema: &'static str,
    source_sha256: String,
    pages: Vec<PageReceipt>,
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

    let bytes = fs::read(&fixture).expect("read Virginia PUB");
    let sha = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    assert_eq!(sha, EXPECTED_SHA256, "exact Virginia source identity");

    let bundle = open_pub_bundle(&bytes, viewer_geometry_environment_v0_1())
        .expect("open exact Virginia through shared Viewer bundle");
    assert_eq!(bundle.geometry.document.pages.len(), 25);

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

    let grounded = bundle
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
            .expect("selected page exists");
        let parent = layout_page.id.into_canonical();
        let mut receipt = PageReceipt {
            viewer_page_index,
            ..PageReceipt::default()
        };

        for table_node in bundle.resolved_graph.nodes.values().filter(|node| {
            node.header.parent_id == parent && node.payload.table.is_some()
        }) {
            let table = table_node.payload.table.as_ref().expect("filtered TABLE");
            let table_seq = table_node.payload.contents_seq_num;
            receipt.table_count += 1;
            receipt.cell_count += table.cells.len();
            receipt.painted_cell_count += table.cells.iter().filter(|cell| cell.paint.is_some()).count();

            let linked = inventory
                .shapes
                .iter()
                .filter(|shape| table_owner_ref(shape) == Some(table_seq))
                .filter(|shape| !client_data_grounded(shape, &grounded))
                .collect::<Vec<_>>();

            let mut exact = BTreeMap::<u32, Vec<&SpContainerObservation>>::new();
            let mut unsupported_mapped = BTreeMap::<u32, Vec<&SpContainerObservation>>::new();

            for shape in linked {
                if let Some(ordinal) = native_t840_ordinal(shape, table_seq) {
                    if usize::try_from(ordinal).ok().is_some_and(|v| v < table.cells.len()) {
                        exact.entry(ordinal).or_default().push(shape);
                    }
                    continue;
                }

                bump(&mut receipt.unsupported_anchor_signatures, anchor_signature(shape));
                bump(
                    &mut receipt.unsupported_shape_types,
                    shape
                        .fsp
                        .as_ref()
                        .map(|fsp| format!("0x{:04x}", fsp.shape_type))
                        .unwrap_or_else(|| "absent".into()),
                );
                bump(&mut receipt.unsupported_paint_families, paint_family(shape));

                let Some(anchor) = shape.client_anchor.as_ref() else {
                    receipt.unsupported_unmapped_carrier_count += 1;
                    continue;
                };
                let Some(ordinal) = unique_field(anchor, PUBLISHER_FIELD_XE) else {
                    receipt.unsupported_unmapped_carrier_count += 1;
                    continue;
                };
                if usize::try_from(ordinal).ok().is_some_and(|v| v < table.cells.len()) {
                    unsupported_mapped.entry(ordinal).or_default().push(shape);
                } else {
                    receipt.unsupported_unmapped_carrier_count += 1;
                }
            }

            for cell in &table.cells {
                let ordinal = cell.stored_record_index;
                match exact.get(&ordinal).map(Vec::as_slice) {
                    Some([_]) => receipt.exact_native_unique_cells += 1,
                    Some(_) => receipt.exact_native_ambiguous_cells += 1,
                    None => receipt.exact_native_absent_cells += 1,
                }

                if cell.paint.is_some() {
                    continue;
                }
                match exact.get(&ordinal).map(Vec::as_slice) {
                    Some([_]) => {
                        receipt.unpainted_with_exact_native_carrier += 1;
                        continue;
                    }
                    Some(_) => {
                        receipt.unpainted_with_no_mapped_carrier += 1;
                        continue;
                    }
                    None => {}
                }
                match unsupported_mapped.get(&ordinal).map(Vec::as_slice) {
                    Some([_]) => receipt.unpainted_with_unsupported_mapped_carrier += 1,
                    Some(_) => {
                        receipt.unsupported_mapped_ambiguous_cells += 1;
                        receipt.unpainted_with_no_mapped_carrier += 1;
                    }
                    None => receipt.unpainted_with_no_mapped_carrier += 1,
                }
            }
        }

        receipt.unpainted_cell_count = receipt.cell_count - receipt.painted_cell_count;
        assert_eq!(
            receipt.exact_native_unique_cells
                + receipt.exact_native_ambiguous_cells
                + receipt.exact_native_absent_cells,
            receipt.cell_count,
            "every TABLE cell must be classified against exact native law"
        );
        assert_eq!(
            receipt.unpainted_with_exact_native_carrier
                + receipt.unpainted_with_unsupported_mapped_carrier
                + receipt.unpainted_with_no_mapped_carrier,
            receipt.unpainted_cell_count,
            "every unpainted TABLE cell must have one bounded remainder class"
        );
        pages.push(receipt);
    }

    let p6 = pages.iter().find(|p| p.viewer_page_index == 6).unwrap();
    let p7 = pages.iter().find(|p| p.viewer_page_index == 7).unwrap();
    assert_eq!((p6.table_count, p6.cell_count, p6.painted_cell_count), (12, 588, 209));
    assert_eq!((p7.table_count, p7.cell_count, p7.painted_cell_count), (1, 192, 60));

    fs::create_dir_all(output.parent().expect("receipt parent")).expect("create receipt dir");
    fs::write(
        &output,
        serde_json::to_vec_pretty(&Receipt {
            schema: "chaptera.virginia-p6-p7-table-remainder.v1",
            source_sha256: sha,
            pages,
        })
        .expect("serialize receipt"),
    )
    .expect("write receipt");
}
