//! Exact real-PUB LOGO (raw 0x31) materialization and source stack acceptance.
//! Never check source PUB bytes into the repository.
use pub_contents::{
    parse_0x2c_header, parse_confirmed_0x2c_trailer_root, parse_confirmed_chunk_reference,
};
use pub_core::StreamPath;
use pub_escher::{PUBLISHER_FIELD_SHAPE_ID, SpContainerObservation, inspect_sp_containers};
use pub_model::Sha256Digest;
use pub_reader::{
    PubEffectivePaintAuthority, build_mature_0x2c_source_graph, derive_pub_node_id,
    derive_pub_page_id,
};
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

    // Raw source FOPT census for the still-omitted LOGO rectangles. This is
    // measurement only: property values do not authorize paint projection.
    // Include parent carriers and the ellipse as physical context controls.
    for seq in [385_u32, 387, 406, 408, 410, 411] {
        let shape = shape_for(seq);
        let shape_type = shape.fsp.as_ref().map(|fsp| fsp.shape_type);
        let fsp_flags = shape.fsp.as_ref().map(|fsp| fsp.flags);
        println!(
            "LOGO_RECT_SOURCE seq={seq} shape_type={shape_type:?} fsp_flags={fsp_flags:?} fopt_records={} child_anchor={}",
            shape.fopts.len(),
            shape.child_anchor.is_some()
        );
        for record in &shape.fopts {
            for property in &record.properties {
                let id = property.property_id();
                if !matches!(
                    id,
                    0x0180 | 0x0181 | 0x0182 | 0x01BF | 0x01C0 | 0x01C1 | 0x01CB | 0x01CE | 0x01FF
                ) {
                    continue;
                }
                println!(
                    "LOGO_RECT_FOPT seq={seq} record=0x{:04x} property=0x{id:04x} op=0x{:08x} bid={} complex={}",
                    record.rec_type,
                    property.op,
                    property.f_bid(),
                    property.f_complex(),
                );
            }
        }
    }

    let dgg_inventory =
        pub_escher::inspect_dgg_default_options(StreamPath("/Escher/EscherStm".into()), &escher)
            .expect("pinned DGG color default observation");
    let dgg_defaults = match dgg_inventory.drawing_groups.as_slice() {
        [] => None,
        [one] => Some(one),
        _ => panic!("ambiguous DGG default-options authority"),
    };
    let scheme_references = (0..trailer.directory.slots.len())
        .filter_map(|seq| {
            parse_confirmed_chunk_reference(&contents, &trailer.directory, seq)
                .expect("bounded Contents reference")
        })
        .filter(|reference| matches!(reference.raw_types.as_slice(), [raw] if raw.value == 0x5c))
        .collect::<Vec<_>>();
    let scheme = match scheme_references.as_slice() {
        [] => None,
        [reference] => {
            assert_eq!(reference.chunk_offsets.len(), 1);
            let chunk = pub_contents::parse_confirmed_0x2c_chunk(
                StreamPath("/Contents".into()),
                &contents,
                reference.chunk_offsets[0].value,
            )
            .expect("color scheme chunk");
            Some(
                pub_contents::parse_confirmed_mature_color_scheme(&contents, &chunk)
                    .expect("exact mature current color scheme"),
            )
        }
        _ => panic!("ambiguous current color scheme authority"),
    };
    println!(
        "LOGO_RECT_SCHEME dgg_options={} scheme_slots={}",
        dgg_inventory.drawing_groups.len(),
        scheme.as_ref().map_or(0, |scheme| scheme.slots.len())
    );
    if let Some(scheme) = &scheme {
        for ordinal in [0_usize, 7] {
            let slot = scheme
                .slots
                .get(ordinal)
                .expect("referenced scheme ordinal");
            println!(
                "LOGO_RECT_SCHEME_SLOT ordinal={ordinal} rgb={:?} explicit_rgb={}",
                slot.rgb,
                slot.rgb_source.is_some()
            );
        }
    }
    for seq in [387_u32, 408, 410, 411] {
        let paint = pub_reader::resolve_bounded_effective_officeart_paint(
            shape_for(seq),
            dgg_defaults,
            scheme.as_ref(),
            true,
        )
        .expect("2D shape effective paint");
        println!(
            "LOGO_RECT_EFFECTIVE seq={seq} fill_solid={:?} fill_visible={:?} fill_color={:?} line_visible={:?}",
            paint
                .fill
                .solid
                .as_ref()
                .map(|value| (value.value, value.authority)),
            paint
                .fill
                .visible
                .as_ref()
                .map(|value| (value.value, value.authority)),
            paint
                .fill
                .color_rgb
                .as_ref()
                .map(|value| (value.value, value.authority)),
            paint
                .line
                .visible
                .as_ref()
                .map(|value| (value.value, value.authority))
        );
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

    assert_eq!(page_ordinal + 1, 5, "physical page role must stay exact");
    let source_order = build
        .source_page_paint_orders
        .iter()
        .find(|order| order.page_id == page_id)
        .expect("exact page source paint order");
    let page_nodes = build
        .graph
        .nodes
        .values()
        .filter(|node| node.header.parent_id == page_id.into_canonical())
        .count();
    println!(
        "LOGO_0X31_PROJECTED_PAGE_NODES={page_nodes} SOURCE_PAINT_ORDER_COUNT={}",
        source_order.node_ids.len()
    );
    assert_eq!(
        page_nodes, 19,
        "12 baseline nodes, six bounded LOGO children, and source LINE 405"
    );
    assert_eq!(
        source_order.node_ids.len(),
        page_nodes,
        "PAGE LOGO children need carrier-backed paint order"
    );

    // This LINE is physically PAGE-owned, not a LOGO group descendant,
    // and its source stroke must precede the later LOGO group carrier.
    let line_id = derive_pub_node_id(&source_hash, 405).unwrap();
    let line = build
        .graph
        .nodes
        .get(&line_id)
        .expect("exact source LINE 405 must materialize");
    assert_eq!(line.header.parent_id, page_id.into_canonical());
    assert_eq!(line.payload.officeart_shape_type, Some(0x0014));
    assert_eq!(
        (
            line.header.bounds.x.get(),
            line.header.bounds.y.get(),
            line.header.bounds.width.get(),
            line.header.bounds.height.get(),
        ),
        (1_058_750, 705_474, 6_729_849, 101_600),
        "stroke envelope must preserve PAGE381 native centerline"
    );
    let line_paint = line
        .payload
        .effective_paint
        .as_ref()
        .expect("LINE 405 locally sourced stroke");
    assert!(line_paint.fill.color_rgb.is_none(), "LINE has no fill");
    assert_eq!(
        line_paint.line.color_rgb.as_ref().map(|v| v.value),
        Some([255, 0, 255])
    );
    assert_eq!(
        line_paint.line.width_emu.as_ref().map(|v| v.value),
        Some(101_600)
    );
    assert_eq!(
        line_paint.line.visible.as_ref().map(|v| v.value),
        Some(true)
    );
    for source in [
        line_paint.line.color_rgb.as_ref().unwrap().source.as_ref(),
        line_paint.line.width_emu.as_ref().unwrap().source.as_ref(),
        line_paint.line.visible.as_ref().unwrap().source.as_ref(),
    ] {
        assert!(
            source.is_some(),
            "each LINE property needs local byte authority"
        );
    }
    assert!(
        line.header.source_refs.iter().any(|source| {
            source.path.as_deref() == Some("SpContainer/ClientAnchor/source-horizontal-line-stroke")
        }),
        "horizontal stroke is a separately authorized source projection"
    );
    let source_line_rank = source_order
        .node_ids
        .iter()
        .position(|id| *id == line_id)
        .expect("direct PAGE LINE must be in exact physical paint order");
    println!("NEWSLETTER_SOURCE_LINE_405_RANK={source_line_rank}");

    // The hidden LOGO rectangle 387 stays absent. Visible 408/410 have
    // source-backed effective fill and line suppression; do not guess paint
    // for generic rectangles outside this PAGE-owned LOGO lane.
    assert!(
        !build
            .graph
            .nodes
            .contains_key(&derive_pub_node_id(&source_hash, 387).unwrap()),
        "locally hidden LOGO shape 387 must not be added"
    );
    let candidates = [
        (389u32, "image"),
        (391, "story"),
        (408, "dgg-solid-rectangle"),
        (410, "palette-solid-rectangle"),
        (411, "ellipse"),
        (412, "story"),
    ];
    let mut last_rank = None;
    for (seq, class) in candidates {
        let node_id = derive_pub_node_id(&source_hash, seq).unwrap();
        let node = build
            .graph
            .nodes
            .get(&node_id)
            .expect("bounded PAGE LOGO child must materialize");
        assert_eq!(node.header.parent_id, page_id.into_canonical());
        if seq == 389 {
            assert_eq!(
                node.payload.image_slot,
                Some(5),
                "PAGE-owned LOGO image child must retain physically proven WMF slot 5"
            );
        }
        if matches!(seq, 408 | 410) {
            let effective = node
                .payload
                .effective_paint
                .as_ref()
                .expect("bounded LOGO primitive must retain effective paint");
            let color = effective
                .fill
                .color_rgb
                .as_ref()
                .expect("LOGO rectangle requires resolved RGB");
            let (expected_rgb, authority) = if seq == 408 {
                (
                    [255, 255, 255],
                    PubEffectivePaintAuthority::DrawingGroupPrimary,
                )
            } else {
                ([0, 0, 0], PubEffectivePaintAuthority::ShapeLocal)
            };
            assert_eq!(color.value, expected_rgb);
            assert_eq!(color.authority, authority);
            assert_eq!(effective.fill.visible.as_ref().map(|v| v.value), Some(true));
            assert_eq!(
                effective.line.visible.as_ref().map(|v| v.value),
                Some(false)
            );
        }
        let rank = source_order
            .node_ids
            .iter()
            .position(|id| *id == node_id)
            .expect("PAGE LOGO child must have a source carrier rank");
        if let Some(previous) = last_rank {
            assert!(rank > previous, "serialized LOGO child order must be kept");
        }
        last_rank = Some(rank);
        println!("LOGO_0X31_PROJECTED_CANDIDATE seq={seq} class={class} rank={rank}");
    }
}
