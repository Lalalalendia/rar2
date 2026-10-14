//! Reader-only product CLI and activation ownership.
//!
//! Full Reader Windows package/activation acceptance follows this file. The
//! monolithic Desktop shell keeps only thin dispatch calls, allowing unrelated
//! main.rs changes to use the bounded Windows shared-core smoke instead.

use crate::{diagnostic_sweep, product_smoke};
use std::ffi::{OsStr, OsString};
use std::fs;
use std::iter::Peekable;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub(super) fn try_handle_product_smoke<I>(first_arg: Option<&OsStr>, args: &mut Peekable<I>) -> bool
where
    I: Iterator<Item = OsString>,
{
    if first_arg != Some(OsStr::new("--product-smoke-v1")) {
        return false;
    }

    let output = args.next().map(PathBuf::from);
    if args.next().is_some() {
        eprintln!("usage: chaptera --product-smoke-v1 [OUTPUT.json]");
        std::process::exit(2);
    }
    match product_smoke::run() {
        Ok(receipt) => {
            let encoded = serde_json::to_string(&receipt)
                .expect("product smoke receipt is JSON-serializable");
            if let Some(output) = output {
                if let Err(error) = fs::write(&output, format!("{encoded}\n")) {
                    eprintln!("write product smoke receipt {}: {error}", output.display());
                    std::process::exit(2);
                }
            } else {
                println!("{encoded}");
            }
        }
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    }
    true
}

pub(super) fn try_handle_reader_probe<I>(
    first_arg: Option<&OsStr>,
    args: &mut Peekable<I>,
    reader_only: bool,
) -> bool
where
    I: Iterator<Item = OsString>,
{
    if first_arg == Some(OsStr::new("--reader-activation-probe-v1")) {
        if !reader_only {
            eprintln!("Reader activation probe is reserved for the Reader build");
            std::process::exit(2);
        }
        let Some(path) = args.next().map(PathBuf::from) else {
            eprintln!(
                "usage: chaptera-reader --reader-activation-probe-v1 SOURCE.pub RECEIPT.json HOLD_MS"
            );
            std::process::exit(2);
        };
        let Some(receipt) = args.next().map(PathBuf::from) else {
            eprintln!(
                "usage: chaptera-reader --reader-activation-probe-v1 SOURCE.pub RECEIPT.json HOLD_MS"
            );
            std::process::exit(2);
        };
        let Some(hold_ms) = args
            .next()
            .and_then(|value| value.into_string().ok())
            .and_then(|value| value.parse::<u64>().ok())
        else {
            eprintln!("Reader activation probe HOLD_MS must be an integer");
            std::process::exit(2);
        };
        if args.next().is_some() {
            eprintln!("Reader activation probe accepts exactly source, receipt, and hold_ms");
            std::process::exit(2);
        }
        if let Err(error) = reader_activation_probe(&path, &receipt, hold_ms) {
            eprintln!("Reader activation probe failed: {error}");
            std::process::exit(2);
        }
        return true;
    }

    if first_arg == Some(OsStr::new("--smoke-check")) {
        let Some(path) = args.next().map(PathBuf::from) else {
            std::process::exit(2);
        };
        if smoke_check(&path).is_err() {
            std::process::exit(1);
        }
        return true;
    }

    false
}

fn reader_activation_probe(path: &Path, receipt: &Path, hold_ms: u64) -> Result<(), String> {
    const MAX_HOLD_MS: u64 = 60_000;
    if hold_ms == 0 || hold_ms > MAX_HOLD_MS {
        return Err(format!(
            "hold_ms must be within 1..={MAX_HOLD_MS}, got {hold_ms}"
        ));
    }

    let admitted = chaptera_suite_handoff::AdmittedSource::open(path)?;
    let source_sha256 = admitted.sha256().to_owned();
    let source_byte_len = admitted.bytes().len();
    let visual = diagnostic_sweep::open_for_product(admitted.bytes())
        .map_err(|error| format!("open {}: {error}", path.display()))?;
    let page_count = visual.document.pages.len();

    let write_receipt = |completed: bool, source_unchanged: bool| -> Result<(), String> {
        if let Some(parent) = receipt.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("create activation receipt parent: {error}"))?;
        }
        let value = serde_json::json!({
            "schema_version": "chaptera.reader-activation-session.v1",
            "pid": std::process::id(),
            "source_sha256": source_sha256,
            "source_byte_len": source_byte_len,
            "page_count": page_count,
            "read_only": true,
            "process_model": "independent_process_per_activation",
            "completed": completed,
            "source_unchanged": source_unchanged,
        });
        fs::write(
            receipt,
            format!(
                "{}\n",
                serde_json::to_string(&value)
                    .map_err(|error| format!("serialize activation receipt: {error}"))?
            ),
        )
        .map_err(|error| format!("write activation receipt {}: {error}", receipt.display()))
    };

    write_receipt(false, false)?;
    std::hint::black_box(&visual);
    std::thread::sleep(Duration::from_millis(hold_ms));

    let after = chaptera_suite_handoff::AdmittedSource::open(path)
        .map_err(|error| format!("re-admit {} after activation hold: {error}", path.display()))?;
    if after.sha256() != source_sha256 || after.bytes().len() != source_byte_len {
        return Err("Reader activation probe observed source mutation".to_owned());
    }
    write_receipt(true, true)
}

fn smoke_check(path: &Path) -> Result<(), String> {
    let admitted = chaptera_suite_handoff::AdmittedSource::open(path)?;
    diagnostic_sweep::smoke_check_bytes(admitted.bytes())
}
