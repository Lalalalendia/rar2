use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    io::Cursor,
    path::PathBuf,
};

use pub_model::RectEmu;
use pub_viewer::{open_pub_bundle, viewer_geometry_environment_v0_1};
use serde::Serialize;

#[derive(Debug, Serialize)]
struct Receipt {
    schema: &'static str,
    viewer_page: u32,
    candidate_story_fill_count: usize,
    later_overlapping_image_count: usize,
    later_overlap_relation_count: usize,
    later_image_mime_histogram: BTreeMap<String, usize>,
    overlap_area_histogram: BTreeMap<String, usize>,
    overlap_alpha_histogram: BTreeMap<String, usize>,
    scene_survivor_count: usize,
    embedded_image_materialized_count: usize,
    viewer_paint_entry_count: usize,
    viewer_visible_paint_count: usize,
    claims: Claims,
}

#[derive(Debug, Serialize)]
struct Claims {
    exact_source_identity_checked: bool,
    node_ids_emitted: bool,
    coordinates_emitted: bool,
    colors_emitted: bool,
    text_emitted: bool,
    pdf_used_as_semantic_authority: bool,
}

fn overlaps(a: RectEmu, b: RectEmu) -> bool {
    let (Some(ar), Some(ab), Some(br), Some(bb)) = (a.right(), a.bottom(), b.right(), b.bottom())
    else {
        return false;
    };
    a.x.get() < br.get() && b.x.get() < ar.get() && a.y.get() < bb.get() && b.y.get() < ab.get()
}


fn bump(map: &mut BTreeMap<String, usize>, key: impl Into<String>) {
    *map.entry(key.into()).or_default() += 1;
}

fn intersection(a: RectEmu, b: RectEmu) -> Option<(i64, i64, i64, i64)> {
    let (Some(ar), Some(ab), Some(br), Some(bb)) = (a.right(), a.bottom(), b.right(), b.bottom())
    else {
        return None;
    };
    let left = a.x.get().max(b.x.get());
    let top = a.y.get().max(b.y.get());
    let right = ar.get().min(br.get());
    let bottom = ab.get().min(bb.get());
    (right > left && bottom > top).then_some((left, top, right, bottom))
}

fn overlap_area_class(image_bounds: RectEmu, overlap: (i64, i64, i64, i64)) -> &'static str {
    let image_area =
        i128::from(image_bounds.width.get()) * i128::from(image_bounds.height.get());
    let overlap_area =
        i128::from(overlap.2 - overlap.0) * i128::from(overlap.3 - overlap.1);
    if image_area <= 0 || overlap_area <= 0 {
        return "invalid";
    }
    let basis_points = overlap_area.saturating_mul(10_000) / image_area;
    match basis_points {
        0..=99 => "lt_1pct",
        100..=999 => "1_to_10pct",
        1_000..=4_999 => "10_to_50pct",
        _ => "ge_50pct",
    }
}

fn scaled_floor(offset: i64, pixels: u32, extent: i64) -> Option<usize> {
    if offset < 0 || extent <= 0 {
        return None;
    }
    usize::try_from(
        i128::from(offset)
            .checked_mul(i128::from(pixels))?
            .checked_div(i128::from(extent))?,
    )
    .ok()
}

fn scaled_ceil(offset: i64, pixels: u32, extent: i64) -> Option<usize> {
    if offset < 0 || extent <= 0 {
        return None;
    }
    let numerator = i128::from(offset).checked_mul(i128::from(pixels))?;
    let denominator = i128::from(extent);
    usize::try_from(
        numerator
            .checked_add(denominator - 1)?
            .checked_div(denominator)?,
    )
    .ok()
}

fn png_overlap_alpha_class(
    bytes: &[u8],
    image_bounds: RectEmu,
    overlap: (i64, i64, i64, i64),
) -> &'static str {
    let decoder = png::Decoder::new(Cursor::new(bytes));
    let Ok(mut reader) = decoder.read_info() else {
        return "decode_unavailable";
    };
    let Some(buffer_size) = reader.output_buffer_size() else {
        return "decode_unavailable";
    };
    let mut buffer = vec![0_u8; buffer_size];
    let Ok(info) = reader.next_frame(&mut buffer) else {
        return "decode_unavailable";
    };
    if info.bit_depth != png::BitDepth::Eight || info.width == 0 || info.height == 0 {
        return "unsupported_pixel_format";
    }

    let left_offset = overlap.0 - image_bounds.x.get();
    let top_offset = overlap.1 - image_bounds.y.get();
    let right_offset = overlap.2 - image_bounds.x.get();
    let bottom_offset = overlap.3 - image_bounds.y.get();
    let Some(mut x0) = scaled_floor(left_offset, info.width, image_bounds.width.get()) else {
        return "mapping_unavailable";
    };
    let Some(mut y0) = scaled_floor(top_offset, info.height, image_bounds.height.get()) else {
        return "mapping_unavailable";
    };
    let Some(mut x1) = scaled_ceil(right_offset, info.width, image_bounds.width.get()) else {
        return "mapping_unavailable";
    };
    let Some(mut y1) = scaled_ceil(bottom_offset, info.height, image_bounds.height.get()) else {
        return "mapping_unavailable";
    };
    let width = info.width as usize;
    let height = info.height as usize;
    x0 = x0.min(width);
    x1 = x1.min(width);
    y0 = y0.min(height);
    y1 = y1.min(height);
    if x0 >= x1 || y0 >= y1 {
        return "mapping_unavailable";
    }

    if matches!(info.color_type, png::ColorType::Rgb | png::ColorType::Grayscale) {
        return "opaque_dominant";
    }

    let channels = match info.color_type {
        png::ColorType::Rgba => 4_usize,
        png::ColorType::GrayscaleAlpha => 2_usize,
        _ => return "unsupported_pixel_format",
    };
    let alpha_offset = channels - 1;
    let mut total = 0_usize;
    let mut opaque = 0_usize;
    let mut transparent = 0_usize;
    for y in y0..y1 {
        for x in x0..x1 {
            let index = (y * width + x)
                .checked_mul(channels)
                .and_then(|base| base.checked_add(alpha_offset));
            let Some(alpha) = index.and_then(|index| buffer.get(index)).copied() else {
                return "decode_unavailable";
            };
            total += 1;
            opaque += usize::from(alpha >= 250);
            transparent += usize::from(alpha <= 5);
        }
    }
    if total == 0 {
        return "mapping_unavailable";
    }
    let opaque_bp = opaque.saturating_mul(10_000) / total;
    let transparent_bp = transparent.saturating_mul(10_000) / total;
    if opaque_bp >= 9_000 {
        "opaque_dominant"
    } else if transparent_bp >= 9_000 {
        "transparent_dominant"
    } else {
        "mixed"
    }
}

#[test]
#[ignore = "requires exact public Virginia Remplacante fixture and receipt path"]
fn exact_virginia_p24_later_image_viewer_survival_probe() {
    let (Ok(fixture), Ok(output), Ok(expected_sha)) = (
        env::var("CHAPTERA_VIRGINIA_RESTORED_FILL_FIXTURE"),
        env::var("CHAPTERA_VIRGINIA_OCCLUSION_VIEWER_OUT"),
        env::var("CHAPTERA_VIRGINIA_RESTORED_FILL_SHA256"),
    ) else {
        return;
    };
    let fixture = PathBuf::from(fixture);
    let output = PathBuf::from(output);

    let bytes = fs::read(&fixture).expect("read exact Virginia Remplacante PUB");
    let bundle = open_pub_bundle(&bytes, viewer_geometry_environment_v0_1())
        .expect("open exact Virginia Remplacante through Viewer bundle");
    assert_eq!(
        bundle.geometry.document.source.source_hash.to_string(),
        expected_sha,
        "exact Virginia source identity"
    );

    let viewer_page = 24_u32;
    let page = bundle
        .geometry
        .document
        .pages
        .get((viewer_page - 1) as usize)
        .expect("Viewer p24 exists");
    let order = bundle
        .source_page_paint_orders
        .iter()
        .find(|order| order.page_id == page.id)
        .expect("Viewer p24 has bounded source paint order");

    let graph = &bundle.resolved_graph;
    let mut candidate_story_fill_count = 0_usize;
    let mut later_images = BTreeSet::new();
    let mut overlap_relations = BTreeSet::new();

    for (rank, node_id) in order.node_ids.iter().copied().enumerate() {
        let Some(node) = graph.nodes.get(&node_id) else {
            continue;
        };
        if node.payload.story_frame.is_none() {
            continue;
        }
        let Some(paint) = node.payload.effective_paint.as_ref() else {
            continue;
        };
        let complete_visible_solid = paint.fill.solid.as_ref().is_some_and(|value| value.value)
            && paint.fill.visible.as_ref().is_some_and(|value| value.value)
            && paint.fill.color_rgb.is_some();
        if !complete_visible_solid {
            continue;
        }
        candidate_story_fill_count += 1;

        for later_id in order.node_ids.iter().copied().skip(rank + 1) {
            let Some(later) = graph.nodes.get(&later_id) else {
                continue;
            };
            if later.payload.image_slot.is_none()
                || !overlaps(node.header.bounds, later.header.bounds)
            {
                continue;
            }
            later_images.insert(later_id);
            overlap_relations.insert((node_id, later_id));
        }
    }

    let scene_survivor_count = later_images
        .iter()
        .filter(|node_id| {
            bundle
                .geometry
                .scene
                .nodes
                .iter()
                .any(|node| node.origin == **node_id)
        })
        .count();

    let embedded_image_materialized_count = later_images
        .iter()
        .filter(|node_id| {
            bundle
                .geometry
                .images
                .iter()
                .any(|image| image.node_ids.contains(node_id))
        })
        .count();

    let viewer_paint_entry_count = later_images
        .iter()
        .filter(|node_id| {
            bundle
                .geometry
                .paints
                .iter()
                .any(|paint| paint.node_id == **node_id)
        })
        .count();

    let viewer_visible_paint_count = later_images
        .iter()
        .filter(|node_id| {
            bundle.geometry.paints.iter().any(|paint| {
                paint.node_id == **node_id
                    && (paint.solid_fill_rgb.is_some() || paint.solid_line.is_some())
            })
        })
        .count();

    let mut later_image_mime_histogram = BTreeMap::new();
    let mut overlap_area_histogram = BTreeMap::new();
    let mut overlap_alpha_histogram = BTreeMap::new();
    for (fill_id, image_id) in &overlap_relations {
        let (Some(fill), Some(image_node)) = (
            graph.nodes.get(fill_id),
            graph.nodes.get(image_id),
        ) else {
            continue;
        };
        let Some(overlap) = intersection(fill.header.bounds, image_node.header.bounds) else {
            continue;
        };
        bump(
            &mut overlap_area_histogram,
            overlap_area_class(image_node.header.bounds, overlap),
        );
        let Some(image) = bundle
            .geometry
            .images
            .iter()
            .find(|image| image.node_ids.contains(image_id))
        else {
            bump(&mut overlap_alpha_histogram, "resource_unavailable");
            continue;
        };
        let mime_class = match image.mime.as_str() {
            "image/png" => "png",
            "image/jpeg" | "image/jpg" => "jpeg",
            _ => "other",
        };
        bump(&mut later_image_mime_histogram, mime_class);
        let alpha_class = match mime_class {
            "png" => png_overlap_alpha_class(&image.bytes, image_node.header.bounds, overlap),
            "jpeg" => "opaque_dominant",
            _ => "unsupported_resource",
        };
        bump(&mut overlap_alpha_histogram, alpha_class);
    }

    let receipt = Receipt {
        schema: "chaptera.virginia-p24-occluder-viewer-survival.v1",
        viewer_page,
        candidate_story_fill_count,
        later_overlapping_image_count: later_images.len(),
        later_overlap_relation_count: overlap_relations.len(),
        later_image_mime_histogram,
        overlap_area_histogram,
        overlap_alpha_histogram,
        scene_survivor_count,
        embedded_image_materialized_count,
        viewer_paint_entry_count,
        viewer_visible_paint_count,
        claims: Claims {
            exact_source_identity_checked: true,
            node_ids_emitted: false,
            coordinates_emitted: false,
            colors_emitted: false,
            text_emitted: false,
            pdf_used_as_semantic_authority: false,
        },
    };

    assert!(
        receipt.later_overlapping_image_count > 0,
        "p24 must retain the Stage-L later-overlapping image discriminator"
    );

    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).expect("create Viewer occlusion receipt directory");
    }
    fs::write(
        &output,
        serde_json::to_vec_pretty(&receipt).expect("serialize Viewer occlusion receipt"),
    )
    .expect("write Viewer occlusion receipt");

    println!(
        "VIRGINIA_P24_OCCLUDER_VIEWER_SURVIVAL candidates={} later_images={} relations={} mime={:?} overlap_area={:?} overlap_alpha={:?} scene={} embedded={} paint={} visible_paint={}",
        receipt.candidate_story_fill_count,
        receipt.later_overlapping_image_count,
        receipt.later_overlap_relation_count,
        receipt.later_image_mime_histogram,
        receipt.overlap_area_histogram,
        receipt.overlap_alpha_histogram,
        receipt.scene_survivor_count,
        receipt.embedded_image_materialized_count,
        receipt.viewer_paint_entry_count,
        receipt.viewer_visible_paint_count,
    );
}
