#![forbid(unsafe_code)]

use anyhow::{Context, Result, anyhow, bail};
use chaptera_desktop_open_sandbox::{DEFAULT_WALL_TIMEOUT, launch_contained};
use serde::Deserialize;
use serde_json::json;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Deserialize)]
struct NegativeResult {
    schema_version: String,
    forbidden_file_read_denied: bool,
    network_connect_denied: bool,
    child_process_spawn_denied: bool,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("negative host acceptance failed: {error:#}");
        std::process::exit(2);
    }
}

fn run() -> Result<()> {
    let mut args = std::env::args_os();
    let _program = args.next();
    let probe = PathBuf::from(args.next().ok_or_else(|| anyhow!("missing probe path"))?);
    let secret = PathBuf::from(args.next().ok_or_else(|| anyhow!("missing secret path"))?);
    let receipt_path = PathBuf::from(args.next().ok_or_else(|| anyhow!("missing receipt path"))?);
    if args.next().is_some() {
        bail!("unexpected arguments");
    }

    let request = serde_json::to_vec(&json!({ "forbidden_file": secret }))?;
    let output = launch_contained(&probe, &request, DEFAULT_WALL_TIMEOUT)
        .context("launch negative probe")?;
    if output.receipt.exit_code != 0 {
        bail!(
            "negative probe exited with {}: {}",
            output.receipt.exit_code,
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let result: NegativeResult =
        serde_json::from_slice(&output.stdout).context("parse negative probe response")?;
    if result.schema_version != "chaptera.desktop-pub-containment-negative.v1"
        || !result.forbidden_file_read_denied
        || !result.network_connect_denied
        || !result.child_process_spawn_denied
    {
        bail!("negative containment proof incomplete: {result:?}");
    }

    let receipt = json!({
        "schema_version": "chaptera.desktop-pub-containment-negative-acceptance.v1",
        "filesystem_denied": result.forbidden_file_read_denied,
        "network_denied": result.network_connect_denied,
        "child_process_denied": result.child_process_spawn_denied,
        "sandbox": output.receipt,
    });
    if let Some(parent) = receipt_path.parent() {
        fs::create_dir_all(parent).context("create receipt directory")?;
    }
    fs::write(receipt_path, serde_json::to_vec_pretty(&receipt)?)
        .context("write negative receipt")?;
    Ok(())
}
