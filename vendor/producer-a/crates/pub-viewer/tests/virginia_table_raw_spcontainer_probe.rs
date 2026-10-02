use pub_core::{RawSpan, StreamPath};
use pub_escher::{
    OFFICE_ART_DG_CONTAINER, OFFICE_ART_SP_CONTAINER, PUBLISHER_FIELD_SHAPE_ID,
    PUBLISHER_FIELD_XE, PUBLISHER_FIELD_XS, PUBLISHER_FIELD_YE, PUBLISHER_FIELD_YS,
    OfficeArtBody, OfficeArtRecord, PublisherFieldRecord, SpContainerObservation,
    inspect_sp_containers, parse_officeart_stream,
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

type SpanKey = (u64, u64);

fn span_key(span: &RawSpan) -> SpanKey {
    (span.offset, span.len)
}

fn collect_sp_sources(records: &[OfficeArtRecord], output: &mut Vec<RawSpan>) {
    for record in records {
        if record.header.rec_type == OFFICE_ART_SP_CONTAINER {
            output.push(record.source.clone());
            continue;
        }
        if let OfficeArtBody::Container { children } = &record.body {
            collect_sp_sources(children, output);
        }
    }
}

fn collect_dg_shapes(records: &[OfficeArtRecord], output: &mut Vec<(RawSpan, Vec<RawSpan>)>) {
    for record in records {
        if record.header.rec_type == OFFICE_ART_DG_CONTAINER {
            let mut shapes = Vec::new();
            if let OfficeArtBody::Container { children } = &record.body {
                collect_sp_sources(children, &mut shapes);
            }
            output.push((record.source.clone(), shapes));
            continue;
        }
        if let OfficeArtBody::Container { children } = &record.body {
            collect_dg_shapes(children, output);
        }
    }
}

fn unique_field(record: &PublisherFieldRecord, id: u16) -> Option<u32> {
    let mut matches = record.fields.iter().filter(|field| field.id == id);
    let first = matches.next()?.value;
    if matches.next().is_some() {
        return None;
    }
    Some(first)
}

fn signed_field(record: &PublisherFieldRecord, id: u16) -> Option<i64> {
    let raw = unique_field(record, id)?;
    Some(i64::from(i32::from_le_bytes(raw.to_le_bytes())))
}

fn anchor_tuple(record: &PublisherFieldRecord) -> Option<[i64; 4]> {
    Some([
        signed_field(record, PUBLISHER_FIELD_XS)?,
        signed_field(record, PUBLISHER_FIELD_YS)?,
        signed_field(record, PUBLISHER_FIELD_XE)?,
        signed_field(record, PUBLISHER_FIELD_YE)?,
    ])
}

fn expected_anchor_tuple(
    page_width: i64,
    page_height: i64,
    cell: pub_model::RectEmu,
) -> Option<[i64; 4]> {
    // Exact inverse of pub-reader::page_relative_bounds. This is measurement
    // against the already-shipped geometry law, not a second geometry model.
    let xs = cell.x.get().checked_sub(page_width.checked_div(2)?)?;
    let ys = cell.y.get().checked_sub(page_height.checked_div(2)?)?;
    let xe = xs.checked_add(cell.width.get())?;
    let ye = ys.checked_add(cell.height.get())?;
    Some([xs, ys, xe, ye])
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

fn bump(map: &mut BTreeMap<String, usize>, key: impl Into<String>) {
    *map.entry(key.into()).or_default() += 1;
}

#[derive(Debug, Default, Serialize)]
struct PageReceipt {
    viewer_page_index: u32,
    resolved_dg_count: usize,
    raw_spcontainer_count: usize,
    grounded_spcontainer_count: usize,
    ungrounded_spcontainer_count: usize,
    ungrounded_complete_client_anchor_count: usize,
    table_count: usize,
    table_cell_count: usize,
    exact_unique_cell_match_count: usize,
    exact_ambiguous_cell_match_count: usize,
    exact_absent_cell_match_count: usize,
    unique_match_shape_type_histogram: BTreeMap<String, usize>,
    unique_match_fopt_signature_histogram: BTreeMap<String, usize>,
}

#[derive(Debug, Serialize)]
struct Receipt {
    schema: &'static str,
    source_sha256: String,
    pages: Vec<PageReceipt>,
    unresolved_dg_count: usize,
    multi_page_dg_count: usize,
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
    let stream = StreamPath(pub_reader::ESCHER_STREAM_PATH.into());
    let parsed = parse_officeart_stream(stream.clone(), &escher).expect("parse OfficeArt tree");
    let inventory = inspect_sp_containers(stream, &escher).expect("inspect OfficeArt SpContainers");

    let shape_by_source = inventory
        .shapes
        .iter()
        .map(|shape| (span_key(&shape.source), shape))
        .collect::<BTreeMap<_, _>>();

    let mut dg_shapes = Vec::new();
    collect_dg_shapes(&parsed.records, &mut dg_shapes);

    let page_index_by_parent = bundle
        .geometry
        .document
        .pages
        .iter()
        .enumerate()
        .map(|(index, page)| (page.id.into_canonical(), (index + 1) as u32))
        .collect::<BTreeMap<_, _>>();

    let seq_to_page_index = bundle
        .resolved_graph
        .nodes
        .values()
        .filter_map(|node| {
            page_index_by_parent
                .get(&node.header.parent_id)
                .copied()
                .map(|page_index| (node.payload.contents_seq_num, page_index))
        })
        .collect::<BTreeMap<_, _>>();

    let mut dg_page = BTreeMap::<SpanKey, u32>::new();
    let mut unresolved_dg_count = 0_usize;
    let mut multi_page_dg_count = 0_usize;

    for (dg_source, shape_sources) in &dg_shapes {
        let pages = shape_sources
            .iter()
            .filter_map(|source| shape_by_source.get(&span_key(source)).copied())
            .filter_map(|shape| {
                let client_data = shape.client_data.as_ref()?;
                let seq = unique_field(client_data, PUBLISHER_FIELD_SHAPE_ID)?;
                seq_to_page_index.get(&seq).copied()
            })
            .collect::<BTreeSet<_>>();

        match pages.iter().copied().collect::<Vec<_>>().as_slice() {
            [page] => {
                dg_page.insert(span_key(dg_source), *page);
            }
            [] => unresolved_dg_count += 1,
            _ => multi_page_dg_count += 1,
        }
    }

    let mut pages = Vec::new();
    for viewer_page_index in [21_u32, 22_u32, 23_u32] {
        let layout_page = bundle
            .geometry
            .document
            .pages
            .get((viewer_page_index - 1) as usize)
            .expect("selected Viewer page exists");
        let page_id = layout_page.id;
        let parent = page_id.into_canonical();
        let source_page = bundle
            .resolved_graph
            .pages
            .get(&page_id)
            .expect("selected resolved Page exists");
        let page_width = source_page.size.width.get();
        let page_height = source_page.size.height.get();

        let mut receipt = PageReceipt {
            viewer_page_index,
            ..PageReceipt::default()
        };

        let raw_shapes = dg_shapes
            .iter()
            .filter(|(dg_source, _)| {
                dg_page.get(&span_key(dg_source)) == Some(&viewer_page_index)
            })
            .flat_map(|(_, shapes)| shapes.iter())
            .filter_map(|source| shape_by_source.get(&span_key(source)).copied())
            .collect::<Vec<_>>();
        receipt.resolved_dg_count = dg_shapes
            .iter()
            .filter(|(dg_source, _)| {
                dg_page.get(&span_key(dg_source)) == Some(&viewer_page_index)
            })
            .count();
        receipt.raw_spcontainer_count = raw_shapes.len();

        let candidates = raw_shapes
            .iter()
            .copied()
            .filter(|shape| {
                let grounded = shape
                    .client_data
                    .as_ref()
                    .and_then(|record| unique_field(record, PUBLISHER_FIELD_SHAPE_ID))
                    .is_some_and(|seq| seq_to_page_index.contains_key(&seq));
                if grounded {
                    receipt.grounded_spcontainer_count += 1;
                } else {
                    receipt.ungrounded_spcontainer_count += 1;
                }
                !grounded
            })
            .filter_map(|shape| {
                let anchor = shape.client_anchor.as_ref()?;
                let tuple = anchor_tuple(anchor)?;
                receipt.ungrounded_complete_client_anchor_count += 1;
                Some((shape, tuple))
            })
            .collect::<Vec<_>>();

        for table_node in bundle.resolved_graph.nodes.values().filter(|node| {
            node.header.parent_id == parent && node.payload.table.is_some()
        }) {
            let table = table_node.payload.table.as_ref().expect("filtered TABLE");
            receipt.table_count += 1;
            receipt.table_cell_count += table.cells.len();

            for cell in &table.cells {
                let Some(bounds) = cell.bounds else {
                    receipt.exact_absent_cell_match_count += 1;
                    continue;
                };
                let Some(expected) = expected_anchor_tuple(page_width, page_height, bounds) else {
                    receipt.exact_absent_cell_match_count += 1;
                    continue;
                };
                let matches = candidates
                    .iter()
                    .filter(|(_, anchor)| *anchor == expected)
                    .collect::<Vec<_>>();

                match matches.as_slice() {
                    [(shape, _)] => {
                        receipt.exact_unique_cell_match_count += 1;
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
                    [] => receipt.exact_absent_cell_match_count += 1,
                    _ => receipt.exact_ambiguous_cell_match_count += 1,
                }
            }
        }

        assert_eq!(
            receipt.exact_unique_cell_match_count
                + receipt.exact_ambiguous_cell_match_count
                + receipt.exact_absent_cell_match_count,
            receipt.table_cell_count,
            "every selected TABLE cell must be classified"
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
        schema: "chaptera.virginia-table-raw-spcontainer-probe.v1",
        source_sha256: actual_sha,
        pages,
        unresolved_dg_count,
        multi_page_dg_count,
        guardrails: vec![
            "OfficeArt Dg/SpContainer ancestry comes from the existing pub-escher parser; no byte scanner or second parser is introduced.",
            "A Dg is assigned to a Viewer page only when all already-grounded ClientData-linked shapes in that Dg resolve to one customer page.",
            "ClientAnchor comparison is the exact inverse of the already-shipped pub-reader page_relative_bounds law and admits equality only.",
            "Only ungrounded raw SpContainers are candidates; already-grounded shapes are counted as controls and never double-promoted.",
            "No PDF pixels, proximity matching, object ids, coordinates, text, filenames, property values or raw bytes are emitted.",
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
        "VIRGINIA_TABLE_RAW_SPCONTAINER p21={}/{}/{} p22={}/{}/{} p22_raw={} p22_ungrounded={} p22_anchor={} p23={}/{}/{} unresolved_dg={} multi_page_dg={}",
        receipt.pages[0].exact_unique_cell_match_count,
        receipt.pages[0].exact_ambiguous_cell_match_count,
        receipt.pages[0].exact_absent_cell_match_count,
        receipt.pages[1].exact_unique_cell_match_count,
        receipt.pages[1].exact_ambiguous_cell_match_count,
        receipt.pages[1].exact_absent_cell_match_count,
        receipt.pages[1].raw_spcontainer_count,
        receipt.pages[1].ungrounded_spcontainer_count,
        receipt.pages[1].ungrounded_complete_client_anchor_count,
        receipt.pages[2].exact_unique_cell_match_count,
        receipt.pages[2].exact_ambiguous_cell_match_count,
        receipt.pages[2].exact_absent_cell_match_count,
        receipt.unresolved_dg_count,
        receipt.multi_page_dg_count,
    );
}
