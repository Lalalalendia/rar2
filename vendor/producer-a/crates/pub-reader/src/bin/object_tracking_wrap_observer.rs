use std::{env, fs, path::PathBuf};

use anyhow::{bail, Context, Result};
use pub_reader::observe_object_tracking_wrap_state;
use serde::Serialize;
use sha2::{Digest, Sha256};

const RECEIPT_SCHEMA: &str = "chaptera.object-tracking-wrap-observer/v1";

#[derive(Debug, Serialize)]
struct Receipt {
    schema: &'static str,
    source_sha256: String,
    target_oh_track: u32,
    observer: pub_reader::PubObjectTrackingWrapObserver,
}

fn main() -> Result<()> {
    let mut args = env::args_os().skip(1);
    let input = PathBuf::from(args.next().context(
        "usage: object_tracking_wrap_observer <input.pub> <target_seq_num> <output.json>",
    )?);
    let target = args.next().context(
        "usage: object_tracking_wrap_observer <input.pub> <target_seq_num> <output.json>",
    )?;
    let output = PathBuf::from(args.next().context(
        "usage: object_tracking_wrap_observer <input.pub> <target_seq_num> <output.json>",
    )?);
    if args.next().is_some() {
        bail!("usage: object_tracking_wrap_observer <input.pub> <target_seq_num> <output.json>");
    }

    let target_oh_track: u32 = target
        .to_string_lossy()
        .parse()
        .context("target_seq_num must be u32")?;
    let bytes = fs::read(&input).with_context(|| format!("read {}", input.display()))?;
    let digest = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let observer = observe_object_tracking_wrap_state(&bytes, target_oh_track)
        .with_context(|| format!("observe ObjectTracking wrap state for {}", input.display()))?;

    let receipt = Receipt {
        schema: RECEIPT_SCHEMA,
        source_sha256: digest,
        target_oh_track,
        observer,
    };

    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create output directory {}", parent.display()))?;
    }
    fs::write(
        &output,
        serde_json::to_vec_pretty(&receipt).context("serialize wrap observer receipt")?,
    )
    .with_context(|| format!("write {}", output.display()))?;
    Ok(())
}
