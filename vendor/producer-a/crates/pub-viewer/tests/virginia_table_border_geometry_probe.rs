use pub_core::StreamPath;
use pub_escher::{
    PUBLISHER_FIELD_SHAPE_ID, PUBLISHER_FIELD_XE, PUBLISHER_FIELD_XS, PUBLISHER_FIELD_YE,
    PUBLISHER_FIELD_YS, PublisherFieldRecord, SpContainerObservation, inspect_sp_containers,
};
use pub_model::RectEmu;
use pub_viewer::{open_pub_bundle, viewer_geometry_environment_v0_1};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    env, fs,
    io::Cursor,
    path::PathBuf,
};

const EXPECTED_SHA256: &str =
    "88f57d800aeec808798ea487b9d4ab85dc85c02cd190b987a332709b81018506";
const TABLE_OWNER_REF: u16 = 0x6802;
const CELL_ORDINAL: u16 = 0x2003;
const SEGMENT_ORIENTATION: u16 = 0x2001;
const ROW_START: u16 = 0x2004;
const COLUMN_START: u16 = 0x2005;
const ROW_END: u16 = 0x2006;
const COLUMN_END: u16 = 0x2007;
const RECTANGLE: u16 = 0x0001;
const LINE_WIDTH: u16 = 0x01CB;

fn unique_field(record: &PublisherFieldRecord, id: u16) -> Option<u32> {
    let mut matches = record.fields.iter().filter(|field| field.id == id);
    let first = matches.next()?.value;
    matches.next().is_none().then_some(first)
}

fn unique_or_zero(record: &PublisherFieldRecord, id: u16) -> Option<u32> {
    let mut matches = record.fields.iter().filter(|field| field.id == id);
    let Some(first) = matches.next() else {
        return Some(0);
    };
    matches.next().is_none().then_some(first.value)
}

fn unique_fopt_scalar(shape: &SpContainerObservation, property_id: u16) -> Option<u32> {
    let mut matches = shape
        .fopts
        .iter()
        .flat_map(|record| record.properties.iter())
        .filter(|property| property.property_id() == property_id)
        .filter(|property| !property.f_bid() && !property.f_complex());
    let first = matches.next()?.op;
    matches.next().is_none().then_some(first)
}

fn shape_id(shape: &SpContainerObservation) -> Option<u32> {
    unique_field(shape.client_data.as_ref()?, PUBLISHER_FIELD_SHAPE_ID)
}

fn has_client_data_identity(shape: &SpContainerObservation) -> bool {
    shape_id(shape).is_some()
}

fn is_candidate(shape: &SpContainerObservation, table_seq: u32) -> bool {
    if shape.fsp.as_ref().map(|fsp| fsp.shape_type) != Some(RECTANGLE)
        || has_client_data_identity(shape)
    {
        return false;
    }
    let Some(anchor) = shape.client_anchor.as_ref() else {
        return false;
    };
    if unique_field(anchor, TABLE_OWNER_REF) != Some(table_seq)
        || anchor.fields.iter().any(|field| field.id == CELL_ORDINAL)
    {
        return false;
    }

    let allowed = [
        TABLE_OWNER_REF,
        SEGMENT_ORIENTATION,
        ROW_START,
        COLUMN_START,
        ROW_END,
        COLUMN_END,
    ];
    if anchor.fields.iter().any(|field| !allowed.contains(&field.id)) {
        return false;
    }

    let Some(segment) = decode_segment(shape, u32::MAX, u32::MAX) else {
        return false;
    };
    let valid_nonzero_span = match segment.axis {
        Axis::Horizontal => segment.column_start < segment.column_end,
        Axis::Vertical => segment.row_start < segment.row_end,
    };
    if !valid_nonzero_span {
        return false;
    }

    let direct_color = unique_fopt_scalar(shape, 0x0181)
        .is_some_and(|value| (value >> 24) == 0);
    let explicit_width = unique_fopt_scalar(shape, LINE_WIDTH)
        .is_some_and(|value| value > 0 && value <= 0x0132_F540);
    direct_color && explicit_width
}

#[derive(Debug, Clone, Copy)]
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

fn decode_segment(shape: &SpContainerObservation, rows: u32, columns: u32) -> Option<Segment> {
    let anchor = shape.client_anchor.as_ref()?;
    let allowed = [
        TABLE_OWNER_REF,
        SEGMENT_ORIENTATION,
        ROW_START,
        COLUMN_START,
        ROW_END,
        COLUMN_END,
    ];
    if anchor.fields.iter().any(|field| !allowed.contains(&field.id)) {
        return None;
    }
    let orientation = unique_field(anchor, SEGMENT_ORIENTATION)?;
    let row_start = unique_or_zero(anchor, ROW_START)?;
    let column_start = unique_or_zero(anchor, COLUMN_START)?;
    let row_end = unique_or_zero(anchor, ROW_END)?;
    let column_end = unique_or_zero(anchor, COLUMN_END)?;
    if row_start > rows || row_end > rows || column_start > columns || column_end > columns {
        return None;
    }
    match orientation {
        1 if row_start == row_end && column_start < column_end => Some(Segment {
            axis: Axis::Horizontal,
            row_start,
            column_start,
            row_end,
            column_end,
        }),
        2 if column_start == column_end && row_start < row_end => Some(Segment {
            axis: Axis::Vertical,
            row_start,
            column_start,
            row_end,
            column_end,
        }),
        _ => None,
    }
}

fn rect_i128(rect: &pub_escher::OfficeArtCoordinateRect) -> Option<[i128; 4]> {
    let out = [
        i128::from(rect.x_left),
        i128::from(rect.y_top),
        i128::from(rect.x_right),
        i128::from(rect.y_bottom),
    ];
    (out[2] > out[0] && out[3] > out[1]).then_some(out)
}

fn signed_field(record: &PublisherFieldRecord, id: u16) -> Option<i64> {
    let field = record.fields.iter().filter(|field| field.id == id).collect::<Vec<_>>();
    let [field] = field.as_slice() else {
        return None;
    };
    Some(i64::from(i32::from_le_bytes(field.value.to_le_bytes())))
}

fn publisher_anchor_rect(anchor: &PublisherFieldRecord) -> Option<[i128; 4]> {
    let out = [
        i128::from(signed_field(anchor, PUBLISHER_FIELD_XS)?),
        i128::from(signed_field(anchor, PUBLISHER_FIELD_YS)?),
        i128::from(signed_field(anchor, PUBLISHER_FIELD_XE)?),
        i128::from(signed_field(anchor, PUBLISHER_FIELD_YE)?),
    ];
    (out[2] > out[0] && out[3] > out[1]).then_some(out)
}

fn center_origin_to_page(
    page_width: i64,
    page_height: i64,
    rect: [i128; 4],
) -> Option<[i128; 4]> {
    let half_width = i128::from(page_width) / 2;
    let half_height = i128::from(page_height) / 2;
    Some([
        half_width + rect[0],
        half_height + rect[1],
        half_width + rect[2],
        half_height + rect[3],
    ])
}

fn bounds_i128(bounds: RectEmu) -> Option<[i128; 4]> {
    let right = bounds.right()?.get();
    let bottom = bounds.bottom()?.get();
    Some([
        i128::from(bounds.x.get()),
        i128::from(bounds.y.get()),
        i128::from(right),
        i128::from(bottom),
    ])
}

fn project_axis(
    value: i128,
    source_start: i128,
    source_end: i128,
    target_start: i128,
    target_end: i128,
) -> Option<i128> {
    let source_len = source_end - source_start;
    let target_len = target_end - target_start;
    if source_len <= 0 || target_len <= 0 {
        return None;
    }
    Some(target_start + (value - source_start) * target_len / source_len)
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

fn near(a: i128, b: i128) -> bool {
    (a - b).abs() <= 1
}

fn classify_perpendicular(axis: Axis, rect: [i128; 4], boundary: i128) -> &'static str {
    let (start, end) = match axis {
        Axis::Horizontal => (rect[1], rect[3]),
        Axis::Vertical => (rect[0], rect[2]),
    };
    if (start + end - 2 * boundary).abs() <= 2 {
        "centered"
    } else if near(end, boundary) {
        "before_edge"
    } else if near(start, boundary) {
        "after_edge"
    } else if start < boundary && boundary < end {
        "crosses_off_center"
    } else if end < boundary {
        "before_gap"
    } else if start > boundary {
        "after_gap"
    } else {
        "other"
    }
}

fn classify_thickness(axis: Axis, rect: [i128; 4], width: i128) -> &'static str {
    let thickness = match axis {
        Axis::Horizontal => rect[3] - rect[1],
        Axis::Vertical => rect[2] - rect[0],
    };
    if (thickness - width).abs() <= 1 {
        "equals_line_width"
    } else if (2 * thickness - width).abs() <= 2 {
        "half_line_width"
    } else if (thickness - 2 * width).abs() <= 2 {
        "double_line_width"
    } else if thickness < width {
        "other_thinner"
    } else {
        "other_thicker"
    }
}

fn classify_longitudinal(
    axis: Axis,
    rect: [i128; 4],
    start: i128,
    end: i128,
) -> &'static str {
    let (actual_start, actual_end) = match axis {
        Axis::Horizontal => (rect[0], rect[2]),
        Axis::Vertical => (rect[1], rect[3]),
    };
    if near(actual_start, start) && near(actual_end, end) {
        "exact"
    } else if actual_start >= start && actual_end <= end {
        "inset"
    } else if actual_start <= start && actual_end >= end {
        "outset"
    } else if actual_end <= start || actual_start >= end {
        "disjoint"
    } else {
        "partial_overlap"
    }
}

fn bump(map: &mut BTreeMap<String, usize>, value: impl Into<String>) {
    *map.entry(value.into()).or_default() += 1;
}

fn bind(map: &mut BTreeMap<u32, i128>, index: u32, value: i128) -> bool {
    match map.get(&index) {
        Some(existing) => *existing == value,
        None => {
            map.insert(index, value);
            true
        }
    }
}

fn table_boundaries(
    table: &pub_viewer::ViewerTable,
) -> Option<(BTreeMap<u32, i128>, BTreeMap<u32, i128>)> {
    let mut rows = BTreeMap::new();
    let mut columns = BTreeMap::new();
    for cell in &table.cells {
        if cell.row_span != 1 || cell.column_span != 1 {
            return None;
        }
        let bounds = bounds_i128(cell.bounds?)?;
        if !bind(&mut rows, cell.address.row, bounds[1])
            || !bind(&mut rows, cell.address.row + 1, bounds[3])
            || !bind(&mut columns, cell.address.column, bounds[0])
            || !bind(&mut columns, cell.address.column + 1, bounds[2])
        {
            return None;
        }
    }
    if rows.len() != table.rows as usize + 1 || columns.len() != table.columns as usize + 1 {
        return None;
    }
    Some((rows, columns))
}

#[derive(Debug, Default, Serialize)]
struct PageReceipt {
    viewer_page: u32,
    candidate_count: usize,
    owner_unique_count: usize,
    candidate_parent_group_count: usize,
    parent_group_resolved_count: usize,
    owner_same_parent_group_count: usize,
    group_fspgr_count: usize,
    group_client_anchor_count: usize,
    child_anchor_count: usize,
    projected_count: usize,
    table_boundary_count: usize,
    perpendicular: BTreeMap<String, usize>,
    thickness_vs_line_width: BTreeMap<String, usize>,
    longitudinal: BTreeMap<String, usize>,
    aspect: BTreeMap<String, usize>,
}

#[derive(Debug, Default, Serialize)]
struct Totals {
    candidate_count: usize,
    owner_unique_count: usize,
    candidate_parent_group_count: usize,
    parent_group_resolved_count: usize,
    owner_same_parent_group_count: usize,
    group_fspgr_count: usize,
    group_client_anchor_count: usize,
    child_anchor_count: usize,
    projected_count: usize,
    table_boundary_count: usize,
    perpendicular: BTreeMap<String, usize>,
    thickness_vs_line_width: BTreeMap<String, usize>,
    longitudinal: BTreeMap<String, usize>,
    aspect: BTreeMap<String, usize>,
}

#[derive(Debug, Serialize)]
struct Receipt {
    schema: &'static str,
    source_sha256: String,
    pages: Vec<PageReceipt>,
    totals: Totals,
    claims: Claims,
}

#[derive(Debug, Serialize)]
struct Claims {
    pdf_geometry_used: bool,
    semantic_side_inferred_from_geometry: bool,
    source_geometry_authority: &'static str,
    logical_boundary_authority: &'static str,
}

fn merge_hist(dst: &mut BTreeMap<String, usize>, src: &BTreeMap<String, usize>) {
    for (key, value) in src {
        *dst.entry(key.clone()).or_default() += *value;
    }
}

#[test]
#[ignore = "requires exact public Virginia Remplacante fixture"]
fn exact_virginia_table_border_geometry_probe() {
    let fixture = PathBuf::from(
        env::var_os("CHAPTERA_VIRGINIA_TABLE_BORDER_GEOMETRY_FIXTURE")
            .expect("CHAPTERA_VIRGINIA_TABLE_BORDER_GEOMETRY_FIXTURE"),
    );
    let output = PathBuf::from(
        env::var_os("CHAPTERA_VIRGINIA_TABLE_BORDER_GEOMETRY_OUT")
            .expect("CHAPTERA_VIRGINIA_TABLE_BORDER_GEOMETRY_OUT"),
    );

    let bytes = fs::read(&fixture).expect("read exact Virginia PUB");
    let sha = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    assert_eq!(sha, EXPECTED_SHA256, "exact Virginia source identity");

    let bundle =
        open_pub_bundle(&bytes, viewer_geometry_environment_v0_1()).expect("open Virginia Viewer");
    let escher = pub_cfb::read_stream_reader(
        Cursor::new(bytes.as_slice()),
        pub_reader::ESCHER_STREAM_PATH,
    )
    .expect("read Escher");
    let inventory = inspect_sp_containers(
        StreamPath(pub_reader::ESCHER_STREAM_PATH.into()),
        &escher,
    )
    .expect("inspect OfficeArt");

    let mut pages = Vec::new();
    let mut totals = Totals::default();

    for viewer_page in [6_u32, 7, 9, 10, 11, 21, 22, 23] {
        let page = bundle
            .geometry
            .document
            .pages
            .get((viewer_page - 1) as usize)
            .expect("selected page exists");
        let parent = page.id.into_canonical();
        let mut receipt = PageReceipt {
            viewer_page,
            ..PageReceipt::default()
        };

        for node in bundle
            .resolved_graph
            .nodes
            .values()
            .filter(|node| node.header.parent_id == parent && node.payload.table.is_some())
        {
            let source_table = node.payload.table.as_ref().expect("table");
            let table_seq = node.payload.contents_seq_num;
            let Some(viewer_table) = bundle
                .geometry
                .tables
                .iter()
                .find(|table| table.node_id == node.header.id)
            else {
                continue;
            };
            let Some((row_boundaries, column_boundaries)) = table_boundaries(viewer_table) else {
                continue;
            };

            let owners = inventory
                .shapes
                .iter()
                .filter(|shape| shape_id(shape) == Some(table_seq))
                .collect::<Vec<_>>();
            let owner = match owners.as_slice() {
                [owner] => {
                    receipt.owner_unique_count += 1;
                    Some(*owner)
                }
                _ => None,
            };

            for shape in inventory
                .shapes
                .iter()
                .filter(|shape| is_candidate(shape, table_seq))
            {
                receipt.candidate_count += 1;
                let Some(segment) =
                    decode_segment(shape, source_table.rows, source_table.columns)
                else {
                    continue;
                };

                let Some(parent_source) = shape.parent_group_shape_source.as_ref() else {
                    continue;
                };
                receipt.candidate_parent_group_count += 1;
                let parent_matches = inventory
                    .shapes
                    .iter()
                    .filter(|candidate| &candidate.source == parent_source)
                    .collect::<Vec<_>>();
                let parent_group = match parent_matches.as_slice() {
                    [parent_group] => {
                        receipt.parent_group_resolved_count += 1;
                        *parent_group
                    }
                    _ => continue,
                };
                if owner.is_some_and(|owner| {
                    owner.parent_group_shape_source.as_ref() == Some(parent_source)
                }) {
                    receipt.owner_same_parent_group_count += 1;
                }

                let Some(group_coords) = parent_group.fspgr.as_ref().and_then(rect_i128) else {
                    continue;
                };
                receipt.group_fspgr_count += 1;
                let Some(group_anchor) = parent_group
                    .client_anchor
                    .as_ref()
                    .and_then(publisher_anchor_rect)
                else {
                    continue;
                };
                receipt.group_client_anchor_count += 1;
                let Some(target) = center_origin_to_page(
                    page.size.width.get(),
                    page.size.height.get(),
                    group_anchor,
                ) else {
                    continue;
                };

                let Some(child) = shape.child_anchor.as_ref().and_then(rect_i128) else {
                    continue;
                };
                receipt.child_anchor_count += 1;
                let Some(projected) = project_rect(child, group_coords, target) else {
                    continue;
                };
                receipt.projected_count += 1;

                let Some(width) = unique_fopt_scalar(shape, LINE_WIDTH).map(i128::from) else {
                    continue;
                };
                if width <= 0 {
                    continue;
                }

                let logical = match segment.axis {
                    Axis::Horizontal => {
                        let (Some(&y), Some(&x0), Some(&x1)) = (
                            row_boundaries.get(&segment.row_start),
                            column_boundaries.get(&segment.column_start),
                            column_boundaries.get(&segment.column_end),
                        ) else {
                            continue;
                        };
                        (y, x0, x1)
                    }
                    Axis::Vertical => {
                        let (Some(&x), Some(&y0), Some(&y1)) = (
                            column_boundaries.get(&segment.column_start),
                            row_boundaries.get(&segment.row_start),
                            row_boundaries.get(&segment.row_end),
                        ) else {
                            continue;
                        };
                        (x, y0, y1)
                    }
                };
                receipt.table_boundary_count += 1;

                bump(
                    &mut receipt.perpendicular,
                    classify_perpendicular(segment.axis, projected, logical.0),
                );
                bump(
                    &mut receipt.thickness_vs_line_width,
                    classify_thickness(segment.axis, projected, width),
                );
                bump(
                    &mut receipt.longitudinal,
                    classify_longitudinal(segment.axis, projected, logical.1, logical.2),
                );
                let axis_extent = match segment.axis {
                    Axis::Horizontal => (projected[2] - projected[0], projected[3] - projected[1]),
                    Axis::Vertical => (projected[3] - projected[1], projected[2] - projected[0]),
                };
                bump(
                    &mut receipt.aspect,
                    if axis_extent.0 > axis_extent.1 {
                        "longer_along_segment"
                    } else if axis_extent.0 == axis_extent.1 {
                        "square"
                    } else {
                        "thicker_than_long"
                    },
                );
            }
        }

        totals.candidate_count += receipt.candidate_count;
        totals.owner_unique_count += receipt.owner_unique_count;
        totals.candidate_parent_group_count += receipt.candidate_parent_group_count;
        totals.parent_group_resolved_count += receipt.parent_group_resolved_count;
        totals.owner_same_parent_group_count += receipt.owner_same_parent_group_count;
        totals.group_fspgr_count += receipt.group_fspgr_count;
        totals.group_client_anchor_count += receipt.group_client_anchor_count;
        totals.child_anchor_count += receipt.child_anchor_count;
        totals.projected_count += receipt.projected_count;
        totals.table_boundary_count += receipt.table_boundary_count;
        merge_hist(&mut totals.perpendicular, &receipt.perpendicular);
        merge_hist(
            &mut totals.thickness_vs_line_width,
            &receipt.thickness_vs_line_width,
        );
        merge_hist(&mut totals.longitudinal, &receipt.longitudinal);
        merge_hist(&mut totals.aspect, &receipt.aspect);
        pages.push(receipt);
    }

    let receipt = Receipt {
        schema: "chaptera.virginia-table-border-geometry.v1",
        source_sha256: sha,
        pages,
        totals,
        claims: Claims {
            pdf_geometry_used: false,
            semantic_side_inferred_from_geometry: false,
            source_geometry_authority:
                "carrier parent-group FSPGR + carrier ChildAnchor + parent-group ClientAnchor projected into page EMU",
            logical_boundary_authority:
                "already-resolved Viewer simple TABLE cell bounds + native #740 side grammar",
        },
    };

    fs::create_dir_all(output.parent().expect("receipt parent")).expect("create receipt dir");
    fs::write(
        &output,
        serde_json::to_vec_pretty(&receipt).expect("serialize receipt"),
    )
    .expect("write receipt");

    assert_eq!(
        receipt.totals.candidate_count, 978,
        "must cover the exact native-proven Virginia cohort"
    );
}
