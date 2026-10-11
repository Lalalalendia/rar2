//! Measurement-only #2616: exact real-PUB LOGO (raw 0x31) ownership and
//! pre-fix SourceGraph omission. Never check in source PUB bytes.
use pub_contents::{
    parse_0x2c_header, parse_confirmed_0x2c_trailer_root, parse_confirmed_chunk_reference,
};
use pub_core::StreamPath;
use pub_escher::{PUBLISHER_FIELD_SHAPE_ID, SpContainerObservation, inspect_sp_containers};
use pub_model::Sha256Digest;
use pub_reader::{build_mature_0x2c_source_graph, derive_pub_node_id, derive_pub_page_id};
use sha2::{Digest, Sha256};
use std::{fs, io::Cursor};

const NEWSLETTER_SHA256: &str = "6a825ba26ba35d6e885acdc62e859591ed37cb0ff7480b554b9cb362b644dfcf";

#[test]
#[ignore = "requires CHAPTERA_LOGO_GROUP_FIXTURE (SHA-pinned public Apache POI source)"]
fn exact_public_newsletter_logo_0x31_baseline() {
    let fixture = std::env::var_os("CHAPTERA_LOGO_GROUP_FIXTURE")
        .expect("CHAPTERA_LOGO_GROUP_FIXTURE is required");
    let bytes = fs::read(fixture).expect("read pinned public SampleNewsletter.pub");
    let calculated_sha256 = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    assert_eq!(
        calculated_sha256, NEWSLETTER_SHA256,
        "the source must match the exact verified public PUB"
    );
    let source_hash: Sha256Digest = NEWSLETTER_SHA256.parse().unwrap();

    let contents = pub_cfb::read_stream_reader(Cursor::new(bytes.as_slice()), "/Contents").unwrap();
    let escher =
        pub_cfb::read_stream_reader(Cursor::new(bytes.as_slice()), "/Escher/EscherStm").unwrap();
    let header = parse_0x2c_header(StreamPath("/Contents".into()), &contents).unwrap();
    let trailer = parse_confirmed_0x2c_trailer_root(&contents, &header).unwrap();
    let inventory = inspect_sp_containers(StreamPath("/Escher/EscherStm".into()), &escher)
        .expect("parse pinned OfficeArt SpContainers");

    let reference_for = |seq: u32| {
        parse_confirmed_chunk_reference(&contents, &trailer.directory, seq as usize)
            .expect("parse Contents directory reference")
            .expect("expected Contents directory entry")
    };
    let shape_for = |seq: u32| -> &SpContainerObservation {
        let shapes = inventory
            .shapes
            .iter()
            .filter(|shape| {
                shape.client_data.as_ref().is_some_and(|data| {
                    data.fields
                        .iter()
                        .any(|field| field.id == PUBLISHER_FIELD_SHAPE_ID && field.value == seq)
                })
            })
            .collect::<Vec<_>>();
        assert_eq!(shapes.len(), 1, "unique OfficeArt F011 identity for {seq}");
        shapes[0]
    };

    // Reconfirm the exact Contents ownership and physical OfficeArt ancestry,
    // independent of whether the Reader chooses to materialize these shapes.
    for (carrier_seq, children) in [
        (385u32, vec![387u32, 389, 391]),
        (406u32, vec![408u32, 410, 411, 412]),
    ] {
        let parent = reference_for(carrier_seq);
        assert_eq!(parent.raw_types.len(), 1);
        assert_eq!(parent.raw_types[0].value, 0x31, "LOGO must be raw 0x31");
        assert_eq!(parent.parent_seq_nums.len(), 1);
        assert_eq!(parent.parent_seq_nums[0].value, 381, "PAGE-owned LOGO");

        let group_shape = shape_for(carrier_seq);
        assert!(group_shape.fspgr.is_some(), "LOGO carrier has FSPGR");
        assert!(
            group_shape.client_anchor.is_some(),
            "LOGO has page ClientAnchor"
        );
        for child_seq in children {
            let child = reference_for(child_seq);
            assert_eq!(child.parent_seq_nums.len(), 1);
            assert_eq!(
                child.parent_seq_nums[0].value, carrier_seq,
                "Contents nested child must match LOGO parent"
            );
            let shape = shape_for(child_seq);
            assert_eq!(
                shape.parent_group_shape_source.as_ref(),
                Some(&group_shape.source),
                "OfficeArt F003 ancestry must match Contents"
            );
            let anchor = shape.child_anchor.as_ref().expect("bounded child F00F");
            assert!(anchor.x_right > anchor.x_left);
            assert!(anchor.y_bottom > anchor.y_top);
        }
    }

    let build = build_mature_0x2c_source_graph(Cursor::new(bytes), source_hash)
        .expect("real mature-0x2c Reader SourceGraph");
    let page_id = derive_pub_page_id(&source_hash, 381).unwrap();
    let page_ordinal = build
        .graph
        .document
        .pages
        .iter()
        .position(|id| *id == page_id)
        .expect("PAGE seq381 must be in actual DOCUMENT PageList");
    println!("LOGO_0X31_PAGE_ORDINAL_ONE_BASED={}", page_ordinal + 1);

    let source_order = build
        .source_page_paint_orders
        .iter()
        .find(|order| order.page_id == page_id);
    println!(
        "LOGO_0X31_BASELINE_PAGE_NODES={} SOURCE_PAINT_ORDER_COUNT={:?}",
        build
            .graph
            .nodes
            .values()
            .filter(|node| node.header.parent_id == page_id.into_canonical())
            .count(),
        source_order.map(|order| order.node_ids.len())
    );

    // Four class-eligible descendants from #2616. This is a pre-fix
    // observation, not an assertion that all seven children should render.
    let candidates = [
        (389u32, "image"),
        (391, "story"),
        (411, "ellipse"),
        (412, "story"),
    ];
    let mut materialized = 0usize;
    for (seq, class) in candidates {
        let node_id = derive_pub_node_id(&source_hash, seq).unwrap();
        let node = build.graph.nodes.get(&node_id);
        if node.is_some() {
            materialized += 1;
        }
        println!(
            "LOGO_0X31_BASELINE_CANDIDATE seq={seq} class={class} materialized={}",
            node.is_some()
        );
    }
    assert_eq!(
        materialized, 0,
        "baseline changed: reconcile exact measurements before enabling LOGO projection"
    );
}
