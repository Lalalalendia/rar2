use pub_core::StreamPath;
use pub_model::RectEmu;
use pub_viewer::{open_pub_bundle, viewer_geometry_environment_v0_1};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    env, fs,
    path::PathBuf,
};

const FILL_TYPE: u16 = 0x0180;
const FILL_COLOR: u16 = 0x0181;
const FILL_BOOLEANS: u16 = 0x01BF;
const LINE_COLOR: u16 = 0x01C0;
const LINE_WIDTH: u16 = 0x01CB;
const LINE_BOOLEANS: u16 = 0x01FF;
const FILL_USE_FILLED_BIT: u32 = 1 << 20;
const FILL_FILLED_BIT: u32 = 1 << 4;
const LINE_USE_LINE_BIT: u32 = 1 << 19;
const LINE_LINE_BIT: u32 = 1 << 3;

fn bump(map: &mut BTreeMap<String, usize>, key: impl Into<String>) {
    *map.entry(key.into()).or_default() += 1;
}

fn coordinate_rect_i128(rect: &pub_escher::OfficeArtCoordinateRect) -> Option<[i128; 4]> {
    let left = i128::from(rect.x_left);
    let top = i128::from(rect.y_top);
    let right = i128::from(rect.x_right);
    let bottom = i128::from(rect.y_bottom);
    (right > left && bottom > top).then_some([left, top, right, bottom])
}

fn rect_edges(rect: RectEmu) -> Option<[i128; 4]> {
    let left = i128::from(rect.x.get());
    let top = i128::from(rect.y.get());
    let right = left.checked_add(i128::from(rect.width.get()))?;
    let bottom = top.checked_add(i128::from(rect.height.get()))?;
    (right > left && bottom > top).then_some([left, top, right, bottom])
}

fn project_axis(value: i128, source_start: i128, source_end: i128, target_start: i128, target_end: i128) -> Option<i128> {
    let source_len = source_end.checked_sub(source_start)?;
    let target_len = target_end.checked_sub(target_start)?;
    if source_len <= 0 || target_len <= 0 {
        return None;
    }
    target_start.checked_add(value.checked_sub(source_start)?.checked_mul(target_len)?.checked_div(source_len)?)
}

fn project_rect(child: [i128; 4], group: [i128; 4], target: [i128; 4]) -> Option<[i128; 4]> {
    Some([
        project_axis(child[0], group[0], group[2], target[0], target[2])?,
        project_axis(child[1], group[1], group[3], target[1], target[3])?,
        project_axis(child[2], group[0], group[2], target[0], target[2])?,
        project_axis(child[3], group[1], group[3], target[1], target[3])?,
    ])
}

fn properties<'a>(
    shape: &'a pub_escher::SpContainerObservation,
    property_id: u16,
) -> Vec<&'a pub_escher::Fopte> {
    shape
        .fopts
        .iter()
        .flat_map(|record| record.properties.iter())
        .filter(|property| property.property_id() == property_id)
        .collect()
}

fn scalar_profile(shape: &pub_escher::SpContainerObservation, property_id: u16) -> &'static str {
    let matches = properties(shape, property_id);
    if matches.is_empty() {
        "absent"
    } else if matches.iter().any(|property| property.f_bid() || property.f_complex()) {
        "malformed_or_complex"
    } else if matches.len() == 1 {
        "single_scalar"
    } else {
        "duplicate_scalar"
    }
}

fn color_profile(shape: &pub_escher::SpContainerObservation, property_id: u16) -> &'static str {
    let matches = properties(shape, property_id);
    if matches.is_empty() {
        return "absent";
    }
    if matches.len() != 1 || matches[0].f_bid() || matches[0].f_complex() {
        return "ambiguous";
    }
    match (matches[0].op >> 24) as u8 {
        0x00 => "direct_rgb",
        0x08 => "scheme",
        0x10 => "system_or_extended",
        _ => "other_flagged",
    }
}

fn fill_boolean_profile(shape: &pub_escher::SpContainerObservation) -> &'static str {
    let matches = properties(shape, FILL_BOOLEANS);
    if matches.is_empty() {
        return "absent";
    }
    if matches.len() != 1 || matches[0].f_bid() || matches[0].f_complex() {
        return "ambiguous";
    }
    let raw = matches[0].op;
    if raw & FILL_USE_FILLED_BIT == 0 {
        "nonparticipating"
    } else if raw & FILL_FILLED_BIT != 0 {
        "participating_true"
    } else {
        "participating_false"
    }
}

fn line_boolean_profile(shape: &pub_escher::SpContainerObservation) -> &'static str {
    let matches = properties(shape, LINE_BOOLEANS);
    if matches.is_empty() {
        return "absent";
    }
    if matches.len() != 1 || matches[0].f_bid() || matches[0].f_complex() {
        return "ambiguous";
    }
    let raw = matches[0].op;
    if raw & LINE_USE_LINE_BIT == 0 {
        "nonparticipating"
    } else if raw & LINE_LINE_BIT != 0 {
        "participating_true"
    } else {
        "participating_false"
    }
}

#[derive(Debug, Default, Serialize)]
struct PageReceipt {
    viewer_page_index: u32,
    table_count: usize,
    cell_count: usize,
    admitted_cell_paint_count: usize,
    owner_join_histogram: BTreeMap<String, usize>,
    owner_child_count_histogram: BTreeMap<String, usize>,
    cell_child_match_histogram: BTreeMap<String, usize>,
    exact_match_count: usize,
    fill_type_profile: BTreeMap<String, usize>,
    fill_color_profile: BTreeMap<String, usize>,
    fill_boolean_profile: BTreeMap<String, usize>,
    line_color_profile: BTreeMap<String, usize>,
    line_width_profile: BTreeMap<String, usize>,
    line_boolean_profile: BTreeMap<String, usize>,
}

#[derive(Debug, Serialize)]
struct Receipt {
    schema: &'static str,
    source_sha256: String,
    selected_viewer_page_count: usize,
    pages: Vec<PageReceipt>,
    guardrails: Vec<&'static str>,
}

#[test]
#[ignore = "requires exact public Virginia Remplacante fixture"]
fn exact_virginia_p22_table_paint_carrier_probe() {
    let fixture = env::var_os("CHAPTERA_VIRGINIA_TABLE_PAINT_FIXTURE")
        .map(PathBuf::from)
        .expect("CHAPTERA_VIRGINIA_TABLE_PAINT_FIXTURE");
    let output = env::var_os("CHAPTERA_VIRGINIA_TABLE_PAINT_OUT")
        .map(PathBuf::from)
        .expect("CHAPTERA_VIRGINIA_TABLE_PAINT_OUT");
    let expected_sha = env::var("CHAPTERA_VIRGINIA_TABLE_PAINT_SHA256")
        .expect("CHAPTERA_VIRGINIA_TABLE_PAINT_SHA256");

    let bytes = fs::read(&fixture).expect("read exact Virginia Remplacante PUB");
    let actual_sha = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    assert_eq!(actual_sha, expected_sha, "exact Virginia source identity");

    let bundle = open_pub_bundle(&bytes, viewer_geometry_environment_v0_1())
        .expect("open exact Virginia through Viewer bundle");
    assert_eq!(bundle.geometry.document.pages.len(), 25);

    let escher = pub_cfb::read_stream_path(&fixture, "/Escher/EscherStm")
        .expect("read exact Publisher Escher stream");
    let inventory = pub_escher::inspect_sp_containers(
        StreamPath("/Escher/EscherStm".to_owned()),
        &escher,
    )
    .expect("inspect OfficeArt SpContainers");

    let mut shapes_by_seq = BTreeMap::<u32, Vec<usize>>::new();
    for (shape_index, shape) in inventory.shapes.iter().enumerate() {
        let Some(client_data) = shape.client_data.as_ref() else {
            continue;
        };
        let mut seqs = client_data
            .fields
            .iter()
            .filter(|field| field.id == pub_escher::PUBLISHER_FIELD_SHAPE_ID)
            .map(|field| field.value)
            .collect::<Vec<_>>();
        seqs.sort_unstable();
        seqs.dedup();
        for seq in seqs {
            shapes_by_seq.entry(seq).or_default().push(shape_index);
        }
    }

    let mut pages = Vec::new();
    for viewer_page_index in [21_u32, 22, 23] {
        let page = &bundle.geometry.document.pages[(viewer_page_index - 1) as usize];
        let parent = page.id.into_canonical();
        let mut receipt = PageReceipt {
            viewer_page_index,
            ..PageReceipt::default()
        };

        for node in bundle
            .resolved_graph
            .nodes
            .values()
            .filter(|node| node.header.parent_id == parent)
        {
            let Some(table) = node.payload.table.as_ref() else {
                continue;
            };
            receipt.table_count += 1;
            receipt.cell_count += table.cells.len();
            receipt.admitted_cell_paint_count += table.cells.iter().filter(|cell| cell.paint.is_some()).count();

            let owner_matches = shapes_by_seq
                .get(&node.payload.contents_seq_num)
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            let [owner_index] = owner_matches else {
                bump(
                    &mut receipt.owner_join_histogram,
                    if owner_matches.is_empty() { "missing" } else { "ambiguous" },
                );
                continue;
            };
            bump(&mut receipt.owner_join_histogram, "unique");

            let owner = &inventory.shapes[*owner_index];
            let Some(group_coords) = owner.fspgr.as_ref() else {
                bump(&mut receipt.owner_child_count_histogram, "owner_fspgr_missing");
                continue;
            };
            let Some(group_rect) = coordinate_rect_i128(group_coords) else {
                bump(&mut receipt.owner_child_count_histogram, "owner_fspgr_invalid");
                continue;
            };
            let Some(target_rect) = rect_edges(node.header.bounds) else {
                bump(&mut receipt.owner_child_count_histogram, "owner_bounds_invalid");
                continue;
            };

            let children = inventory
                .shapes
                .iter()
                .filter(|shape| shape.parent_group_shape_source.as_ref() == Some(&owner.source))
                .filter_map(|shape| {
                    let child = shape.child_anchor.as_ref()?;
                    let child_rect = coordinate_rect_i128(child)?;
                    let projected = project_rect(child_rect, group_rect, target_rect)?;
                    Some((shape, projected))
                })
                .collect::<Vec<_>>();
            bump(
                &mut receipt.owner_child_count_histogram,
                format!("count={}", children.len()),
            );

            for cell in &table.cells {
                let Some(bounds) = cell.bounds.and_then(rect_edges) else {
                    bump(&mut receipt.cell_child_match_histogram, "cell_bounds_missing");
                    continue;
                };
                let matches = children
                    .iter()
                    .filter(|(_, projected)| *projected == bounds)
                    .collect::<Vec<_>>();
                let [(shape, _)] = matches.as_slice() else {
                    bump(
                        &mut receipt.cell_child_match_histogram,
                        if matches.is_empty() { "unmatched" } else { "ambiguous" },
                    );
                    continue;
                };
                bump(&mut receipt.cell_child_match_histogram, "unique");
                receipt.exact_match_count += 1;

                bump(&mut receipt.fill_type_profile, scalar_profile(shape, FILL_TYPE));
                bump(&mut receipt.fill_color_profile, color_profile(shape, FILL_COLOR));
                bump(&mut receipt.fill_boolean_profile, fill_boolean_profile(shape));
                bump(&mut receipt.line_color_profile, color_profile(shape, LINE_COLOR));
                bump(&mut receipt.line_width_profile, scalar_profile(shape, LINE_WIDTH));
                bump(&mut receipt.line_boolean_profile, line_boolean_profile(shape));
            }
        }

        pages.push(receipt);
    }

    let p22 = pages.iter().find(|page| page.viewer_page_index == 22).unwrap();
    assert_eq!(p22.cell_count, 110, "p22 exact TableCell population");
    assert_eq!(p22.admitted_cell_paint_count, 0, "current #656 paint boundary");
    let receipt = Receipt {
        schema: "chaptera.virginia-p22-table-paint-carrier-probe.v1",
        source_sha256: actual_sha,
        selected_viewer_page_count: bundle.geometry.document.pages.len(),
        pages,
        guardrails: vec![
            "Viewer p21/p22/p23 are selected through the existing product page projection.",
            "Cell-to-OfficeArt joins require exact already-grounded page-space cell bounds and projected child anchors.",
            "Property output is aggregate class only; no RGB/scalar values, text, object ids, SPIDs, offsets, coordinates, filenames, or source bytes are emitted.",
            "Known fill/line property families are observations only; this probe does not invent border-side semantics or a TableStyle cascade.",
            "Existing #656 literal direct-RGB plus explicit participating fFilled admission remains unchanged by this measurement.",
        ],
    };

    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).expect("create TABLE paint receipt directory");
    }
    fs::write(
        &output,
        serde_json::to_vec_pretty(&receipt).expect("serialize TABLE paint receipt"),
    )
    .expect("write TABLE paint receipt");

    println!(
        "VIRGINIA_TABLE_PAINT p21_exact={} p22_exact={} p23_exact={} p22_paint={}",
        receipt.pages[0].exact_match_count,
        receipt.pages[1].exact_match_count,
        receipt.pages[2].exact_match_count,
        receipt.pages[1].admitted_cell_paint_count,
    );
}
