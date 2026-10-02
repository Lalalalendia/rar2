use pub_model::Sha256Digest;
use pub_reader::{
    PubTableMcldOpaque1dClass, PubTableMcldStyleSignatureClass, PubTableOfficeArtOwnerJoinClass,
    analyze_mature_0x2c_table_default_style_fields, analyze_mature_0x2c_table_mcld_style_fields,
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

fn source_hash(bytes: &[u8]) -> Sha256Digest {
    let digest = Sha256::digest(bytes);
    let mut raw = [0_u8; 32];
    raw.copy_from_slice(&digest);
    Sha256Digest::from_bytes(raw)
}

fn bump(map: &mut BTreeMap<String, usize>, key: impl Into<String>) {
    *map.entry(key.into()).or_default() += 1;
}

fn signature_class(value: PubTableMcldStyleSignatureClass) -> &'static str {
    match value {
        PubTableMcldStyleSignatureClass::LayoutKeyMissing => "layout_key_missing",
        PubTableMcldStyleSignatureClass::McldUnavailable => "mcld_unavailable",
        PubTableMcldStyleSignatureClass::RecordMissing => "mcld_record_missing",
        PubTableMcldStyleSignatureClass::StyleRangeAbsent => "style_range_absent",
        PubTableMcldStyleSignatureClass::StyleRangeUniform => "style_range_uniform",
        PubTableMcldStyleSignatureClass::StyleRangeVariesByChild => "style_range_varies_by_child",
    }
}

fn officeart_owner_join_class(value: PubTableOfficeArtOwnerJoinClass) -> &'static str {
    match value {
        PubTableOfficeArtOwnerJoinClass::Missing => "missing",
        PubTableOfficeArtOwnerJoinClass::Unique => "unique",
        PubTableOfficeArtOwnerJoinClass::Ambiguous => "ambiguous",
    }
}

fn opaque_1d_class(value: PubTableMcldOpaque1dClass) -> &'static str {
    match value {
        PubTableMcldOpaque1dClass::AbsentOrNonSingle => "absent_or_non_single",
        PubTableMcldOpaque1dClass::PartialOrAmbiguous => "partial_or_ambiguous",
        PubTableMcldOpaque1dClass::Uniform => "uniform",
        PubTableMcldOpaque1dClass::VariesByChild => "varies_by_child",
    }
}

fn owner_join_class(value: PubTableOfficeArtOwnerJoinClass) -> &'static str {
    match value {
        PubTableOfficeArtOwnerJoinClass::Missing => "missing",
        PubTableOfficeArtOwnerJoinClass::Unique => "unique",
        PubTableOfficeArtOwnerJoinClass::Ambiguous => "ambiguous",
    }
}

fn presence_signature(values: &BTreeSet<String>) -> String {
    if values.is_empty() {
        "absent".to_owned()
    } else {
        values.iter().cloned().collect::<Vec<_>>().join(",")
    }
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
    opaque_1d_class_histogram: BTreeMap<String, usize>,
    opaque_1d_single_child_count: usize,
    opaque_1d_distinct_payload_count_histogram: BTreeMap<String, usize>,
    opaque_1d_payload_length_histogram: BTreeMap<String, usize>,
    table_field_presence_histogram: BTreeMap<String, usize>,
    table_candidate_field_presence_histogram: BTreeMap<String, usize>,
    table_candidate_signature_histogram: BTreeMap<String, usize>,
    table_unsupported_tail_count: usize,
    default_table_field_signature_histogram: BTreeMap<String, usize>,
    default_table_tail_signature_histogram: BTreeMap<String, usize>,
    officeart_owner_join_histogram: BTreeMap<String, usize>,
    owner_fopt_signature_histogram: BTreeMap<String, usize>,
    owner_paint_family_signature_histogram: BTreeMap<String, usize>,
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

    let observations = analyze_mature_0x2c_table_mcld_style_fields(
        Cursor::new(bytes.as_slice()),
        source_hash(&bytes),
    )
    .expect("observe exact Virginia TABLE MCLD style field presence");
    let observation_by_seq = observations
        .into_iter()
        .map(|observation| (observation.contents_seq_num, observation))
        .collect::<BTreeMap<_, _>>();

    let default_style_observations = analyze_mature_0x2c_table_default_style_fields(
        Cursor::new(bytes.as_slice()),
        source_hash(&bytes),
    )
    .expect("observe exact Virginia TABLE default-style field presence");
    let default_style_by_seq = default_style_observations
        .into_iter()
        .map(|observation| (observation.contents_seq_num, observation))
        .collect::<BTreeMap<_, _>>();

    let bundle = open_pub_bundle(&bytes, viewer_geometry_environment_v0_1())
        .expect("open exact Virginia through Viewer bundle");
    assert_eq!(
        bundle.geometry.document.pages.len(),
        25,
        "bounded Virginia family profile must expose 25 customer pages"
    );

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

            let observation = observation_by_seq
                .get(&node.payload.contents_seq_num)
                .expect("selected TABLE must have a source MCLD observation");
            receipt.layout_key_present_table_count += usize::from(observation.layout_key_present);
            receipt.layout_key_missing_table_count += usize::from(!observation.layout_key_present);
            receipt.mcld_record_present_table_count += usize::from(observation.mcld_record_present);
            receipt.mcld_record_missing_table_count +=
                usize::from(observation.layout_key_present && !observation.mcld_record_present);
            receipt.child_count_matches_cell_count +=
                usize::from(observation.child_count_matches_cell_count);
            receipt.child_count_mismatches_cell_count += usize::from(
                observation.mcld_record_present && !observation.child_count_matches_cell_count,
            );
            receipt.joined_mcld_child_count += observation.mcld_child_count;
            bump(
                &mut receipt.style_signature_class_histogram,
                signature_class(observation.signature_class),
            );

            for (key, count) in &observation.style_field_child_presence {
                *receipt
                    .style_field_child_presence_histogram
                    .entry(key.clone())
                    .or_default() += count;
            }
            for (key, count) in &observation.control_field_child_presence {
                *receipt
                    .control_field_child_presence_histogram
                    .entry(key.clone())
                    .or_default() += count;
            }

            for (key, count) in &observation.table_field_presence {
                *receipt
                    .table_field_presence_histogram
                    .entry(key.clone())
                    .or_default() += count;
            }
            for (key, count) in &observation.table_candidate_field_presence {
                *receipt
                    .table_candidate_field_presence_histogram
                    .entry(key.clone())
                    .or_default() += count;
            }
            let candidate_signature = if observation.table_candidate_field_presence.is_empty() {
                "none".to_owned()
            } else {
                observation
                    .table_candidate_field_presence
                    .iter()
                    .map(|(key, count)| format!("{key}x{count}"))
                    .collect::<Vec<_>>()
                    .join(",")
            };
            bump(
                &mut receipt.table_candidate_signature_histogram,
                candidate_signature,
            );
            receipt.table_unsupported_tail_count +=
                usize::from(observation.table_unsupported_tail_present);

            let default_style = default_style_by_seq
                .get(&node.payload.contents_seq_num)
                .expect("selected TABLE must have a TABLE-level/default-style observation");
            let set_signature = |items: &std::collections::BTreeSet<String>| {
                if items.is_empty() {
                    "none".to_owned()
                } else {
                    items.iter().cloned().collect::<Vec<_>>().join(",")
                }
            };
            bump(
                &mut receipt.table_default_decoded_signature_histogram,
                set_signature(&default_style.table_field_presence),
            );
            bump(
                &mut receipt.table_default_tail_signature_histogram,
                set_signature(&default_style.table_tail_field_presence),
            );
            bump(
                &mut receipt.table_officeart_owner_join_histogram,
                officeart_owner_join_class(default_style.officeart_owner_join_class),
            );
            bump(
                &mut receipt.table_owner_fopt_signature_histogram,
                set_signature(&default_style.owner_fopt_property_presence),
            );
            bump(
                &mut receipt.table_owner_paint_family_signature_histogram,
                set_signature(&default_style.owner_paint_family_property_presence),
            );

            if let Some(class) = observation.opaque_1d_class {
                bump(
                    &mut receipt.opaque_1d_class_histogram,
                    opaque_1d_class(class),
                );
            }
            receipt.opaque_1d_single_child_count += observation.opaque_1d_single_child_count;
            if observation.opaque_1d_class.is_some() {
                bump(
                    &mut receipt.opaque_1d_distinct_payload_count_histogram,
                    format!("distinct={}", observation.opaque_1d_distinct_payload_count),
                );
            }
            for (length, count) in &observation.opaque_1d_payload_length_histogram {
                *receipt
                    .opaque_1d_payload_length_histogram
                    .entry(format!("bytes={length}"))
                    .or_default() += count;
            }

            let default_style = default_style_by_seq
                .get(&node.payload.contents_seq_num)
                .expect("selected TABLE must have a default-style observation");
            bump(
                &mut receipt.default_table_field_signature_histogram,
                presence_signature(&default_style.table_field_presence),
            );
            bump(
                &mut receipt.default_table_tail_signature_histogram,
                presence_signature(&default_style.table_tail_field_presence),
            );
            bump(
                &mut receipt.officeart_owner_join_histogram,
                owner_join_class(default_style.officeart_owner_join_class),
            );
            bump(
                &mut receipt.owner_fopt_signature_histogram,
                presence_signature(&default_style.owner_fopt_property_presence),
            );
            bump(
                &mut receipt.owner_paint_family_signature_histogram,
                presence_signature(&default_style.owner_paint_family_property_presence),
            );
        }

        pages.push(receipt);
    }

    let p22 = pages
        .iter()
        .find(|page| page.viewer_page_index == 22)
        .expect("p22 receipt");
    assert_eq!(p22.table_count, 3, "p22 exact TABLE cohort");
    assert_eq!(
        p22.joined_mcld_child_count, 110,
        "p22 exact MCLD child cohort"
    );

    let receipt = Receipt {
        schema: "chaptera.virginia-table-mcld-style-carrier-probe.v4",
        source_sha256: actual_sha,
        pages,
        guardrails: vec![
            "MCLD joins use the existing TABLE story-layout key authority; no byte-pattern scan is used.",
            "The open 0x1D..0x2C range is reported only as field-id/wire-type presence and uniformity; no Publisher border/fill semantics are assigned.",
            "For 0x1D/wire0x8A opaque nested state, only equality class, distinct-count, and byte-length histograms are emitted; no payload bytes or hashes leave the probe.",
            "Confirmed 0x04..0x09 fields are retained only as join controls.",
            "TABLE-level Contents state is emitted only as field-id/block-type presence; known identity/topology/geometry fields are separated from the open candidate signature and no values are emitted.",
            "TABLE owner OfficeArt state is emitted only as property-id presence and paint-family membership; no property values or color interpretation are emitted.",
            "No field values, RGB colors, widths, style ordinals, cell coordinates, text, object ids, offsets, filenames, or raw bytes are emitted.",
            "TABLE/default-style census emits only field-id/wire-type and OfficeArt FOPT property-id presence signatures; no property values or selector ordinals are emitted.",
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
        "VIRGINIA_TABLE_MCLD_STYLE p21_tables={} p22_tables={} p23_tables={} p22_layout_keys={} p22_mcld_records={} p22_child_match={} p22_style_classes={:?} p22_opaque_classes={:?} p22_distinct={:?} p22_lengths={:?} p22_table_candidates={:?} p22_table_signatures={:?} p22_unsupported_tail={} p22_default_fields={:?} p22_default_tail={:?} p22_owner_join={:?} p22_owner_fopt={:?} p22_owner_paint_family={:?}",
        receipt.pages[0].table_count,
        receipt.pages[1].table_count,
        receipt.pages[2].table_count,
        receipt.pages[1].layout_key_present_table_count,
        receipt.pages[1].mcld_record_present_table_count,
        receipt.pages[1].child_count_matches_cell_count,
        receipt.pages[1].style_signature_class_histogram,
        receipt.pages[1].opaque_1d_class_histogram,
        receipt.pages[1].opaque_1d_distinct_payload_count_histogram,
        receipt.pages[1].opaque_1d_payload_length_histogram,
        receipt.pages[1].table_candidate_field_presence_histogram,
        receipt.pages[1].table_candidate_signature_histogram,
        receipt.pages[1].table_unsupported_tail_count,
        receipt.pages[1].default_table_field_signature_histogram,
        receipt.pages[1].default_table_tail_signature_histogram,
        receipt.pages[1].officeart_owner_join_histogram,
        receipt.pages[1].owner_fopt_signature_histogram,
        receipt.pages[1].owner_paint_family_signature_histogram,
    );
}
