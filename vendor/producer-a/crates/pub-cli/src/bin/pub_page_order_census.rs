use anyhow::{Context, Result};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::env;
use std::fs;
use std::path::Path;

fn main() -> Result<()> {
    let path = env::args()
        .nth(1)
        .context("usage: pub-page-order-census INPUT.pub")?;
    let path = Path::new(&path);
    let bytes = fs::read(path).with_context(|| format!("read {}", path.display()))?;
    let source_sha256 = format!("{:x}", Sha256::digest(&bytes));
    let classification = pub_viewer::classify_pub_family(&bytes);
    let bundle =
        pub_viewer::open_pub_bundle(&bytes, pub_viewer::viewer_geometry_environment_v0_1())
            .context("open PUB through Viewer")?;

    let viewer_order = bundle
        .geometry
        .document
        .pages
        .iter()
        .map(|page| page.id)
        .collect::<Vec<_>>();
    let mut canonical_order = viewer_order.clone();
    canonical_order.sort();

    let permutation = canonical_order
        .iter()
        .map(|page_id| {
            viewer_order
                .iter()
                .position(|candidate| candidate == page_id)
                .map(|index| index + 1)
                .context("canonical page must exist in Viewer order")
        })
        .collect::<Result<Vec<_>>>()?;
    let changed_position_count = permutation
        .iter()
        .enumerate()
        .filter(|(index, viewer_ordinal)| **viewer_ordinal != index + 1)
        .count();

    let result = json!({
        "schema": "chaptera.pub-pdf-page-order-census.v1",
        "fixture": path.file_stem().and_then(|value| value.to_str()).unwrap_or("input"),
        "source_sha256": source_sha256,
        "route": classification.route.as_str(),
        "page_count": viewer_order.len(),
        "order_changed": changed_position_count > 0,
        "changed_position_count": changed_position_count,
        "canonical_pdf_order_as_viewer_ordinals": permutation,
    });
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}
