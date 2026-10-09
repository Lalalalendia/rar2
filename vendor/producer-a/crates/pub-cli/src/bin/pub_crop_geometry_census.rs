use anyhow::{Context, Result};
use pub_cfb::read_stream_path;
use pub_core::StreamPath;
use pub_escher::{
    DggDefaultOptionsObservation, FoptObservation, PUBLISHER_FIELD_SHAPE_ID,
    inspect_dgg_default_options, inspect_sp_containers,
};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::path::Path;

const GEO_LEFT: u16 = 0x0140;
const GEO_TOP: u16 = 0x0141;
const GEO_RIGHT: u16 = 0x0142;
const GEO_BOTTOM: u16 = 0x0143;
const SHAPE_PATH: u16 = 0x0144;
const P_VERTICES: u16 = 0x0145;
const P_SEGMENT_INFO: u16 = 0x0146;
const SHAPE_PATH_LINES_CLOSED: u32 = 0x0000_0001;

#[derive(Clone, Copy)]
enum ScalarLayer {
    Absent,
    Value(u32),
    Unresolved,
}

fn scalar_layer(records: &[FoptObservation], property_id: u16) -> ScalarLayer {
    let matches = records
        .iter()
        .flat_map(|record| record.properties.iter())
        .filter(|property| property.property_id() == property_id)
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [] => ScalarLayer::Absent,
        [property] if !property.f_bid() && !property.f_complex() => ScalarLayer::Value(property.op),
        _ => ScalarLayer::Unresolved,
    }
}

fn scalar_profile(records: &[FoptObservation], property_id: u16) -> &'static str {
    match scalar_layer(records, property_id) {
        ScalarLayer::Absent => "absent",
        ScalarLayer::Value(_) => "single_scalar",
        ScalarLayer::Unresolved => "unresolved",
    }
}

fn complex_profile(records: &[FoptObservation], property_id: u16) -> &'static str {
    let matches = records
        .iter()
        .flat_map(|record| record.properties.iter())
        .filter(|property| property.property_id() == property_id)
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [] => "absent",
        [property] if property.f_complex() => "single_complex",
        [property] if !property.f_bid() && property.op == 0 => "empty_default",
        [_] => "malformed_or_noncomplex",
        _ => "duplicate",
    }
}

fn geometry_layer(records: &[FoptObservation]) -> &'static str {
    let geo = [
        scalar_profile(records, GEO_LEFT),
        scalar_profile(records, GEO_TOP),
        scalar_profile(records, GEO_RIGHT),
        scalar_profile(records, GEO_BOTTOM),
    ];
    let shape_path = scalar_layer(records, SHAPE_PATH);
    let vertices = complex_profile(records, P_VERTICES);
    let segments = complex_profile(records, P_SEGMENT_INFO);

    if geo.iter().any(|value| *value == "unresolved")
        || matches!(shape_path, ScalarLayer::Unresolved)
        || matches!(vertices, "malformed_or_noncomplex" | "duplicate")
        || matches!(segments, "malformed_or_noncomplex" | "duplicate")
    {
        return "unresolved";
    }
    if vertices == "single_complex"
        || segments == "single_complex"
        || matches!(shape_path, ScalarLayer::Value(value) if value != SHAPE_PATH_LINES_CLOSED)
    {
        return "custom_path";
    }
    if geo.iter().any(|value| *value != "absent") {
        return "explicit_rect_space";
    }
    if matches!(shape_path, ScalarLayer::Value(_))
        || vertices == "empty_default"
        || segments == "empty_default"
    {
        return "default_rect";
    }
    "absent"
}

fn effective_geometry(
    shape: &pub_escher::SpContainerObservation,
    dgg: Option<&DggDefaultOptionsObservation>,
) -> String {
    let local = geometry_layer(&shape.fopts);
    if local != "absent" {
        return format!("shape_local:{local}");
    }
    if let Some(dgg) = dgg {
        let primary = geometry_layer(&dgg.primary_options);
        if primary != "absent" {
            return format!("drawing_group_primary:{primary}");
        }
        let tertiary = geometry_layer(&dgg.tertiary_options);
        if tertiary != "absent" {
            return format!("drawing_group_tertiary:{tertiary}");
        }
    }
    "normative_default:default_rect".to_owned()
}

fn main() -> Result<()> {
    let input = env::args()
        .nth(1)
        .context("usage: pub-crop-geometry-census INPUT.pub")?;
    let path = Path::new(&input);
    let bytes = fs::read(path).with_context(|| format!("read {}", path.display()))?;
    let source_sha256 = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();

    let bundle =
        pub_viewer::open_pub_bundle(&bytes, pub_viewer::viewer_geometry_environment_v0_1())
            .context("open source through Viewer")?;

    let window_nodes = bundle
        .geometry
        .images
        .iter()
        .flat_map(|image| {
            image
                .placements
                .iter()
                .filter(|placement| placement.source_window.is_some())
                .map(|placement| placement.node_id)
        })
        .collect::<BTreeSet<_>>();

    let escher = read_stream_path(path, "/Escher/EscherStm").context("read Escher stream")?;
    let inventory = inspect_sp_containers(StreamPath("/Escher/EscherStm".to_owned()), &escher)
        .context("inspect OfficeArt shapes")?;
    let dgg_inventory =
        inspect_dgg_default_options(StreamPath("/Escher/EscherStm".to_owned()), &escher)
            .context("inspect OfficeArt defaults")?;
    let dgg = (dgg_inventory.drawing_groups.len() == 1).then(|| &dgg_inventory.drawing_groups[0]);

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

    let mut classes = BTreeMap::<String, usize>::new();
    let mut shape_types = BTreeMap::<String, usize>::new();
    let mut joins = BTreeMap::<String, usize>::new();

    for node_id in window_nodes.iter().copied() {
        let Some(node) = bundle.resolved_graph.nodes.get(&node_id) else {
            *joins.entry("resolved_node_missing".into()).or_default() += 1;
            continue;
        };
        let matches = shapes_by_seq
            .get(&node.payload.contents_seq_num)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let [shape_index] = matches else {
            *joins
                .entry(
                    if matches.is_empty() {
                        "shape_join_missing"
                    } else {
                        "shape_join_ambiguous"
                    }
                    .into(),
                )
                .or_default() += 1;
            continue;
        };
        let shape = &inventory.shapes[*shape_index];
        *joins.entry("exact".into()).or_default() += 1;
        *classes.entry(effective_geometry(shape, dgg)).or_default() += 1;
        *shape_types
            .entry(
                shape
                    .fsp
                    .as_ref()
                    .map(|fsp| format!("0x{:04X}", fsp.shape_type))
                    .unwrap_or_else(|| "none".to_owned()),
            )
            .or_default() += 1;
    }

    let output = json!({
        "schema": "chaptera.pub-crop-shape-geometry-census.v1",
        "fixture": path.file_stem().and_then(|value| value.to_str()).unwrap_or("input"),
        "source_sha256": source_sha256,
        "source_window_node_count": window_nodes.len(),
        "geometry_class_counts": classes,
        "shape_type_counts": shape_types,
        "join_counts": joins,
        "claims": {
            "source_only": true,
            "publisher_pdf_used_as_authority": false,
            "node_ids_emitted": false,
            "geometry_coordinates_emitted": false,
            "path_vertices_emitted": false,
            "raw_property_values_emitted": false
        }
    });
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}
