// Measurement-only control: Continuity V2 owner admission; never merge this comment.
use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

use super::acceptance_v2;

pub(super) fn try_handle(
    first_arg: Option<&OsStr>,
    args: &mut impl Iterator<Item = OsString>,
) -> bool {
    if first_arg != Some(OsStr::new("--desktop-acceptance-v2")) {
        return false;
    }

    let Some(fixture) = args.next().map(PathBuf::from) else {
        eprintln!(
            "usage: chaptera --desktop-acceptance-v2 FIXTURE REPLACEMENT_IMAGE PROJECT EXPORT"
        );
        std::process::exit(2);
    };
    let Some(replacement) = args.next().map(PathBuf::from) else {
        eprintln!(
            "usage: chaptera --desktop-acceptance-v2 FIXTURE REPLACEMENT_IMAGE PROJECT EXPORT"
        );
        std::process::exit(2);
    };
    let Some(project) = args.next().map(PathBuf::from) else {
        eprintln!(
            "usage: chaptera --desktop-acceptance-v2 FIXTURE REPLACEMENT_IMAGE PROJECT EXPORT"
        );
        std::process::exit(2);
    };
    let Some(export) = args.next().map(PathBuf::from) else {
        eprintln!(
            "usage: chaptera --desktop-acceptance-v2 FIXTURE REPLACEMENT_IMAGE PROJECT EXPORT"
        );
        std::process::exit(2);
    };
    if args.next().is_some() {
        eprintln!("desktop continuity V2 mode accepts exactly four path arguments");
        std::process::exit(2);
    }

    match acceptance_v2::run(&fixture, &replacement, &project, &export) {
        Ok(observation) => {
            println!(
                "{}",
                serde_json::to_string(&observation)
                    .expect("desktop continuity V2 observation is JSON-serializable")
            );
            true
        }
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    }
}
