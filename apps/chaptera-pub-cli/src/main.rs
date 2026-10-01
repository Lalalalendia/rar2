use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use pub_model::Sha256Digest;
use sha2::{Digest, Sha256};
use std::{fs, io::Cursor, path::PathBuf};

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
}

fn source_sha256(bytes: &[u8]) -> Sha256Digest {
    let digest = Sha256::digest(bytes);
    let mut output = [0_u8; 32];
    output.copy_from_slice(&digest);
    Sha256Digest::from_bytes(output)
}

fn extract_assets(file: PathBuf, output: PathBuf, json: bool) -> Result<()> {
    let bytes = fs::read(&file)
        .with_context(|| format!("read Publisher source {}", file.display()))?;
    let source_hash = source_sha256(&bytes);
    let source = pub_reader::build_mature_0x2c_source_graph(
        Cursor::new(bytes.as_slice()),
        source_hash,
    )
    .context("build mature-0x2C source graph")?;
    let bundle =
        pub_reader::build_mature_0x2c_asset_export_bundle_from_bytes(&bytes, &source.graph)
            .context("build exact asset export bundle")?;

    pub_reader::write_pub_asset_export_bundle(&bundle, &output)
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
            output.join(pub_reader::PUB_ASSET_MANIFEST_FILENAME).display()
        );
    }

    Ok(())
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::ExtractAssets { file, output, json } => extract_assets(file, output, json),
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
        }
    }

    #[test]
    fn source_hash_is_sha256_of_exact_input_bytes() {
        let digest = source_sha256(b"chaptera");
        assert_eq!(
            digest.to_string(),
            "e7e70f41184b1ab78ef65f24f92a7201d8fb1f8c1df900a37687a330cfd5fb1d"
        );
    }
}
