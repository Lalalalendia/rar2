use pub_core::StreamPath;
use pub_escher::{
    Fopte, SpContainerObservation, inspect_sp_containers, OFFICE_ART_FOPT,
    OFFICE_ART_TERTIARY_FOPT, PUBLISHER_FIELD_SHAPE_ID,
};
use pub_viewer::{open_pub_bundle, viewer_geometry_environment_v0_1};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    io::Cursor,
    path::PathBuf,
};

const EXPECTED_SHA256: &str =
    "88f57d800aeec808798ea487b9d4ab85dc85c02cd190b987a332709b81018506";

const PICTURE_CONTRAST: u16 = 0x0108;
const PICTURE_BRIGHTNESS: u16 = 0x0109;
const PICTURE_RECOLOR: u16 = 0x011A;

const FILL_TYPE: u16 = 0x0180;
const FILL_COLOR: u16 = 0x0181;
const FILL_BACK_COLOR: u16 = 0x0183;
const FILL_BLIP: u16 = 0x0186;

fn unique_client_shape_id(shape: &SpContainerObservation) -> Option<u32> {
    let record = shape.client_data.as_ref()?;
    let mut values = record
        .fields
        .iter()
        .filter(|field| field.id == PUBLISHER_FIELD_SHAPE_ID)
        .map(|field| field.value);
    let first = values.next()?;
    values.next().is_none().then_some(first)
}

fn properties<'a>(shape: &'a SpContainerObservation, id: u16) -> Vec<(&'a Fopte, u16)> {
    shape
        .fopts
        .iter()
        .flat_map(|record| {
            record
                .properties
                .iter()
                .filter(move |property| property.property_id() == id)
                .map(move |property| (property, record.rec_type))
        })
        .collect()
}

fn scalar_property<'a>(shape: &'a SpContainerObservation, id: u16) -> Option<&'a Fopte> {
    let values = properties(shape, id);
    let [(property, _)] = values.as_slice() else {
        return None;
    };
    (!property.f_bid() && !property.f_complex()).then_some(*property)
}

fn property_storage_class(shape: &SpContainerObservation, id: u16) -> String {
    let values = properties(shape, id);
    if values.is_empty() {
        return "absent".into();
    }
    if values
        .iter()
        .any(|(property, _)| property.f_bid() || property.f_complex())
    {
        return "flagged_or_complex".into();
    }

    let record_types = values
        .iter()
        .map(|(_, rec_type)| *rec_type)
        .collect::<BTreeSet<_>>();
    match record_types.as_slice() {
        [OFFICE_ART_FOPT] => "primary_fopt".into(),
        [OFFICE_ART_TERTIARY_FOPT] => "tertiary_fopt".into(),
        [_] => "other_fopt".into(),
        _ => "multiple_fopt_layers".into(),
    }
}

fn colorref_class(shape: &SpContainerObservation, id: u16) -> String {
    let Some(property) = scalar_property(shape, id) else {
        return if properties(shape, id).is_empty() {
            "absent".into()
        } else {
            "ambiguous_or_flagged".into()
        };
    };
    match (property.op >> 24) as u8 {
        0x00 => "direct_rgb".into(),
        0x08 => "scheme".into(),
        _ => "other_colorref".into(),
    }
}

fn fill_type_class(shape: &SpContainerObservation) -> String {
    let Some(property) = scalar_property(shape, FILL_TYPE) else {
        return if properties(shape, FILL_TYPE).is_empty() {
            "absent".into()
        } else {
            "ambiguous_or_flagged".into()
        };
    };
    match property.op {
        0 => "solid".into(),
        1 => "pattern".into(),
        2 => "texture".into(),
        3 => "picture".into(),
        4..=8 => "shade_family".into(),
        9 => "background".into(),
        _ => "other".into(),
    }
}

fn bg_pxid_class(shape: &SpContainerObservation) -> String {
    let values = properties(shape, FILL_BLIP);
    if values.is_empty() {
        return "absent".into();
    }
    let bids = values
        .iter()
        .filter(|(property, _)| property.f_bid() && !property.f_complex())
        .map(|(property, _)| property.op)
        .collect::<BTreeSet<_>>();
    if bids.is_empty() {
        "present_not_bid".into()
    } else if bids.len() == 1 && values.len() == 1 {
        "unique_bstore_ref".into()
    } else {
        "ambiguous".into()
    }
}

fn bump(map: &mut BTreeMap<String, usize>, key: impl Into<String>) {
    *map.entry(key.into()).or_default() += 1;
}

fn positive_area_ratio_at_least_one_percent(
    bounds_width: i64,
    bounds_height: i64,
    page_width: i64,
    page_height: i64,
) -> bool {
    if bounds_width <= 0 || bounds_height <= 0 || page_width <= 0 || page_height <= 0 {
        return false;
    }
    let area = i128::from(bounds_width) * i128::from(bounds_height);
    let page_area = i128::from(page_width) * i128::from(page_height);
    area * 100 >= page_area
}

#[derive(Debug, Default, Serialize)]
struct PageReceipt {
    viewer_page: u32,
    scene_node_count: usize,
    canonical_node_count: usize,
    exact_shape_join_count: usize,
    missing_shape_join_count: usize,
    ambiguous_shape_join_count: usize,

    image_like_count: usize,
    image_resource_bound_count: usize,
    image_recolor_count: usize,
    image_recolor_resource_bound_count: usize,
    image_brightness_count: usize,
    image_contrast_count: usize,
    recolor_storage_classes: BTreeMap<String, usize>,
    recolor_colorref_classes: BTreeMap<String, usize>,

    non_table_shape_count: usize,
    large_non_table_shape_count: usize,
    fill_type_classes: BTreeMap<String, usize>,
    large_fill_type_classes: BTreeMap<String, usize>,
    fill_fore_colorref_classes: BTreeMap<String, usize>,
    fill_back_colorref_classes: BTreeMap<String, usize>,

    pattern_fill_count: usize,
    pattern_fill_bg_pxid_count: usize,
    pattern_fill_viewer_solid_fill_count: usize,
    pattern_bg_pxid_classes: BTreeMap<String, usize>,
}

#[derive(Debug, Serialize)]
struct Receipt {
    schema: &'static str,
    source_sha256: String,
    pages: Vec<PageReceipt>,
    claims: Claims,
}

#[derive(Debug, Serialize)]
struct Claims {
    pdf_geometry_used: bool,
    pdf_color_used: bool,
    page_or_hash_product_rule_used: bool,
    canonical_picture_recolor_field_available: bool,
    viewer_pattern_fill_surface_available: bool,
    large_shape_threshold: &'static str,
    recolor_authority_note: &'static str,
    pattern_authority_note: &'static str,
}

#[test]
#[ignore = "requires exact public Virginia Remplacante fixture"]
fn exact_virginia_p21_color_effect_source_census() {
    let fixture = PathBuf::from(
        env::var_os("CHAPTERA_VIRGINIA_P21_COLOR_FIXTURE")
            .expect("CHAPTERA_VIRGINIA_P21_COLOR_FIXTURE"),
    );
    let output = PathBuf::from(
        env::var_os("CHAPTERA_VIRGINIA_P21_COLOR_OUT")
            .expect("CHAPTERA_VIRGINIA_P21_COLOR_OUT"),
    );

    let bytes = fs::read(&fixture).expect("read Virginia fixture");
    let sha = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    assert_eq!(sha, EXPECTED_SHA256, "exact Virginia source identity");

    let bundle =
        open_pub_bundle(&bytes, viewer_geometry_environment_v0_1()).expect("open Viewer bundle");
    let escher = pub_cfb::read_stream_reader(
        Cursor::new(bytes.as_slice()),
        pub_reader::ESCHER_STREAM_PATH,
    )
    .expect("read Escher stream");
    let inventory =
        inspect_sp_containers(StreamPath(pub_reader::ESCHER_STREAM_PATH.into()), &escher)
            .expect("inspect SpContainers");

    let mut shapes_by_seq = BTreeMap::<u32, Vec<usize>>::new();
    for (index, shape) in inventory.shapes.iter().enumerate() {
        if let Some(seq_num) = unique_client_shape_id(shape) {
            shapes_by_seq.entry(seq_num).or_default().push(index);
        }
    }

    let viewer_paints = bundle
        .geometry
        .paints
        .iter()
        .map(|paint| (paint.node_id, paint))
        .collect::<BTreeMap<_, _>>();
    let image_bound_nodes = bundle
        .geometry
        .images
        .iter()
        .flat_map(|image| image.node_ids.iter().copied())
        .collect::<BTreeSet<_>>();

    let mut pages = Vec::new();

    for viewer_page in [6_u32, 20, 21, 22] {
        let page = bundle
            .geometry
            .document
            .pages
            .iter()
            .find(|page| page.index == viewer_page)
            .expect("selected Viewer page");
        let page_origin = page.id.into_canonical();

        let scene_node_ids = bundle
            .geometry
            .scene
            .nodes
            .iter()
            .filter(|node| node.parent_origin == page_origin)
            .map(|node| node.origin)
            .collect::<BTreeSet<_>>();

        let mut receipt = PageReceipt {
            viewer_page,
            scene_node_count: scene_node_ids.len(),
            ..PageReceipt::default()
        };

        for node_id in scene_node_ids {
            let Some(node) = bundle.resolved_graph.nodes.get(&node_id) else {
                continue;
            };
            receipt.canonical_node_count += 1;

            let matches = shapes_by_seq
                .get(&node.payload.contents_seq_num)
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            let shape = match matches {
                [] => {
                    receipt.missing_shape_join_count += 1;
                    continue;
                }
                [index] => {
                    receipt.exact_shape_join_count += 1;
                    &inventory.shapes[*index]
                }
                _ => {
                    receipt.ambiguous_shape_join_count += 1;
                    continue;
                }
            };

            let image_like = node.payload.image_slot.is_some();
            if image_like {
                receipt.image_like_count += 1;
                if image_bound_nodes.contains(&node_id) {
                    receipt.image_resource_bound_count += 1;
                }

                let recolor = properties(shape, PICTURE_RECOLOR);
                if !recolor.is_empty() {
                    receipt.image_recolor_count += 1;
                    if image_bound_nodes.contains(&node_id) {
                        receipt.image_recolor_resource_bound_count += 1;
                    }
                    bump(
                        &mut receipt.recolor_storage_classes,
                        property_storage_class(shape, PICTURE_RECOLOR),
                    );
                    bump(
                        &mut receipt.recolor_colorref_classes,
                        colorref_class(shape, PICTURE_RECOLOR),
                    );
                }

                if !properties(shape, PICTURE_BRIGHTNESS).is_empty() {
                    receipt.image_brightness_count += 1;
                }
                if !properties(shape, PICTURE_CONTRAST).is_empty() {
                    receipt.image_contrast_count += 1;
                }
            }

            if node.payload.table.is_some() || node.payload.officeart_shape_type.is_none() {
                continue;
            }

            receipt.non_table_shape_count += 1;
            let fill_class = fill_type_class(shape);
            bump(&mut receipt.fill_type_classes, fill_class.clone());
            bump(
                &mut receipt.fill_fore_colorref_classes,
                colorref_class(shape, FILL_COLOR),
            );
            bump(
                &mut receipt.fill_back_colorref_classes,
                colorref_class(shape, FILL_BACK_COLOR),
            );

            let large = positive_area_ratio_at_least_one_percent(
                node.header.bounds.width.get(),
                node.header.bounds.height.get(),
                page.width_emu,
                page.height_emu,
            );
            if large {
                receipt.large_non_table_shape_count += 1;
                bump(&mut receipt.large_fill_type_classes, fill_class.clone());
            }

            if fill_class == "pattern" {
                receipt.pattern_fill_count += 1;
                let bg_class = bg_pxid_class(shape);
                if bg_class == "unique_bstore_ref" {
                    receipt.pattern_fill_bg_pxid_count += 1;
                }
                bump(&mut receipt.pattern_bg_pxid_classes, bg_class);

                if viewer_paints
                    .get(&node_id)
                    .is_some_and(|paint| paint.solid_fill_rgb.is_some())
                {
                    receipt.pattern_fill_viewer_solid_fill_count += 1;
                }
            }
        }

        println!(
            "P21_COLOR_CENSUS page={} scene={} canonical={} exact_shape={} missing_shape={} ambiguous_shape={} images={} image_bound={} recolor={} recolor_bound={} brightness={} contrast={} recolor_storage={:?} recolor_colorref={:?} shapes={} large_shapes={} fill_types={:?} large_fill_types={:?} pattern={} pattern_bg_pxid={} pattern_viewer_solid={} pattern_bg_classes={:?}",
            receipt.viewer_page,
            receipt.scene_node_count,
            receipt.canonical_node_count,
            receipt.exact_shape_join_count,
            receipt.missing_shape_join_count,
            receipt.ambiguous_shape_join_count,
            receipt.image_like_count,
            receipt.image_resource_bound_count,
            receipt.image_recolor_count,
            receipt.image_recolor_resource_bound_count,
            receipt.image_brightness_count,
            receipt.image_contrast_count,
            receipt.recolor_storage_classes,
            receipt.recolor_colorref_classes,
            receipt.non_table_shape_count,
            receipt.large_non_table_shape_count,
            receipt.fill_type_classes,
            receipt.large_fill_type_classes,
            receipt.pattern_fill_count,
            receipt.pattern_fill_bg_pxid_count,
            receipt.pattern_fill_viewer_solid_fill_count,
            receipt.pattern_bg_pxid_classes,
        );

        pages.push(receipt);
    }

    let receipt = Receipt {
        schema: "chaptera.virginia-p21-color-effect-source-census.v1",
        source_sha256: sha,
        pages,
        claims: Claims {
            pdf_geometry_used: false,
            pdf_color_used: false,
            page_or_hash_product_rule_used: false,
            canonical_picture_recolor_field_available: false,
            viewer_pattern_fill_surface_available: false,
            large_shape_threshold: "source-backed node area >= 1% of source-backed page area",
            recolor_authority_note:
                "0x011A classification only; semantic authority pre-exists in Notion OBS-ECP-DUAL-PROJECTION-11-01",
            pattern_authority_note:
                "fillType=pattern/BG_PXID classification only; semantic chain pre-exists in Notion OBS-019",
        },
    };

    fs::create_dir_all(output.parent().expect("receipt parent")).expect("create output dir");
    fs::write(
        &output,
        serde_json::to_vec_pretty(&receipt).expect("serialize receipt"),
    )
    .expect("write receipt");
}
