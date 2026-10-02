use pub_cfb::read_stream_path;
use pub_core::StreamPath;
use pub_escher::{
    inspect_dgg_default_options, inspect_sp_containers, DggDefaultOptionsObservation,
    FoptObservation, PUBLISHER_FIELD_SHAPE_ID,
};
use pub_model::{NodeId, PageId, Sha256Digest};
use pub_reader::{build_mature_0x2c_source_graph, PubEffectivePaintAuthority};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    io::Cursor,
    path::PathBuf,
};

const FILL_BOOLEANS: u16 = 0x01BF;
const FILL_USE_FILLED_BIT: u32 = 1 << 20;
const FILL_FILLED_BIT: u32 = 1 << 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LegacyScalarLayer {
    Absent,
    Value(u32),
    Unresolved,
}

fn legacy_scalar_layer(records: &[FoptObservation]) -> LegacyScalarLayer {
    let matches = records
        .iter()
        .flat_map(|record| record.properties.iter())
        .filter(|property| property.property_id() == FILL_BOOLEANS)
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [] => LegacyScalarLayer::Absent,
        [property] if !property.f_bid() && !property.f_complex() => {
            LegacyScalarLayer::Value(property.op)
        }
        _ => LegacyScalarLayer::Unresolved,
    }
}

fn legacy_fill_visibility(
    shape: &pub_escher::SpContainerObservation,
    dgg: Option<&DggDefaultOptionsObservation>,
) -> Option<bool> {
    let mut layers = vec![legacy_scalar_layer(&shape.fopts)];
    if let Some(dgg) = dgg {
        layers.push(legacy_scalar_layer(&dgg.primary_options));
        layers.push(legacy_scalar_layer(&dgg.tertiary_options));
    }

    for layer in layers {
        match layer {
            LegacyScalarLayer::Absent => {}
            LegacyScalarLayer::Unresolved => return None,
            LegacyScalarLayer::Value(raw) => {
                if raw & FILL_USE_FILLED_BIT == 0 {
                    continue;
                }
                return Some(raw & FILL_FILLED_BIT != 0);
            }
        }
    }

    Some(true)
}

fn authority_bucket(authority: PubEffectivePaintAuthority) -> &'static str {
    match authority {
        PubEffectivePaintAuthority::ShapeLocal => "shape_local",
        PubEffectivePaintAuthority::DrawingGroupPrimary => "drawing_group_primary",
        PubEffectivePaintAuthority::DrawingGroupTertiary => "drawing_group_tertiary",
        PubEffectivePaintAuthority::NormativeDefault => "normative_default",
    }
}

fn bump(map: &mut BTreeMap<String, usize>, key: impl Into<String>) {
    *map.entry(key.into()).or_default() += 1;
}

#[derive(Debug, Default, Serialize)]
struct PageReceipt {
    raw_page_ordinal: usize,
    node_count: usize,
    source_order_node_count: usize,
    complete_visible_solid_fill_count: usize,
    restored_visible_solid_fill_count: usize,
    restored_in_source_order_count: usize,
    restored_outside_source_order_count: usize,
    restored_direct_count: usize,
    restored_grouped_count: usize,
    restored_shape_join_unavailable_count: usize,
    restored_family_histogram: BTreeMap<String, usize>,
    restored_shape_type_histogram: BTreeMap<String, usize>,
    restored_visibility_authority_histogram: BTreeMap<String, usize>,
    restored_color_authority_histogram: BTreeMap<String, usize>,
}

#[derive(Debug, Serialize)]
struct Receipt {
    schema: &'static str,
    source_sha256: String,
    raw_page_count: usize,
    dgg_default_group_count: usize,
    pages: Vec<PageReceipt>,
    restored_visible_solid_fill_total: usize,
    restored_in_source_order_total: usize,
    restored_outside_source_order_total: usize,
    guardrails: Vec<&'static str>,
}

#[test]
#[ignore = "requires exact public Virginia Remplacante fixture and receipt path"]
fn exact_virginia_restored_fill_stack_probe() {
    let fixture = env::var_os("CHAPTERA_VIRGINIA_RESTORED_FILL_FIXTURE")
        .map(PathBuf::from)
        .expect("CHAPTERA_VIRGINIA_RESTORED_FILL_FIXTURE");
    let output = env::var_os("CHAPTERA_VIRGINIA_RESTORED_FILL_OUT")
        .map(PathBuf::from)
        .expect("CHAPTERA_VIRGINIA_RESTORED_FILL_OUT");
    let expected_sha = env::var("CHAPTERA_VIRGINIA_RESTORED_FILL_SHA256")
        .expect("CHAPTERA_VIRGINIA_RESTORED_FILL_SHA256");

    let bytes = fs::read(&fixture).expect("read exact public Virginia PUB");
    let actual_sha = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    assert_eq!(actual_sha, expected_sha, "exact Virginia source identity");

    let source_hash: Sha256Digest = expected_sha.parse().expect("valid source SHA-256");
    let build = build_mature_0x2c_source_graph(Cursor::new(bytes.as_slice()), source_hash)
        .expect("build mature source graph");

    let escher = read_stream_path(&fixture, "/Escher/EscherStm")
        .expect("read exact Publisher Escher stream");
    let inventory = inspect_sp_containers(StreamPath("/Escher/EscherStm".to_owned()), &escher)
        .expect("inspect OfficeArt SpContainers");
    let dgg_inventory =
        inspect_dgg_default_options(StreamPath("/Escher/EscherStm".to_owned()), &escher)
            .expect("inspect OfficeArt DGG defaults");
    assert!(
        dgg_inventory.drawing_groups.len() <= 1,
        "Stage-A fixture requires unambiguous DGG defaults"
    );
    let dgg = dgg_inventory.drawing_groups.first();

    let mut shapes_by_seq = BTreeMap::<u32, Vec<usize>>::new();
    for (index, shape) in inventory.shapes.iter().enumerate() {
        let Some(client_data) = shape.client_data.as_ref() else {
            continue;
        };
        let mut seqs = client_data
            .fields
            .iter()
            .filter(|field| field.id == PUBLISHER_FIELD_SHAPE_ID)
            .map(|field| field.value)
            .collect::<Vec<_>>();
        seqs.sort_unstable();
        seqs.dedup();
        for seq in seqs {
            shapes_by_seq.entry(seq).or_default().push(index);
        }
    }

    let source_order_by_page = build
        .source_page_paint_orders
        .iter()
        .map(|order| {
            (
                order.page_id,
                order.node_ids.iter().copied().collect::<BTreeSet<NodeId>>(),
            )
        })
        .collect::<BTreeMap<PageId, BTreeSet<NodeId>>>();

    let mut pages = Vec::new();
    for (page_index, page_id) in build.graph.document.pages.iter().copied().enumerate() {
        let page_canonical = page_id.into_canonical();
        let source_order = source_order_by_page.get(&page_id);
        let mut page = PageReceipt {
            raw_page_ordinal: page_index + 1,
            source_order_node_count: source_order.map_or(0, BTreeSet::len),
            ..PageReceipt::default()
        };

        for node in build
            .graph
            .nodes
            .values()
            .filter(|node| node.header.parent_id == page_canonical)
        {
            page.node_count += 1;

            let Some(paint) = node.payload.effective_paint.as_ref() else {
                continue;
            };
            let complete_visible_solid = paint
                .fill
                .solid
                .as_ref()
                .is_some_and(|value| value.value)
                && paint
                    .fill
                    .visible
                    .as_ref()
                    .is_some_and(|value| value.value)
                && paint.fill.color_rgb.is_some();
            if !complete_visible_solid {
                continue;
            }
            page.complete_visible_solid_fill_count += 1;

            let matches = shapes_by_seq
                .get(&node.payload.contents_seq_num)
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            let [shape_index] = matches else {
                page.restored_shape_join_unavailable_count += 1;
                continue;
            };
            let shape = &inventory.shapes[*shape_index];
            if legacy_fill_visibility(shape, dgg).is_some() {
                continue;
            }

            page.restored_visible_solid_fill_count += 1;
            if source_order.is_some_and(|order| order.contains(&node.header.id)) {
                page.restored_in_source_order_count += 1;
            } else {
                page.restored_outside_source_order_count += 1;
            }

            let grouped = node.header.source_refs.iter().any(|source| {
                source
                    .object_key
                    .as_deref()
                    .is_some_and(|key| key.starts_with("escher/group-ancestor/"))
            });
            if grouped {
                page.restored_grouped_count += 1;
            } else {
                page.restored_direct_count += 1;
            }

            let family = if node.payload.table.is_some() {
                "table"
            } else if node.payload.image_slot.is_some() {
                "image"
            } else if node.payload.story_frame.is_some() {
                "story"
            } else {
                "other_shape"
            };
            bump(&mut page.restored_family_histogram, family);
            bump(
                &mut page.restored_shape_type_histogram,
                shape
                    .fsp
                    .as_ref()
                    .map(|fsp| format!("0x{:04X}", fsp.shape_type))
                    .unwrap_or_else(|| "none".to_owned()),
            );
            if let Some(visible) = paint.fill.visible.as_ref() {
                bump(
                    &mut page.restored_visibility_authority_histogram,
                    authority_bucket(visible.authority),
                );
            }
            if let Some(color) = paint.fill.color_rgb.as_ref() {
                bump(
                    &mut page.restored_color_authority_histogram,
                    authority_bucket(color.authority),
                );
            }
        }

        pages.push(page);
    }

    let restored_visible_solid_fill_total =
        pages.iter().map(|page| page.restored_visible_solid_fill_count).sum();
    let restored_in_source_order_total =
        pages.iter().map(|page| page.restored_in_source_order_count).sum();
    let restored_outside_source_order_total = pages
        .iter()
        .map(|page| page.restored_outside_source_order_count)
        .sum();

    let receipt = Receipt {
        schema: "chaptera.virginia-restored-fill-stack-probe.v1",
        source_sha256: actual_sha,
        raw_page_count: pages.len(),
        dgg_default_group_count: dgg_inventory.drawing_groups.len(),
        pages,
        restored_visible_solid_fill_total,
        restored_in_source_order_total,
        restored_outside_source_order_total,
        guardrails: vec![
            "The legacy resolver is reproduced only inside this measurement test to classify the #614 A/B boundary.",
            "MS-ODRAW effective fill semantics are not changed by this probe.",
            "PDF/raster comparison is validation only and is not used as stack, color, or visibility authority.",
            "No source text, object ids, SPIDs, offsets, filenames, or raw bytes are emitted.",
            "Grouped/direct and source-order buckets come only from current persisted source provenance.",
        ],
    };

    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).expect("create receipt directory");
    }
    fs::write(
        &output,
        serde_json::to_vec_pretty(&receipt).expect("serialize receipt"),
    )
    .expect("write receipt");

    println!(
        "VIRGINIA_RESTORED_FILL_STACK restored={} ordered={} outside_order={} raw_pages={}",
        receipt.restored_visible_solid_fill_total,
        receipt.restored_in_source_order_total,
        receipt.restored_outside_source_order_total,
        receipt.raw_page_count
    );
}
