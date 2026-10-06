use pub_core::StreamPath;
use pub_quill::{
    QuillMcldChild, QuillMcldFieldValue, parse_bounded_mcld, parse_confirmed_story_catalog,
};
use std::collections::BTreeMap;
use std::io::Cursor;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FieldState {
    Value(u32),
    Missing,
    Duplicate,
    WrongType,
}

fn field_state(child: &QuillMcldChild, field_id: u8) -> FieldState {
    let mut fields = child.fields.iter().filter(|field| field.id == field_id);
    let Some(first) = fields.next() else {
        return FieldState::Missing;
    };
    if fields.next().is_some() {
        return FieldState::Duplicate;
    }
    match first.value {
        QuillMcldFieldValue::U32(value) => FieldState::Value(value),
        _ => FieldState::WrongType,
    }
}

#[test]
#[ignore = "requires exact public Carlton March PUB path"]
fn exact_carlton_mcld_per_child_inset_profile_is_source_safe() {
    let path = std::env::var_os("CHAPTERA_GOLDEN_CARLTON_MARCH")
        .map(PathBuf::from)
        .expect("CHAPTERA_GOLDEN_CARLTON_MARCH");

    let pub_bytes = std::fs::read(path).expect("read exact Carlton PUB");
    let quill = pub_cfb::read_stream_reader(
        Cursor::new(pub_bytes.as_slice()),
        "/Quill/QuillSub/CONTENTS",
    )
    .expect("read Carlton Quill stream");
    let stream = StreamPath("/Quill/QuillSub/CONTENTS".into());
    let stories =
        parse_confirmed_story_catalog(stream.clone(), &quill).expect("parse Carlton Story catalog");
    let mcld =
        parse_bounded_mcld(stream, &quill, &stories.descriptor_nodes).expect("parse Carlton MCLD");

    let mut multi_child_records = 0_usize;
    eprintln!("CARLTON_MCLD_CELL_INSET_PROFILE_BEGIN");

    for record in mcld.records.iter().filter(|record| record.children.len() > 1) {
        multi_child_records += 1;
        let mut complete_children = 0_usize;
        let mut symmetric_children = 0_usize;
        let mut asymmetric_children = 0_usize;
        let mut missing_field_children = 0_usize;
        let mut duplicate_field_children = 0_usize;
        let mut wrong_type_children = 0_usize;
        let mut symmetric_values = BTreeMap::<u32, usize>::new();

        for child in &record.children {
            let states = [
                field_state(child, 0x06),
                field_state(child, 0x07),
                field_state(child, 0x08),
                field_state(child, 0x09),
            ];

            if states.iter().any(|state| *state == FieldState::Missing) {
                missing_field_children += 1;
                continue;
            }
            if states.iter().any(|state| *state == FieldState::Duplicate) {
                duplicate_field_children += 1;
                continue;
            }
            if states.iter().any(|state| *state == FieldState::WrongType) {
                wrong_type_children += 1;
                continue;
            }

            let values = states.map(|state| match state {
                FieldState::Value(value) => value,
                _ => unreachable!("non-value state escaped completeness fence"),
            });
            complete_children += 1;

            if values.iter().all(|value| *value == values[0]) {
                symmetric_children += 1;
                *symmetric_values.entry(values[0]).or_default() += 1;
            } else {
                asymmetric_children += 1;
            }
        }

        eprintln!(
            "MCLD_CELL_INSET_PROFILE record_id={} children={} complete={} symmetric={} asymmetric={} missing={} duplicate={} wrong_type={} symmetric_values={:?}",
            record.record_id,
            record.children.len(),
            complete_children,
            symmetric_children,
            asymmetric_children,
            missing_field_children,
            duplicate_field_children,
            wrong_type_children,
            symmetric_values,
        );
    }

    eprintln!(
        "MCLD_CELL_INSET_SUMMARY multi_child_records={}",
        multi_child_records
    );
    eprintln!("CARLTON_MCLD_CELL_INSET_PROFILE_END");

    assert!(
        multi_child_records > 0,
        "Carlton must expose at least one multi-child MCLD record for this classifier"
    );
}
