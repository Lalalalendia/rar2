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

/// Native PUB-T-840 matched-diff authority:
/// AutoFormat-created TABLE cell rectangles carry ClientAnchor 0x6802 equal
/// to the owning TABLE Contents seqNum. 0x2003 is absent for cell ordinal 0
/// and is 1..N-1 for the remaining N-1 cells. This probe tests whether the
/// same bounded carrier law exists in the exact Virginia source.
const PUBLISHER_FIELD_TABLE_OWNER_REF: u16 = 0x6802;
const OFFICE_ART_RECTANGLE: u16 = 0x0001;

fn unique_field(record: &PublisherFieldRecord, id: u16) -> Option<u32> {
    let mut matches = record.fields.iter().filter(|field| field.id == id);
    let first = matches.next()?.value;
    if matches.next().is_some() {
        return None;
    }
    Some(first)
}

fn client_data_grounded(
    shape: &SpContainerObservation,
    grounded_seq_nums: &BTreeSet<u32>,
) -> bool {
    shape
        .client_data
        .as_ref()
        .and_then(|record| unique_field(record, PUBLISHER_FIELD_SHAPE_ID))
        .is_some_and(|seq| grounded_seq_nums.contains(&seq))
}

fn table_owner_ref(shape: &SpContainerObservation) -> Option<u32> {
    unique_field(shape.client_anchor.as_ref()?, PUBLISHER_FIELD_TABLE_OWNER_REF)
}

fn native_t840_cell_ordinal(
    shape: &SpContainerObservation,
    table_seq_num: u32,
) -> Option<u32> {
    if shape.fsp.as_ref()?.shape_type != OFFICE_ART_RECTANGLE {
        return None;
    }
    let anchor = shape.client_anchor.as_ref()?;
    if unique_field(anchor, PUBLISHER_FIELD_TABLE_OWNER_REF)? != table_seq_num {
        return None;
    }

    match anchor.fields.as_slice() {
        [only] if only.id == PUBLISHER_FIELD_TABLE_OWNER_REF => Some(0),
        [first, second]
            if (first.id == PUBLISHER_FIELD_TABLE_OWNER_REF
                && second.id == PUBLISHER_FIELD_XE)
                || (first.id == PUBLISHER_FIELD_XE
                    && second.id == PUBLISHER_FIELD_TABLE_OWNER_REF) =>
        {
            unique_field(anchor, PUBLISHER_FIELD_XE)
        }
        _ => None,
    }
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

fn anchor_id_signature(record: &PublisherFieldRecord) -> String {
    let ids = record.fields.iter().map(|field| field.id).collect::<Vec<_>>();
    if ids.is_empty() {
        "absent".to_owned()
    } else {
        ids.into_iter()
            .map(|id| format!("0x{id:04x}"))
            .collect::<Vec<_>>()
            .join(",")
    }
}

fn bump(map: &mut BTreeMap<String, usize>, key: impl Into<String>) {
    *map.entry(key.into()).or_default() += 1;
}

#[derive(Debug, Default, Serialize)]
struct PageReceipt {
    viewer_page_index: u32,
    table_count: usize,
    table_cell_count: usize,
    table_linked_spcontainer_count: usize,
    table_linked_ungrounded_spcontainer_count: usize,
    cell_carrier_candidate_count: usize,
    other_table_linked_anchor_count: usize,
    out_of_range_cell_ordinal_count: usize,
    exact_unique_cell_ordinal_count: usize,
    exact_ambiguous_cell_ordinal_count: usize,
    exact_absent_cell_ordinal_count: usize,
    tables_with_complete_unique_ordinal_cover: usize,
    unique_match_shape_type_histogram: BTreeMap<String, usize>,
    unique_match_fopt_signature_histogram: BTreeMap<String, usize>,
    other_table_linked_anchor_signature_histogram: BTreeMap<String, usize>,
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
fn exact_virginia_raw_spcontainer_table_cell_probe() {
    let fixture = PathBuf::from(
        env::var_os("CHAPTERA_VIRGINIA_TABLE_RAW_SP_FIXTURE")
            .expect("CHAPTERA_VIRGINIA_TABLE_RAW_SP_FIXTURE"),
    );
    let output = PathBuf::from(
        env::var_os("CHAPTERA_VIRGINIA_TABLE_RAW_SP_OUT")
            .expect("CHAPTERA_VIRGINIA_TABLE_RAW_SP_OUT"),
    );

    let bytes = fs::read(&fixture).expect("read exact Virginia Remplacante PUB");
    let actual_sha = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    assert_eq!(actual_sha, EXPECTED_SHA256, "exact Virginia source identity");

    let bundle = open_pub_bundle(&bytes, viewer_geometry_environment_v0_1())
        .expect("open exact Virginia through shared Viewer bundle");
    assert_eq!(
        bundle.geometry.document.pages.len(),
        25,
        "bounded Virginia family profile must expose 25 customer pages"
    );

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
    for viewer_page_index in [21_u32, 22_u32, 23_u32] {
        let layout_page = bundle
            .geometry
            .document
            .pages
            .get((viewer_page_index - 1) as usize)
            .expect("selected Viewer page exists");
        let parent = layout_page.id.into_canonical();

        let mut receipt = PageReceipt {
            viewer_page_index,
            ..PageReceipt::default()
        };

        for table_node in bundle.resolved_graph.nodes.values().filter(|node| {
            node.header.parent_id == parent && node.payload.table.is_some()
        }) {
            let table = table_node.payload.table.as_ref().expect("filtered TABLE");
            let table_seq_num = table_node.payload.contents_seq_num;
            let cell_count = table.cells.len();
            receipt.table_count += 1;
            receipt.table_cell_count += cell_count;

            let linked = inventory
                .shapes
                .iter()
                .filter(|shape| table_owner_ref(shape) == Some(table_seq_num))
                .collect::<Vec<_>>();
            receipt.table_linked_spcontainer_count += linked.len();

            let ungrounded = linked
                .iter()
                .copied()
                .filter(|shape| !client_data_grounded(shape, &grounded_seq_nums))
                .collect::<Vec<_>>();
            receipt.table_linked_ungrounded_spcontainer_count += ungrounded.len();

            let mut by_ordinal = BTreeMap::<u32, Vec<&SpContainerObservation>>::new();
            for shape in &ungrounded {
                let Some(anchor) = shape.client_anchor.as_ref() else {
                    continue;
                };
                let Some(ordinal) = native_t840_cell_ordinal(shape, table_seq_num) else {
                    receipt.other_table_linked_anchor_count += 1;
                    bump(
                        &mut receipt.other_table_linked_anchor_signature_histogram,
                        anchor_id_signature(anchor),
                    );
                    continue;
                };
                receipt.cell_carrier_candidate_count += 1;
                let Ok(ordinal_usize) = usize::try_from(ordinal) else {
                    receipt.out_of_range_cell_ordinal_count += 1;
                    continue;
                };
                if ordinal_usize >= cell_count {
                    receipt.out_of_range_cell_ordinal_count += 1;
                    continue;
                }
                by_ordinal.entry(ordinal).or_default().push(*shape);
            }

            let mut table_complete = true;
            for ordinal in 0..u32::try_from(cell_count).expect("cell count fits u32") {
                match by_ordinal.get(&ordinal).map(Vec::as_slice) {
                    Some([shape]) => {
                        receipt.exact_unique_cell_ordinal_count += 1;
                        bump(
                            &mut receipt.unique_match_shape_type_histogram,
                            shape
                                .fsp
                                .as_ref()
                                .map(|fsp| format!("0x{:04x}", fsp.shape_type))
                                .unwrap_or_else(|| "absent".to_owned()),
                        );
                        bump(
                            &mut receipt.unique_match_fopt_signature_histogram,
                            fopt_signature(shape),
                        );
                    }
                    Some(_) => {
                        receipt.exact_ambiguous_cell_ordinal_count += 1;
                        table_complete = false;
                    }
                    None => {
                        receipt.exact_absent_cell_ordinal_count += 1;
                        table_complete = false;
                    }
                }
            }
            if table_complete {
                receipt.tables_with_complete_unique_ordinal_cover += 1;
            }
        }

        assert_eq!(
            receipt.exact_unique_cell_ordinal_count
                + receipt.exact_ambiguous_cell_ordinal_count
                + receipt.exact_absent_cell_ordinal_count,
            receipt.table_cell_count,
            "every selected TABLE cell ordinal must be classified"
        );
        pages.push(receipt);
    }

    let p22 = pages
        .iter()
        .find(|page| page.viewer_page_index == 22)
        .expect("p22 receipt");
    assert_eq!(p22.table_count, 3, "p22 exact TABLE cohort");
    assert_eq!(p22.table_cell_count, 110, "p22 exact TABLE cell cohort");

    let receipt = Receipt {
        schema: "chaptera.virginia-table-raw-spcontainer-probe.v2",
        source_sha256: actual_sha,
        pages,
        guardrails: vec![
            "The 0x6802 TABLE-owner ClientAnchor relation and bounded 0x2003 cell-ordinal form come from native PUB-T-840 baseline/AutoFormat/growth matched diffs; Virginia is only tested for the same exact source pattern.",
            "A candidate must be ungrounded by current ClientData/Contents identity and must reference the exact TABLE Contents seqNum through one unique ClientAnchor 0x6802 field.",
            "Cell ordinal 0 is admitted only for the exact one-field {0x6802} native form; nonzero ordinals require the exact two-field {0x6802,0x2003} native form.",
            "No geometry proximity, PDF pixels, default black grid, synthetic TableStyleId, object ids, coordinates, text, filenames, property values or raw bytes are used as authority.",
            "Other 0x6802-linked anchor forms are counted separately as decoration/border candidates and receive no semantics in this probe.",
        ],
    };

    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).expect("create raw SpContainer receipt directory");
    }
    fs::write(
        &output,
        serde_json::to_vec_pretty(&receipt).expect("serialize raw SpContainer receipt"),
    )
    .expect("write raw SpContainer receipt");

    println!(
        "VIRGINIA_TABLE_RAW_SPCONTAINER_V2 p21={}/{}/{} p22={}/{}/{} p22_linked={} p22_ungrounded={} p22_candidates={} p22_complete_tables={} p23={}/{}/{}",
        receipt.pages[0].exact_unique_cell_ordinal_count,
        receipt.pages[0].exact_ambiguous_cell_ordinal_count,
        receipt.pages[0].exact_absent_cell_ordinal_count,
        receipt.pages[1].exact_unique_cell_ordinal_count,
        receipt.pages[1].exact_ambiguous_cell_ordinal_count,
        receipt.pages[1].exact_absent_cell_ordinal_count,
        receipt.pages[1].table_linked_spcontainer_count,
        receipt.pages[1].table_linked_ungrounded_spcontainer_count,
        receipt.pages[1].cell_carrier_candidate_count,
        receipt.pages[1].tables_with_complete_unique_ordinal_cover,
        receipt.pages[2].exact_unique_cell_ordinal_count,
        receipt.pages[2].exact_ambiguous_cell_ordinal_count,
        receipt.pages[2].exact_absent_cell_ordinal_count,
    );
}
