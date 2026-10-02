use std::{collections::BTreeMap, env, fs, io::Cursor, path::PathBuf};

use pub_cfb::read_stream_path;
use pub_core::StreamPath;
use pub_escher::{FoptObservation, PUBLISHER_FIELD_SHAPE_ID, inspect_sp_containers};
use pub_model::Sha256Digest;
use pub_reader::build_mature_0x2c_source_graph;
use serde::Serialize;
use sha2::{Digest, Sha256};

const EXPECTED_REMPLACANTE_SHA256: &str =
    "88f57d800aeec808798ea487b9d4ab85dc85c02cd190b987a332709b81018506";
const ROTATION: u16 = 0x0004;
const FSP_FLIP_H: u32 = 1 << 6;
const FSP_FLIP_V: u32 = 1 << 7;
const FIXED_ONE_DEGREE: i64 = 65_536;
const FULL_TURN: i64 = 360 * FIXED_ONE_DEGREE;
const HALF_TURN: i64 = 180 * FIXED_ONE_DEGREE;

fn bump(map: &mut BTreeMap<String, usize>, key: impl Into<String>) {
    *map.entry(key.into()).or_default() += 1;
}

fn scalar_rotation(records: &[FoptObservation]) -> Result<Option<i64>, &'static str> {
    let matches = records
        .iter()
        .flat_map(|record| record.properties.iter())
        .filter(|property| property.property_id() == ROTATION)
        .collect::<Vec<_>>();

    if matches.is_empty() {
        return Ok(None);
    }
    if matches
        .iter()
        .any(|property| property.f_bid() || property.f_complex())
    {
        return Err("malformed_or_complex");
    }
    if matches.len() != 1 {
        return Err("duplicate_scalar");
    }

    let mut angle = i64::from(matches[0].op as i32) % FULL_TURN;
    if angle > HALF_TURN {
        angle -= FULL_TURN;
    } else if angle < -HALF_TURN {
        angle += FULL_TURN;
    }
    Ok(Some(angle))
}

fn rotation_profile(records: &[FoptObservation]) -> &'static str {
    match scalar_rotation(records) {
        Ok(None) => "absent",
        Ok(Some(0)) => "single_scalar_zero",
        Ok(Some(_)) => "single_scalar_nonzero",
        Err(class) => class,
    }
}

fn sign_class(angle: i64) -> &'static str {
    if angle > 0 {
        "positive"
    } else if angle < 0 {
        "negative"
    } else {
        "zero"
    }
}

fn magnitude_class(angle: i64) -> &'static str {
    let magnitude = angle.abs();
    if magnitude < 15 * FIXED_ONE_DEGREE {
        "lt_15"
    } else if magnitude < 45 * FIXED_ONE_DEGREE {
        "15_to_45"
    } else if magnitude < 90 * FIXED_ONE_DEGREE {
        "45_to_90"
    } else {
        "90_to_180"
    }
}

fn cardinal_class(angle: i64) -> &'static str {
    let magnitude = angle.abs();
    if magnitude == 90 * FIXED_ONE_DEGREE || magnitude == 180 * FIXED_ONE_DEGREE {
        "exact_cardinal"
    } else {
        "non_cardinal"
    }
}

fn aspect_class(width: i64, height: i64) -> &'static str {
    let delta = (width - height).abs();
    let max_side = width.max(height);
    if max_side > 0 && delta * 10 <= max_side {
        "near_square"
    } else if width > height {
        "landscape"
    } else {
        "portrait"
    }
}

#[derive(Debug, Default, Serialize)]
struct PageReceipt {
    viewer_page: usize,
    direct_image_count: usize,
    image_shape_join_count: usize,
    rotation_profile_histogram: BTreeMap<String, usize>,
    nonzero_rotation_sign_histogram: BTreeMap<String, usize>,
    nonzero_rotation_magnitude_histogram: BTreeMap<String, usize>,
    nonzero_rotation_cardinal_histogram: BTreeMap<String, usize>,
    nonzero_rotation_flip_histogram: BTreeMap<String, usize>,
    nonzero_rotation_aspect_histogram: BTreeMap<String, usize>,
    nonzero_rotation_crop_histogram: BTreeMap<String, usize>,
}

#[derive(Debug, Serialize)]
struct Receipt {
    schema: &'static str,
    exact_source_identity_checked: bool,
    pages: Vec<PageReceipt>,
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
fn exact_virginia_direct_image_rotation_applicability_probe() {
    let fixture = PathBuf::from(
        env::var("CHAPTERA_VIRGINIA_ROTATION_APPLICABILITY_PUB").expect(
            "CHAPTERA_VIRGINIA_ROTATION_APPLICABILITY_PUB must name the exact public fixture",
        ),
    );
    let output = PathBuf::from(
        env::var("CHAPTERA_VIRGINIA_ROTATION_APPLICABILITY_RECEIPT").expect(
            "CHAPTERA_VIRGINIA_ROTATION_APPLICABILITY_RECEIPT must name the sanitized receipt",
        ),
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

    let escher =
        read_stream_path(&fixture, "/Escher/EscherStm").expect("read exact Virginia Escher stream");
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

    let mut pages = Vec::new();
    for (page_index, page_id) in build.graph.document.pages.iter().copied().enumerate() {
        let page_canonical = page_id.into_canonical();
        let mut receipt = PageReceipt {
            viewer_page: page_index + 1,
            ..PageReceipt::default()
        };

        for node in build
            .graph
            .nodes
            .values()
            .filter(|node| node.header.parent_id == page_canonical)
            .filter(|node| node.payload.image_slot.is_some())
            .filter(|node| {
                !node.header.source_refs.iter().any(|source| {
                    source
                        .object_key
                        .as_deref()
                        .is_some_and(|key| key.starts_with("escher/group-ancestor/"))
                })
            })
        {
            receipt.direct_image_count += 1;

            let matches = shapes_by_seq
                .get(&node.payload.contents_seq_num)
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            let [shape_index] = matches else {
                bump(
                    &mut receipt.rotation_profile_histogram,
                    "shape_join_unavailable",
                );
                continue;
            };
            receipt.image_shape_join_count += 1;

            let shape = &inventory.shapes[*shape_index];
            bump(
                &mut receipt.rotation_profile_histogram,
                rotation_profile(&shape.fopts),
            );

            let Ok(Some(angle)) = scalar_rotation(&shape.fopts) else {
                continue;
            };
            if angle == 0 {
                continue;
            }

            bump(
                &mut receipt.nonzero_rotation_sign_histogram,
                sign_class(angle),
            );
            bump(
                &mut receipt.nonzero_rotation_magnitude_histogram,
                magnitude_class(angle),
            );
            bump(
                &mut receipt.nonzero_rotation_cardinal_histogram,
                cardinal_class(angle),
            );

            let flags = shape.fsp.as_ref().map_or(0, |fsp| fsp.flags);
            let flip = match (flags & FSP_FLIP_H != 0, flags & FSP_FLIP_V != 0) {
                (false, false) => "none",
                (true, false) => "h_only",
                (false, true) => "v_only",
                (true, true) => "hv",
            };
            bump(&mut receipt.nonzero_rotation_flip_histogram, flip);

            bump(
                &mut receipt.nonzero_rotation_aspect_histogram,
                aspect_class(
                    node.header.bounds.width.get(),
                    node.header.bounds.height.get(),
                ),
            );
            bump(
                &mut receipt.nonzero_rotation_crop_histogram,
                if node.payload.explicit_image_crop.is_some() {
                    "explicit_crop"
                } else {
                    "no_explicit_crop"
                },
            );
        }

        pages.push(receipt);
    }

    let receipt = Receipt {
        schema: "chaptera.viewer-virginia-direct-image-rotation-applicability.v1",
        exact_source_identity_checked: true,
        pages,
        claims: Claims {
            raw_rotation_values_emitted: false,
            coordinates_emitted: false,
            object_ids_emitted: false,
            source_text_emitted: false,
            pdf_used_as_semantic_authority: false,
        },
    };

    assert_eq!(
        receipt.pages.len(),
        25,
        "exact Virginia Remplacante Viewer page count drift"
    );
    assert!(
        receipt.pages[0]
            .rotation_profile_histogram
            .get("single_scalar_nonzero")
            .copied()
            .unwrap_or(0)
            >= 1,
        "p1 must retain at least one direct IMAGE with scalar nonzero rotation"
    );

    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).expect("create rotation applicability receipt directory");
    }
    fs::write(
        &output,
        serde_json::to_vec_pretty(&receipt).expect("serialize rotation applicability receipt"),
    )
    .expect("write rotation applicability receipt");

    println!(
        "VIRGINIA_ROTATION_APPLICABILITY_PROBE {}",
        serde_json::to_string(&receipt).expect("serialize rotation applicability receipt for log")
    );
}
