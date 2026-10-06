use pub_contents::{
    CONTENTS_RAW_TYPE_STORY_CATALOG, RawContentsBlockBody, parse_0x2c_header,
    parse_confirmed_0x2c_chunk, parse_confirmed_0x2c_trailer_root,
    parse_confirmed_chunk_reference, parse_confirmed_mature_story_catalog,
};
use pub_core::StreamPath;
use pub_model::Sha256Digest;
use pub_quill::{
    QuillMcldFieldValue, bounded_mcld_table_metrics, parse_bounded_mcld,
    parse_confirmed_story_catalog,
};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs;
use std::io::Cursor;
use std::path::PathBuf;

fn source_hash(bytes: &[u8]) -> Sha256Digest {
    let digest = Sha256::digest(bytes);
    let mut raw = [0_u8; 32];
    raw.copy_from_slice(&digest);
    Sha256Digest::from_bytes(raw)
}

fn scalar_value(body: &RawContentsBlockBody) -> Option<u32> {
    match body {
        RawContentsBlockBody::U16 { value, .. } => Some(u32::from(*value)),
        RawContentsBlockBody::U32 { value, .. } => Some(*value),
        _ => None,
    }
}

#[test]
#[ignore = "requires exact public Carlton March PUB path"]
fn exact_carlton_table_mcld_relation_candidates_are_source_safe() {
    let fixture = PathBuf::from(
        std::env::var_os("CHAPTERA_GOLDEN_CARLTON_MARCH")
            .expect("CHAPTERA_GOLDEN_CARLTON_MARCH"),
    );
    let bytes = fs::read(fixture).expect("read exact Carlton PUB");
    let build = pub_reader::build_mature_0x2c_source_graph(
        Cursor::new(bytes.as_slice()),
        source_hash(&bytes),
    )
    .expect("build exact Carlton graph");

    let tables = build
        .graph
        .nodes
        .values()
        .filter_map(|node| node.payload.table.as_ref())
        .collect::<Vec<_>>();
    assert_eq!(tables.len(), 1, "exact Carlton must expose one TABLE");
    let table = tables[0];
    assert_eq!(table.cells.len(), 17, "exact Carlton TABLE cell cohort");
    assert!(
        table.layout_metrics.is_none(),
        "probe is only for the missing current TABLE layout-key path"
    );

    let contents = pub_cfb::read_stream_reader(Cursor::new(bytes.as_slice()), "/Contents")
        .expect("read Carlton Contents");
    let contents_stream = StreamPath("/Contents".into());
    let header = parse_0x2c_header(contents_stream.clone(), &contents).expect("0x2c header");
    let trailer =
        parse_confirmed_0x2c_trailer_root(&contents, &header).expect("0x2c trailer");

    let mut catalogs = Vec::new();
    for seq_num in 0..trailer.directory.slots.len() {
        let Some(reference) =
            parse_confirmed_chunk_reference(&contents, &trailer.directory, seq_num)
                .expect("chunk reference")
        else {
            continue;
        };
        if !reference
            .raw_types
            .iter()
            .any(|field| field.value == CONTENTS_RAW_TYPE_STORY_CATALOG)
        {
            continue;
        }
        for offset in &reference.chunk_offsets {
            let chunk =
                parse_confirmed_0x2c_chunk(contents_stream.clone(), &contents, offset.value)
                    .expect("Story catalog chunk");
            catalogs.push(
                parse_confirmed_mature_story_catalog(&contents, &chunk)
                    .expect("mature Story catalog"),
            );
        }
    }
    assert_eq!(catalogs.len(), 1, "exact Carlton Story catalog cardinality");
    let entry = catalogs[0]
        .entries
        .iter()
        .find(|entry| entry.text_id == table.text_id)
        .expect("Carlton TABLE Story catalog entry");
    let layout_key = entry
        .layout_key
        .expect("Carlton TABLE Story catalog entry must retain its exact layout key");
    assert!(
        entry.layout_key_source.is_some(),
        "Carlton TABLE layout key must retain exact source provenance"
    );


    let quill = pub_cfb::read_stream_reader(
        Cursor::new(bytes.as_slice()),
        "/Quill/QuillSub/CONTENTS",
    )
    .expect("read Carlton Quill");
    let quill_stream = StreamPath("/Quill/QuillSub/CONTENTS".into());
    let stories =
        parse_confirmed_story_catalog(quill_stream.clone(), &quill).expect("Quill Story catalog");
    let mcld = parse_bounded_mcld(quill_stream, &quill, &stories.descriptor_nodes)
        .expect("bounded Carlton MCLD");
    let record_ids = mcld
        .records
        .iter()
        .map(|record| record.record_id)
        .collect::<BTreeSet<_>>();
    let layout_record_present = record_ids.contains(&layout_key);
    assert!(
        layout_record_present,
        "exact Carlton TABLE layout key must select one bounded MCLD record"
    );
    let record = mcld
        .records
        .iter()
        .find(|record| record.record_id == layout_key)
        .expect("layout-key-selected MCLD record");

    let classify_field = |field_id: u8| {
        let mut exact_single_u32_children = 0_usize;
        let mut missing_children = 0_usize;
        let mut duplicate_children = 0_usize;
        let mut wrong_type_children = 0_usize;
        let mut values = BTreeSet::new();

        for child in &record.children {
            let fields = child
                .fields
                .iter()
                .filter(|field| field.id == field_id)
                .collect::<Vec<_>>();
            match fields.as_slice() {
                [] => missing_children += 1,
                [field] => match field.value {
                    QuillMcldFieldValue::U32(value) if field.wire_type == 0x22 => {
                        exact_single_u32_children += 1;
                        values.insert(value);
                    }
                    _ => wrong_type_children += 1,
                },
                _ => duplicate_children += 1,
            }
        }

        (
            exact_single_u32_children,
            missing_children,
            duplicate_children,
            wrong_type_children,
            values.len(),
        )
    };

    let field04 = classify_field(0x04);
    let field05 = classify_field(0x05);
    let metrics_admitted = bounded_mcld_table_metrics(&mcld, layout_key).is_ok();

    let tcd = stories
        .tcd
        .iter()
        .filter(|tcd| tcd.story_syid.value.0 == table.text_id)
        .collect::<Vec<_>>();
    assert_eq!(tcd.len(), 1, "Carlton TABLE Story must have one exact TCD");

    eprintln!(
        "CARLTON_TABLE_MCLD_RELATION layout_key_source_present=true layout_record_present={} record_child_count={} child_count_matches_table={} field04_exact={} field04_missing={} field04_duplicate={} field04_wrong_type={} field04_distinct_classes={} field05_exact={} field05_missing={} field05_duplicate={} field05_wrong_type={} field05_distinct_classes={} metrics_admitted={} tcd_story_identity=true",
        layout_record_present,
        record.children.len(),
        record.children.len() == table.cells.len(),
        field04.0,
        field04.1,
        field04.2,
        field04.3,
        field04.4,
        field05.0,
        field05.1,
        field05.2,
        field05.3,
        field05.4,
        metrics_admitted,
    );

}
