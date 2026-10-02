use pub_core::StreamPath;
use pub_escher::{
    PUBLISHER_FIELD_SHAPE_ID, PublisherFieldRecord, SpContainerObservation, inspect_sp_containers,
};
use pub_model::RectEmu;
use pub_viewer::{ViewerTable, open_pub_bundle, viewer_geometry_environment_v0_1};
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
const EXPECTED_CARRIERS: usize = 978;
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

fn owner_ref(shape: &SpContainerObservation) -> Option<u32> {
    unique_field(shape.client_anchor.as_ref()?, TABLE_OWNER_REF)
}

fn owner_contents_id(shape: &SpContainerObservation) -> Option<u32> {
    unique_field(shape.client_data.as_ref()?, PUBLISHER_FIELD_SHAPE_ID)
}

fn has_client_data_identity(shape: &SpContainerObservation) -> bool {
    shape
        .client_data
        .as_ref()
        .is_some_and(|record| record.fields.iter().any(|field| field.id == PUBLISHER_FIELD_SHAPE_ID))
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
    unique_field(anchor, TABLE_OWNER_REF) == Some(table_seq)
        && !anchor.fields.iter().any(|field| field.id == CELL_ORDINAL)
        && anchor.fields.iter().any(|field| {
            matches!(
                field.id,
                SEGMENT_ORIENTATION | ROW_START | COLUMN_START | ROW_END | COLUMN_END
            )
        })
}

#[derive(Debug, Clone, Copy, Serialize)]
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

fn decode_segment(
    shape: &SpContainerObservation,
    rows: u32,
    columns: u32,
) -> Result<Segment, &'static str> {
    let anchor = shape.client_anchor.as_ref().ok_or("anchor_absent")?;
    let allowed = [
        TABLE_OWNER_REF,
        SEGMENT_ORIENTATION,
        ROW_START,
        COLUMN_START,
        ROW_END,
        COLUMN_END,
    ];
    if anchor.fields.iter().any(|field| !allowed.contains(&field.id)) {
        return Err("unexpected_anchor_field");
    }

    let orientation =
        unique_field(anchor, SEGMENT_ORIENTATION).ok_or("orientation_missing_or_ambiguous")?;
    let row_start = unique_or_zero(anchor, ROW_START).ok_or("row_start_ambiguous")?;
    let column_start = unique_or_zero(anchor, COLUMN_START).ok_or("column_start_ambiguous")?;
    let row_end = unique_or_zero(anchor, ROW_END).ok_or("row_end_ambiguous")?;
    let column_end = unique_or_zero(anchor, COLUMN_END).ok_or("column_end_ambiguous")?;

    if row_start > rows || row_end > rows || column_start > columns || column_end > columns {
        return Err("boundary_out_of_grid");
    }

    match orientation {
        1 if row_start == row_end && column_start < column_end => Ok(Segment {
            axis: Axis::Horizontal,
            row_start,
            column_start,
            row_end,
            column_end,
        }),
        2 if column_start == column_end && row_start < row_end => Ok(Segment {
            axis: Axis::Vertical,
            row_start,
            column_start,
            row_end,
            column_end,
        }),
        1 => Err("horizontal_invalid"),
        2 => Err("vertical_invalid"),
        _ => Err("orientation_unknown"),
    }
}

fn rect_edges(rect: RectEmu) -> Option<[i128; 4]> {
    let left = i128::from(rect.x.get());
    let top = i128::from(rect.y.get());
    Some([
        left,
        top,
        left.checked_add(i128::from(rect.width.get()))?,
        top.checked_add(i128::from(rect.height.get()))?,
    ])
}

fn coordinate_rect(shape: &pub_escher::OfficeArtCoordinateRect) -> Option<[i128; 4]> {
    let rect = [
        i128::from(shape.x_left),
        i128::from(shape.y_top),
        i128::from(shape.x_right),
        i128::from(shape.y_bottom),
    ];
    (rect[2] > rect[0] && rect[3] > rect[1]).then_some(rect)
}

fn project_axis(
    value: i128,
    source_start: i128,
    source_end: i128,
    target_start: i128,
    target_end: i128,
) -> Option<i128> {
    let source_span = source_end.checked_sub(source_start)?;
    let target_span = target_end.checked_sub(target_start)?;
    if source_span <= 0 || target_span <= 0 {
        return None;
    }
    let delta = value.checked_sub(source_start)?;
    target_start.checked_add(delta.checked_mul(target_span)?.checked_div(source_span)?)
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

fn viewer_grid_rect(table: &ViewerTable) -> Option<[i128; 4]> {
    let mut left = None::<i128>;
    let mut top = None::<i128>;
    let mut right = None::<i128>;
    let mut bottom = None::<i128>;

    for cell in &table.cells {
        let bounds = cell.bounds?;
        let rect = rect_edges(bounds)?;
        left = Some(left.map_or(rect[0], |value| value.min(rect[0])));
        top = Some(top.map_or(rect[1], |value| value.min(rect[1])));
        right = Some(right.map_or(rect[2], |value| value.max(rect[2])));
        bottom = Some(bottom.map_or(rect[3], |value| value.max(rect[3])));
    }

    Some([left?, top?, right?, bottom?])
}

fn boundary_x(table: &ViewerTable, column: u32) -> Option<i128> {
    if column > table.columns || table.columns == 0 {
        return None;
    }
    if column == table.columns {
        let cell = table
            .cells
            .iter()
            .find(|cell| cell.address.row == 0 && cell.address.column + 1 == table.columns)?;
        let bounds = rect_edges(cell.bounds?)?;
        return Some(bounds[2]);
    }
    table
        .cells
        .iter()
        .find(|cell| cell.address.row == 0 && cell.address.column == column)
        .and_then(|cell| cell.bounds)
        .map(|bounds| i128::from(bounds.x.get()))
}

fn boundary_y(table: &ViewerTable, row: u32) -> Option<i128> {
    if row > table.rows || table.rows == 0 {
        return None;
    }
    if row == table.rows {
        let cell = table
            .cells
            .iter()
            .find(|cell| cell.address.column == 0 && cell.address.row + 1 == table.rows)?;
        let bounds = rect_edges(cell.bounds?)?;
        return Some(bounds[3]);
    }
    table
        .cells
        .iter()
        .find(|cell| cell.address.column == 0 && cell.address.row == row)
        .and_then(|cell| cell.bounds)
        .map(|bounds| i128::from(bounds.y.get()))
}

fn scaled_grid_coordinate(
    value: i128,
    grid_start: i128,
    grid_end: i128,
    owner_start: i128,
    owner_end: i128,
) -> Option<i128> {
    project_axis(value, grid_start, grid_end, owner_start, owner_end)
}

fn center_matches(center2: i128, boundary: i128) -> bool {
    center2
        .checked_sub(boundary.saturating_mul(2))
        .is_some_and(|delta| delta.abs() <= 2)
}

fn exact_endpoints(start: i128, end: i128, expected_start: i128, expected_end: i128) -> bool {
    (start - expected_start).abs() <= 1 && (end - expected_end).abs() <= 1
}

fn thickness_class(thickness: i128, line_width: Option<u32>) -> String {
    let Some(width) = line_width.map(i128::from) else {
        return "line_width_missing_or_ambiguous".to_owned();
    };
    if thickness == width {
        "equals_line_width".to_owned()
    } else if thickness.checked_mul(2) == Some(width) {
        "half_line_width".to_owned()
    } else if width.checked_mul(2) == Some(thickness) {
        "double_line_width".to_owned()
    } else {
        format!("other:thickness={thickness}:line_width={width}")
    }
}

fn bump(map: &mut BTreeMap<String, usize>, key: impl Into<String>) {
    *map.entry(key.into()).or_default() += 1;
}

#[derive(Debug, Default, Serialize)]
struct Stats {
    carrier_count: usize,
    decoded_count: usize,
    rejected_count: usize,
    child_anchor_count: usize,
    parent_group_match_count: usize,
    projected_count: usize,
    horizontal_count: usize,
    vertical_count: usize,
    raw_center_match_count: usize,
    scaled_center_match_count: usize,
    raw_only_center_match_count: usize,
    scaled_only_center_match_count: usize,
    both_center_match_count: usize,
    neither_center_match_count: usize,
    raw_longitudinal_match_count: usize,
    scaled_longitudinal_match_count: usize,
    thickness_classes: BTreeMap<String, usize>,
    rejected_reasons: BTreeMap<String, usize>,
    unresolved_reasons: BTreeMap<String, usize>,
}

#[derive(Debug, Serialize)]
struct TableReceipt {
    viewer_page: u32,
    table_seq_num: u32,
    rows: u32,
    columns: u32,
    owner_minus_grid_right_emu: i64,
    owner_minus_grid_bottom_emu: i64,
    stats: Stats,
}

#[derive(Debug, Default, Serialize)]
struct PageReceipt {
    viewer_page: u32,
    stats: Stats,
}

#[derive(Debug, Serialize)]
struct Receipt {
    schema: &'static str,
    source_sha256: String,
    pages: Vec<PageReceipt>,
    tables: Vec<TableReceipt>,
    totals: Stats,
    claims: Claims,
}

#[derive(Debug, Serialize)]
struct Claims {
    pdf_geometry_used: bool,
    geometry_proximity_used: bool,
    page_or_hash_product_rule_used: bool,
    carrier_geometry_source: &'static str,
    raw_boundary_source: &'static str,
    scaled_boundary_candidate: &'static str,
}

fn merge_stats(target: &mut Stats, source: &Stats) {
    target.carrier_count += source.carrier_count;
    target.decoded_count += source.decoded_count;
    target.rejected_count += source.rejected_count;
    target.child_anchor_count += source.child_anchor_count;
    target.parent_group_match_count += source.parent_group_match_count;
    target.projected_count += source.projected_count;
    target.horizontal_count += source.horizontal_count;
    target.vertical_count += source.vertical_count;
    target.raw_center_match_count += source.raw_center_match_count;
    target.scaled_center_match_count += source.scaled_center_match_count;
    target.raw_only_center_match_count += source.raw_only_center_match_count;
    target.scaled_only_center_match_count += source.scaled_only_center_match_count;
    target.both_center_match_count += source.both_center_match_count;
    target.neither_center_match_count += source.neither_center_match_count;
    target.raw_longitudinal_match_count += source.raw_longitudinal_match_count;
    target.scaled_longitudinal_match_count += source.scaled_longitudinal_match_count;
    for (key, value) in &source.thickness_classes {
        *target.thickness_classes.entry(key.clone()).or_default() += value;
    }
    for (key, value) in &source.rejected_reasons {
        *target.rejected_reasons.entry(key.clone()).or_default() += value;
    }
    for (key, value) in &source.unresolved_reasons {
        *target.unresolved_reasons.entry(key.clone()).or_default() += value;
    }
}

#[test]
#[ignore = "requires exact public Virginia Remplacante fixture"]
fn exact_virginia_table_border_child_anchor_census() {
    let fixture = PathBuf::from(
        env::var_os("CHAPTERA_VIRGINIA_TABLE_CHILD_ANCHOR_FIXTURE")
            .expect("CHAPTERA_VIRGINIA_TABLE_CHILD_ANCHOR_FIXTURE"),
    );
    let output = PathBuf::from(
        env::var_os("CHAPTERA_VIRGINIA_TABLE_CHILD_ANCHOR_OUT")
            .expect("CHAPTERA_VIRGINIA_TABLE_CHILD_ANCHOR_OUT"),
    );

    let bytes = fs::read(&fixture).expect("read exact Virginia PUB");
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

    let expected_tables = BTreeMap::from([
        (6_u32, 12_usize),
        (7, 1),
        (9, 1),
        (10, 1),
        (11, 1),
        (21, 2),
        (22, 3),
        (23, 1),
    ]);

    let mut pages = Vec::new();
    let mut tables = Vec::new();
    let mut totals = Stats::default();

    for (viewer_page, expected_table_count) in expected_tables {
        let page = &bundle.geometry.document.pages[(viewer_page - 1) as usize];
        let parent = page.id.into_canonical();
        let page_nodes = bundle
            .resolved_graph
            .nodes
            .values()
            .filter(|node| node.header.parent_id == parent && node.payload.table.is_some())
            .collect::<Vec<_>>();
        assert_eq!(page_nodes.len(), expected_table_count);

        let mut page_receipt = PageReceipt {
            viewer_page,
            stats: Stats::default(),
        };

        for node in page_nodes {
            let source = node.payload.table.as_ref().expect("filtered TABLE");
            let viewer = bundle
                .geometry
                .tables
                .iter()
                .find(|table| table.node_id == node.header.id)
                .expect("Viewer TABLE");
            let table_seq = node.payload.contents_seq_num;

            let owner_matches = inventory
                .shapes
                .iter()
                .filter(|shape| owner_contents_id(shape) == Some(table_seq))
                .collect::<Vec<_>>();
            let [owner] = owner_matches.as_slice() else {
                panic!("TABLE owner OfficeArt shape must be unique for seq {table_seq}");
            };
            let owner_group_rect = coordinate_rect(
                owner.child_anchor
                    .as_ref()
                    .expect("TABLE owner must expose ChildAnchor in shared group space"),
            )
            .expect("TABLE owner ChildAnchor must be positive");
            let owner_rect = rect_edges(node.header.bounds).expect("TABLE owner rect");
            let grid_rect = viewer_grid_rect(viewer).expect("complete Viewer grid");

            let owner_minus_grid_right = owner_rect[2] - grid_rect[2];
            let owner_minus_grid_bottom = owner_rect[3] - grid_rect[3];
            let mut stats = Stats::default();

            for shape in inventory
                .shapes
                .iter()
                .filter(|shape| owner_ref(shape) == Some(table_seq))
                .filter(|shape| is_candidate(shape, table_seq))
            {
                stats.carrier_count += 1;
                let segment = match decode_segment(shape, source.rows, source.columns) {
                    Ok(segment) => segment,
                    Err(reason) => {
                        stats.rejected_count += 1;
                        bump(&mut stats.rejected_reasons, reason);
                        continue;
                    }
                };
                stats.decoded_count += 1;

                match segment.axis {
                    Axis::Horizontal => stats.horizontal_count += 1,
                    Axis::Vertical => stats.vertical_count += 1,
                }

                let Some(anchor) = shape.child_anchor.as_ref() else {
                    bump(&mut stats.unresolved_reasons, "child_anchor_missing");
                    continue;
                };
                stats.child_anchor_count += 1;

                if shape.parent_group_shape_source.as_ref()
                    != owner.parent_group_shape_source.as_ref()
                    || shape.parent_group_shape_source.is_none()
                {
                    bump(&mut stats.unresolved_reasons, "shared_parent_group_mismatch");
                    continue;
                }
                stats.parent_group_match_count += 1;

                let Some(child_rect) = coordinate_rect(anchor) else {
                    bump(&mut stats.unresolved_reasons, "child_anchor_non_positive");
                    continue;
                };
                let Some(projected) = project_rect(child_rect, owner_group_rect, owner_rect) else {
                    bump(&mut stats.unresolved_reasons, "projection_failed");
                    continue;
                };
                stats.projected_count += 1;

                let (
                    raw_cross,
                    scaled_cross,
                    raw_long_start,
                    raw_long_end,
                    scaled_long_start,
                    scaled_long_end,
                    thickness,
                    center2,
                ) = match segment.axis {
                    Axis::Horizontal => {
                        let raw_cross = boundary_y(viewer, segment.row_start)
                            .expect("horizontal row boundary");
                        let raw_long_start = boundary_x(viewer, segment.column_start)
                            .expect("horizontal start boundary");
                        let raw_long_end = boundary_x(viewer, segment.column_end)
                            .expect("horizontal end boundary");
                        let scaled_cross = scaled_grid_coordinate(
                            raw_cross,
                            grid_rect[1],
                            grid_rect[3],
                            owner_rect[1],
                            owner_rect[3],
                        )
                        .expect("scaled horizontal boundary");
                        let scaled_long_start = scaled_grid_coordinate(
                            raw_long_start,
                            grid_rect[0],
                            grid_rect[2],
                            owner_rect[0],
                            owner_rect[2],
                        )
                        .expect("scaled horizontal start");
                        let scaled_long_end = scaled_grid_coordinate(
                            raw_long_end,
                            grid_rect[0],
                            grid_rect[2],
                            owner_rect[0],
                            owner_rect[2],
                        )
                        .expect("scaled horizontal end");
                        (
                            raw_cross,
                            scaled_cross,
                            raw_long_start,
                            raw_long_end,
                            scaled_long_start,
                            scaled_long_end,
                            projected[3] - projected[1],
                            projected[1] + projected[3],
                        )
                    }
                    Axis::Vertical => {
                        let raw_cross = boundary_x(viewer, segment.column_start)
                            .expect("vertical column boundary");
                        let raw_long_start =
                            boundary_y(viewer, segment.row_start).expect("vertical start boundary");
                        let raw_long_end =
                            boundary_y(viewer, segment.row_end).expect("vertical end boundary");
                        let scaled_cross = scaled_grid_coordinate(
                            raw_cross,
                            grid_rect[0],
                            grid_rect[2],
                            owner_rect[0],
                            owner_rect[2],
                        )
                        .expect("scaled vertical boundary");
                        let scaled_long_start = scaled_grid_coordinate(
                            raw_long_start,
                            grid_rect[1],
                            grid_rect[3],
                            owner_rect[1],
                            owner_rect[3],
                        )
                        .expect("scaled vertical start");
                        let scaled_long_end = scaled_grid_coordinate(
                            raw_long_end,
                            grid_rect[1],
                            grid_rect[3],
                            owner_rect[1],
                            owner_rect[3],
                        )
                        .expect("scaled vertical end");
                        (
                            raw_cross,
                            scaled_cross,
                            raw_long_start,
                            raw_long_end,
                            scaled_long_start,
                            scaled_long_end,
                            projected[2] - projected[0],
                            projected[0] + projected[2],
                        )
                    }
                };

                let raw_match = center_matches(center2, raw_cross);
                let scaled_match = center_matches(center2, scaled_cross);
                stats.raw_center_match_count += usize::from(raw_match);
                stats.scaled_center_match_count += usize::from(scaled_match);
                match (raw_match, scaled_match) {
                    (true, true) => stats.both_center_match_count += 1,
                    (true, false) => stats.raw_only_center_match_count += 1,
                    (false, true) => stats.scaled_only_center_match_count += 1,
                    (false, false) => stats.neither_center_match_count += 1,
                }

                let (projected_long_start, projected_long_end) = match segment.axis {
                    Axis::Horizontal => (projected[0], projected[2]),
                    Axis::Vertical => (projected[1], projected[3]),
                };
                stats.raw_longitudinal_match_count += usize::from(exact_endpoints(
                    projected_long_start,
                    projected_long_end,
                    raw_long_start,
                    raw_long_end,
                ));
                stats.scaled_longitudinal_match_count += usize::from(exact_endpoints(
                    projected_long_start,
                    projected_long_end,
                    scaled_long_start,
                    scaled_long_end,
                ));

                bump(
                    &mut stats.thickness_classes,
                    thickness_class(thickness, unique_fopt_scalar(shape, LINE_WIDTH)),
                );
            }

            println!(
                "TABLE_CHILD_ANCHOR page={} seq={} rows={} cols={} delta_right={} delta_bottom={} carriers={} projected={} raw_center={} scaled_center={} raw_only={} scaled_only={} both={} neither={} raw_long={} scaled_long={} thickness={:?} unresolved={:?}",
                viewer_page,
                table_seq,
                source.rows,
                source.columns,
                owner_minus_grid_right,
                owner_minus_grid_bottom,
                stats.carrier_count,
                stats.projected_count,
                stats.raw_center_match_count,
                stats.scaled_center_match_count,
                stats.raw_only_center_match_count,
                stats.scaled_only_center_match_count,
                stats.both_center_match_count,
                stats.neither_center_match_count,
                stats.raw_longitudinal_match_count,
                stats.scaled_longitudinal_match_count,
                stats.thickness_classes,
                stats.unresolved_reasons,
            );

            merge_stats(&mut page_receipt.stats, &stats);
            merge_stats(&mut totals, &stats);
            tables.push(TableReceipt {
                viewer_page,
                table_seq_num: table_seq,
                rows: source.rows,
                columns: source.columns,
                owner_minus_grid_right_emu: i64::try_from(owner_minus_grid_right)
                    .expect("right delta fits i64"),
                owner_minus_grid_bottom_emu: i64::try_from(owner_minus_grid_bottom)
                    .expect("bottom delta fits i64"),
                stats,
            });
        }

        println!(
            "TABLE_CHILD_ANCHOR_PAGE page={} carriers={} projected={} raw_center={} scaled_center={} raw_only={} scaled_only={} both={} neither={} raw_long={} scaled_long={}",
            viewer_page,
            page_receipt.stats.carrier_count,
            page_receipt.stats.projected_count,
            page_receipt.stats.raw_center_match_count,
            page_receipt.stats.scaled_center_match_count,
            page_receipt.stats.raw_only_center_match_count,
            page_receipt.stats.scaled_only_center_match_count,
            page_receipt.stats.both_center_match_count,
            page_receipt.stats.neither_center_match_count,
            page_receipt.stats.raw_longitudinal_match_count,
            page_receipt.stats.scaled_longitudinal_match_count,
        );
        pages.push(page_receipt);
    }

    println!(
        "TABLE_CHILD_ANCHOR_TOTAL carriers={} decoded={} rejected={} child_anchor={} parent_match={} projected={} raw_center={} scaled_center={} raw_only={} scaled_only={} both={} neither={} raw_long={} scaled_long={} thickness={:?} unresolved={:?}",
        totals.carrier_count,
        totals.decoded_count,
        totals.rejected_count,
        totals.child_anchor_count,
        totals.parent_group_match_count,
        totals.projected_count,
        totals.raw_center_match_count,
        totals.scaled_center_match_count,
        totals.raw_only_center_match_count,
        totals.scaled_only_center_match_count,
        totals.both_center_match_count,
        totals.neither_center_match_count,
        totals.raw_longitudinal_match_count,
        totals.scaled_longitudinal_match_count,
        totals.thickness_classes,
        totals.unresolved_reasons,
    );

    let receipt = Receipt {
        schema: "chaptera.virginia-table-border-child-anchor.v1",
        source_sha256: sha,
        pages,
        tables,
        totals,
        claims: Claims {
            pdf_geometry_used: false,
            geometry_proximity_used: false,
            page_or_hash_product_rule_used: false,
            carrier_geometry_source: "OfficeArt ChildAnchor projected through exact TABLE owner FSPGR",
            raw_boundary_source: "current exact TABLE/rowcol_array Viewer cell bounds",
            scaled_boundary_candidate: "raw grid coordinate projected to exact TABLE owner extent",
        },
    };

    fs::create_dir_all(output.parent().expect("receipt parent")).expect("create receipt dir");
    fs::write(
        &output,
        serde_json::to_vec_pretty(&receipt).expect("serialize receipt"),
    )
    .expect("write receipt");

    assert_eq!(receipt.totals.carrier_count, EXPECTED_CARRIERS);
    assert_eq!(receipt.totals.decoded_count, EXPECTED_CARRIERS);
    assert_eq!(receipt.totals.rejected_count, 0);
}
