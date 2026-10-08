#![forbid(unsafe_code)]

use anyhow::{Context, Result, anyhow, bail};
use chaptera_desktop_open_sandbox::{DEFAULT_WALL_TIMEOUT, launch_contained};
use std::path::PathBuf;

const MARKER: &[u8] = b"CHAPTERA_SANDBOX_BOOTSTRAP_OK\n";

fn main() {
    if let Err(error) = run() {
        eprintln!("bootstrap host acceptance failed: {error:#}");
        std::process::exit(2);
    }
}

fn run() -> Result<()> {
    let mut args = std::env::args_os();
    let _program = args.next();
    let probe = PathBuf::from(args.next().ok_or_else(|| anyhow!("missing probe path"))?);
    if args.next().is_some() {
        bail!("unexpected arguments");
    }

    let output = launch_contained(&probe, b"", DEFAULT_WALL_TIMEOUT)
        .context("launch strict bootstrap probe")?;
    if output.receipt.exit_code != 0 {
        bail!(
            "bootstrap probe exited with {}: {}",
            output.receipt.exit_code,
            String::from_utf8_lossy(&output.stderr)
        );
    }
    if output.stdout != MARKER {
        bail!(
            "bootstrap probe marker mismatch: {:?}",
            String::from_utf8_lossy(&output.stdout)
        );
    }
    Ok(())
}
