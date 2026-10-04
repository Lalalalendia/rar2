use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

use super::acceptance;

pub(super) fn try_handle(
    first_arg: Option<&OsStr>,
    args: &mut impl Iterator<Item = OsString>,
) -> bool {
    if first_arg != Some(OsStr::new("--desktop-acceptance-v1")) {
        return false;
    }

    let Some(fixture) = args.next().map(PathBuf::from) else {
        eprintln!("usage: chaptera --desktop-acceptance-v1 FIXTURE PROJECT EXPORT");
        std::process::exit(2);
    };
    let Some(project) = args.next().map(PathBuf::from) else {
        eprintln!("usage: chaptera --desktop-acceptance-v1 FIXTURE PROJECT EXPORT");
        std::process::exit(2);
    };
    let Some(export) = args.next().map(PathBuf::from) else {
        eprintln!("usage: chaptera --desktop-acceptance-v1 FIXTURE PROJECT EXPORT");
        std::process::exit(2);
    };
    if args.next().is_some() {
        eprintln!("desktop acceptance mode accepts exactly three path arguments");
        std::process::exit(2);
    }

    match acceptance::run(&fixture, &project, &export) {
        Ok(observation) => {
            println!(
                "{}",
                serde_json::to_string(&observation)
                    .expect("desktop acceptance observation is JSON-serializable")
            );
            true
        }
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    }
}
