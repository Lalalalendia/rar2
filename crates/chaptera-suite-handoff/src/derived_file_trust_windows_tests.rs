use crate::{AdmittedSource, DerivedFileTrustDisposition};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

fn root(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "chaptera-derived-trust-windows-{label}-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir_all(&root).unwrap();
    root
}

fn ads(path: &Path) -> PathBuf {
    PathBuf::from(format!("{}:Zone.Identifier", path.display()))
}

fn emit_receipt(case: &str, source_hash: &str, output_hash: &str, facts: serde_json::Value) {
    let Some(root) = std::env::var_os("CHAPTERA_DERIVED_TRUST_PROOF_DIR") else {
        return;
    };
    let commit = std::env::var("CHAPTERA_CANDIDATE_SHA").expect("candidate SHA for hosted proof");
    let receipt = json!({
        "schema": "chaptera.win-derived-file-trust.v1",
        "case": case,
        "git_commit": commit,
        "source_sha256": source_hash,
        "output_sha256": output_hash,
        "source_bytes_serialized": false,
        "source_path_serialized": false,
        "facts": facts,
    });
    fs::write(
        PathBuf::from(root).join(format!("{case}.json")),
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();
}

#[test]
fn internet_referrer_propagates_zone_without_mutating_source() {
    let root = root("internet");
    let source_path = root.join("internet.pub");
    let source_bytes = b"publisher-internet-source";
    fs::write(&source_path, source_bytes).unwrap();
    fs::write(
        ads(&source_path),
        b"[ZoneTransfer]\r\nZoneId=3\r\nHostUrl=https://example.invalid/source.pub\r\n",
    )
    .unwrap();
    let source_hash = format!("{:x}", Sha256::digest(source_bytes));
    let source = AdmittedSource::open(&source_path).unwrap();
    let output = root.join("derived.bin");
    let output_bytes = b"exact-derived-output";
    let receipt = source.commit_derived_output(&output, output_bytes).unwrap();
    let output_hash = format!("{:x}", Sha256::digest(output_bytes));
    let zone = fs::read_to_string(ads(&output)).unwrap_or_default();

    assert_eq!(receipt.disposition, DerivedFileTrustDisposition::Applied);
    assert!(zone.contains("ZoneId=3"), "output ADS was {zone:?}");
    assert_eq!(fs::read(&source_path).unwrap(), source_bytes);
    assert_eq!(fs::read(&output).unwrap(), output_bytes);
    emit_receipt(
        "internet_referrer",
        &source_hash,
        &output_hash,
        json!({
            "attachment_services_applied": true,
            "internet_zone_propagated": true,
            "source_main_stream_unchanged": true,
            "output_main_stream_exact": true,
        }),
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn local_referrer_does_not_fabricate_internet_zone() {
    let root = root("local");
    let source_path = root.join("local.pub");
    let source_bytes = b"publisher-local-source";
    fs::write(&source_path, source_bytes).unwrap();
    let source_hash = format!("{:x}", Sha256::digest(source_bytes));
    let source = AdmittedSource::open(&source_path).unwrap();
    let output = root.join("derived.bin");
    let output_bytes = b"exact-local-derived-output";
    let receipt = source.commit_derived_output(&output, output_bytes).unwrap();
    let output_hash = format!("{:x}", Sha256::digest(output_bytes));
    let zone = fs::read_to_string(ads(&output)).unwrap_or_default();

    assert_eq!(receipt.disposition, DerivedFileTrustDisposition::Applied);
    assert!(
        !zone.contains("ZoneId=3"),
        "fabricated Internet zone: {zone:?}"
    );
    assert_eq!(fs::read(&source_path).unwrap(), source_bytes);
    assert_eq!(fs::read(&output).unwrap(), output_bytes);
    emit_receipt(
        "local_referrer",
        &source_hash,
        &output_hash,
        json!({
            "attachment_services_applied": true,
            "internet_zone_not_fabricated": true,
            "source_main_stream_unchanged": true,
            "output_main_stream_exact": true,
        }),
    );
    fs::remove_dir_all(root).unwrap();
}
