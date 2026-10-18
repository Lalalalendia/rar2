mod legacy_probe;
mod table_cell_proof;
mod text_export;

use anyhow::{Context, Result};
use chaptera_suite_handoff::AdmittedSource;
use clap::{Parser, Subcommand};
use pub_model::Sha256Digest;
use sha2::{Digest, Sha256};
use std::{io::Cursor, path::PathBuf};

#[derive(Debug, Parser)]
#[command(name = "pub", version, about = "Bounded Chaptera Publisher utilities")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Extract exact source-backed embedded image assets without transcoding.
    ExtractAssets {
        /// Input Microsoft Publisher file.
        file: PathBuf,
        /// Directory that receives exact assets plus manifest.json.
        #[arg(long)]
        output: PathBuf,
        /// Emit the canonical manifest JSON to stdout.
        #[arg(long)]
        json: bool,
    },
    /// Extract only source-proven semantic Story ranges to local private files.
    /// This command does not reconstruct page layout or a repaired PUB.
    ExtractText {
        /// Exact source PUB file (kept unmodified).
        file: PathBuf,
        /// Dedicated directory for private stories.json and stories.txt.
        #[arg(long)]
        output: PathBuf,
        /// Opt in to strict, source-proven old 0x22 (Quill/no-Quill) Story readers.
        #[arg(long)]
        include_legacy: bool,
    },
    /// Count strict old-family source-backed Stories, without displaying text.
    ProbeLegacyText {
        /// Exact source PUB (the output contains only its SHA and typed counts).
        file: PathBuf,
    },
    /// Independently check source-backed table cell projections without exposing text.
    ProbeLegacyTableCells {
        /// Exact source PUB (only typed counts and SHA are emitted).
        file: PathBuf,
    },
}

fn source_sha256(bytes: &[u8]) -> Sha256Digest {
    let digest = Sha256::digest(bytes);
    let mut output = [0_u8; 32];
    output.copy_from_slice(&digest);
    Sha256Digest::from_bytes(output)
}

fn extract_assets(file: PathBuf, output: PathBuf, json: bool) -> Result<()> {
    let admitted = AdmittedSource::open(&file).map_err(anyhow::Error::msg)?;
    let bytes = admitted.bytes();
    let source_hash = source_sha256(bytes);
    let source = pub_reader::build_mature_0x2c_source_graph(Cursor::new(bytes), source_hash)
        .context("build mature-0x2C source graph")?;
    let bundle = pub_reader::build_mature_0x2c_asset_export_bundle_from_bytes(bytes, &source.graph)
        .context("build exact asset export bundle")?;

    pub_reader::write_pub_asset_export_bundle(&bundle, &output, |path, bytes| {
        admitted
            .commit_derived_output(path, bytes)
            .map(|_| ())
            .map_err(anyhow::Error::new)
    })
    .with_context(|| format!("write exact asset bundle to {}", output.display()))?;

    if json {
        let manifest = pub_reader::pub_asset_manifest_json(&bundle.manifest)
            .context("serialize canonical asset manifest")?;
        print!("{manifest}");
    } else {
        println!(
            "extracted {} exact asset(s) to {} ({} diagnostic(s)); manifest={}",
            bundle.files.len(),
            output.display(),
            bundle.manifest.diagnostics.len(),
            output
                .join(pub_reader::PUB_ASSET_MANIFEST_FILENAME)
                .display()
        );
    }

    Ok(())
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::ExtractAssets { file, output, json } => extract_assets(file, output, json),
        Command::ExtractText {
            file,
            output,
            include_legacy,
        } => {
            let count = text_export::extract_text(&file, &output, include_legacy)?;
            println!("confirmed_story_ranges={count}; native_pub_repair=false");
            Ok(())
        }
        Command::ProbeLegacyText { file } => {
            let value = legacy_probe::probe(&file).context("source_safe_legacy_probe_failed")?;
            println!(
                "{}",
                serde_json::to_string(&value).context("serialize_legacy_probe")?
            );
            Ok(())
        }
        Command::ProbeLegacyTableCells { file } => {
            let value = table_cell_proof::probe(&file).context("source_safe_cell_probe_failed")?;
            println!(
                "{}",
                serde_json::to_string(&value).context("serialize_cell_probe")?
            );
            Ok(())
        }
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error:#}");
        std::process::exit(2);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_accepts_exact_asset_command_shape() {
        let cli = Cli::try_parse_from([
            "pub",
            "extract-assets",
            "example.pub",
            "--output",
            "assets",
            "--json",
        ])
        .expect("CLI shape");

        match cli.command {
            Command::ExtractAssets { file, output, json } => {
                assert_eq!(file, PathBuf::from("example.pub"));
                assert_eq!(output, PathBuf::from("assets"));
                assert!(json);
            }
            Command::ExtractText { .. }
            | Command::ProbeLegacyText { .. }
            | Command::ProbeLegacyTableCells { .. } => {
                panic!("must not enter story command")
            }
        }
    }

    #[test]
    fn cli_accepts_bounded_story_extract_without_source_text_in_stdout() {
        let cli = Cli::try_parse_from([
            "pub",
            "extract-text",
            "example.pub",
            "--output",
            "recovered",
        ])
        .expect("CLI shape");
        match cli.command {
            Command::ExtractText {
                file,
                output,
                include_legacy,
            } => {
                assert_eq!(file, PathBuf::from("example.pub"));
                assert_eq!(output, PathBuf::from("recovered"));
                assert!(!include_legacy);
            }
            Command::ExtractAssets { .. }
            | Command::ProbeLegacyText { .. }
            | Command::ProbeLegacyTableCells { .. } => {
                panic!("must not enter asset command")
            }
        }
    }

    #[test]
    fn legacy_story_export_requires_explicit_opt_in() {
        let cli = Cli::try_parse_from([
            "pub",
            "extract-text",
            "legacy.pub",
            "--output",
            "private",
            "--include-legacy",
        ])
        .expect("bounded legacy opt-in");
        match cli.command {
            Command::ExtractText { include_legacy, .. } => assert!(include_legacy),
            _ => panic!("must enter Story extraction"),
        }
    }

    #[test]
    fn legacy_probe_cli_accepts_source_only_without_export_path() {
        let cli = Cli::try_parse_from(["pub", "probe-legacy-text", "source.pub"])
            .expect("legacy probe CLI shape");
        match cli.command {
            Command::ProbeLegacyText { file } => {
                assert_eq!(file, PathBuf::from("source.pub"));
            }
            _ => panic!("must be the source-safe probe"),
        }
    }

    #[test]
    fn table_cell_source_probe_is_not_an_export_command() {
        let cli = Cli::try_parse_from(["pub", "probe-legacy-table-cells", "source.pub"])
            .expect("table cell source probe CLI");
        match cli.command {
            Command::ProbeLegacyTableCells { file } => {
                assert_eq!(file, PathBuf::from("source.pub"));
            }
            _ => panic!("must be source-safe table cell probe"),
        }
    }

    #[test]
    fn source_hash_is_sha256_of_exact_input_bytes() {
        let digest = source_sha256(b"chaptera");
        assert_eq!(
            digest.to_string(),
            "fa0bba13982a384d4c42ef19105ae2bf88c66d63b9b085d1d4022842235d17f8"
        );
    }
}
