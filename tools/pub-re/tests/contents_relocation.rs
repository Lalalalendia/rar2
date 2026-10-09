use cfb::CompoundFile;
use pub_re::{
    EXPERIMENT_SCHEMA_V1, ExperimentInputV1, ExperimentManifestV1, ExperimentPolicyV1,
    attribute_contents_manifest,
};
use std::{
    fs,
    io::{Cursor, Write},
};
use tempfile::TempDir;

fn contents(path: &str, width: u32, parent: u32, broken_offset: bool) -> Vec<u8> {
    let mut bytes = vec![0; 0x1e];
    bytes[..4].copy_from_slice(&pub_contents::CONTENTS_0X2C_MAGIC);
    for unit in path.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    let chunk_offset = bytes.len() as u32;
    bytes.extend_from_slice(&10_u32.to_le_bytes());
    bytes.extend_from_slice(&[0xaa, 0x20]);
    bytes.extend_from_slice(&width.to_le_bytes());
    let trailer_offset = bytes.len() as u32;
    bytes[0x1a..0x1e].copy_from_slice(&trailer_offset.to_le_bytes());

    let mut reference = vec![2, 0x18, 1, 0];
    reference.extend_from_slice(&[4, 0xb8]);
    reference.extend_from_slice(
        &if broken_offset {
            trailer_offset
        } else {
            chunk_offset
        }
        .to_le_bytes(),
    );
    reference.extend_from_slice(&[5, 0x68]);
    reference.extend_from_slice(&parent.to_le_bytes());

    let mut directory = vec![0, 0x78];
    directory.extend_from_slice(&[0, 0x88]);
    directory.extend_from_slice(&(reference.len() as u32 + 4).to_le_bytes());
    directory.extend_from_slice(&reference);

    let mut trailer = vec![0; 4];
    trailer.extend_from_slice(&[1, 0x20, 2, 0, 0, 0]);
    trailer.extend_from_slice(&[2, 0x20, 1, 0, 0, 0]);
    trailer.extend_from_slice(&[3, 0x90]);
    trailer.extend_from_slice(&(directory.len() as u32 + 4).to_le_bytes());
    trailer.extend_from_slice(&directory);
    let trailer_len = trailer.len() as u32;
    trailer[..4].copy_from_slice(&trailer_len.to_le_bytes());
    bytes.extend_from_slice(&trailer);
    bytes
}

fn compound(contents: &[u8]) -> Vec<u8> {
    let mut file = CompoundFile::create(Cursor::new(Vec::new())).unwrap();
    file.create_stream("/Contents")
        .unwrap()
        .write_all(contents)
        .unwrap();
    file.create_stream("/Stable")
        .unwrap()
        .write_all(b"stable")
        .unwrap();
    file.flush().unwrap();
    file.into_inner().into_inner()
}

fn pair(
    path_a: &str,
    path_b: &str,
    a: u32,
    b: u32,
    parent_a: u32,
    parent_b: u32,
    corrupt_b: bool,
) -> anyhow::Result<pub_re::ContentsDiffReceipt> {
    let dir = TempDir::new()?;
    fs::write(
        dir.path().join("a.pub"),
        compound(&contents(path_a, a, parent_a, false)),
    )?;
    fs::write(
        dir.path().join("b.pub"),
        compound(&contents(path_b, b, parent_b, corrupt_b)),
    )?;
    let m = ExperimentManifestV1 {
        schema: EXPERIMENT_SCHEMA_V1.into(),
        experiment_id: "synthetic-path-join".into(),
        question: "are referenced chunks actually changed?".into(),
        before: ExperimentInputV1 {
            path: "a.pub".into(),
            expected_sha256: None,
        },
        after: ExperimentInputV1 {
            path: "b.pub".into(),
            expected_sha256: None,
        },
        policy: ExperimentPolicyV1::default(),
    };
    let result = attribute_contents_manifest(&m, dir.path())?;
    let json = serde_json::to_string(&result)?;
    assert!(!json.contains(dir.path().to_string_lossy().as_ref()));
    assert!(!json.contains("stable"));
    assert!(!result.whole_pub_semantic_equality_claimed);
    Ok(result)
}

#[test]
fn relocated_chunks_remain_identical_in_byte_content() {
    let r = pair("x.pub", "arms/longer.pub", 100, 100, 40, 40, false).unwrap();
    assert_eq!(r.status, "referenced_chunks_unchanged");
    assert_eq!(r.compared_chunks, 1);
    assert_eq!(r.unchanged_chunks, 1);
    assert_eq!(r.changed_chunks, 0);
    let delta = 2 * ("arms/longer.pub".len() - "x.pub".len()) as i64;
    assert_eq!(r.trailer_offset_delta, delta);
    assert_eq!(r.offset_delta_histogram.get(&delta), Some(&1));
    assert_eq!(r.before_slot_count, 2);
    assert_eq!(r.after_slot_count, 2);
}

#[test]
fn equal_length_different_filename_is_not_false_change() {
    let r = pair("aaaaa.pub", "zzzzz.pub", 100, 100, 40, 40, false).unwrap();
    assert_ne!(r.before_sha256, r.after_sha256);
    assert_eq!(r.trailer_offset_delta, 0);
    assert_eq!(r.status, "referenced_chunks_unchanged");
}

#[test]
fn actual_payload_mutation_survives_offset_normalization() {
    let r = pair("x.pub", "arms/longer.pub", 100, 101, 40, 40, false).unwrap();
    assert_eq!(r.status, "referenced_chunk_payload_changed");
    assert_eq!(r.changed_chunks, 1);
    assert_eq!(r.changed_chunk_ordinals, vec![1]);
}

#[test]
fn changed_parent_is_not_hidden_as_an_offset() {
    let r = pair("x.pub", "arms/longer.pub", 100, 100, 40, 41, false).unwrap();
    assert_eq!(r.status, "reference_or_slot_changed");
    assert_eq!(r.changed_reference_ordinals, vec![1]);
    assert_eq!(r.unchanged_chunks, 1);
}

#[test]
fn corrupt_pointer_fails_closed() {
    let e = pair("x.pub", "y.pub", 100, 100, 40, 40, true).unwrap_err();
    assert!(e.to_string().contains("after not admissible"));
}

#[test]
fn no_payload_or_filename_bytes_are_serialized() {
    let r = pair("private-a.pub", "private-b.pub", 100, 100, 40, 40, false).unwrap();
    let s = serde_json::to_string(&r).unwrap();
    assert!(!s.contains("private-a"));
    assert!(!s.contains("private-b"));
    assert!(!r.raw_bytes_emitted);
    assert!(!r.absolute_paths_emitted);
}
