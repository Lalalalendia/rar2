use pub_core::StreamPath;
use pub_escher::{PUBLISHER_FIELD_SHAPE_ID, PublisherFieldRecord, SpContainerObservation, inspect_sp_containers};
use pub_model::{AuthorityClass, ReadConfidence, SourceRef, SourceRole};
use pub_viewer::{open_pub_bundle, viewer_geometry_environment_v0_1};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{collections::{BTreeMap, BTreeSet}, env, fs, io::Cursor, path::PathBuf};

const EXPECTED_SHA256: &str = "88f57d800aeec808798ea487b9d4ab85dc85c02cd190b987a332709b81018506";
const EXPECTED_BORDER_CARRIERS: usize = 978;
const TABLE_OWNER_REF: u16 = 0x6802;
const CELL_ORDINAL: u16 = 0x2003;
const SEGMENT_ORIENTATION: u16 = 0x2001;
const ROW_START: u16 = 0x2004;
const COLUMN_START: u16 = 0x2005;
const ROW_END: u16 = 0x2006;
const COLUMN_END: u16 = 0x2007;
const RECTANGLE: u16 = 0x0001;
const FILL_COLOR: u16 = 0x0181;
const LINE_WIDTH: u16 = 0x01CB;

fn unique_field(record: &PublisherFieldRecord, id: u16) -> Option<u32> {
    let mut it = record.fields.iter().filter(|f| f.id == id);
    let v = it.next()?.value;
    it.next().is_none().then_some(v)
}
fn unique_or_zero(record: &PublisherFieldRecord, id: u16) -> Option<u32> {
    let mut it = record.fields.iter().filter(|f| f.id == id);
    let Some(first) = it.next() else { return Some(0); };
    it.next().is_none().then_some(first.value)
}
fn unique_fopt_scalar(shape: &SpContainerObservation, id: u16) -> Option<u32> {
    let mut it = shape.fopts.iter().flat_map(|r| r.properties.iter())
        .filter(|p| p.property_id() == id && !p.f_bid() && !p.f_complex());
    let v = it.next()?.op;
    it.next().is_none().then_some(v)
}
fn owner_ref(shape: &SpContainerObservation) -> Option<u32> {
    unique_field(shape.client_anchor.as_ref()?, TABLE_OWNER_REF)
}
fn has_client_data_identity(shape: &SpContainerObservation) -> bool {
    shape.client_data.as_ref().is_some_and(|r| r.fields.iter().any(|f| f.id == PUBLISHER_FIELD_SHAPE_ID))
}
fn is_candidate(shape: &SpContainerObservation, table_seq: u32) -> bool {
    if shape.fsp.as_ref().map(|fsp| fsp.shape_type) != Some(RECTANGLE) || has_client_data_identity(shape) { return false; }
    let Some(anchor) = shape.client_anchor.as_ref() else { return false; };
    unique_field(anchor, TABLE_OWNER_REF) == Some(table_seq)
        && !anchor.fields.iter().any(|f| f.id == CELL_ORDINAL)
        && anchor.fields.iter().any(|f| matches!(f.id, SEGMENT_ORIENTATION|ROW_START|COLUMN_START|ROW_END|COLUMN_END))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all="snake_case")]
enum Axis { Horizontal, Vertical }

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct SegmentKey { axis: Axis, row_start: u32, column_start: u32, row_end: u32, column_end: u32 }

fn decode_segment(shape: &SpContainerObservation, rows: u32, columns: u32) -> Result<SegmentKey, &'static str> {
    let anchor = shape.client_anchor.as_ref().ok_or("anchor_absent")?;
    let allowed = [TABLE_OWNER_REF, SEGMENT_ORIENTATION, ROW_START, COLUMN_START, ROW_END, COLUMN_END];
    if anchor.fields.iter().any(|f| !allowed.contains(&f.id)) { return Err("unexpected_anchor_field"); }
    let orientation = unique_field(anchor, SEGMENT_ORIENTATION).ok_or("orientation_missing_or_ambiguous")?;
    let rs = unique_or_zero(anchor, ROW_START).ok_or("row_start_ambiguous")?;
    let cs = unique_or_zero(anchor, COLUMN_START).ok_or("column_start_ambiguous")?;
    let re = unique_or_zero(anchor, ROW_END).ok_or("row_end_ambiguous")?;
    let ce = unique_or_zero(anchor, COLUMN_END).ok_or("column_end_ambiguous")?;
    if rs > rows || re > rows || cs > columns || ce > columns { return Err("boundary_out_of_grid"); }
    let key = match orientation {
        1 if rs == re && cs < ce => SegmentKey{axis:Axis::Horizontal,row_start:rs,column_start:cs,row_end:re,column_end:ce},
        2 if cs == ce && rs < re => SegmentKey{axis:Axis::Vertical,row_start:rs,column_start:cs,row_end:re,column_end:ce},
        1 => return Err("horizontal_invalid"),
        2 => return Err("vertical_invalid"),
        _ => return Err("orientation_unknown"),
    };
    let color = unique_fopt_scalar(shape, FILL_COLOR).ok_or("color_missing_or_ambiguous")?;
    if color >> 24 != 0 { return Err("color_not_direct"); }
    if unique_fopt_scalar(shape, LINE_WIDTH).ok_or("width_missing_or_ambiguous")? == 0 { return Err("width_zero"); }
    Ok(key)
}

fn same_span(reference: &SourceRef, shape: &SpContainerObservation) -> bool {
    let Some(range) = reference.byte_range else { return false; };
    reference.carrier == shape.source.stream.0 && range.offset == shape.source.offset && range.length == shape.source.len
}
fn generic_fopt_ref(reference: &SourceRef, shape: &SpContainerObservation) -> bool {
    same_span(reference, shape)
        && reference.path.as_deref() == Some("SpContainer/FOPT")
        && reference.authority == AuthorityClass::Authoritative
        && reference.confidence == Some(ReadConfidence::Exact)
        && matches!(reference.role, SourceRole::Semantic | SourceRole::Projection)
}
fn bump(map: &mut BTreeMap<String, usize>, key: &str) { *map.entry(key.to_owned()).or_default() += 1; }

#[derive(Default, Serialize)]
struct PageReceipt {
    viewer_page: u32,
    table_count: usize,
    carrier_count: usize,
    decoded_count: usize,
    rejected_count: usize,
    existing_generic_paint_count: usize,
    absent_from_generic_paint_count: usize,
    ambiguous_join_count: usize,
    same_span_node_count: usize,
    generic_fopt_node_count: usize,
    viewer_paint_node_count: usize,
    scene_backed_paint_node_count: usize,
    rejected_reasons: BTreeMap<String, usize>,
}
#[derive(Default, Serialize)]
struct Totals {
    table_count: usize,
    carrier_count: usize,
    decoded_count: usize,
    rejected_count: usize,
    existing_generic_paint_count: usize,
    absent_from_generic_paint_count: usize,
    ambiguous_join_count: usize,
    same_span_node_count: usize,
    generic_fopt_node_count: usize,
    viewer_paint_node_count: usize,
    scene_backed_paint_node_count: usize,
}
#[derive(Serialize)]
struct Receipt {
    schema: &'static str,
    source_sha256: String,
    pages: Vec<PageReceipt>,
    totals: Totals,
    exact_source_span_join_only: bool,
    geometry_or_proximity_join_used: bool,
    product_render_behavior_changed: bool,
}

#[test]
#[ignore = "requires exact public Virginia Remplacante fixture"]
fn exact_virginia_table_border_composition_probe() {
    let fixture = PathBuf::from(env::var_os("CHAPTERA_VIRGINIA_TABLE_COMPOSITION_FIXTURE").expect("fixture env"));
    let output = PathBuf::from(env::var_os("CHAPTERA_VIRGINIA_TABLE_COMPOSITION_OUT").expect("output env"));
    let bytes = fs::read(&fixture).expect("read fixture");
    let sha = Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect::<String>();
    assert_eq!(sha, EXPECTED_SHA256);

    let bundle = open_pub_bundle(&bytes, viewer_geometry_environment_v0_1()).expect("open Viewer bundle");
    let escher = pub_cfb::read_stream_reader(Cursor::new(bytes.as_slice()), pub_reader::ESCHER_STREAM_PATH).expect("read Escher");
    let inventory = inspect_sp_containers(StreamPath(pub_reader::ESCHER_STREAM_PATH.into()), &escher).expect("inspect SpContainers");

    let mut pages = Vec::new();
    let mut totals = Totals::default();

    for viewer_page in [6_u32,7,9,10,11,21,22,23] {
        let page = &bundle.geometry.document.pages[(viewer_page - 1) as usize];
        let parent = page.id.into_canonical();
        let mut pr = PageReceipt{viewer_page, ..PageReceipt::default()};

        for node in bundle.resolved_graph.nodes.values().filter(|n| n.header.parent_id == parent && n.payload.table.is_some()) {
            let table = node.payload.table.as_ref().unwrap();
            let table_seq = node.payload.contents_seq_num;
            pr.table_count += 1;
            let mut semantic = BTreeSet::<SegmentKey>::new();

            for shape in inventory.shapes.iter().filter(|s| owner_ref(s) == Some(table_seq)).filter(|s| is_candidate(s, table_seq)) {
                pr.carrier_count += 1;
                let segment = match decode_segment(shape, table.rows, table.columns) {
                    Ok(s) => s,
                    Err(reason) => { pr.rejected_count += 1; bump(&mut pr.rejected_reasons, reason); continue; }
                };
                pr.decoded_count += 1;
                assert!(semantic.insert(segment), "duplicate semantic segment");

                let same_span_nodes = bundle.resolved_graph.nodes.values()
                    .filter(|n| n.header.source_refs.iter().any(|r| same_span(r, shape)))
                    .collect::<Vec<_>>();
                let generic_nodes = same_span_nodes.iter().copied()
                    .filter(|n| n.header.source_refs.iter().any(|r| generic_fopt_ref(r, shape)))
                    .collect::<Vec<_>>();

                let mut viewer_paints = 0usize;
                let mut scene_backed = 0usize;
                for n in &generic_nodes {
                    let paints = bundle.geometry.paints.iter().filter(|p| p.node_id == n.header.id).count();
                    let scene_count = bundle.geometry.scene.nodes.iter().filter(|s| s.origin == n.header.id).count();
                    viewer_paints += paints;
                    if scene_count > 0 { scene_backed += paints; }
                }

                let ambiguous = same_span_nodes.len() > 1 || generic_nodes.len() > 1 || viewer_paints > 1;
                if ambiguous { pr.ambiguous_join_count += 1; }
                if scene_backed > 0 { pr.existing_generic_paint_count += 1; } else { pr.absent_from_generic_paint_count += 1; }

                pr.same_span_node_count += same_span_nodes.len();
                pr.generic_fopt_node_count += generic_nodes.len();
                pr.viewer_paint_node_count += viewer_paints;
                pr.scene_backed_paint_node_count += scene_backed;
            }
        }

        println!("TABLE_COMPOSITION page={} tables={} carriers={} decoded={} rejected={} generic_paint={} absent={} ambiguous={} same_span_nodes={} generic_fopt_nodes={} viewer_paints={} scene_backed_paints={}",
            pr.viewer_page, pr.table_count, pr.carrier_count, pr.decoded_count, pr.rejected_count, pr.existing_generic_paint_count,
            pr.absent_from_generic_paint_count, pr.ambiguous_join_count, pr.same_span_node_count, pr.generic_fopt_node_count,
            pr.viewer_paint_node_count, pr.scene_backed_paint_node_count);

        totals.table_count += pr.table_count;
        totals.carrier_count += pr.carrier_count;
        totals.decoded_count += pr.decoded_count;
        totals.rejected_count += pr.rejected_count;
        totals.existing_generic_paint_count += pr.existing_generic_paint_count;
        totals.absent_from_generic_paint_count += pr.absent_from_generic_paint_count;
        totals.ambiguous_join_count += pr.ambiguous_join_count;
        totals.same_span_node_count += pr.same_span_node_count;
        totals.generic_fopt_node_count += pr.generic_fopt_node_count;
        totals.viewer_paint_node_count += pr.viewer_paint_node_count;
        totals.scene_backed_paint_node_count += pr.scene_backed_paint_node_count;
        pages.push(pr);
    }

    println!("TABLE_COMPOSITION totals tables={} carriers={} decoded={} rejected={} generic_paint={} absent={} ambiguous={} same_span_nodes={} generic_fopt_nodes={} viewer_paints={} scene_backed_paints={}",
        totals.table_count, totals.carrier_count, totals.decoded_count, totals.rejected_count, totals.existing_generic_paint_count,
        totals.absent_from_generic_paint_count, totals.ambiguous_join_count, totals.same_span_node_count, totals.generic_fopt_node_count,
        totals.viewer_paint_node_count, totals.scene_backed_paint_node_count);

    let receipt = Receipt {
        schema: "chaptera.virginia-table-border-composition-probe.v1",
        source_sha256: sha,
        pages,
        totals,
        exact_source_span_join_only: true,
        geometry_or_proximity_join_used: false,
        product_render_behavior_changed: false,
    };
    fs::create_dir_all(output.parent().unwrap()).expect("create output dir");
    fs::write(&output, serde_json::to_vec_pretty(&receipt).unwrap()).expect("write receipt");

    assert_eq!(receipt.totals.rejected_count, 0);
    assert_eq!(receipt.totals.ambiguous_join_count, 0);
    assert_eq!(receipt.totals.decoded_count, EXPECTED_BORDER_CARRIERS);
}
