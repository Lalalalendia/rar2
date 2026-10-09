use anyhow::{Context, Result};
use image::{AnimationDecoder, ImageDecoder, codecs::gif::GifDecoder};
use pub_model::Affine2D;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::io::Cursor;
use std::path::Path;

fn main() -> Result<()> {
    let path = env::args().nth(1).context("usage: pub-gif-census INPUT.pub")?;
    let path = Path::new(&path);
    let bytes = fs::read(path).with_context(|| format!("read {}", path.display()))?;
    let source_sha256 = format!("{:x}", Sha256::digest(&bytes));
    let bundle = pub_viewer::open_pub_bundle(
        &bytes,
        pub_viewer::viewer_geometry_environment_v0_1(),
    )
    .context("open PUB through Viewer")?;
    let visual = bundle.geometry;
    let nodes = visual
        .scene
        .nodes
        .iter()
        .map(|node| (node.origin, node))
        .collect::<BTreeMap<_, _>>();

    let mut resources = Vec::new();
    for image in visual.images.iter().filter(|image| image.mime == "image/gif") {
        let decoder = GifDecoder::new(Cursor::new(image.bytes.as_slice()))
            .context("decode exact Viewer GIF")?;
        let (width, height) = decoder.dimensions();
        let frames = decoder
            .into_frames()
            .collect_frames()
            .context("decode GIF frames")?;

        let mut transparent_pixel_count = 0_u64;
        let mut partial_alpha_pixel_count = 0_u64;
        let mut pixel_count = 0_u64;
        for frame in &frames {
            for pixel in frame.buffer().pixels() {
                let alpha = pixel.0[3];
                pixel_count += 1;
                if alpha < 255 {
                    transparent_pixel_count += 1;
                }
                if alpha > 0 && alpha < 255 {
                    partial_alpha_pixel_count += 1;
                }
            }
        }

        let source_window_count = image
            .placements
            .iter()
            .filter(|placement| placement.source_window.is_some())
            .count();
        let rotation_count = image
            .placements
            .iter()
            .filter(|placement| placement.content_rotation_degrees.is_some())
            .count();
        let recolor_count = image
            .placements
            .iter()
            .filter(|placement| placement.recolor.is_some())
            .count();

        let mut scene_use_count = 0_usize;
        let mut transformed_use_count = 0_usize;
        let mut use_bounds_emu = Vec::new();
        for node_id in &image.node_ids {
            if let Some(node) = nodes.get(node_id) {
                scene_use_count += 1;
                if node.transform != Affine2D::identity() {
                    transformed_use_count += 1;
                }
                use_bounds_emu.push([
                    node.bounds.width.get(),
                    node.bounds.height.get(),
                ]);
            }
        }
        use_bounds_emu.sort();

        resources.push(json!({
            "source_exact": image.source_exact,
            "frame_count": frames.len(),
            "intrinsic_width_px": width,
            "intrinsic_height_px": height,
            "pixel_count_across_frames": pixel_count,
            "transparent_pixel_count": transparent_pixel_count,
            "partial_alpha_pixel_count": partial_alpha_pixel_count,
            "use_count": image.node_ids.len(),
            "scene_use_count": scene_use_count,
            "transformed_use_count": transformed_use_count,
            "placement_count": image.placements.len(),
            "source_window_count": source_window_count,
            "rotation_count": rotation_count,
            "recolor_count": recolor_count,
            "use_bounds_emu": use_bounds_emu,
        }));
    }

    let result = json!({
        "schema": "chaptera.pub-pdf-gif-semantics-census.v1",
        "fixture": path.file_stem().and_then(|value| value.to_str()).unwrap_or("input"),
        "source_sha256": source_sha256,
        "gif_resource_count": resources.len(),
        "gif_resources": resources,
    });
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}
