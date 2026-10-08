mod fixed_pdf;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand, ValueEnum};
use pub_editor::{EditorEditableTarget, open_mature_0x2c_editor};
use pub_model::Sha256Digest;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Cursor,
    path::{Path, PathBuf},
};

#[derive(Debug, Parser)]
#[command(name = "pub", about = "Bounded local Microsoft Publisher conversion")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Convert {
        input: PathBuf,
        #[arg(long = "to", value_enum)]
        target: TargetArg,
        #[arg(long)]
        output: PathBuf,
        /// Explicit user-supplied TTF/TTC resource for bounded PDF typography.
        #[arg(long, value_name = "FONT")]
        fallback_font: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum TargetArg {
    Idml,
    Odg,
    Pdf,
}

impl TargetArg {
    fn editor_target(self) -> EditorEditableTarget {
        match self {
            Self::Idml => EditorEditableTarget::Idml,
            Self::Odg => EditorEditableTarget::Odg,
            Self::Pdf => unreachable!("PDF does not use the editable export target"),
        }
    }

    fn extension(self) -> &'static str {
        match self {
            Self::Idml => "idml",
            Self::Odg => "odg",
            Self::Pdf => "pdf",
        }
    }
}

fn sha256_digest(bytes: &[u8]) -> Sha256Digest {
    let digest = Sha256::digest(bytes);
    let mut raw = [0_u8; 32];
    raw.copy_from_slice(&digest);
    Sha256Digest::from_bytes(raw)
}

fn sidecar_path(output: &Path, suffix: &str) -> PathBuf {
    let mut value = output.as_os_str().to_os_string();
    value.push(suffix);
    PathBuf::from(value)
}

fn source_label(input: &Path) -> String {
    input
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("input.pub")
        .to_owned()
}

fn convert(input: &Path, target: TargetArg, output: &Path, emit_json: bool) -> Result<()> {
    if input == output {
        bail!("input and output paths must differ");
    }
    if output.extension().and_then(|value| value.to_str()) != Some(target.extension()) {
        bail!(
            "output extension must be .{} for --to {}",
            target.extension(),
            target.extension()
        );
    }

    let bytes = fs::read(input).with_context(|| format!("read {}", input.display()))?;
    let source_hash = sha256_digest(&bytes);

    let source =
        pub_reader::build_mature_0x2c_source_graph(Cursor::new(bytes.as_slice()), source_hash)
            .context("build mature Publisher source graph")?;
    let resolved =
        pub_reader::resolve_pub_source_graph(&source.graph).context("resolve Publisher graph")?;
    if !resolved.diagnostics.is_empty() {
        bail!(
            "resolver diagnostics prevent public conversion: {} diagnostic(s)",
            resolved.diagnostics.len()
        );
    }

    let session = open_mature_0x2c_editor(&bytes, source_hash)
        .context("open current editable PUB session")?;
    let editor_target = target.editor_target();
    let label = source_label(input);
    let preview = session
        .preview_editable_export(editor_target, label.clone())
        .context("preview editable export")?;
    if !preview.report.can_serialize {
        if emit_json {
            println!("{}", serde_json::to_string_pretty(&preview.report)?);
        } else {
            eprint!("{}", preview.human_summary);
        }
        bail!(
            "conversion blocked by {} required loss(es)",
            preview.report.counts.blocking
        );
    }

    let export = session
        .export_editable(editor_target, label)
        .context("serialize editable target")?;
    if preview.report != export.report {
        bail!("preview/export LossReport mismatch");
    }

    fs::write(output, &export.bytes).with_context(|| format!("write {}", output.display()))?;

    let json_sidecar = sidecar_path(output, ".loss.json");
    let text_sidecar = sidecar_path(output, ".loss.txt");
    let mut report_json = serde_json::to_vec_pretty(&export.report)?;
    report_json.push(b'\n');
    fs::write(&json_sidecar, report_json)
        .with_context(|| format!("write {}", json_sidecar.display()))?;
    fs::write(&text_sidecar, &export.human_summary)
        .with_context(|| format!("write {}", text_sidecar.display()))?;

    if emit_json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "schema": "chaptera.pub-convert-cli.v1",
                "source_sha256": source_hash,
                "target": target.extension(),
                "output": output,
                "byte_len": export.bytes.len(),
                "loss_report": export.report,
                "loss_json": json_sidecar,
                "loss_text": text_sidecar,
            }))?
        );
    } else {
        print!("{}", export.human_summary);
    }
    Ok(())
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Convert {
            input,
            target,
            output,
            fallback_font,
            json,
        } => match target {
            TargetArg::Pdf => {
                if output.extension().and_then(|value| value.to_str()) != Some("pdf") {
                    bail!("output extension must be .pdf for --to pdf");
                }
                let fallback_font = fallback_font.as_deref().ok_or_else(|| {
                    anyhow::anyhow!(
                        "--to pdf requires --fallback-font FILE so typography substitution is explicit"
                    )
                })?;
                let result = fixed_pdf::convert_pdf(&input, &output, fallback_font)?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&result.report)?);
                } else {
                    println!(
                        "output={} target=pdf typography=explicit_user_fallback_not_source_font",
                        output.display()
                    );
                    println!("loss_json={}", result.report_json_path.display());
                    println!("loss_text={}", result.report_text_path.display());
                }
                Ok(())
            }
            TargetArg::Idml | TargetArg::Odg => {
                if fallback_font.is_some() {
                    bail!("--fallback-font is only valid with --to pdf");
                }
                convert(&input, target, &output, json)
            }
        },
    }
}
