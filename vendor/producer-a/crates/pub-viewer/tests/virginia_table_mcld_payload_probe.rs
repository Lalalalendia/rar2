use pub_core::StreamPath;
use pub_model::Sha256Digest;
use pub_quill::{QuillMcldFieldValue, QuillMcldReadError, parse_bounded_mcld, parse_confirmed_story_catalog};
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

#[derive(Debug, Default, Serialize)]
struct PageReceipt {
    viewer_page_index: u32,
    table_count: usize,
    joined_child_count: usize,
    opaque_1d_single_child_count: usize,
    payload_class_histogram: BTreeMap<String, usize>,
    distinct_payload_count_histogram: BTreeMap<String, usize>,
    payload_length_histogram: BTreeMap<String, usize>,
}

#[derive(Debug, Serialize)]
struct Receipt {
    schema: &'static str,
    source_sha256: String,
    pages: Vec<PageReceipt>,
    guardrails: Vec<&'static str>,
}

#[test]
#[ignore = "requires exact public Virginia Remplacante fixture and receipt path"]
fn exact_virginia_table_mcld_opaque_payload_probe() {
    let fixture = env::var_os("CHAPTERA_VIRGINIA_TABLE_PAYLOAD_FIXTURE")
        .map(PathBuf::from)
        .expect("CHAPTERA_VIRGINIA_TABLE_PAYLOAD_FIXTURE");
    let output = env::var_os("CHAPTERA_VIRGINIA_TABLE_PAYLOAD_OUT")
        .map(PathBuf::from)
        .expect("CHAPTERA_VIRGINIA_TABLE_PAYLOAD_OUT");
    let expected_sha = env::var("CHAPTERA_VIRGINIA_TABLE_PAYLOAD_SHA256")
        .expect("CHAPTERA_VIRGINIA_TABLE_PAYLOAD_SHA256");

    let bytes = fs::read(&fixture).expect("read exact Virginia Remplacante PUB");
    let actual_sha = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    assert_eq!(actual_sha, expected_sha, "exact Virginia source identity");

    let source = build_mature_0x2c_source_graph(
        Cursor::new(bytes.as_slice()),
        source_hash(&bytes),
    )
    .expect("build exact Virginia mature source graph");
    let bundle = open_pub_bundle(&bytes, viewer_geometry_environment_v0_1())
        .expect("open exact Virginia through Viewer bundle");
    assert_eq!(bundle.geometry.document.pages.len(), 25);

    let quill = pub_cfb::read_stream_reader(
        Cursor::new(bytes.as_slice()),
        pub_reader::QUILL_STREAM_PATH,
    )
    .expect("read exact Virginia Quill stream");
    let quill_stream = StreamPath(pub_reader::QUILL_STREAM_PATH.into());
    let catalog = parse_confirmed_story_catalog(quill_stream.clone(), &quill)
        .expect("parse exact Virginia Quill story catalog");
    let mcld = match parse_bounded_mcld(quill_stream, &quill, &catalog.descriptor_nodes) {
        Ok(mcld) => mcld,
        Err(QuillMcldReadError::MissingMcldDescriptor) => {
            panic!("exact Virginia source must contain bounded MCLD")
        }
        Err(error) => panic!("parse exact Virginia MCLD: {error}"),
    };

    let mut diagnostic_layout_key_by_seq = BTreeMap::<u32, Option<u32>>::new();
    for diagnostic in &source.diagnostics {
        if let PubBridgeDiagnostic::TableLayoutMetricsUnavailable {
            seq_num,
            layout_key,
            ..
        } = diagnostic
        {
            diagnostic_layout_key_by_seq.insert(*seq_num, *layout_key);
        }
    }

    let mut pages = Vec::new();
    for viewer_page_index in [21_u32, 22, 23] {
        let page = &bundle.geometry.document.pages[(viewer_page_index - 1) as usize];
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

            let layout_key = table
                .layout_metrics
                .as_ref()
                .map(|metrics| metrics.story_layout_key)
                .or_else(|| {
                    diagnostic_layout_key_by_seq
                        .get(&node.payload.contents_seq_num)
                        .copied()
                        .flatten()
                })
                .expect("selected TABLE has unique story layout key");
            let record = mcld
                .records
                .iter()
                .find(|record| record.record_id == layout_key)
                .expect("selected TABLE layout key joins MCLD record");
            assert_eq!(
                record.children.len(),
                table.cells.len(),
                "selected TABLE has one MCLD child per cell"
            );
            receipt.joined_child_count += record.children.len();

            let mut distinct_payloads = BTreeSet::<Vec<u8>>::new();
            let mut single_count = 0usize;
            for child in &record.children {
                let payloads = child
                    .fields
                    .iter()
                    .filter(|field| field.id == 0x1d && field.wire_type == 0x8a)
                    .filter_map(|field| match &field.value {
                        QuillMcldFieldValue::OpaqueNested(payload) => Some(payload),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                if let [payload] = payloads.as_slice() {
                    single_count += 1;
                    distinct_payloads.insert((*payload).clone());
                    bump(
                        &mut receipt.payload_length_histogram,
                        format!("bytes={}", payload.len()),
                    );
                }
            }
            receipt.opaque_1d_single_child_count += single_count;

            let class = if single_count == 0 {
                "absent_or_non_single"
            } else if single_count != record.children.len() {
                "partial_or_ambiguous"
            } else if distinct_payloads.len() == 1 {
                "uniform"
            } else {
                "varies_by_child"
            };
            bump(&mut receipt.payload_class_histogram, class);
            bump(
                &mut receipt.distinct_payload_count_histogram,
                format!("distinct={}", distinct_payloads.len()),
            );
        }

        pages.push(receipt);
    }

    let p22 = pages
        .iter()
        .find(|page| page.viewer_page_index == 22)
        .expect("p22 receipt");
    assert_eq!(p22.table_count, 3, "p22 exact TABLE cohort");
    assert_eq!(p22.joined_child_count, 110, "p22 exact MCLD child cohort");

    let receipt = Receipt {
        schema: "chaptera.virginia-table-mcld-opaque-payload-probe.v1",
        source_sha256: actual_sha,
        pages,
        guardrails: vec![
            "TABLE to MCLD joins reuse the existing story-layout key authority.",
            "Only equality class, distinct payload count, and byte-length histograms are emitted for 0x1D/wire0x8A opaque nested payloads.",
            "No payload bytes, hashes, nested scalar values, RGB values, border sides, semantic field names, coordinates, text, object ids, offsets, or filenames are emitted.",
            "Historical numeric analogies are not promoted to Publisher table-style semantics.",
            "Publisher PDF is not used as semantic authority.",
        ],
    };

    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).expect("create MCLD payload receipt directory");
    }
    fs::write(
        &output,
        serde_json::to_vec_pretty(&receipt).expect("serialize MCLD payload receipt"),
    )
    .expect("write MCLD payload receipt");

    println!(
        "VIRGINIA_TABLE_MCLD_PAYLOAD p21={:?} p22={:?} p23={:?} p22_distinct={:?} p22_lengths={:?}",
        receipt.pages[0].payload_class_histogram,
        receipt.pages[1].payload_class_histogram,
        receipt.pages[2].payload_class_histogram,
        receipt.pages[1].distinct_payload_count_histogram,
        receipt.pages[1].payload_length_histogram,
    );
}
