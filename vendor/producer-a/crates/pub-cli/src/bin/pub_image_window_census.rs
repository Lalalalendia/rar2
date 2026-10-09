use anyhow::{Context, Result};
use image::GenericImageView;
use pub_model::Affine2D;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::Path;

const Q16_ONE: i64 = 1 << 16;

fn main() -> Result<()> {
    let path = env::args()
        .nth(1)
        .context("usage: pub-image-window-census INPUT.pub")?;
    let path = Path::new(&path);
    let bytes = fs::read(path).with_context(|| format!("read {}", path.display()))?;
    let source_sha256 = format!("{:x}", Sha256::digest(&bytes));
    let bundle =
        pub_viewer::open_pub_bundle(&bytes, pub_viewer::viewer_geometry_environment_v0_1())
            .context("open PUB through Viewer")?;
    let visual = bundle.geometry;
    let nodes = visual
        .scene
        .nodes
        .iter()
        .map(|node| (node.origin, node))
        .collect::<BTreeMap<_, _>>();

    let mut windows = Vec::new();
    for image in visual.images.iter().filter(|image| image.source_exact) {
        let dimensions = image::load_from_memory(&image.bytes)
            .ok()
            .map(|decoded| decoded.dimensions());
        for placement in &image.placements {
            let Some(window) = placement.source_window.as_ref() else {
                continue;
            };
            let node = nodes.get(&placement.node_id);
            let inside_unit = window.left_q16 >= 0
                && window.top_q16 >= 0
                && window.right_q16 <= Q16_ONE
                && window.bottom_q16 <= Q16_ONE;
            let source_width_q16 = window.right_q16 - window.left_q16;
            let source_height_q16 = window.bottom_q16 - window.top_q16;

            windows.push(json!({
                "mime": image.mime,
                "source_exact": image.source_exact,
                "intrinsic_width_px": dimensions.map(|value| value.0),
                "intrinsic_height_px": dimensions.map(|value| value.1),
                "left_q16": window.left_q16,
                "top_q16": window.top_q16,
                "right_q16": window.right_q16,
                "bottom_q16": window.bottom_q16,
                "source_width_q16": source_width_q16,
                "source_height_q16": source_height_q16,
                "inside_unit": inside_unit,
                "extends_left": window.left_q16 < 0,
                "extends_top": window.top_q16 < 0,
                "extends_right": window.right_q16 > Q16_ONE,
                "extends_bottom": window.bottom_q16 > Q16_ONE,
                "rotation_degrees": placement.content_rotation_degrees,
                "recolor_present": placement.recolor.is_some(),
                "scene_node_present": node.is_some(),
                "scene_transform_identity": node.map(|value| value.transform == Affine2D::identity()),
                "frame_width_emu": node.map(|value| value.bounds.width.get()),
                "frame_height_emu": node.map(|value| value.bounds.height.get()),
                "image_resource_use_count": image.node_ids.len(),
            }));
        }
    }
    windows.sort_by_key(|value| {
        (
            value["mime"].as_str().unwrap_or_default().to_owned(),
            value["left_q16"].as_i64().unwrap_or_default(),
            value["top_q16"].as_i64().unwrap_or_default(),
            value["right_q16"].as_i64().unwrap_or_default(),
            value["bottom_q16"].as_i64().unwrap_or_default(),
        )
    });

    let result = json!({
        "schema": "chaptera.pub-pdf-source-window-semantics.v1",
        "fixture": path.file_stem().and_then(|value| value.to_str()).unwrap_or("input"),
        "source_sha256": source_sha256,
        "exact_source_window_count": windows.len(),
        "windows": windows,
    });
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}
