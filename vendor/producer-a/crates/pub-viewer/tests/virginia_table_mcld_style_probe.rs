use pub_core::StreamPath;
use pub_model::Sha256Digest;
use pub_quill::{parse_bounded_mcld, parse_confirmed_story_catalog};
use pub_reader::{PubBridgeDiagnostic, build_mature_0x2c_source_graph};
use pub_viewer::{open_pub_bundle, viewer_geometry_environment_v0_1};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    io::Cursor,
    path::PathBuf,
};

fn source_hash(bytes: &[u8]) -> Sha256Digest {
    let digest = Sha256::digest(bytes);
    let mut raw = [0_u8; 32];
    raw.copy_from_slice(&digest);
    Sha256Digest::from_bytes(raw)
}

fn bump(map: &mut BTreeMap<String, usize>, key: impl Into<String>) {
    *map.entry(key.into()).or_default() += 1;
}

fn field_key(id: u8, wire_type: u8) -> String {
    format!("0x{id:02x}/wire_0x{wire_type:02x}")
}

#[derive(Debug, Default, Serialize)]
struct PageReceipt {
    viewer_page_index: u32,
    table_count: usize,
    total_table_cells: usize,
    layout_key_present_table_count: usize,
    layout_key_missing_table_count: usize,
    mcld_record_present_table_count: usize,
    mcld_record_missing_table_count: usize,
    child_count_matches_cell_count: usize,
    child_count_mismatches_cell_count: usize,
    joined_mcld_child_count: usize,
    style_signature_class_histogram: BTreeMap<String, usize>,
    style_field_child_presence_histogram: BTreeMap<String, usize>,
    control_field_child_presence_histogram: BTreeMap<String, usize>,
}

#[derive(Debug, Serialize)]
struct Receipt {
    schema: &'static str,
    source_sha256: String,
    mcld_available: bool,
    pages: Vec<PageReceipt>,
    guardrails: Vec<&'static str>,
}

#[test]
#[ignore = "requires exact public Virginia Remplacante fixture and receipt path"]
fn exact_virginia_table_mcld_style_carrier_probe() {
    let fixture_env = env::var_os("CHAPTERA_VIRGINIA_TABLE_STYLE_FIXTURE");
    let output_env = env::var_os("CHAPTERA_VIRGINIA_TABLE_STYLE_OUT");
    if fixture_env.is_none() && output_env.is_none() {
        return;
    }
    let fixture = fixture_env
        .map(PathBuf::from)
        .expect("CHAPTERA_VIRGINIA_TABLE_STYLE_FIXTURE");
    let output = output_env
        .map(PathBuf::from)
        .expect("CHAPTERA_VIRGINIA_TABLE_STYLE_OUT");
    let expected_sha = env::var("CHAPTERA_VIRGINIA_TABLE_STYLE_SHA256")
        .expect("CHAPTERA_VIRGINIA_TABLE_STYLE_SHA256");

    let bytes = fs::read(&fixture).expect("read exact Virginia Remplacante PUB");
    let actual_sha = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    assert_eq!(actual_sha, expected_sha, "exact Virginia source identity");

    let source = build_mature_0x2c_source_graph(Cursor::new(bytes.as_slice()), source_hash(&bytes))
        .expect("build exact Virginia mature source graph");
    let bundle = open_pub_bundle(&bytes, viewer_geometry_environment_v0_1())
        .expect("open exact Virginia through Viewer bundle");
    assert_eq!(
        bundle.geometry.document.pages.len(),
        25,
        "bounded Virginia family profile must expose 25 customer pages"
    );

    let quill = pub_cfb::read_stream_reader(
        Cursor::new(bytes.as_slice()),
        pub_reader::QUILL_STREAM_PATH,
    )
    .expect("read exact Virginia Quill stream");
    let quill_stream = StreamPath(pub_reader::QUILL_STREAM_PATH.into());
    let catalog = parse_confirmed_story_catalog(quill_stream.clone(), &quill)
        .expect("parse exact Virginia Quill story catalog");
    let mcld = parse_bounded_mcld(quill_stream, &quill, &catalog.descriptor_nodes).ok();

    let mut layout_key_by_seq = BTreeMap::<u32, Option<u32>>::new();
    for diagnostic in &source.diagnostics {
        if let PubBridgeDiagnostic::TableLayoutMetricsUnavailable {
            seq_num,
            layout_key,
            ..
        } = diagnostic
        {
            layout_key_by_seq.insert(*seq_num, *layout_key);
        }
    }

    let mut pages = Vec::new();
    for viewer_page_index in [21_u32, 22_u32, 23_u32] {
        let page = bundle
            .geometry
            .document
            .pages
            .get((viewer_page_index - 1) as usize)
            .expect("selected Viewer page exists");
        let parent = page.id.into_canonical();
        let mut receipt = PageReceipt {
            viewer_page_index,
            ..PageReceipt::default()
        };

        for node in bundle
            .resolved_graph
            .nodes
            .values()
            .filter(|node| node.header.parent_id == parent)
        {
            let Some(table) = node.payload.table.as_ref() else {
                continue;
            };
            receipt.table_count += 1;
            receipt.total_table_cells += table.cells.len();

            let layout_key = table
                .layout_metrics
                .as_ref()
                .map(|metrics| metrics.story_layout_key)
                .or_else(|| {
                    layout_key_by_seq
                        .get(&node.payload.contents_seq_num)
                        .copied()
                        .flatten()
                });

            let Some(layout_key) = layout_key else {
                receipt.layout_key_missing_table_count += 1;
                bump(
                    &mut receipt.style_signature_class_histogram,
                    "layout_key_missing",
                );
                continue;
            };
            receipt.layout_key_present_table_count += 1;

            let Some(mcld) = mcld.as_ref() else {
                receipt.mcld_record_missing_table_count += 1;
                bump(
                    &mut receipt.style_signature_class_histogram,
                    "mcld_unavailable",
                );
                continue;
            };
            let Some(record) = mcld
                .records
                .iter()
                .find(|record| record.record_id == layout_key)
            else {
                receipt.mcld_record_missing_table_count += 1;
                bump(
                    &mut receipt.style_signature_class_histogram,
                    "mcld_record_missing",
                );
                continue;
            };
            receipt.mcld_record_present_table_count += 1;
            receipt.joined_mcld_child_count += record.children.len();
            if record.children.len() == table.cells.len() {
                receipt.child_count_matches_cell_count += 1;
            } else {
                receipt.child_count_mismatches_cell_count += 1;
            }

            let mut child_signatures = BTreeSet::<Vec<String>>::new();
            let mut any_style_field = false;
            for child in &record.children {
                let style_signature = child
                    .fields
                    .iter()
                    .filter(|field| (0x1d..=0x2c).contains(&field.id))
                    .map(|field| field_key(field.id, field.wire_type))
                    .collect::<BTreeSet<_>>();
                any_style_field |= !style_signature.is_empty();
                for key in &style_signature {
                    bump(
                        &mut receipt.style_field_child_presence_histogram,
                        key.clone(),
                    );
                }
                child_signatures.insert(style_signature.into_iter().collect());

                let control_signature = child
                    .fields
                    .iter()
                    .filter(|field| (0x04..=0x09).contains(&field.id))
                    .map(|field| field_key(field.id, field.wire_type))
                    .collect::<BTreeSet<_>>();
                for key in control_signature {
                    bump(&mut receipt.control_field_child_presence_histogram, key);
                }
            }

            let signature_class = if !any_style_field {
                "style_range_absent"
            } else if child_signatures.len() == 1 {
                "style_range_uniform"
            } else {
                "style_range_varies_by_child"
            };
            bump(
                &mut receipt.style_signature_class_histogram,
                signature_class,
            );
        }

        pages.push(receipt);
    }

    let p22 = pages
        .iter()
        .find(|page| page.viewer_page_index == 22)
        .expect("p22 receipt");
    assert_eq!(p22.table_count, 3, "p22 exact TABLE cohort");

    let receipt = Receipt {
        schema: "chaptera.virginia-table-mcld-style-carrier-probe.v1",
        source_sha256: actual_sha,
        mcld_available: mcld.is_some(),
        pages,
        guardrails: vec![
            "MCLD joins use the existing TABLE story-layout key authority; no byte-pattern scan is used.",
            "The open 0x1D..0x2C range is reported only as field-id/wire-type presence and uniformity; no Publisher border/fill semantics are assigned.",
            "Confirmed 0x04..0x09 fields are retained only as join controls.",
            "No field values, RGB colors, widths, style ordinals, cell coordinates, text, object ids, offsets, filenames, or raw bytes are emitted.",
            "Publisher PDF is not used as semantic authority.",
        ],
    };

    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).expect("create MCLD style receipt directory");
    }
    fs::write(
        &output,
        serde_json::to_vec_pretty(&receipt).expect("serialize MCLD style receipt"),
    )
    .expect("write MCLD style receipt");

    println!(
        "VIRGINIA_TABLE_MCLD_STYLE p21_tables={} p22_tables={} p23_tables={} p22_layout_keys={} p22_mcld_records={} p22_child_match={} p22_style_classes={:?}",
        receipt.pages[0].table_count,
        receipt.pages[1].table_count,
        receipt.pages[2].table_count,
        receipt.pages[1].layout_key_present_table_count,
        receipt.pages[1].mcld_record_present_table_count,
        receipt.pages[1].child_count_matches_cell_count,
        receipt.pages[1].style_signature_class_histogram,
    );
}
