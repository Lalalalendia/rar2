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
    let mut fields = record.fields.iter().filter(|field| field.id == id);
    let first = fields.next()?.value;
    fields.next().is_none().then_some(first)
}

fn has_client_data_identity(shape: &SpContainerObservation) -> bool {
    shape.client_data.as_ref().is_some_and(|record| {
        record
            .fields
            .iter()
            .any(|field| field.id == PUBLISHER_FIELD_SHAPE_ID)
    })
}

fn table_owner(shape: &SpContainerObservation) -> Option<u32> {
    unique_field(shape.client_anchor.as_ref()?, TABLE_OWNER_REF)
}

fn exact_t840_cell_carrier(shape: &SpContainerObservation, table_seq: u32) -> bool {
    if has_client_data_identity(shape) || shape.fsp.as_ref().map(|fsp| fsp.shape_type) != Some(RECTANGLE) {
        return false;
    }
    let Some(anchor) = shape.client_anchor.as_ref() else {
        return false;
    };
    if unique_field(anchor, TABLE_OWNER_REF) != Some(table_seq) {
        return false;
    }
    match anchor.fields.as_slice() {
        [only] => only.id == TABLE_OWNER_REF,
        [first, second] => {
            ((first.id == TABLE_OWNER_REF && second.id == PUBLISHER_FIELD_XE)
                || (first.id == PUBLISHER_FIELD_XE && second.id == TABLE_OWNER_REF))
                && unique_field(anchor, PUBLISHER_FIELD_XE).is_some()
        }
        _ => false,
    }
}

fn anchor_signature(shape: &SpContainerObservation) -> String {
    shape
        .client_anchor
        .as_ref()
        .map(|anchor| {
            anchor
                .fields
                .iter()
                .map(|field| format!("0x{:04x}", field.id))
                .collect::<Vec<_>>()
                .join(",")
        })
        .unwrap_or_else(|| "absent".into())
}

fn fopt_family(shape: &SpContainerObservation) -> String {
    let ids = shape
        .fopts
        .iter()
        .flat_map(|record| record.properties.iter())
        .map(|property| property.property_id())
        .collect::<BTreeSet<_>>();
    let has_fill = ids.iter().any(|id| (0x0180..=0x01bf).contains(id));
    let has_line = ids.iter().any(|id| (0x01c0..=0x01ff).contains(id));
    match (has_fill, has_line) {
        (true, true) => "fill_and_line",
        (true, false) => "fill_only",
        (false, true) => "line_only",
        (false, false) => "neither",
    }
    .to_owned()
}

fn bump(map: &mut BTreeMap<String, usize>, key: impl Into<String>) {
    *map.entry(key.into()).or_default() += 1;
}

#[derive(Debug, Default, Serialize)]
struct PageReceipt {
    viewer_page: u32,
    table_count: usize,
    table_cell_count: usize,
    exact_t840_cell_carrier_count: usize,
    table_linked_no_identity_noncell_carrier_count: usize,
    border_decor_signature_count: usize,
    anchor_signature_histogram: BTreeMap<String, usize>,
    shape_type_histogram: BTreeMap<String, usize>,
    fopt_family_histogram: BTreeMap<String, usize>,
}

#[derive(Debug, Serialize)]
struct Receipt {
    schema: &'static str,
    source_sha256: String,
    pages: Vec<PageReceipt>,
    claims: BTreeMap<&'static str, bool>,
}

#[test]
#[ignore = "requires exact public Virginia Remplacante fixture"]
fn exact_virginia_p10_table_border_carrier_census() {
    let fixture = PathBuf::from(
        env::var_os("CHAPTERA_VIRGINIA_P10_BORDER_FIXTURE")
            .expect("CHAPTERA_VIRGINIA_P10_BORDER_FIXTURE"),
    );
    let output = PathBuf::from(
        env::var_os("CHAPTERA_VIRGINIA_P10_BORDER_OUT")
            .expect("CHAPTERA_VIRGINIA_P10_BORDER_OUT"),
    );

    let bytes = fs::read(&fixture).expect("read exact Virginia PUB");
    let sha = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    assert_eq!(sha, EXPECTED_SHA256, "exact Virginia source identity");

    let bundle = open_pub_bundle(&bytes, viewer_geometry_environment_v0_1())
        .expect("open exact Virginia Viewer bundle");
    let escher = pub_cfb::read_stream_reader(
        Cursor::new(bytes.as_slice()),
        pub_reader::ESCHER_STREAM_PATH,
    )
    .expect("read Escher stream");
    let inventory = inspect_sp_containers(
        StreamPath(pub_reader::ESCHER_STREAM_PATH.into()),
        &escher,
    )
    .expect("inspect SpContainers");

    let mut pages = Vec::new();
    for viewer_page in [9_u32, 10_u32, 11_u32] {
        let page = bundle
            .geometry
            .document
            .pages
            .get((viewer_page - 1) as usize)
            .expect("selected page exists");
        let parent = page.id.into_canonical();
        let tables = bundle
            .resolved_graph
            .nodes
            .values()
            .filter(|node| node.header.parent_id == parent && node.payload.table.is_some())
            .collect::<Vec<_>>();

        let mut receipt = PageReceipt {
            viewer_page,
            table_count: tables.len(),
            table_cell_count: tables
                .iter()
                .map(|node| node.payload.table.as_ref().expect("TABLE").cells.len())
                .sum(),
            ..PageReceipt::default()
        };

        for table_node in tables {
            let table_seq = table_node.payload.contents_seq_num;
            for shape in inventory
                .shapes
                .iter()
                .filter(|shape| table_owner(shape) == Some(table_seq))
                .filter(|shape| !has_client_data_identity(shape))
            {
                if exact_t840_cell_carrier(shape, table_seq) {
                    receipt.exact_t840_cell_carrier_count += 1;
                    continue;
                }

                receipt.table_linked_no_identity_noncell_carrier_count += 1;
                let signature = anchor_signature(shape);
                bump(&mut receipt.anchor_signature_histogram, signature.clone());
                bump(
                    &mut receipt.shape_type_histogram,
                    shape
                        .fsp
                        .as_ref()
                        .map(|fsp| format!("0x{:04x}", fsp.shape_type))
                        .unwrap_or_else(|| "absent".into()),
                );
                bump(&mut receipt.fopt_family_histogram, fopt_family(shape));

                let ids = shape
                    .client_anchor
                    .as_ref()
                    .map(|anchor| anchor.fields.iter().map(|field| field.id).collect::<BTreeSet<_>>())
                    .unwrap_or_default();
                if ids.contains(&TABLE_OWNER_REF)
                    && ids.contains(&0x2001)
                    && ids.iter().any(|id| (0x2004..=0x2007).contains(id))
                {
                    receipt.border_decor_signature_count += 1;
                }
            }
        }
        pages.push(receipt);
    }

    let claims = BTreeMap::from([
        ("publisher_pdf_used_for_semantics", false),
        ("raw_property_values_emitted", false),
        ("coordinates_emitted", false),
        ("object_ids_emitted", false),
        ("border_side_semantics_assigned", false),
    ]);
    let receipt = Receipt {
        schema: "chaptera.virginia-p10-table-border-carrier-census.v1",
        source_sha256: sha,
        pages,
        claims,
    };

    fs::create_dir_all(output.parent().expect("receipt parent")).expect("create receipt dir");
    fs::write(
        &output,
        serde_json::to_vec_pretty(&receipt).expect("serialize receipt"),
    )
    .expect("write receipt");

    println!(
        "VIRGINIA_P10_TABLE_BORDER_CARRIER {}",
        serde_json::to_string(&receipt).expect("serialize log receipt")
    );
}
