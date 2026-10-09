use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use pub_re::{
    analyze_manifest_file, attribute_contents_manifest_file,
    attribute_officeart_manifest_file,
};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Parser)]
#[command(
    name = "pub-re",
    about = "Source-safe differential reverse-engineering harness for Publisher files"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Compare two exact PUB/CFB inputs described by a reproducible experiment manifest.
    Analyze {
        #[arg(long)]
        manifest: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
    /// Join changed byte ranges in one explicitly selected stream to OfficeArt RawSpan owners.
    AttributeContents {
        #[arg(long)]
        manifest: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
    AttributeOfficeart {
        #[arg(long)]
        manifest: PathBuf,
        #[arg(long)]
        stream: String,
        #[arg(long)]
        output: PathBuf,
    },
}

fn write_json<T: serde::Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)
            .with_context(|| format!("create receipt directory {}", parent.display()))?;
    }
    let mut bytes = serde_json::to_vec_pretty(value).context("serialize PUB RE receipt")?;
    bytes.push(b'\n');
    fs::write(path, bytes).with_context(|| format!("write receipt {}", path.display()))
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Analyze { manifest, output } => {
            let receipt = analyze_manifest_file(&manifest)?;
            write_json(&output, &receipt)?;
            println!(
                "pub-re status={} experiment_id={} changed_streams={} added_entries={} removed_entries={} receipt={}",
                receipt.status,
                receipt.experiment_id,
                receipt.cfb.changed_streams.len(),
                receipt.cfb.added_entries.len(),
                receipt.cfb.removed_entries.len(),
                output.display(),
            );
            Ok(())
        }
        Command::AttributeContents { manifest, output } => {
            let receipt = attribute_contents_manifest_file(&manifest)?;
            write_json(&output, &receipt)?;
            println!(
                "pub-re contents status={} chunks={} changed={} receipt={}",
                receipt.status, receipt.compared_chunks, receipt.changed_chunks,
                output.display()
            );
            Ok(())
        }
        Command::AttributeOfficeart {
            manifest,
            stream,
            output,
        } => {
            let receipt = attribute_officeart_manifest_file(&manifest, &stream)?;
            write_json(&output, &receipt)?;
            println!(
                "pub-re officeart experiment_id={} stream={} ranges={} receipt={}",
                receipt.experiment_id,
                receipt.stream,
                receipt.ranges.len(),
                output.display(),
            );
            Ok(())
        }
    }
}
