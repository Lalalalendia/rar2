use std::{
    collections::BTreeMap,
    env, fs,
    io::Cursor,
    path::PathBuf,
};

use pub_cfb::read_stream_path;
use pub_core::StreamPath;
use pub_escher::{FoptObservation, PUBLISHER_FIELD_SHAPE_ID, inspect_sp_containers};
use pub_model::{Affine2D, Sha256Digest};
use pub_reader::build_mature_0x2c_source_graph;
use serde::Serialize;
use sha2::{Digest, Sha256};

const EXPECTED_REMPLACANTE_SHA256: &str =
    "88f57d800aeec808798ea487b9d4ab85dc85c02cd190b987a332709b81018506";
const ROTATION: u16 = 0x0004;

fn bump(map: &mut BTreeMap<String, usize>, key: impl Into<String>) {
    *map.entry(key.into()).or_default() += 1;
}

fn rotation_profile(records: &[FoptObservation]) -> &'static str {
    let matches = records
        .iter()
        .flat_map(|record| record.properties.iter())
        .filter(|property| property.property_id() == ROTATION)
        .collect::<Vec<_>>();
    if matches.is_empty() {
        return "absent";
    }
    if matches
        .iter()
        .any(|property| property.f_bid() || property.f_complex())
    {
        return "malformed_or_complex";
    }
    if matches.len() != 1 {
        return "duplicate_scalar";
    }
    if matches[0].op == 0 {
        "single_scalar_zero"
    } else {
        "single_scalar_nonzero"
    }
}

fn transform_class(transform: &Affine2D) -> &'static str {
    if *transform == Affine2D::identity() {
        "identity"
    } else if transform.a.as_str() == "1"
        && transform.b.as_str() == "0"
        && transform.c.as_str() == "0"
        && transform.d.as_str() == "1"
    {
        "translate_only"
    } else if transform.b.as_str() == "0" && transform.c.as_str() == "0" {
        "axis_scale_translate"
    } else {
        "general_affine"
    }
}

#[derive(Debug, Serialize)]
struct Receipt {
    schema: &'static str,
    exact_source_identity_checked: bool,
    candidate_page_count: usize,
    candidate_node_count: usize,
    candidate_image_count: usize,
    candidate_story_count: usize,
    image_shape_join_count: usize,
    image_raw_rotation_histogram: BTreeMap<String, usize>,
    image_source_transform_histogram: BTreeMap<String, usize>,
    claims: Claims,
}

#[derive(Debug, Serialize)]
struct Claims {
    raw_rotation_values_emitted: bool,
    coordinates_emitted: bool,
    object_ids_emitted: bool,
    source_text_emitted: bool,
    pdf_used_as_semantic_authority: bool,
}

#[test]
#[ignore = "requires exact public Virginia Remplacante fixture"]
fn exact_virginia_p1_image_rotation_source_probe() {
    let fixture = PathBuf::from(
        env::var("CHAPTERA_VIRGINIA_P1_PUB")
            .expect("CHAPTERA_VIRGINIA_P1_PUB must name the exact public fixture"),
    );
    let output = PathBuf::from(
        env::var("CHAPTERA_VIRGINIA_P1_ROTATION_RECEIPT")
            .expect("CHAPTERA_VIRGINIA_P1_ROTATION_RECEIPT must name the sanitized receipt"),
    );

    let bytes = fs::read(&fixture).expect("read exact public Virginia Remplacante fixture");
    let actual_sha256 = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    assert_eq!(
        actual_sha256, EXPECTED_REMPLACANTE_SHA256,
        "exact Virginia Remplacante source identity drift"
    );

    let source_hash: Sha256Digest = actual_sha256.parse().expect("valid exact source SHA-256");
    let build = build_mature_0x2c_source_graph(Cursor::new(bytes.as_slice()), source_hash)
        .expect("build exact Virginia source graph");

    let escher = read_stream_path(&fixture, "/Escher/EscherStm")
        .expect("read exact Virginia Escher stream");
    let inventory = inspect_sp_containers(StreamPath("/Escher/EscherStm".to_owned()), &escher)
        .expect("inspect exact Virginia OfficeArt shapes");

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

    let mut candidates = Vec::new();
    for page_id in build.graph.document.pages.iter().copied() {
        let page_canonical = page_id.into_canonical();
        let nodes = build
            .graph
            .nodes
            .values()
            .filter(|node| node.header.parent_id == page_canonical)
            .collect::<Vec<_>>();
        let image_count = nodes
            .iter()
            .filter(|node| node.payload.image_slot.is_some())
            .count();
        let story_count = nodes
            .iter()
            .filter(|node| node.payload.story_frame.is_some())
            .count();
        let table_count = nodes
            .iter()
            .filter(|node| node.payload.table.is_some())
            .count();
        let other_count = nodes.len() - image_count - story_count - table_count;

        if nodes.len() == 4
            && image_count == 2
            && story_count == 2
            && table_count == 0
            && other_count == 0
        {
            candidates.push((page_id, nodes));
        }
    }

    assert_eq!(
        candidates.len(),
        1,
        "exact p1 source cohort must be uniquely identified by 2 IMAGE + 2 Story direct-page nodes"
    );

    let (_page_id, nodes) = candidates.pop().expect("unique p1 candidate");
    let mut image_raw_rotation_histogram = BTreeMap::<String, usize>::new();
    let mut image_source_transform_histogram = BTreeMap::<String, usize>::new();
    let mut image_shape_join_count = 0_usize;

    for node in &nodes {
        if node.payload.image_slot.is_none() {
            continue;
        }
        bump(
            &mut image_source_transform_histogram,
            transform_class(&node.header.transform),
        );

        let matches = shapes_by_seq
            .get(&node.payload.contents_seq_num)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let [shape_index] = matches else {
            bump(&mut image_raw_rotation_histogram, "shape_join_unavailable");
            continue;
        };
        image_shape_join_count += 1;
        bump(
            &mut image_raw_rotation_histogram,
            rotation_profile(&inventory.shapes[*shape_index].fopts),
        );
    }

    let receipt = Receipt {
        schema: "chaptera.viewer-virginia-p1-image-rotation-source.v1",
        exact_source_identity_checked: true,
        candidate_page_count: 1,
        candidate_node_count: nodes.len(),
        candidate_image_count: nodes
            .iter()
            .filter(|node| node.payload.image_slot.is_some())
            .count(),
        candidate_story_count: nodes
            .iter()
            .filter(|node| node.payload.story_frame.is_some())
            .count(),
        image_shape_join_count,
        image_raw_rotation_histogram,
        image_source_transform_histogram,
        claims: Claims {
            raw_rotation_values_emitted: false,
            coordinates_emitted: false,
            object_ids_emitted: false,
            source_text_emitted: false,
            pdf_used_as_semantic_authority: false,
        },
    };

    assert_eq!(receipt.candidate_image_count, 2, "p1 image count drift");
    assert_eq!(
        receipt.image_shape_join_count, 2,
        "both p1 IMAGE nodes must uniquely join to OfficeArt shapes"
    );

    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).expect("create p1 rotation receipt directory");
    }
    fs::write(
        &output,
        serde_json::to_vec_pretty(&receipt).expect("serialize p1 rotation receipt"),
    )
    .expect("write p1 rotation receipt");

    println!(
        "VIRGINIA_P1_ROTATION_PROBE {}",
        serde_json::to_string(&receipt).expect("serialize p1 rotation receipt for log")
    );
}
