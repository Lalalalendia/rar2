use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

use pub_viewer::{classify_failure_candidate, FailureIntakeClass};

use super::diagnostic_sweep;

fn reader_only_mode() -> bool {
    cfg!(feature = "reader-only")
}

pub(super) fn try_handle(
    first_arg: Option<&OsStr>,
    args: &mut impl Iterator<Item = OsString>,
) -> bool {
    if first_arg == Some(OsStr::new("--handoff-create-v1")) {
        if !reader_only_mode() {
            eprintln!("only the Chaptera Reader product may create a V1 suite handoff");
            std::process::exit(2);
        }
        let Some(target) = args.next().and_then(|value| value.into_string().ok()) else {
            eprintln!(
                "usage: chaptera-reader --handoff-create-v1 TARGET_PRODUCT SOURCE.pub OUTPUT.json"
            );
            std::process::exit(2);
        };
        let Some(source) = args.next().map(PathBuf::from) else {
            eprintln!(
                "usage: chaptera-reader --handoff-create-v1 TARGET_PRODUCT SOURCE.pub OUTPUT.json"
            );
            std::process::exit(2);
        };
        let Some(output) = args.next().map(PathBuf::from) else {
            eprintln!(
                "usage: chaptera-reader --handoff-create-v1 TARGET_PRODUCT SOURCE.pub OUTPUT.json"
            );
            std::process::exit(2);
        };
        if args.next().is_some() {
            eprintln!("Reader handoff creation accepts exactly target, source, and output");
            std::process::exit(2);
        }

        let admitted = match chaptera_suite_handoff::AdmittedSource::open(&source) {
            Ok(admitted) => admitted,
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(2);
            }
        };
        let reader_supported = diagnostic_sweep::smoke_check_bytes(admitted.bytes()).is_ok();
        let rescue_eligible = !reader_supported
            && matches!(
                classify_failure_candidate(admitted.bytes()).class,
                FailureIntakeClass::PubDamaged
            );
        match chaptera_suite_handoff::create_reader_handoff_from_admitted(
            &admitted,
            &target,
            reader_supported,
            rescue_eligible,
        )
        .and_then(|packet| {
            chaptera_suite_handoff::write_packet(&packet, &output)?;
            Ok(packet)
        }) {
            Ok(packet) => {
                println!(
                    "{{\"protocol_version\":\"{}\",\"sender_product_id\":\"{}\",\"target_product_id\":\"{}\",\"requested_job\":\"{}\",\"source_sha256\":\"{}\"}}",
                    chaptera_suite_handoff::PACKET_VERSION,
                    chaptera_suite_handoff::READER_PRODUCT_ID,
                    packet.target_product_id,
                    packet.requested_job,
                    packet.source.sha256
                );
                return true;
            }
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(2);
            }
        }
    }

    if first_arg == Some(OsStr::new("--handoff-accept-v1")) {
        if reader_only_mode() {
            eprintln!("Chaptera Reader is a handoff sender, not an Editor receiver");
            std::process::exit(2);
        }
        let Some(packet_path) = args.next().map(PathBuf::from) else {
            eprintln!("usage: chaptera-editor --handoff-accept-v1 PACKET.json ACCEPTANCE.json");
            std::process::exit(2);
        };
        let Some(output) = args.next().map(PathBuf::from) else {
            eprintln!("usage: chaptera-editor --handoff-accept-v1 PACKET.json ACCEPTANCE.json");
            std::process::exit(2);
        };
        if args.next().is_some() {
            eprintln!("Editor handoff acceptance accepts exactly packet and output");
            std::process::exit(2);
        }

        let result = chaptera_suite_handoff::load_for_receiver(
            &packet_path,
            chaptera_suite_handoff::EDITOR_PRODUCT_ID,
        )
        .and_then(|validated| {
            let admitted = diagnostic_sweep::smoke_check_bytes(validated.source_bytes()).is_ok();
            chaptera_suite_handoff::finish_acceptance(validated, admitted)
        })
        .and_then(|receipt| {
            chaptera_suite_handoff::write_acceptance(&receipt, &output)?;
            Ok(receipt)
        });

        match result {
            Ok(receipt) => {
                println!(
                    "{}",
                    serde_json::to_string(&receipt)
                        .expect("suite handoff acceptance is JSON-serializable")
                );
                return true;
            }
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(2);
            }
        }
    }

    false
}
