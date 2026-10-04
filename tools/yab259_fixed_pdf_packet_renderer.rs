use anyhow::{Context, Result, bail};
use pub_layout::BoundedResolvedScene;
use pub_pdf::{FixedPdfResources, PdfRenderDisposition, PdfTargetProfile, render_bounded_pdf};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::io::{self, Read};

const REQUEST_VERSION: &str = "chaptera.fixed-pdf-packet-render-request.v1";
const RESULT_VERSION: &str = "chaptera.fixed-pdf-packet-render-result.v1";

#[derive(Debug, Deserialize)]
struct RenderRequest {
    protocol_version: String,
    binding: Value,
    scene: BoundedResolvedScene,
    resources: FixedPdfResources,
    target: PdfTargetProfile,
}

#[derive(Debug, Serialize)]
struct RenderSummary {
    page_count: usize,
    node_painted: usize,
    node_partial: usize,
    node_unsupported: usize,
    diagnostic_codes: Vec<String>,
}

#[derive(Debug, Serialize)]
struct RenderResult {
    protocol_version: &'static str,
    binding: Value,
    renderer_revision: String,
    target_profile: String,
    summary: RenderSummary,
}

fn main() -> Result<()> {
    let mut raw = String::new();
    io::stdin()
        .read_to_string(&mut raw)
        .context("read source-neutral fixed-PDF packet request")?;
    let request: RenderRequest =
        serde_json::from_str(&raw).context("parse fixed-PDF packet request")?;
    if request.protocol_version != REQUEST_VERSION {
        bail!("unsupported fixed-PDF packet request protocol");
    }

    let output = render_bounded_pdf(&request.scene, &request.resources, &request.target)
        .context("render source-neutral fixed-PDF packet")?;

    let output_path = env::var("CHAPTERA_PDF_OUTPUT")
        .context("CHAPTERA_PDF_OUTPUT is required")?;
    fs::write(&output_path, &output.bytes)
        .with_context(|| format!("write fixed-PDF artifact {output_path}"))?;

    let mut node_painted = 0usize;
    let mut node_partial = 0usize;
    let mut node_unsupported = 0usize;
    for node in &output.report.nodes {
        match node.disposition {
            PdfRenderDisposition::Painted => node_painted += 1,
            PdfRenderDisposition::Partial => node_partial += 1,
            PdfRenderDisposition::Unsupported => node_unsupported += 1,
        }
    }
    let diagnostic_codes = output
        .report
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();

    let result = RenderResult {
        protocol_version: RESULT_VERSION,
        binding: request.binding,
        renderer_revision: output.report.target.renderer_revision,
        target_profile: output.report.target.profile,
        summary: RenderSummary {
            page_count: output.report.pages.len(),
            node_painted,
            node_partial,
            node_unsupported,
            diagnostic_codes,
        },
    };
    println!("{}", serde_json::to_string(&result)?);
    Ok(())
}
