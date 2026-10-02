use pub_cfb::read_stream_path;
use pub_core::StreamPath;
use pub_escher::{
    DggDefaultOptionsObservation, FoptObservation, PUBLISHER_FIELD_SHAPE_ID,
    inspect_dgg_default_options, inspect_sp_containers,
};
use pub_model::{NodeId, RectEmu, Sha256Digest};
use pub_reader::build_mature_0x2c_source_graph;
use pub_viewer::{open_pub_bundle, viewer_geometry_environment_v0_1};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    io::Cursor,
    path::PathBuf,
};

const FILL_BOOLEANS: u16 = 0x01BF;
const FILL_USE_FILLED_BIT: u32 = 1 << 20;
const FILL_FILLED_BIT: u32 = 1 << 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LegacyScalarLayer {
    Absent,
    Value(u32),
    Unresolved,
}

fn legacy_scalar_layer(records: &[FoptObservation]) -> LegacyScalarLayer {
    let matches = records
        .iter()
        .flat_map(|record| record.properties.iter())
        .filter(|property| property.property_id() == FILL_BOOLEANS)
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [] => LegacyScalarLayer::Absent,
        [property] if !property.f_bid() && !property.f_complex() => {
            LegacyScalarLayer::Value(property.op)
        }
        _ => LegacyScalarLayer::Unresolved,
    }
}

fn legacy_fill_visibility(
    shape: &pub_escher::SpContainerObservation,
    dgg: Option<&DggDefaultOptionsObservation>,
) -> Option<bool> {
    let mut layers = vec![legacy_scalar_layer(&shape.fopts)];
    if let Some(dgg) = dgg {
        layers.push(legacy_scalar_layer(&dgg.primary_options));
        layers.push(legacy_scalar_layer(&dgg.tertiary_options));
    }

    for layer in layers {
        match layer {
            LegacyScalarLayer::Absent => {}
            LegacyScalarLayer::Unresolved => return None,
            LegacyScalarLayer::Value(raw) => {
                if raw & FILL_USE_FILLED_BIT == 0 {
                    continue;
                }
                return Some(raw & FILL_FILLED_BIT != 0);
            }
        }
    }
    Some(true)
}

fn rects_overlap(a: RectEmu, b: RectEmu) -> bool {
    let (Some(ar), Some(ab), Some(br), Some(bb)) = (a.right(), a.bottom(), b.right(), b.bottom())
    else {
        return false;
    };
    a.x < br && b.x < ar && a.y < bb && b.y < ab
}

#[derive(Debug, Serialize)]
struct Receipt {
    schema: &'static str,
    raw_page_ordinal: usize,
    restored_fill_count: usize,
    later_image_occluder_count: usize,
    viewer_page_selected: bool,
    viewer_scene_survival_count: usize,
    viewer_image_resource_binding_count: usize,
    viewer_image_nonempty_bytes_count: usize,
    viewer_scene_source_order_preserved_count: usize,
    guardrails: Vec<&'static str>,
}

#[test]
#[ignore = "requires exact public Virginia Remplacante fixture and receipt path"]
fn exact_virginia_p24_later_image_product_boundary_probe() {
    let fixture = env::var_os("CHAPTERA_VIRGINIA_OCCLUSION_FIXTURE")
        .map(PathBuf::from)
        .expect("CHAPTERA_VIRGINIA_OCCLUSION_FIXTURE");
    let output = env::var_os("CHAPTERA_VIRGINIA_OCCLUSION_OUT")
        .map(PathBuf::from)
        .expect("CHAPTERA_VIRGINIA_OCCLUSION_OUT");
    let expected_sha =
        env::var("CHAPTERA_VIRGINIA_OCCLUSION_SHA256").expect("CHAPTERA_VIRGINIA_OCCLUSION_SHA256");

    let bytes = fs::read(&fixture).expect("read exact Virginia PUB");
    let actual_sha = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    assert_eq!(actual_sha, expected_sha, "exact Virginia source identity");

    let source_hash: Sha256Digest = expected_sha.parse().expect("valid source SHA-256");
    let build = build_mature_0x2c_source_graph(Cursor::new(bytes.as_slice()), source_hash)
        .expect("build exact Virginia source graph");

    let escher =
        read_stream_path(&fixture, "/Escher/EscherStm").expect("read exact Virginia Escher stream");
    let inventory = inspect_sp_containers(StreamPath("/Escher/EscherStm".to_owned()), &escher)
        .expect("inspect exact Virginia OfficeArt shapes");
    let dgg_inventory =
        inspect_dgg_default_options(StreamPath("/Escher/EscherStm".to_owned()), &escher)
            .expect("inspect exact Virginia DGG defaults");
    assert!(dgg_inventory.drawing_groups.len() <= 1);
    let dgg = dgg_inventory.drawing_groups.first();

    let mut shapes_by_seq = BTreeMap::<u32, Vec<usize>>::new();
    for (index, shape) in inventory.shapes.iter().enumerate() {
        let Some(client_data) = shape.client_data.as_ref() else {
            continue;
        };
        let mut seqs = client_data
            .fields
            .iter()
            .filter(|field| field.id == PUBLISHER_FIELD_SHAPE_ID)
            .map(|field| field.value)
            .collect::<Vec<_>>();
        seqs.sort_unstable();
        seqs.dedup();
        for seq in seqs {
            shapes_by_seq.entry(seq).or_default().push(index);
        }
    }

    let raw_page_ordinal = 25usize;
    let page_id = build.graph.document.pages[raw_page_ordinal - 1];
    let page_canonical = page_id.into_canonical();
    let source_order = build
        .source_page_paint_orders
        .iter()
        .find(|order| order.page_id == page_id)
        .expect("raw PAGE25 source order");

    let restored = build
        .graph
        .nodes
        .values()
        .filter(|node| node.header.parent_id == page_canonical)
        .filter(|node| {
            let Some(paint) = node.payload.effective_paint.as_ref() else {
                return false;
            };
            let complete_visible_solid = paint.fill.solid.as_ref().is_some_and(|value| value.value)
                && paint.fill.visible.as_ref().is_some_and(|value| value.value)
                && paint.fill.color_rgb.is_some();
            if !complete_visible_solid {
                return false;
            }
            let matches = shapes_by_seq
                .get(&node.payload.contents_seq_num)
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            let [shape_index] = matches else {
                return false;
            };
            legacy_fill_visibility(&inventory.shapes[*shape_index], dgg).is_none()
        })
        .map(|node| node.header.id)
        .collect::<BTreeSet<_>>();
    assert_eq!(restored.len(), 3, "raw PAGE25 restored-fill cohort");

    let mut later_pairs = Vec::<(NodeId, NodeId)>::new();
    for restored_id in &restored {
        let restored_node = build.graph.nodes.get(restored_id).expect("restored node");
        let rank = source_order
            .node_ids
            .iter()
            .position(|candidate| candidate == restored_id)
            .expect("restored node source rank");
        for later_id in source_order.node_ids.iter().skip(rank + 1) {
            let Some(later) = build.graph.nodes.get(later_id) else {
                continue;
            };
            if later.payload.image_slot.is_some()
                && rects_overlap(restored_node.header.bounds, later.header.bounds)
            {
                later_pairs.push((*restored_id, *later_id));
            }
        }
    }
    let later_images = later_pairs
        .iter()
        .map(|(_, later_id)| *later_id)
        .collect::<BTreeSet<_>>();
    assert_eq!(
        later_images.len(),
        1,
        "Stage-L p24 later image occluder cohort"
    );

    let bundle = open_pub_bundle(&bytes, viewer_geometry_environment_v0_1())
        .expect("open exact Virginia through Viewer product boundary");
    let viewer_page_selected = bundle
        .geometry
        .document
        .pages
        .iter()
        .any(|page| page.id == page_id);

    let scene_positions = bundle
        .geometry
        .scene
        .nodes
        .iter()
        .enumerate()
        .map(|(index, node)| (node.origin, index))
        .collect::<BTreeMap<_, _>>();

    let viewer_scene_survival_count = later_images
        .iter()
        .filter(|node_id| scene_positions.contains_key(node_id))
        .count();
    let viewer_image_resource_binding_count = later_images
        .iter()
        .filter(|node_id| {
            bundle
                .geometry
                .images
                .iter()
                .any(|image| image.node_ids.contains(node_id))
        })
        .count();
    let viewer_image_nonempty_bytes_count = later_images
        .iter()
        .filter(|node_id| {
            bundle
                .geometry
                .images
                .iter()
                .any(|image| image.node_ids.contains(node_id) && !image.bytes.is_empty())
        })
        .count();
    let viewer_scene_source_order_preserved_count = later_pairs
        .iter()
        .filter(|(restored_id, later_id)| {
            let (Some(restored_rank), Some(later_rank)) = (
                scene_positions.get(restored_id),
                scene_positions.get(later_id),
            ) else {
                return false;
            };
            later_rank > restored_rank
        })
        .count();

    let receipt = Receipt {
        schema: "chaptera.virginia-p24-later-image-product-boundary.v1",
        raw_page_ordinal,
        restored_fill_count: restored.len(),
        later_image_occluder_count: later_images.len(),
        viewer_page_selected,
        viewer_scene_survival_count,
        viewer_image_resource_binding_count,
        viewer_image_nonempty_bytes_count,
        viewer_scene_source_order_preserved_count,
        guardrails: vec![
            "The restored-fill cohort is selected by the same source-side #614 A/B boundary as Stage L.",
            "The occluder cohort is restricted to later source-ordered overlapping image nodes on raw PAGE25.",
            "Viewer survival is measured only by canonical node identity; no source IDs are emitted.",
            "Image-resource admission is measured only as count + nonempty-byte class; no MIME, bytes, filenames, offsets, or image content are emitted.",
            "No PDF/raster evidence participates in this discriminator.",
        ],
    };

    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).expect("create occlusion receipt directory");
    }
    fs::write(
        &output,
        serde_json::to_vec_pretty(&receipt).expect("serialize occlusion receipt"),
    )
    .expect("write occlusion receipt");

    println!(
        "VIRGINIA_P24_OCCLUSION viewer_page_selected={} later_images={} scene_survival={} image_resources={} nonempty={} scene_order_preserved={}",
        receipt.viewer_page_selected,
        receipt.later_image_occluder_count,
        receipt.viewer_scene_survival_count,
        receipt.viewer_image_resource_binding_count,
        receipt.viewer_image_nonempty_bytes_count,
        receipt.viewer_scene_source_order_preserved_count,
    );
}
