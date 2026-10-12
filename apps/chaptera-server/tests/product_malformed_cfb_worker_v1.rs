#![cfg(all(target_os = "linux", target_arch = "x86_64"))]

// A source with a valid SHA is NOT a validated or safe PUB. Exercise the
// real Product subprocess boundary with two deliberately malformed CFB
// sources, not a fake parser or in-process "safe" fallback. A separate
// pinned-Sample3 positive test in this same server CI proves the worker path
// can successfully parse an admitted real publication.
use std::{path::PathBuf, time::Duration};

use chaptera_server::{
    product_replay_worker::IsolatedProductReplayProducer,
    source_baseline::SourceBaselineProducerConfig,
};
use sha2::{Digest, Sha256};

const CFB_MAGIC: [u8; 8] = [0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1];

// This must pass under the *same* worker limits as the malformed inputs.
// Otherwise `worker_failed` could just mean the sandbox cannot start.
fn sample3_pub() -> Vec<u8> {
    let source = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../vendor/producer-a/crates/pub-quill/tests/fixtures/Sample3.pub.b64"
    ));
    let digits = source
        .bytes()
        .filter(|b| !b.is_ascii_whitespace())
        .collect::<Vec<_>>();
    assert_eq!(digits.len() % 4, 0);
    let mut bytes = Vec::with_capacity(digits.len() / 4 * 3);
    for c in digits.chunks_exact(4) {
        let a = b64(c[0]);
        let b = b64(c[1]);
        let third = if c[2] == b'=' { 0 } else { b64(c[2]) };
        let fourth = if c[3] == b'=' { 0 } else { b64(c[3]) };
        bytes.push((a << 2) | (b >> 4));
        if c[2] != b'=' {
            bytes.push((b << 4) | (third >> 2));
        }
        if c[3] != b'=' {
            bytes.push((third << 6) | fourth);
        }
    }
    bytes
}

fn b64(value: u8) -> u8 {
    match value {
        b'A'..=b'Z' => value - b'A',
        b'a'..=b'z' => value - b'a' + 26,
        b'0'..=b'9' => value - b'0' + 52,
        b'+' => 62,
        b'/' => 63,
        _ => panic!("invalid pinned fixture base64"),
    }
}

fn truncated_cfb_header() -> Vec<u8> {
    // Magic is recognized, but there is no sector directory or FAT.
    CFB_MAGIC.to_vec()
}

fn forged_fat_sector_count() -> Vec<u8> {
    // This is an intentionally invalid synthetic CFB header with a FAT
    // sector count which cannot exist within the supplied 512 bytes.
    // It must never cause in-process allocation or parsing by the host.
    let mut bytes = vec![0_u8; 512];
    bytes[..CFB_MAGIC.len()].copy_from_slice(&CFB_MAGIC);
    bytes[24..26].copy_from_slice(&3_u16.to_le_bytes());
    bytes[26..28].copy_from_slice(&0xfffe_u16.to_le_bytes());
    bytes[28..30].copy_from_slice(&9_u16.to_le_bytes());
    bytes[30..32].copy_from_slice(&6_u16.to_le_bytes());
    bytes[44..48].copy_from_slice(&u32::MAX.to_le_bytes());
    bytes[48..52].copy_from_slice(&u32::MAX.to_le_bytes());
    bytes
}

fn real_product_worker() -> IsolatedProductReplayProducer {
    IsolatedProductReplayProducer::new(SourceBaselineProducerConfig {
        isolation_python: PathBuf::from("python3"),
        isolation_harness: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tools/migration_pdf_worker_isolation.py"),
        worker_binary: PathBuf::from(env!("CARGO_BIN_EXE_chaptera")),
        worker_wall_timeout: Duration::from_secs(8),
        worker_address_space_mb: 2048,
        worker_cpu_seconds: 6,
        worker_open_files: 64,
        worker_output_file_mb: 32,
        temp_root: std::env::temp_dir(),
    })
    .expect("physical worker limits and harness must be valid")
}

#[tokio::test]
async fn syntactically_host_authorized_but_malformed_cfb_is_denied_by_real_sandbox() {
    let producer = real_product_worker();
    let accepted_source = sample3_pub();
    let accepted_sha256 = format!("{:x}", Sha256::digest(&accepted_source));
    tokio::time::timeout(
        Duration::from_secs(18),
        producer.baseline_project(
            "document-sample3-same-limits",
            &accepted_sha256,
            &accepted_source,
        ),
    )
    .await
    .expect("same-limits positive worker timed out")
    .expect("same-limits real Sample3 must reach a valid isolated parser");

    for (case, source) in [
        ("truncated-magic", truncated_cfb_header()),
        ("oversized-fat", forged_fat_sector_count()),
    ] {
        // Hash is deliberately exact: the test must reach the untrusted
        // parser in the actual isolated worker, not stop at host SHA fence.
        let source_sha256 = format!("{:x}", Sha256::digest(&source));
        let denied = tokio::time::timeout(
            Duration::from_secs(18),
            producer.baseline_project("document-malformed-cfb", &source_sha256, &source),
        )
        .await
        .unwrap_or_else(|_| panic!("{case}: outer time fence exceeded"))
        .expect_err("invalid CFB cannot yield a canonical EditorProject");
        assert_eq!(
            denied.code, "product_replay_worker_failed",
            "{case}: unexpected non-parser failure: {}",
            denied.code
        );
        assert_eq!(
            denied.message, "isolated worker failed without a usable receipt",
            "{case}: host must not surface parser diagnostics or source contents"
        );
    }
}

#[tokio::test]
async fn malformed_cfb_identity_is_rejected_before_any_worker_invocation() {
    let source = truncated_cfb_header();
    let denied = real_product_worker()
        .baseline_project("document-malformed-cfb", &"0".repeat(64), &source)
        .await
        .expect_err("forged source SHA cannot reach the PUB worker");
    assert_eq!(denied.code, "product_replay_source_mismatch");
}
