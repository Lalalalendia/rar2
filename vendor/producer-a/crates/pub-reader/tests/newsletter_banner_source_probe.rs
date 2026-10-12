//! Read-only source census for the LO-proxy top magenta band, exact SampleNewsletter.
//! No native Microsoft Publisher pixels are available in this test; a LO stripe
//! is a hypothesis, not authorization to add a colored Reader object.
use pub_contents::{
    parse_0x2c_header, parse_confirmed_0x2c_trailer_root, parse_confirmed_chunk_reference,
};
use pub_core::StreamPath;
use pub_escher::{PUBLISHER_FIELD_SHAPE_ID, inspect_sp_containers};
use pub_model::Sha256Digest;
use pub_reader::{build_mature_0x2c_source_graph, derive_pub_node_id, derive_pub_page_id};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs, io::Cursor};

const NEWSLETTER_SHA256: &str = "6a825ba26ba35d6e885acdc62e859591ed37cb0ff7480b554b9cb362b644dfcf";
const PAGE381: u32 = 381;
const MAGENTA_RGB: [u8; 3] = [255, 0, 255];

#[test]
#[ignore = "requires exact SHA-pinned public SampleNewsletter.pub"]
fn exact_newsletter_top_band_source_attribution() {
    let source = std::env::var_os("CHAPTERA_NEWSLETTER_BAND_FIXTURE")
        .expect("CHAPTERA_NEWSLETTER_BAND_FIXTURE must be set");
    let bytes = fs::read(source).expect("read exact SampleNewsletter source");
    assert_eq!(bytes.len(), 291840, "source byte length drift");
    let digest = Sha256::digest(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    assert_eq!(digest, NEWSLETTER_SHA256, "source SHA-256 mismatch");
    let source_hash: Sha256Digest = NEWSLETTER_SHA256.parse().expect("source digest");
    let contents = pub_cfb::read_stream_reader(Cursor::new(bytes.as_slice()), "/Contents")
        .expect("source Contents");
    let escher = pub_cfb::read_stream_reader(Cursor::new(bytes.as_slice()), "/Escher/EscherStm")
        .expect("source Escher");
    let header = parse_0x2c_header(StreamPath("/Contents".into()), &contents).unwrap();
    let trailer = parse_confirmed_0x2c_trailer_root(&contents, &header).unwrap();
    let inventory = inspect_sp_containers(StreamPath("/Escher/EscherStm".into()), &escher)
        .expect("source OfficeArt inventory");

    let references = (0..trailer.directory.slots.len())
        .filter_map(|seq| {
            parse_confirmed_chunk_reference(&contents, &trailer.directory, seq)
                .expect("source Contents ref")
                .map(|reference| (seq as u32, reference))
        })
        .collect::<BTreeMap<_, _>>();
    let scheme_refs = references
        .values()
        .filter(|r| matches!(r.raw_types.as_slice(), [raw] if raw.value == 0x5c))
        .collect::<Vec<_>>();
    let scheme = match scheme_refs.as_slice() {
        [] => None,
        [reference] if reference.chunk_offsets.len() == 1 => {
            let chunk = pub_contents::parse_confirmed_0x2c_chunk(
                StreamPath("/Contents".into()),
                &contents,
                reference.chunk_offsets[0].value,
            )
            .expect("bounded source palette");
            Some(
                pub_contents::parse_confirmed_mature_color_scheme(&contents, &chunk)
                    .expect("confirmed current color scheme"),
            )
        }
        _ => panic!("ambiguous source color scheme"),
    };
    let defaults =
        pub_escher::inspect_dgg_default_options(StreamPath("/Escher/EscherStm".into()), &escher)
            .expect("source DGG");
    let dgg = match defaults.drawing_groups.as_slice() {
        [] => None,
        [one] => Some(one),
        _ => panic!("ambiguous source DGG"),
    };

    let build =
        build_mature_0x2c_source_graph(Cursor::new(bytes), source_hash).expect("real SourceGraph");
    let page_id = derive_pub_page_id(&source_hash, PAGE381).expect("page identity");
    assert!(build.graph.pages.contains_key(&page_id), "PAGE381 missing");
    let mut scanned = 0usize;
    let mut page_owned = 0usize;
    let mut direct_magenta = 0usize;
    let mut effective_magenta = 0usize;
    let mut page_magenta = 0usize;
    let mut effective_magenta_line = 0usize;
    let mut page_magenta_line = 0usize;
    let mut projected_page_nodes = 0usize;

    // Owner is derived from the Contents parent chain, not guessed from pixels.
    // Every shape with a physical Contents seq is visited, including shapes
    // excluded from Reader materialization and shapes on OTHER/master pages.
    for (ordinal, shape) in inventory.shapes.iter().enumerate() {
        let Some(data) = shape.client_data.as_ref() else {
            continue;
        };
        let seqs = data
            .fields
            .iter()
            .filter(|field| field.id == PUBLISHER_FIELD_SHAPE_ID)
            .map(|field| field.value)
            .collect::<Vec<_>>();
        let [seq] = seqs.as_slice() else { continue };
        let Some(reference) = references.get(seq) else {
            continue;
        };
        scanned += 1;

        let mut chain = vec![*seq];
        let mut cursor = *seq;
        for _ in 0..8 {
            let Some(reference) = references.get(&cursor) else {
                break;
            };
            let [parent] = reference.parent_seq_nums.as_slice() else {
                break;
            };
            if chain.contains(&parent.value) {
                panic!("Contents owner cycle")
            }
            cursor = parent.value;
            chain.push(cursor);
        }
        let owner_page381 = chain.contains(&PAGE381);
        page_owned += usize::from(owner_page381);
        let raw_type = reference.raw_types.first().map(|raw| raw.value);
        let shape_type = shape.fsp.as_ref().map(|fsp| fsp.shape_type);
        let fsp_flags = shape.fsp.as_ref().map(|fsp| fsp.flags);
        let colors = shape
            .fopts
            .iter()
            .flat_map(|r| r.properties.iter())
            .filter(|p| {
                matches!(
                    p.property_id(),
                    0x0180 | 0x0181 | 0x0182 | 0x0183 | 0x01bf | 0x01c0 | 0x01ff
                )
            })
            .map(|p| format!("{:04x}={:08x}", p.property_id(), p.op))
            .collect::<Vec<_>>();
        let local_magenta = shape
            .fopts
            .iter()
            .flat_map(|r| r.properties.iter())
            .any(|p| {
                p.property_id() == 0x0181 && !p.f_bid() && !p.f_complex() && p.op == 0x00ff00ff
            });
        let effective = pub_reader::resolve_bounded_effective_officeart_paint(
            shape,
            dgg,
            scheme.as_ref(),
            true,
        );
        let resolved_rgb = effective
            .as_ref()
            .and_then(|paint| paint.fill.color_rgb.as_ref())
            .map(|c| c.value);
        let effective_is_magenta = resolved_rgb == Some(MAGENTA_RGB);
        let line_rgb = effective
            .as_ref()
            .and_then(|paint| paint.line.color_rgb.as_ref())
            .map(|c| c.value);
        let line_visible = effective
            .as_ref()
            .and_then(|paint| paint.line.visible.as_ref())
            .map(|c| (c.value, c.authority));
        let line_width = effective
            .as_ref()
            .and_then(|paint| paint.line.width_emu.as_ref())
            .map(|c| (c.value, c.authority));
        let line_is_magenta = line_rgb == Some(MAGENTA_RGB);
        if *seq == 405 {
            // An actual PAGE381 native OfficeArt line, not a LO raster guess.
            assert!(owner_page381, "line owner must be PAGE381");
            assert_eq!(shape_type, Some(0x0014), "OfficeArt line primitive");
            assert_eq!(line_rgb, Some(MAGENTA_RGB), "palette slot2 = magenta");
            let anchor = shape.client_anchor.as_ref().expect("line F010 anchor");
            let signed = |field_id: u16| -> i64 {
                let values = anchor
                    .fields
                    .iter()
                    .filter(|f| f.id == field_id)
                    .map(|f| f.value)
                    .collect::<Vec<_>>();
                let [value] = values.as_slice() else {
                    panic!("line must have a unique anchor coordinate {field_id:04x}");
                };
                i64::from(i32::from_le_bytes(value.to_le_bytes()))
            };
            let page = build.graph.pages.get(&page_id).expect("source PAGE381");
            let half_width = page.size.width.get() / 2;
            let half_height = page.size.height.get() / 2;
            let x0 = half_width + signed(pub_escher::PUBLISHER_FIELD_XS);
            let x1 = half_width + signed(pub_escher::PUBLISHER_FIELD_XE);
            let y0 = half_height + signed(pub_escher::PUBLISHER_FIELD_YS);
            let y1 = half_height + signed(pub_escher::PUBLISHER_FIELD_YE);
            assert_eq!(y0, y1, "physical line is horizontal");
            assert!(x1 > x0, "positive source stroke span");
            println!(
                "NEWSLETTER_BAND_SOURCE_LINE seq=405 page_width_emu={} page_height_emu={} x0_emu={x0} x1_emu={x1} y_emu={y0} effective_line_rgb={line_rgb:?} line_visible={line_visible:?} line_width_emu={line_width:?} native_publisher_stroke_verified=false",
                page.size.width.get(),
                page.size.height.get(),
            );
            assert_eq!((x0, x1, y0), (1058750, 7788599, 756274));
        }
        direct_magenta += usize::from(local_magenta);
        effective_magenta += usize::from(effective_is_magenta);
        page_magenta += usize::from(owner_page381 && (local_magenta || effective_is_magenta));
        effective_magenta_line += usize::from(line_is_magenta);
        page_magenta_line += usize::from(owner_page381 && line_is_magenta);
        let node_id = derive_pub_node_id(&source_hash, *seq).unwrap();
        let projected = build
            .graph
            .nodes
            .get(&node_id)
            .filter(|n| n.header.parent_id == page_id.into_canonical());
        projected_page_nodes += usize::from(projected.is_some());

        // Print page-owned shapes and any source magenta candidate anywhere.
        // Do not leak PUB bytes, Story text, or external PDF data.
        if owner_page381 || local_magenta || effective_is_magenta || line_is_magenta {
            let ca = shape.client_anchor.as_ref().map(|anchor| {
                anchor
                    .fields
                    .iter()
                    .filter(|f| matches!(f.id, 0x2001..=0x2004))
                    .map(|f| format!("{:04x}={:08x}", f.id, f.value))
                    .collect::<Vec<_>>()
            });
            let child = shape
                .child_anchor
                .as_ref()
                .map(|a| [a.x_left, a.y_top, a.x_right, a.y_bottom]);
            let bounds = projected.map(|n| format!("{:?}", n.header.bounds));
            println!(
                "NEWSLETTER_BAND_CANDIDATE seq={} escher_rank={ordinal} raw_type={raw_type:?} owner_chain={chain:?} on_page381={} shape_type={shape_type:?} fsp_flags={fsp_flags:?} local_fopt={colors:?} local_magenta={} effective_fill_rgb={resolved_rgb:?} effective_magenta={} effective_line_rgb={line_rgb:?} line_magenta={} line_visible={line_visible:?} line_width_emu={line_width:?} child_anchor={child:?} client_anchor={ca:?} projected_bounds={bounds:?}",
                seq, owner_page381, local_magenta, effective_is_magenta, line_is_magenta,
            );
        }
    }

    assert!(scanned > 0, "no physical OfficeArt shape identities");
    assert!(page_owned > 0, "no source shape ownership for PAGE381");
    assert!(projected_page_nodes > 0, "no Reader geometry for PAGE381");
    println!(
        "NEWSLETTER_BAND_SOURCE_DISPOSITION page381={PAGE381} scanned={scanned} owned={page_owned} reader_nodes={projected_page_nodes} magenta_fill_local_all={direct_magenta} magenta_fill_effective_all={effective_magenta} magenta_fill_owned_page381={page_magenta} magenta_line_effective_all={effective_magenta_line} magenta_line_owned_page381={page_magenta_line} native_publisher_stripe_proven=false"
    );
    // No positive assertion about the stripe: LO is a proxy and may have
    // painted effects that the native Publisher does not.
}
