use pub_core::StreamPath;
use pub_escher::{
    PUBLISHER_FIELD_SHAPE_ID, OfficeArtCoordinateRect, PublisherFieldRecord, SpContainerObservation,
    inspect_sp_containers,
};
use pub_model::{RectEmu, SourceRef};
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
const EXPECTED_CARRIERS: usize = 978;
const TABLE_OWNER_REF: u16 = 0x6802;
const CELL_ORDINAL: u16 = 0x2003;
const ORIENTATION: u16 = 0x2001;
const ROW_START: u16 = 0x2004;
const COLUMN_START: u16 = 0x2005;
const ROW_END: u16 = 0x2006;
const COLUMN_END: u16 = 0x2007;
const RECTANGLE: u16 = 0x0001;
const FILL_COLOR: u16 = 0x0181;
const LINE_WIDTH: u16 = 0x01CB;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
enum Axis {
    Horizontal,
    Vertical,
}

#[derive(Debug, Clone, Copy)]
struct Segment {
    axis: Axis,
    row_start: u32,
    column_start: u32,
    row_end: u32,
    column_end: u32,
}

fn unique_field(record: &PublisherFieldRecord, id: u16) -> Option<u32> {
    let mut fields = record.fields.iter().filter(|field| field.id == id);
    let first = fields.next()?.value;
    fields.next().is_none().then_some(first)
}

fn unique_or_zero(record: &PublisherFieldRecord, id: u16) -> Option<u32> {
    let mut fields = record.fields.iter().filter(|field| field.id == id);
    let Some(first) = fields.next() else {
        return Some(0);
    };
    fields.next().is_none().then_some(first.value)
}

fn unique_fopt_scalar(shape: &SpContainerObservation, property_id: u16) -> Option<u32> {
    let mut values = shape
        .fopts
        .iter()
        .flat_map(|record| record.properties.iter())
        .filter(|property| property.property_id() == property_id)
        .filter(|property| !property.f_bid() && !property.f_complex());
    let first = values.next()?.op;
    values.next().is_none().then_some(first)
}

fn shape_client_data_id(shape: &SpContainerObservation) -> Option<u32> {
    unique_field(shape.client_data.as_ref()?, PUBLISHER_FIELD_SHAPE_ID)
}

fn owner_ref(shape: &SpContainerObservation) -> Option<u32> {
    unique_field(shape.client_anchor.as_ref()?, TABLE_OWNER_REF)
}

fn decode_border_carrier(
    shape: &SpContainerObservation,
    table_seq: u32,
    rows: u32,
    columns: u32,
) -> Option<(Segment, u32)> {
    if shape.fsp.as_ref().map(|fsp| fsp.shape_type) != Some(RECTANGLE)
        || shape_client_data_id(shape).is_some()
    {
        return None;
    }

    let anchor = shape.client_anchor.as_ref()?;
    if unique_field(anchor, TABLE_OWNER_REF) != Some(table_seq)
        || anchor.fields.iter().any(|field| field.id == CELL_ORDINAL)
    {
        return None;
    }

    let allowed = [
        TABLE_OWNER_REF,
        ORIENTATION,
        ROW_START,
        COLUMN_START,
        ROW_END,
        COLUMN_END,
    ];
    if anchor.fields.iter().any(|field| !allowed.contains(&field.id)) {
        return None;
    }

    let orientation = unique_field(anchor, ORIENTATION)?;
    let row_start = unique_or_zero(anchor, ROW_START)?;
    let column_start = unique_or_zero(anchor, COLUMN_START)?;
    let row_end = unique_or_zero(anchor, ROW_END)?;
    let column_end = unique_or_zero(anchor, COLUMN_END)?;
    if row_start > rows || row_end > rows || column_start > columns || column_end > columns {
        return None;
    }

    let axis = match orientation {
        1 if row_start == row_end && column_start < column_end => Axis::Horizontal,
        2 if column_start == column_end && row_start < row_end => Axis::Vertical,
        _ => return None,
    };

    let color = unique_fopt_scalar(shape, FILL_COLOR)?;
    if color >> 24 != 0 {
        return None;
    }
    let width = unique_fopt_scalar(shape, LINE_WIDTH)?;
    if width == 0 || width > 0x0132_F540 {
        return None;
    }

    Some((
        Segment {
            axis,
            row_start,
            column_start,
            row_end,
            column_end,
        },
        width,
    ))
}

fn rect_i128(rect: &OfficeArtCoordinateRect) -> Option<[i128; 4]> {
    let out = [
        i128::from(rect.x_left),
        i128::from(rect.y_top),
        i128::from(rect.x_right),
        i128::from(rect.y_bottom),
    ];
    (out[2] > out[0] && out[3] > out[1]).then_some(out)
}

fn page_rect_i128(rect: RectEmu) -> Option<[i128; 4]> {
    let x0 = i128::from(rect.x.get());
    let y0 = i128::from(rect.y.get());
    let x1 = x0.checked_add(i128::from(rect.width.get()))?;
    let y1 = y0.checked_add(i128::from(rect.height.get()))?;
    (x1 > x0 && y1 > y0).then_some([x0, y0, x1, y1])
}

fn project_axis(
    value: i128,
    source_start: i128,
    source_end: i128,
    target_start: i128,
    target_end: i128,
) -> Option<i128> {
    let source_len = source_end.checked_sub(source_start)?;
    let target_len = target_end.checked_sub(target_start)?;
    if source_len <= 0 || target_len <= 0 {
        return None;
    }
    target_start.checked_add(
        value
            .checked_sub(source_start)?
            .checked_mul(target_len)?
            .checked_div(source_len)?,
    )
}

fn project_rect(
    rect: [i128; 4],
    source: [i128; 4],
    target: [i128; 4],
) -> Option<[i128; 4]> {
    let out = [
        project_axis(rect[0], source[0], source[2], target[0], target[2])?,
        project_axis(rect[1], source[1], source[3], target[1], target[3])?,
        project_axis(rect[2], source[0], source[2], target[0], target[2])?,
        project_axis(rect[3], source[1], source[3], target[1], target[3])?,
    ];
    (out[2] > out[0] && out[3] > out[1]).then_some(out)
}

fn unique_boundary_values(
    table: &pub_reader::PubTableSource,
) -> Option<(Vec<i128>, Vec<i128>)> {
    let mut xs = BTreeMap::<u32, BTreeSet<i128>>::new();
    let mut ys = BTreeMap::<u32, BTreeSet<i128>>::new();

    for cell in &table.cells {
        let coordinates = cell.coordinates?;
        let bounds = cell.bounds?;
        let rect = page_rect_i128(bounds)?;

        xs.entry(coordinates.start_column)
            .or_default()
            .insert(rect[0]);
        xs.entry(coordinates.end_column.checked_add(1)?)
            .or_default()
            .insert(rect[2]);
        ys.entry(coordinates.start_row)
            .or_default()
            .insert(rect[1]);
        ys.entry(coordinates.end_row.checked_add(1)?)
            .or_default()
            .insert(rect[3]);
    }

    let columns = (0..=table.columns)
        .map(|boundary| {
            let values = xs.get(&boundary)?;
            (values.len() == 1).then(|| *values.iter().next().unwrap())
        })
        .collect::<Option<Vec<_>>>()?;
    let rows = (0..=table.rows)
        .map(|boundary| {
            let values = ys.get(&boundary)?;
            (values.len() == 1).then(|| *values.iter().next().unwrap())
        })
        .collect::<Option<Vec<_>>>()?;

    Some((columns, rows))
}

fn scale_boundary(
    value: i128,
    grid_start: i128,
    grid_end: i128,
    owner_start: i128,
    owner_end: i128,
) -> Option<i128> {
    project_axis(value, grid_start, grid_end, owner_start, owner_end)
}

fn source_ref_is_exact_track(reference: &SourceRef) -> bool {
    reference.path.as_deref() == Some("TABLE/rowcol_array")
}

#[derive(Debug, Default, Serialize)]
struct PlacementCounts {
    carriers: usize,
    owner_shape_unique: usize,
    parent_link_exact: usize,
    child_anchor_present: usize,
    projected: usize,
    raw_boundary_inside: usize,
    scaled_boundary_inside: usize,
    raw_centered_band_exact: usize,
    scaled_centered_band_exact: usize,
    center_closer_raw: usize,
    center_closer_scaled: usize,
    center_distance_tie: usize,
    thickness_eq_linewidth: usize,
    thickness_half_linewidth: usize,
    thickness_double_linewidth: usize,
    thickness_other: usize,
    longitudinal_raw_exact: usize,
    longitudinal_scaled_exact: usize,
    raw_scaled_boundary_same: usize,
    raw_scaled_boundary_different: usize,
}

#[derive(Debug, Serialize)]
struct TableReceipt {
    viewer_page: u32,
    table_seq_num: u32,
    rows: u32,
    columns: u32,
    exact_track_cells: usize,
    owner_minus_grid_bottom_emu: i64,
    placement: PlacementCounts,
}

#[derive(Debug, Default, Serialize)]
struct PageReceipt {
    viewer_page: u32,
    tables: usize,
    placement: PlacementCounts,
}

#[derive(Debug, Default, Serialize)]
struct Totals {
    tables: usize,
    placement: PlacementCounts,
}

#[derive(Debug, Serialize)]
struct Receipt {
    schema: &'static str,
    source_sha256: String,
    pages: Vec<PageReceipt>,
    tables: Vec<TableReceipt>,
    totals: Totals,
    claims: Claims,
}

#[derive(Debug, Serialize)]
struct Claims {
    pdf_geometry_used: bool,
    proximity_side_inference_used: bool,
    boundary_sources: &'static str,
    carrier_geometry_source: &'static str,
}

fn add_counts(total: &mut PlacementCounts, value: &PlacementCounts) {
    total.carriers += value.carriers;
    total.owner_shape_unique += value.owner_shape_unique;
    total.parent_link_exact += value.parent_link_exact;
    total.child_anchor_present += value.child_anchor_present;
    total.projected += value.projected;
    total.raw_boundary_inside += value.raw_boundary_inside;
    total.scaled_boundary_inside += value.scaled_boundary_inside;
    total.raw_centered_band_exact += value.raw_centered_band_exact;
    total.scaled_centered_band_exact += value.scaled_centered_band_exact;
    total.center_closer_raw += value.center_closer_raw;
    total.center_closer_scaled += value.center_closer_scaled;
    total.center_distance_tie += value.center_distance_tie;
    total.thickness_eq_linewidth += value.thickness_eq_linewidth;
    total.thickness_half_linewidth += value.thickness_half_linewidth;
    total.thickness_double_linewidth += value.thickness_double_linewidth;
    total.thickness_other += value.thickness_other;
    total.longitudinal_raw_exact += value.longitudinal_raw_exact;
    total.longitudinal_scaled_exact += value.longitudinal_scaled_exact;
    total.raw_scaled_boundary_same += value.raw_scaled_boundary_same;
    total.raw_scaled_boundary_different += value.raw_scaled_boundary_different;
}

#[test]
#[ignore = "requires exact public Virginia Remplacante fixture"]
fn exact_virginia_table_border_carrier_placement_probe() {
    let fixture = PathBuf::from(
        env::var_os("CHAPTERA_VIRGINIA_TABLE_BORDER_PLACEMENT_FIXTURE")
            .expect("CHAPTERA_VIRGINIA_TABLE_BORDER_PLACEMENT_FIXTURE"),
    );
    let output = PathBuf::from(
        env::var_os("CHAPTERA_VIRGINIA_TABLE_BORDER_PLACEMENT_OUT")
            .expect("CHAPTERA_VIRGINIA_TABLE_BORDER_PLACEMENT_OUT"),
    );

    let bytes = fs::read(&fixture).expect("read exact Virginia PUB");
    let sha = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    assert_eq!(sha, EXPECTED_SHA256);

    let bundle =
        open_pub_bundle(&bytes, viewer_geometry_environment_v0_1()).expect("open Viewer bundle");
    let escher = pub_cfb::read_stream_reader(
        Cursor::new(bytes.as_slice()),
        pub_reader::ESCHER_STREAM_PATH,
    )
    .expect("read Escher stream");
    let inventory = inspect_sp_containers(
        StreamPath(pub_reader::ESCHER_STREAM_PATH.into()),
        &escher,
    )
    .expect("inspect OfficeArt");

    let mut pages = Vec::new();
    let mut tables = Vec::new();
    let mut totals = Totals::default();

    for viewer_page in [6_u32, 7, 9, 10, 11, 21, 22, 23] {
        let page = &bundle.geometry.document.pages[(viewer_page - 1) as usize];
        let parent = page.id.into_canonical();
        let source_tables = bundle
            .resolved_graph
            .nodes
            .values()
            .filter(|node| node.header.parent_id == parent && node.payload.table.is_some())
            .collect::<Vec<_>>();

        let mut page_receipt = PageReceipt {
            viewer_page,
            tables: source_tables.len(),
            ..PageReceipt::default()
        };

        for node in source_tables {
            let table = node.payload.table.as_ref().unwrap();
            let table_seq = node.payload.contents_seq_num;
            let exact_track_cells = table
                .cells
                .iter()
                .filter(|cell| {
                    cell.bounds.is_some()
                        && cell.source_refs.iter().any(source_ref_is_exact_track)
                })
                .count();

            let (raw_x, raw_y) = unique_boundary_values(table)
                .expect("selected #772 TABLE must have complete exact boundaries");
            let owner_rect = page_rect_i128(node.header.bounds).expect("positive TABLE owner bounds");
            let grid_bottom = *raw_y.last().unwrap();
            let owner_minus_grid_bottom = i64::try_from(owner_rect[3] - grid_bottom)
                .expect("owner/grid delta fits i64");

            let owners = inventory
                .shapes
                .iter()
                .filter(|shape| shape_client_data_id(shape) == Some(table_seq))
                .collect::<Vec<_>>();

            let mut placement = PlacementCounts::default();

            for shape in inventory
                .shapes
                .iter()
                .filter(|shape| owner_ref(shape) == Some(table_seq))
            {
                let Some((segment, line_width)) =
                    decode_border_carrier(shape, table_seq, table.rows, table.columns)
                else {
                    continue;
                };
                placement.carriers += 1;

                let [owner] = owners.as_slice() else {
                    continue;
                };
                placement.owner_shape_unique += 1;

                if shape.parent_group_shape_source.as_ref() != Some(&owner.source) {
                    continue;
                }
                placement.parent_link_exact += 1;

                let (Some(child_anchor), Some(fspgr)) =
                    (shape.child_anchor.as_ref(), owner.fspgr.as_ref())
                else {
                    continue;
                };
                placement.child_anchor_present += 1;

                let Some(projected) = rect_i128(child_anchor)
                    .and_then(|rect| {
                        rect_i128(fspgr)
                            .and_then(|group| project_rect(rect, group, owner_rect))
                    })
                else {
                    continue;
                };
                placement.projected += 1;

                let (raw_cross, scaled_cross, raw_long_start, raw_long_end, scaled_long_start, scaled_long_end, cross_start, cross_end, long_start, long_end) =
                    match segment.axis {
                        Axis::Horizontal => {
                            let raw_cross = raw_y[segment.row_start as usize];
                            let scaled_cross = scale_boundary(
                                raw_cross,
                                raw_y[0],
                                *raw_y.last().unwrap(),
                                owner_rect[1],
                                owner_rect[3],
                            )
                            .unwrap();
                            let raw_long_start = raw_x[segment.column_start as usize];
                            let raw_long_end = raw_x[segment.column_end as usize];
                            let scaled_long_start = scale_boundary(
                                raw_long_start,
                                raw_x[0],
                                *raw_x.last().unwrap(),
                                owner_rect[0],
                                owner_rect[2],
                            )
                            .unwrap();
                            let scaled_long_end = scale_boundary(
                                raw_long_end,
                                raw_x[0],
                                *raw_x.last().unwrap(),
                                owner_rect[0],
                                owner_rect[2],
                            )
                            .unwrap();
                            (
                                raw_cross,
                                scaled_cross,
                                raw_long_start,
                                raw_long_end,
                                scaled_long_start,
                                scaled_long_end,
                                projected[1],
                                projected[3],
                                projected[0],
                                projected[2],
                            )
                        }
                        Axis::Vertical => {
                            let raw_cross = raw_x[segment.column_start as usize];
                            let scaled_cross = scale_boundary(
                                raw_cross,
                                raw_x[0],
                                *raw_x.last().unwrap(),
                                owner_rect[0],
                                owner_rect[2],
                            )
                            .unwrap();
                            let raw_long_start = raw_y[segment.row_start as usize];
                            let raw_long_end = raw_y[segment.row_end as usize];
                            let scaled_long_start = scale_boundary(
                                raw_long_start,
                                raw_y[0],
                                *raw_y.last().unwrap(),
                                owner_rect[1],
                                owner_rect[3],
                            )
                            .unwrap();
                            let scaled_long_end = scale_boundary(
                                raw_long_end,
                                raw_y[0],
                                *raw_y.last().unwrap(),
                                owner_rect[1],
                                owner_rect[3],
                            )
                            .unwrap();
                            (
                                raw_cross,
                                scaled_cross,
                                raw_long_start,
                                raw_long_end,
                                scaled_long_start,
                                scaled_long_end,
                                projected[0],
                                projected[2],
                                projected[1],
                                projected[3],
                            )
                        }
                    };

                if raw_cross == scaled_cross {
                    placement.raw_scaled_boundary_same += 1;
                } else {
                    placement.raw_scaled_boundary_different += 1;
                }

                if cross_start <= raw_cross && raw_cross <= cross_end {
                    placement.raw_boundary_inside += 1;
                }
                if cross_start <= scaled_cross && scaled_cross <= cross_end {
                    placement.scaled_boundary_inside += 1;
                }

                let width = i128::from(line_width);
                let raw_band_exact = cross_start.checked_mul(2)
                    == raw_cross.checked_mul(2).and_then(|v| v.checked_sub(width))
                    && cross_end.checked_mul(2)
                        == raw_cross.checked_mul(2).and_then(|v| v.checked_add(width));
                let scaled_band_exact = cross_start.checked_mul(2)
                    == scaled_cross.checked_mul(2).and_then(|v| v.checked_sub(width))
                    && cross_end.checked_mul(2)
                        == scaled_cross.checked_mul(2).and_then(|v| v.checked_add(width));
                if raw_band_exact {
                    placement.raw_centered_band_exact += 1;
                }
                if scaled_band_exact {
                    placement.scaled_centered_band_exact += 1;
                }

                let center2 = cross_start + cross_end;
                let raw_distance = (center2 - 2 * raw_cross).abs();
                let scaled_distance = (center2 - 2 * scaled_cross).abs();
                match raw_distance.cmp(&scaled_distance) {
                    std::cmp::Ordering::Less => placement.center_closer_raw += 1,
                    std::cmp::Ordering::Greater => placement.center_closer_scaled += 1,
                    std::cmp::Ordering::Equal => placement.center_distance_tie += 1,
                }

                let thickness = cross_end - cross_start;
                if thickness == width {
                    placement.thickness_eq_linewidth += 1;
                } else if thickness.checked_mul(2) == Some(width) {
                    placement.thickness_half_linewidth += 1;
                } else if width.checked_mul(2) == Some(thickness) {
                    placement.thickness_double_linewidth += 1;
                } else {
                    placement.thickness_other += 1;
                }

                if long_start == raw_long_start && long_end == raw_long_end {
                    placement.longitudinal_raw_exact += 1;
                }
                if long_start == scaled_long_start && long_end == scaled_long_end {
                    placement.longitudinal_scaled_exact += 1;
                }
            }

            assert_eq!(
                placement.carriers,
                match viewer_page {
                    6 => 24 * 12,
                    7 | 9 | 10 => 177,
                    11 => 36,
                    21 => if table_seq == 552 { 10 } else { 25 },
                    22 => match table_seq {
                        709 | 715 => 19,
                        704 => 25,
                        _ => panic!("unexpected p22 TABLE"),
                    },
                    23 => 25,
                    _ => unreachable!(),
                },
                "per-table #772 carrier cohort must stay pinned"
            );

            add_counts(&mut page_receipt.placement, &placement);
            add_counts(&mut totals.placement, &placement);
            totals.tables += 1;

            tables.push(TableReceipt {
                viewer_page,
                table_seq_num: table_seq,
                rows: table.rows,
                columns: table.columns,
                exact_track_cells,
                owner_minus_grid_bottom_emu: owner_minus_grid_bottom,
                placement,
            });
        }

        pages.push(page_receipt);
    }

    assert_eq!(totals.tables, 22);
    assert_eq!(totals.placement.carriers, EXPECTED_CARRIERS);

    let receipt = Receipt {
        schema: "chaptera.virginia-table-border-carrier-placement.v1",
        source_sha256: sha,
        pages,
        tables,
        totals,
        claims: Claims {
            pdf_geometry_used: false,
            proximity_side_inference_used: false,
            boundary_sources: "TABLE/rowcol_array exact cell boundaries + TABLE owner bounds",
            carrier_geometry_source: "OfficeArt childAnchor projected through TABLE owner FSPGR",
        },
    };

    fs::create_dir_all(output.parent().unwrap()).expect("create receipt dir");
    fs::write(
        &output,
        serde_json::to_vec_pretty(&receipt).expect("serialize receipt"),
    )
    .expect("write receipt");

    println!(
        "TABLE_BORDER_PLACEMENT carriers={} projected={} raw_inside={} scaled_inside={} closer_raw={} closer_scaled={} tie={} raw_band={} scaled_band={}",
        receipt.totals.placement.carriers,
        receipt.totals.placement.projected,
        receipt.totals.placement.raw_boundary_inside,
        receipt.totals.placement.scaled_boundary_inside,
        receipt.totals.placement.center_closer_raw,
        receipt.totals.placement.center_closer_scaled,
        receipt.totals.placement.center_distance_tie,
        receipt.totals.placement.raw_centered_band_exact,
        receipt.totals.placement.scaled_centered_band_exact,
    );
}
