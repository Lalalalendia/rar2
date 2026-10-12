//! Source-safe exact SampleBrochure LOGO 0x31 rectangle admission census.
use pub_contents::{
    parse_0x2c_header, parse_confirmed_0x2c_trailer_root, parse_confirmed_chunk_reference,
};
use pub_core::StreamPath;
use pub_model::Sha256Digest;
use pub_reader::build_mature_0x2c_source_graph;
use sha2::{Digest, Sha256};
use std::{fs, io::Cursor};

const BROCHURE_SHA256: &str = "ffed034ac87e679f0bd08ff9cf74ad11c0e0e510a42b1bc1a7502415f6c29c87";

#[test]
#[ignore = "requires CHAPTERA_LOGO_BROCHURE_FIXTURE exact public source"]
fn exact_public_brochure_admitted_logo_rectangle_census() {
    let fixture = std::env::var_os("CHAPTERA_LOGO_BROCHURE_FIXTURE")
        .expect("CHAPTERA_LOGO_BROCHURE_FIXTURE");
    let bytes = fs::read(fixture).expect("pinned Brochure source");
    let sha = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    assert_eq!(sha, BROCHURE_SHA256);
    let hash: Sha256Digest = BROCHURE_SHA256.parse().unwrap();

    let contents =
        pub_cfb::read_stream_reader(Cursor::new(bytes.as_slice()), "/Contents").unwrap();
    let header = parse_0x2c_header(StreamPath("/Contents".into()), &contents).unwrap();
    let trailer = parse_confirmed_0x2c_trailer_root(&contents, &header).unwrap();
    let raw_type = |seq: u32| -> Option<u16> {
        parse_confirmed_chunk_reference(&contents, &trailer.directory, seq as usize)
            .expect("confirmed source Contents reference")
            .and_then(|reference| {
                reference.raw_types.as_slice().first().filter(|_| reference.raw_types.len() == 1).map(|x| x.value)
            })
    };
    let parent = |seq: u32| -> Option<u32> {
        parse_confirmed_chunk_reference(&contents, &trailer.directory, seq as usize)
            .expect("confirmed parent Contents reference")
            .and_then(|reference| {
                reference.parent_seq_nums.as_slice().first().filter(|_| reference.parent_seq_nums.len() == 1).map(|x| x.value)
            })
    };

    let build = build_mature_0x2c_source_graph(Cursor::new(bytes), hash)
        .expect("exact public Brochure graph");
    let mut candidates = Vec::new();
    for (index, page_id) in build.graph.document.pages.iter().enumerate() {
        let page_nodes = build
            .graph
            .nodes
            .values()
            .filter(|node| node.header.parent_id == page_id.into_canonical())
            .count();
        println!("BROCHURE_LOGO_RAW_PAGE index={} nodes={page_nodes}", index + 1);
        for node in build
            .graph
            .nodes
            .values()
            .filter(|node| node.header.parent_id == page_id.into_canonical())
        {
            let seq = node.payload.contents_seq_num;
            let Some(group_seq) = parent(seq) else { continue };
            if raw_type(group_seq) != Some(0x31) || node.payload.officeart_shape_type != Some(1) {
                continue;
            }
            let Some(paint) = &node.payload.effective_paint else { continue };
            let color = paint.fill.color_rgb.as_ref().map(|value| (value.value, value.authority));
            let visible = paint.fill.visible.as_ref().map(|value| (value.value, value.authority));
            let rank = build
                .source_page_paint_orders
                .iter()
                .find(|order| order.page_id == *page_id)
                .and_then(|order| order.node_ids.iter().position(|id| *id == node.header.id));
            println!(
                "BROCHURE_LOGO_CANDIDATE seq={seq} group={group_seq} raw_parent={:?} page={} rgb={color:?} visible={visible:?} rank={rank:?}",
                parent(group_seq),
                index + 1,
            );
            candidates.push(seq);
        }
    }
    candidates.sort_unstable();
    println!("BROCHURE_LOGO_RECTANGLES_ADMITTED={candidates:?}");
}
