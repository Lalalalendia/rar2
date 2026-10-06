use pub_contents::{
    CONTENTS_RAW_TYPE_STORY_CATALOG, RawContentsBlockBody, parse_0x2c_header,
    parse_confirmed_0x2c_chunk, parse_confirmed_0x2c_trailer_root,
    parse_confirmed_chunk_reference, parse_confirmed_mature_story_catalog,
};
use pub_core::StreamPath;
use pub_model::Sha256Digest;
use pub_quill::{parse_bounded_mcld, parse_confirmed_story_catalog};
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
    assert_eq!(
        entry.layout_key, None,
        "this probe starts from the exact missing 0x07 relation"
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

    let matching_entry_fields = entry
        .fields
        .iter()
        .filter_map(|field| {
            let value = scalar_value(&field.body)?;
            record_ids
                .contains(&value)
                .then(|| format!("0x{:02x}/wire_0x{:02x}", field.id, field.block_type))
        })
        .collect::<BTreeSet<_>>();

    let tcd = stories
        .tcd
        .iter()
        .filter(|tcd| tcd.story_syid.value.0 == table.text_id)
        .collect::<Vec<_>>();
    assert_eq!(tcd.len(), 1, "Carlton TABLE Story must have one exact TCD");
    let tcd = tcd[0];

    let descriptor = stories
        .descriptor_nodes
        .iter()
        .flat_map(|node| node.descriptors.iter())
        .filter(|descriptor| descriptor.source == tcd.descriptor_source)
        .collect::<Vec<_>>();
    assert_eq!(descriptor.len(), 1, "TCD descriptor identity must be unique");
    let descriptor = descriptor[0];

    let candidates = [
        ("tcd_story_ordinal", u32::from(tcd.story_ordinal.value)),
        ("tcd_header_word_1", tcd.header_word_1.value),
        ("tcd_header_word_2", tcd.header_word_2.value),
        ("tcd_descriptor_option_a", u32::from(descriptor.option_a.value)),
        ("tcd_descriptor_option_b", u32::from(descriptor.option_b.value)),
        ("tcd_descriptor_option_c", u32::from(descriptor.option_c.value)),
    ];
    let matching_tcd_candidates = candidates
        .iter()
        .filter_map(|(name, value)| record_ids.contains(value).then_some(*name))
        .collect::<BTreeSet<_>>();

    let child_count_matches = mcld
        .records
        .iter()
        .filter(|record| usize::try_from(record.child_count.value).ok() == Some(table.cells.len()))
        .count();

    let matched_record_child_count_classes = candidates
        .iter()
        .filter_map(|(name, value)| {
            let record = mcld.records.iter().find(|record| record.record_id == *value)?;
            Some(format!(
                "{name}:child_count_matches_table={}",
                usize::try_from(record.child_count.value).ok() == Some(table.cells.len())
            ))
        })
        .collect::<BTreeSet<_>>();

    eprintln!(
        "CARLTON_TABLE_MCLD_RELATION entry_field_presence={:?} matching_entry_fields={:?} tcd_match_labels={:?} matched_record_child_classes={:?} child_count_match_record_count={} tcd_story_identity=true tcd_descriptor_identity=true",
        entry
            .fields
            .iter()
            .map(|field| format!("0x{:02x}/wire_0x{:02x}", field.id, field.block_type))
            .collect::<BTreeSet<_>>(),
        matching_entry_fields,
        matching_tcd_candidates,
        matched_record_child_count_classes,
        child_count_matches,
    );
}
