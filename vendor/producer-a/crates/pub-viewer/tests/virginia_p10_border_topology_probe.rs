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
const RECTANGLE: u16 = 0x0001;

fn unique_field(record: &PublisherFieldRecord, id: u16) -> Option<u32> {
    let mut matches = record.fields.iter().filter(|field| field.id == id);
    let first = matches.next()?.value;
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

fn anchor_signature(shape: &SpContainerObservation) -> String {
    let Some(anchor) = shape.client_anchor.as_ref() else {
        return "absent".into();
    };
    let mut ids = anchor.fields.iter().map(|field| field.id).collect::<Vec<_>>();
    ids.sort_unstable();
    ids.iter()
        .map(|id| format!("0x{id:04x}"))
        .collect::<Vec<_>>()
        .join(",")
}

fn fopt_family(shape: &SpContainerObservation) -> &'static str {
    let ids = shape
        .fopts
        .iter()
        .flat_map(|record| record.properties.iter())
        .map(|property| property.property_id())
        .collect::<BTreeSet<_>>();
    let fill = ids.iter().any(|id| matches!(*id, 0x0180..=0x01bf));
    let line = ids.iter().any(|id| matches!(*id, 0x01c0..=0x01ff));
    match (fill, line) {
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
    viewer_page: u32,
    table_count: usize,
    cell_count: usize,
    painted_cell_count: usize,
    table_linked_ungrounded_rectangles: usize,
    exact_fill_carriers: usize,
    border_decor_candidates: usize,
    other_table_linked_rectangles: usize,
    border_anchor_signatures: BTreeMap<String, usize>,
    border_fopt_families: BTreeMap<String, usize>,
}

#[derive(Debug, Serialize)]
struct Receipt {
    schema: &'static str,
    source_sha256: String,
    pages: Vec<PageReceipt>,
    claims: Claims,
}

#[derive(Debug, Serialize)]
struct Claims {
    raw_anchor_values_emitted: bool,
    object_ids_emitted: bool,
    coordinates_emitted: bool,
    colors_emitted: bool,
    pdf_used_as_semantic_authority: bool,
}

#[test]
#[ignore = "requires exact public Virginia Remplacante fixture"]
fn exact_virginia_p10_border_topology_probe() {
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
    for viewer_page in [9_u32, 10_u32, 11_u32] {
        let page = bundle
            .geometry
            .document
            .pages
            .get((viewer_page - 1) as usize)
            .expect("selected page exists");
        let parent = page.id.into_canonical();
        let mut receipt = PageReceipt {
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
            receipt.table_count += 1;
            receipt.cell_count += table.cells.len();
            receipt.painted_cell_count += table.cells.iter().filter(|cell| cell.paint.is_some()).count();

            for shape in inventory
                .shapes
                .iter()
                .filter(|shape| owner_ref(shape) == Some(table_seq))
                .filter(|shape| !grounded(shape, &grounded_ids))
                .filter(|shape| shape.fsp.as_ref().is_some_and(|fsp| fsp.shape_type == RECTANGLE))
            {
                receipt.table_linked_ungrounded_rectangles += 1;
                let Some(anchor) = shape.client_anchor.as_ref() else {
                    receipt.other_table_linked_rectangles += 1;
                    continue;
                };
                let has_ordinal = anchor.fields.iter().any(|field| field.id == CELL_ORDINAL);
                let has_border_decor_field = anchor
                    .fields
                    .iter()
                    .any(|field| matches!(field.id, 0x2001 | 0x2004..=0x2007));

                if has_ordinal {
                    receipt.exact_fill_carriers += 1;
                } else if has_border_decor_field {
                    receipt.border_decor_candidates += 1;
                    bump(&mut receipt.border_anchor_signatures, anchor_signature(shape));
                    bump(&mut receipt.border_fopt_families, fopt_family(shape));
                } else {
                    receipt.other_table_linked_rectangles += 1;
                }
            }
        }

        pages.push(receipt);
    }

    let receipt = Receipt {
        schema: "chaptera.virginia-p10-border-topology.v1",
        source_sha256: sha,
        pages,
        claims: Claims {
            raw_anchor_values_emitted: false,
            object_ids_emitted: false,
            coordinates_emitted: false,
            colors_emitted: false,
            pdf_used_as_semantic_authority: false,
        },
    };

    fs::create_dir_all(output.parent().expect("receipt parent")).expect("create receipt dir");
    fs::write(
        &output,
        serde_json::to_vec_pretty(&receipt).expect("serialize receipt"),
    )
    .expect("write receipt");
}
