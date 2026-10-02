use pub_core::{RawSpan, StreamPath};
use pub_escher::{
    PUBLISHER_FIELD_SHAPE_ID, PublisherFieldRecord, SpContainerObservation, inspect_sp_containers,
};
use pub_model::{NodeId, SourceRef};
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
const TABLE_OWNER_REF: u16 = 0x6802;
const CELL_ORDINAL: u16 = 0x2003;
const SEGMENT_ORIENTATION: u16 = 0x2001;
const ROW_START: u16 = 0x2004;
const COLUMN_START: u16 = 0x2005;
const ROW_END: u16 = 0x2006;
const COLUMN_END: u16 = 0x2007;
const RECTANGLE: u16 = 0x0001;

fn unique_field(record: &PublisherFieldRecord, id: u16) -> Option<u32> {
    let mut matches = record.fields.iter().filter(|field| field.id == id);
    let first = matches.next()?.value;
    matches.next().is_none().then_some(first)
}

fn grounded(shape: &SpContainerObservation, grounded: &BTreeSet<u32>) -> bool {
    shape
        .client_data
        .as_ref()
        .and_then(|record| unique_field(record, PUBLISHER_FIELD_SHAPE_ID))
        .is_some_and(|seq| grounded.contains(&seq))
}

fn owner_ref(shape: &SpContainerObservation) -> Option<u32> {
    unique_field(shape.client_anchor.as_ref()?, TABLE_OWNER_REF)
}

fn is_candidate(shape: &SpContainerObservation, table_seq: u32) -> bool {
    if shape.fsp.as_ref().map(|fsp| fsp.shape_type) != Some(RECTANGLE) {
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

fn overlaps(a: &RawSpan, b: &RawSpan) -> bool {
    if a.stream != b.stream {
        return false;
    }
    let Some(a_end) = a.end() else { return false };
    let Some(b_end) = b.end() else { return false };
    a.offset < b_end && b.offset < a_end
}

fn exact_source_ref(span: &RawSpan, source_ref: &SourceRef) -> bool {
    source_ref.carrier == span.stream.0
        && source_ref
            .byte_range
            .is_some_and(|range| range.offset == span.offset && range.length == span.len)
}

fn overlapping_source_ref(span: &RawSpan, source_ref: &SourceRef) -> bool {
    if source_ref.carrier != span.stream.0 {
        return false;
    }
    let Some(range) = source_ref.byte_range else {
        return false;
    };
    overlaps(
        span,
        &RawSpan {
            stream: span.stream.clone(),
            offset: range.offset,
            len: range.length,
        },
    )
}

fn effective_paint_spans(
    node: &pub_model::Node<pub_reader::PubResolvedNodePayload>,
) -> Vec<&RawSpan> {
    let mut out = Vec::new();
    let Some(paint) = node.payload.effective_paint.as_ref() else {
        return out;
    };
    if let Some(v) = paint.fill.solid.as_ref().and_then(|v| v.source.as_ref()) {
        out.push(v);
    }
    if let Some(v) = paint.fill.color_rgb.as_ref().and_then(|v| v.source.as_ref()) {
        out.push(v);
    }
    if let Some(v) = paint.fill.visible.as_ref().and_then(|v| v.source.as_ref()) {
        out.push(v);
    }
    if let Some(v) = paint.line.color_rgb.as_ref().and_then(|v| v.source.as_ref()) {
        out.push(v);
    }
    if let Some(v) = paint.line.width_emu.as_ref().and_then(|v| v.source.as_ref()) {
        out.push(v);
    }
    if let Some(v) = paint.line.visible.as_ref().and_then(|v| v.source.as_ref()) {
        out.push(v);
    }
    out
}

fn bump(map: &mut BTreeMap<String, usize>, key: impl Into<String>) {
    *map.entry(key.into()).or_default() += 1;
}

#[derive(Debug, Default, Serialize)]
struct PageReceipt {
    viewer_page: u32,
    table_count: usize,
    candidate_count: usize,
    exact_header_source_ref_count: usize,
    overlapping_header_source_ref_count: usize,
    effective_paint_source_overlap_count: usize,
    existing_viewer_paint_overlap_count: usize,
    ambiguous_node_overlap_count: usize,
    fopt_property_ids: BTreeMap<String, usize>,
    fopt_scalar_values: BTreeMap<String, BTreeMap<String, usize>>,
}

#[derive(Debug, Default, Serialize)]
struct Totals {
    table_count: usize,
    candidate_count: usize,
    exact_header_source_ref_count: usize,
    overlapping_header_source_ref_count: usize,
    effective_paint_source_overlap_count: usize,
    existing_viewer_paint_overlap_count: usize,
    ambiguous_node_overlap_count: usize,
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
    pdf_used_as_semantic_authority: bool,
    geometry_proximity_used: bool,
    page_or_hash_product_rule_used: bool,
    native_border_authority: &'static str,
}

#[test]
#[ignore = "requires exact public Virginia Remplacante fixture"]
fn exact_virginia_table_border_composition_ownership_probe() {
    let fixture = PathBuf::from(
        env::var_os("CHAPTERA_VIRGINIA_TABLE_BORDER_FIXTURE")
            .expect("CHAPTERA_VIRGINIA_TABLE_BORDER_FIXTURE"),
    );
    let output = PathBuf::from(
        env::var_os("CHAPTERA_VIRGINIA_TABLE_BORDER_OWNERSHIP_OUT")
            .expect("CHAPTERA_VIRGINIA_TABLE_BORDER_OWNERSHIP_OUT"),
    );

    let bytes = fs::read(&fixture).expect("read exact Virginia PUB");
    let sha = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    assert_eq!(sha, EXPECTED_SHA256, "exact Virginia source identity");

    let bundle = open_pub_bundle(&bytes, viewer_geometry_environment_v0_1())
        .expect("open exact Virginia through Viewer");
    let escher = pub_cfb::read_stream_reader(
        Cursor::new(bytes.as_slice()),
        pub_reader::ESCHER_STREAM_PATH,
    )
    .expect("read Escher stream");
    let inventory = inspect_sp_containers(
        StreamPath(pub_reader::ESCHER_STREAM_PATH.into()),
        &escher,
    )
    .expect("inspect OfficeArt SpContainers");

    let grounded_ids = bundle
        .resolved_graph
        .nodes
        .values()
        .map(|node| node.payload.contents_seq_num)
        .collect::<BTreeSet<_>>();
    let viewer_paint_nodes = bundle
        .geometry
        .paints
        .iter()
        .map(|paint| paint.node_id)
        .collect::<BTreeSet<NodeId>>();

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

        for table_node in bundle
            .resolved_graph
            .nodes
            .values()
            .filter(|node| node.header.parent_id == parent && node.payload.table.is_some())
        {
            receipt.table_count += 1;
            let table_seq = table_node.payload.contents_seq_num;

            for shape in inventory
                .shapes
                .iter()
                .filter(|shape| owner_ref(shape) == Some(table_seq))
                .filter(|shape| !grounded(shape, &grounded_ids))
                .filter(|shape| is_candidate(shape, table_seq))
            {
                receipt.candidate_count += 1;

                for fopt in &shape.fopts {
                    for property in &fopt.properties {
                        let id = property.property_id();
                        bump(&mut receipt.fopt_property_ids, format!("0x{id:04x}"));
                        if !property.f_bid() && !property.f_complex() {
                            let values = receipt
                                .fopt_scalar_values
                                .entry(format!("0x{id:04x}"))
                                .or_default();
                            bump(values, format!("0x{:08x}", property.op));
                        }
                    }
                }

                let mut exact_nodes = BTreeSet::<NodeId>::new();
                let mut overlapping_nodes = BTreeSet::<NodeId>::new();
                let mut effective_nodes = BTreeSet::<NodeId>::new();

                for node in bundle.resolved_graph.nodes.values() {
                    if node
                        .header
                        .source_refs
                        .iter()
                        .any(|source_ref| exact_source_ref(&shape.source, source_ref))
                    {
                        exact_nodes.insert(node.header.id);
                    }
                    if node
                        .header
                        .source_refs
                        .iter()
                        .any(|source_ref| overlapping_source_ref(&shape.source, source_ref))
                    {
                        overlapping_nodes.insert(node.header.id);
                    }
                    if effective_paint_spans(node)
                        .into_iter()
                        .any(|span| overlaps(&shape.source, span))
                    {
                        effective_nodes.insert(node.header.id);
                    }
                }

                if !exact_nodes.is_empty() {
                    receipt.exact_header_source_ref_count += 1;
                }
                if !overlapping_nodes.is_empty() {
                    receipt.overlapping_header_source_ref_count += 1;
                }
                if !effective_nodes.is_empty() {
                    receipt.effective_paint_source_overlap_count += 1;
                }

                let mut all_nodes = overlapping_nodes;
                all_nodes.extend(effective_nodes);
                let painted = all_nodes
                    .iter()
                    .filter(|node_id| viewer_paint_nodes.contains(node_id))
                    .count();
                if painted > 0 {
                    receipt.existing_viewer_paint_overlap_count += 1;
                }
                if all_nodes.len() > 1 {
                    receipt.ambiguous_node_overlap_count += 1;
                }
            }
        }

        totals.table_count += receipt.table_count;
        totals.candidate_count += receipt.candidate_count;
        totals.exact_header_source_ref_count += receipt.exact_header_source_ref_count;
        totals.overlapping_header_source_ref_count += receipt.overlapping_header_source_ref_count;
        totals.effective_paint_source_overlap_count += receipt.effective_paint_source_overlap_count;
        totals.existing_viewer_paint_overlap_count += receipt.existing_viewer_paint_overlap_count;
        totals.ambiguous_node_overlap_count += receipt.ambiguous_node_overlap_count;
        pages.push(receipt);
    }

    let receipt = Receipt {
        schema: "chaptera.virginia-table-border-composition-ownership.v1",
        source_sha256: sha,
        pages,
        totals,
        claims: Claims {
            pdf_used_as_semantic_authority: false,
            geometry_proximity_used: false,
            page_or_hash_product_rule_used: false,
            native_border_authority:
                "#740/#773 Publisher native side/color/weight authority + #772 exact Virginia segment discriminator",
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
        "composition probe must cover the exact #772 native-proven carrier set"
    );
    assert_eq!(
        receipt.totals.ambiguous_node_overlap_count, 0,
        "source-provenance ownership joins must remain unambiguous"
    );
}
