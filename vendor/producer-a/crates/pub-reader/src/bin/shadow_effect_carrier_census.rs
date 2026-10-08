use anyhow::{Context, Result};
use pub_cfb::read_stream_path;
use pub_core::StreamPath;
use pub_escher::{
    Fopte, PUBLISHER_FIELD_SHAPE_ID, inspect_dgg_default_options, inspect_sp_containers,
};
use pub_model::{NodeId, PageId, Sha256Digest};
use pub_reader::{PubSourceGraph, build_mature_0x2c_source_graph, derive_pub_node_id};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    io::Cursor,
    path::PathBuf,
};

const SHADOW_FAMILY_START: u16 = 0x0200;
const SHADOW_FAMILY_END: u16 = 0x023f;
const NATIVE_SHADOW_CANDIDATE_02BF: u16 = 0x02bf;

#[derive(Debug, Default, Serialize)]
struct CandidateCounts {
    effective_page_count: usize,
    effective_page_node_count: usize,
    escher_shape_count: usize,
    joined_shape_count: usize,
    effective_page_joined_shape_count: usize,
    effective_page_shape_with_candidate_count: usize,
    effective_page_candidate_observation_count: usize,
    dgg_candidate_observation_count: usize,
}

#[derive(Debug, Default, Serialize)]
struct CandidateHistograms {
    property_id_hex: BTreeMap<String, usize>,
    record_layer: BTreeMap<String, usize>,
    property_record_layer: BTreeMap<String, usize>,
    node_kind: BTreeMap<String, usize>,
    property_node_kind: BTreeMap<String, usize>,
    property_form: BTreeMap<String, usize>,
    scalar_raw_by_property: BTreeMap<String, usize>,
    dgg_property_id_hex: BTreeMap<String, usize>,
    dgg_property_record_layer: BTreeMap<String, usize>,
    dgg_property_form: BTreeMap<String, usize>,
    dgg_scalar_raw_by_property: BTreeMap<String, usize>,
}

#[derive(Debug, Serialize)]
struct Receipt {
    schema: &'static str,
    source_sha256: String,
    source_bytes: usize,
    counts: CandidateCounts,
    histograms: CandidateHistograms,
    claims: BTreeMap<&'static str, bool>,
}

fn bump(map: &mut BTreeMap<String, usize>, key: impl Into<String>) {
    *map.entry(key.into()).or_default() += 1;
}

fn is_candidate(property_id: u16) -> bool {
    (SHADOW_FAMILY_START..=SHADOW_FAMILY_END).contains(&property_id)
        || property_id == NATIVE_SHADOW_CANDIDATE_02BF
}

fn property_form(entry: &Fopte) -> &'static str {
    match (entry.f_bid(), entry.f_complex()) {
        (false, false) => "scalar",
        (true, false) => "bid",
        (false, true) => "complex",
        (true, true) => "bid_complex",
    }
}

fn record_layer(rec_type: u16) -> String {
    match rec_type {
        pub_escher::OFFICE_ART_FOPT => "primary".to_owned(),
        pub_escher::OFFICE_ART_TERTIARY_FOPT => "tertiary".to_owned(),
        other => format!("other_0x{other:04X}"),
    }
}

fn resolve_node_page(graph: &PubSourceGraph, node_id: NodeId) -> Option<PageId> {
    let mut parent = graph.nodes.get(&node_id)?.header.parent_id;
    for _ in 0..64 {
        if let Some((page_id, _)) = graph
            .pages
            .iter()
            .find(|(page_id, _)| page_id.as_canonical() == &parent)
        {
            return Some(*page_id);
        }
        let Some((_, node)) = graph
            .nodes
            .iter()
            .find(|(candidate_id, _)| candidate_id.as_canonical() == &parent)
        else {
            return None;
        };
        parent = node.header.parent_id;
    }
    None
}

fn source_hash(bytes: &[u8]) -> Sha256Digest {
    let digest = Sha256::digest(bytes);
    let mut raw = [0_u8; 32];
    raw.copy_from_slice(&digest);
    Sha256Digest::from_bytes(raw)
}

fn observe_candidate(
    histograms: &mut CandidateHistograms,
    entry: &Fopte,
    rec_type: u16,
    node_kind: Option<String>,
    dgg: bool,
) {
    let property_id = entry.property_id();
    let property = format!("0x{property_id:04X}");
    let layer = record_layer(rec_type);
    let form = property_form(entry);

    if dgg {
        bump(&mut histograms.dgg_property_id_hex, property.clone());
        bump(
            &mut histograms.dgg_property_record_layer,
            format!("{property}|{layer}"),
        );
        bump(
            &mut histograms.dgg_property_form,
            format!("{property}|{form}"),
        );
        if form == "scalar" {
            bump(
                &mut histograms.dgg_scalar_raw_by_property,
                format!("{property}|0x{:08X}", entry.op),
            );
        }
        return;
    }

    bump(&mut histograms.property_id_hex, property.clone());
    bump(&mut histograms.record_layer, layer.clone());
    bump(
        &mut histograms.property_record_layer,
        format!("{property}|{layer}"),
    );
    bump(&mut histograms.property_form, format!("{property}|{form}"));
    if let Some(node_kind) = node_kind {
        bump(&mut histograms.node_kind, node_kind.clone());
        bump(
            &mut histograms.property_node_kind,
            format!("{property}|{node_kind}"),
        );
    }
    if form == "scalar" {
        bump(
            &mut histograms.scalar_raw_by_property,
            format!("{property}|0x{:08X}", entry.op),
        );
    }
}

fn main() -> Result<()> {
    let mut args = env::args_os().skip(1);
    let source_path = PathBuf::from(
        args.next()
            .context("usage: shadow_effect_carrier_census SOURCE.pub LABEL")?,
    );
    let label = args
        .next()
        .context("usage: shadow_effect_carrier_census SOURCE.pub LABEL")?
        .to_string_lossy()
        .into_owned();
    if args.next().is_some() {
        anyhow::bail!("unexpected extra arguments");
    }

    let bytes =
        fs::read(&source_path).with_context(|| format!("read {}", source_path.display()))?;
    let hash = source_hash(&bytes);
    let source = build_mature_0x2c_source_graph(Cursor::new(bytes.as_slice()), hash.clone())
        .context("build mature SourceGraph")?;
    let effective_pages = source
        .effective_pages
        .page_ids
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();

    let escher = read_stream_path(&source_path, "/Escher/EscherStm")
        .context("read Publisher Escher stream")?;
    let shapes = inspect_sp_containers(StreamPath("/Escher/EscherStm".to_owned()), &escher)
        .context("inspect OfficeArt shape containers")?;
    let dgg = inspect_dgg_default_options(StreamPath("/Escher/EscherStm".to_owned()), &escher)
        .context("inspect OfficeArt DGG defaults")?;

    let mut counts = CandidateCounts {
        effective_page_count: effective_pages.len(),
        effective_page_node_count: source
            .graph
            .nodes
            .keys()
            .filter_map(|node_id| resolve_node_page(&source.graph, *node_id))
            .filter(|page_id| effective_pages.contains(page_id))
            .count(),
        escher_shape_count: shapes.shapes.len(),
        ..CandidateCounts::default()
    };
    let mut histograms = CandidateHistograms::default();

    for shape in &shapes.shapes {
        let seqs = shape
            .client_data
            .as_ref()
            .map(|record| record.values(PUBLISHER_FIELD_SHAPE_ID).collect::<Vec<_>>())
            .unwrap_or_default();
        if seqs.len() != 1 {
            continue;
        }
        let node_id = derive_pub_node_id(&hash, seqs[0])?;
        let Some(node) = source.graph.nodes.get(&node_id) else {
            continue;
        };
        counts.joined_shape_count += 1;
        let Some(page_id) = resolve_node_page(&source.graph, node_id) else {
            continue;
        };
        if !effective_pages.contains(&page_id) {
            continue;
        }
        counts.effective_page_joined_shape_count += 1;

        let mut shape_candidate_count = 0_usize;
        let node_kind = format!("{:?}", node.kind);
        for fopt in &shape.fopts {
            for entry in &fopt.properties {
                if !is_candidate(entry.property_id()) {
                    continue;
                }
                shape_candidate_count += 1;
                counts.effective_page_candidate_observation_count += 1;
                observe_candidate(
                    &mut histograms,
                    entry,
                    fopt.rec_type,
                    Some(node_kind.clone()),
                    false,
                );
            }
        }
        if shape_candidate_count > 0 {
            counts.effective_page_shape_with_candidate_count += 1;
        }
    }

    for drawing_group in &dgg.drawing_groups {
        for fopt in drawing_group
            .primary_options
            .iter()
            .chain(drawing_group.tertiary_options.iter())
        {
            for entry in &fopt.properties {
                if !is_candidate(entry.property_id()) {
                    continue;
                }
                counts.dgg_candidate_observation_count += 1;
                observe_candidate(&mut histograms, entry, fopt.rec_type, None, true);
            }
        }
    }

    let claims = BTreeMap::from([
        ("aggregate_only", true),
        ("effective_pages_only_for_shape_local_counts", true),
        ("raw_object_ids_emitted", false),
        ("raw_page_ids_emitted", false),
        ("raw_coordinates_emitted", false),
        ("story_text_emitted", false),
        ("candidate_ids_are_not_semantic_authority", true),
        ("product_semantics_changed", false),
    ]);
    let receipt = Receipt {
        schema: "chaptera.shadow-effect-carrier-census.v1",
        source_sha256: hash.to_string(),
        source_bytes: bytes.len(),
        counts,
        histograms,
        claims,
    };
    println!(
        "SHADOW_EFFECT_CARRIER_CENSUS label={} {}",
        label,
        serde_json::to_string(&receipt)?
    );
    Ok(())
}
