use anyhow::{Context, Result};
use pub_viewer::{open_mature_0x2c_geometry, viewer_geometry_environment_v0_1};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{env, fs};

fn main() -> Result<()> {
    let mut args = env::args_os().skip(1);
    let source = args
        .next()
        .context("usage: page-projection-fingerprint-receipt SOURCE.pub OUTPUT.json")?;
    let output = args.next().context("missing output path")?;
    let bytes = fs::read(&source).with_context(|| format!("read {:?}", source))?;
    let visual = open_mature_0x2c_geometry(&bytes, viewer_geometry_environment_v0_1())
        .context("open PUB through current Viewer page projection")?;

    let per_page = visual
        .document
        .pages
        .iter()
        .map(|page| {
            let fingerprint = Sha256::digest(page.id.as_canonical().as_bytes())
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
            json!({
                "viewer_page_index": page.index,
                "page_identity_fingerprint_sha256": fingerprint,
            })
        })
        .collect::<Vec<_>>();

    let receipt = json!({
        "schema": "chaptera.viewer-page-fingerprint-receipt.v1",
        "viewer_page_count": visual.document.pages.len(),
        "per_page": per_page,
        "claims": {
            "raw_page_identity_emitted": false,
            "page_identity_fingerprint_emitted": true,
            "story_text_emitted": false,
        },
    });
    fs::write(&output, serde_json::to_vec_pretty(&receipt)?)
        .with_context(|| format!("write {:?}", output))?;
    Ok(())
}
