use chaptera_suite_handoff::{
    EDITOR_PRODUCT_ID, HandoffAcceptance, HandoffPacket, HandoffPacketAdmissionError as Error,
    HandoffPacketAdmissionV1 as Admission, RESCUE_PRODUCT_ID, create_reader_handoff,
    finish_acceptance, load_for_receiver, validate_packet, write_packet,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::PathBuf;

fn packet() -> HandoffPacket {
    serde_json::from_value(json!({
        "protocol_version": "chaptera.suite-handoff.v1",
        "sender_product_id": "chaptera.reader",
        "target_product_id": "chaptera.editor",
        "requested_job": "edit_supported_pub",
        "source": {"path": "synthetic-not-opened.pub", "sha256": "a".repeat(64), "kind": "pub"},
        "context": {"capability": "reader_supported", "loss_state": "none_observed"},
        "provenance": {"source_identity_verified": true, "mutable_document_state_included": false},
        "consent": {"local_file_handoff": true, "user_initiated": true}
    }))
    .unwrap()
}

fn encoded(value: &impl serde::Serialize) -> Vec<u8> {
    serde_json::to_vec(value).unwrap()
}

struct TempFiles(PathBuf, PathBuf);

impl TempFiles {
    fn new(name: &str) -> Self {
        let source = std::env::temp_dir().join(format!(
            "chaptera-packet-admission-{name}-{}.pub",
            std::process::id()
        ));
        let packet = source.with_extension("packet.json");
        Self(source, packet)
    }
}

impl Drop for TempFiles {
    fn drop(&mut self) {
        fs::remove_file(&self.0).ok();
        fs::remove_file(&self.1).ok();
    }
}

#[test]
fn exact_packet_byte_boundary_accepts_valid_json_and_plus_one_rejects() {
    let expected = packet();
    let mut bytes = encoded(&expected);
    assert!(bytes.len() < 1024, "measure actual small V1 control packet");
    bytes.resize(Admission::MAX_PACKET_BYTES, b' ');
    assert_eq!(Admission::decode(&bytes).unwrap(), expected);
    bytes.push(b' ');
    assert_eq!(
        Admission::decode(&bytes).unwrap_err(),
        Error::PacketSizeLimit
    );
}

#[test]
fn every_packet_string_has_an_exact_utf8_length_boundary() {
    for pointer in [
        "/protocol_version",
        "/sender_product_id",
        "/target_product_id",
        "/requested_job",
        "/source/path",
        "/source/sha256",
        "/source/kind",
        "/context/capability",
        "/context/loss_state",
    ] {
        let maximum = match pointer {
            "/source/path" => Admission::MAX_SOURCE_PATH_BYTES,
            "/source/sha256" => 64,
            _ => Admission::MAX_FIELD_BYTES,
        };
        let mut value = serde_json::to_value(packet()).unwrap();
        *value.pointer_mut(pointer).unwrap() = Value::String("a".repeat(maximum));
        Admission::decode(&encoded(&value)).unwrap();
        *value.pointer_mut(pointer).unwrap() = Value::String("a".repeat(maximum + 1));
        let error = Admission::decode(&encoded(&value)).unwrap_err();
        assert_eq!(
            error.code(),
            "handoff_packet_field_length_limit",
            "{pointer}"
        );
        assert!(error.to_string().contains(&pointer[1..].replace('/', ".")));
    }
    let mut unicode = packet();
    unicode.source.path = "資".repeat(Admission::MAX_SOURCE_PATH_BYTES / 3 + 1);
    assert_eq!(
        Admission::decode(&encoded(&unicode)).unwrap_err(),
        Error::FieldLengthLimit("source.path")
    );
}

#[test]
fn unicode_combining_and_fully_escaped_windows_long_path_headroom_is_preserved() {
    let mut expected = packet();
    expected.source.path = format!(
        "C:\\資料\\Издатель-cafe\u{301}\\{}publication.pub",
        "segment\\".repeat(50)
    );
    assert!(expected.source.path.len() > 260);
    assert_eq!(Admission::decode(&encoded(&expected)).unwrap(), expected);
    let mut value = serde_json::to_value(packet()).unwrap();
    value["source"]["path"] = Value::String("__ESCAPED_LONG_PATH__".to_owned());
    let escaped = String::from_utf8(encoded(&value))
        .unwrap()
        .replace("__ESCAPED_LONG_PATH__", &"\\u8cc7".repeat(32767));
    assert!(escaped.len() < Admission::MAX_PACKET_BYTES);
    assert_eq!(
        Admission::decode(escaped.as_bytes()).unwrap().source.path,
        "資".repeat(32767)
    );
}

#[test]
fn unknown_fields_at_every_v1_object_level_and_duplicates_are_rejected() {
    for pointer in ["", "/source", "/context", "/provenance", "/consent"] {
        let mut value = serde_json::to_value(packet()).unwrap();
        value
            .pointer_mut(pointer)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("unknown_v1_semantics".to_owned(), json!(true));
        assert_eq!(
            Admission::decode(&encoded(&value)).unwrap_err(),
            Error::InvalidJson,
            "{pointer}"
        );
    }
    let base = String::from_utf8(encoded(&packet())).unwrap();
    let duplicate = format!(
        "{{\"protocol_version\":\"chaptera.suite-handoff.v1\",{}",
        &base[1..]
    );
    assert_eq!(
        Admission::decode(duplicate.as_bytes()).unwrap_err(),
        Error::InvalidJson
    );
    let duplicate_source = base.replace("\"path\":", "\"path\":\"duplicate.pub\",\"path\":");
    assert_eq!(
        Admission::decode(duplicate_source.as_bytes()).unwrap_err(),
        Error::InvalidJson
    );
}

#[test]
fn malformed_truncated_deep_and_trailing_json_have_one_source_free_code() {
    let deep = format!("{}0{}", "[".repeat(256), "]".repeat(256));
    for bytes in [b"{".as_slice(), b"not JSON", &[0xff], deep.as_bytes()] {
        let error = Admission::decode(bytes).unwrap_err();
        assert_eq!(error, Error::InvalidJson);
        assert_eq!(error.to_string(), "handoff_packet_json_invalid");
    }
    let valid = encoded(&packet());
    assert_eq!(
        Admission::decode(&valid[..valid.len() - 1]).unwrap_err(),
        Error::InvalidJson
    );
    let mut trailing = valid;
    trailing.extend_from_slice(b" {}");
    assert_eq!(
        Admission::decode(&trailing).unwrap_err(),
        Error::InvalidJson
    );
}

#[test]
fn invalid_packet_and_wrong_receiver_fail_before_missing_source_is_opened() {
    let files = TempFiles::new("before-source");
    let mut value = serde_json::to_value(packet()).unwrap();
    value["source"]["path"] = json!(files.0.to_str().unwrap());
    value["source"]["unknown"] = json!(true);
    assert!(!files.0.exists());
    fs::write(&files.1, encoded(&value)).unwrap();
    assert_eq!(
        load_for_receiver(&files.1, EDITOR_PRODUCT_ID).unwrap_err(),
        "handoff_packet_json_invalid"
    );
    value["source"].as_object_mut().unwrap().remove("unknown");
    fs::write(&files.1, encoded(&value)).unwrap();
    assert!(
        load_for_receiver(&files.1, RESCUE_PRODUCT_ID)
            .unwrap_err()
            .contains("does not match receiver")
    );
}

#[test]
fn future_version_consent_provenance_and_route_validation_remain_fail_closed() {
    let files = TempFiles::new("semantic-gates");
    let mut future = packet();
    future.protocol_version = "chaptera.suite-handoff.v2".to_owned();
    fs::write(&files.1, encoded(&future)).unwrap();
    assert!(
        load_for_receiver(&files.1, EDITOR_PRODUCT_ID)
            .unwrap_err()
            .contains("unsupported suite handoff version")
    );
    let mut consent = packet();
    consent.consent.user_initiated = false;
    assert!(
        validate_packet(&consent)
            .unwrap_err()
            .contains("user intent")
    );
    let mut provenance = packet();
    provenance.provenance.mutable_document_state_included = true;
    assert!(
        validate_packet(&provenance)
            .unwrap_err()
            .contains("no mutable document state")
    );
    let mut job = packet();
    job.requested_job = "unexpected_job".to_owned();
    assert!(
        validate_packet(&job)
            .unwrap_err()
            .contains("context is inconsistent")
    );
}

#[test]
fn exact_admitted_packet_bytes_are_hashed_and_unicode_source_is_unchanged() {
    let files = TempFiles::new("資料-cafe\u{301}");
    let source = b"synthetic immutable handoff source";
    fs::write(&files.0, source).unwrap();
    let expected = create_reader_handoff(&files.0, EDITOR_PRODUCT_ID, true, false).unwrap();
    let mut bytes = encoded(&expected);
    bytes.extend_from_slice(b"\n \t");
    bytes.resize(Admission::MAX_PACKET_BYTES, b' ');
    fs::write(&files.1, &bytes).unwrap();
    let validated = load_for_receiver(&files.1, EDITOR_PRODUCT_ID).unwrap();
    assert_eq!(validated.packet(), &expected);
    let receipt = finish_acceptance(validated, true).unwrap();
    assert_eq!(
        receipt.handoff_packet_sha256,
        format!("{:x}", Sha256::digest(&bytes))
    );
    assert_eq!(fs::read(&files.0).unwrap(), source);
    assert!(receipt.source_unchanged);
    assert!(!receipt.source_path_serialized);
    let mut invalid_receipt = serde_json::to_value(&receipt).unwrap();
    invalid_receipt["unknown_v1_semantics"] = json!(true);
    assert!(serde_json::from_value::<HandoffAcceptance>(invalid_receipt).is_err());
}

#[test]
fn oversized_file_and_sender_fields_reject_before_source_or_output_mutation() {
    let files = TempFiles::new("oversize-file");
    let file = fs::File::create(&files.1).unwrap();
    file.set_len(Admission::MAX_PACKET_BYTES as u64 + 1)
        .unwrap();
    drop(file);
    assert_eq!(
        load_for_receiver(&files.1, EDITOR_PRODUCT_ID).unwrap_err(),
        "handoff_packet_size_limit"
    );
    fs::write(&files.1, b"existing destination must remain unchanged").unwrap();
    let mut too_long = packet();
    too_long.source.path = "a".repeat(Admission::MAX_SOURCE_PATH_BYTES + 1);
    assert!(
        write_packet(&too_long, &files.1)
            .unwrap_err()
            .starts_with("handoff_packet_field_length_limit")
    );
    assert_eq!(
        fs::read(&files.1).unwrap(),
        b"existing destination must remain unchanged"
    );
    let mut expands = packet();
    expands.source.path = "\u{1}".repeat(Admission::MAX_SOURCE_PATH_BYTES);
    assert_eq!(
        write_packet(&expands, &files.1).unwrap_err(),
        "handoff_packet_size_limit"
    );
    assert_eq!(
        fs::read(&files.1).unwrap(),
        b"existing destination must remain unchanged"
    );
}
