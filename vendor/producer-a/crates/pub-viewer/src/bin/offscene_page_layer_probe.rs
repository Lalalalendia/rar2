#[cfg(feature = "cmo-slot-compose")]
use anyhow::{Context, Result};
#[cfg(feature = "cmo-slot-compose")]
use pub_model::Sha256Digest;
#[cfg(feature = "cmo-slot-compose")]
use pub_reader::{
    analyze_mature_0x2c_page_roles, build_mature_0x2c_master_projection_bridge_v1,
    build_mature_0x2c_source_graph,
};
#[cfg(feature = "cmo-slot-compose")]
use pub_viewer::{open_mature_0x2c_geometry, viewer_geometry_environment_v0_1};
#[cfg(feature = "cmo-slot-compose")]
use serde_json::json;
#[cfg(feature = "cmo-slot-compose")]
use sha2::{Digest, Sha256};
#[cfg(feature = "cmo-slot-compose")]
use std::{collections::BTreeMap, env, fs, io::Cursor};

#[cfg(feature = "cmo-slot-compose")]
fn source_hash(bytes: &[u8]) -> Sha256Digest {
    let digest = Sha256::digest(bytes);
    let mut raw = [0_u8; 32];
    raw.copy_from_slice(&digest);
    Sha256Digest::from_bytes(raw)
}

#[cfg(feature = "cmo-slot-compose")]
fn main() -> Result<()> {
    let input = env::args()
        .nth(1)
        .context("usage: offscene_page_layer_probe INPUT.pub LABEL")?;
    let label = env::args()
        .nth(2)
        .context("usage: offscene_page_layer_probe INPUT.pub LABEL")?;

    let bytes = fs::read(&input).with_context(|| format!("read {input}"))?;
    let hash = source_hash(&bytes);

    let roles = analyze_mature_0x2c_page_roles(Cursor::new(bytes.as_slice()))
        .context("analyze mature PAGE roles")?;
    let source = build_mature_0x2c_source_graph(Cursor::new(bytes.as_slice()), hash.clone())
        .context("build mature SourceGraph")?;
    let master_bridge =
        build_mature_0x2c_master_projection_bridge_v1(&bytes, hash.clone(), &source.graph)
            .context("build master projection bridge")?;
    let visual = open_mature_0x2c_geometry(&bytes, viewer_geometry_environment_v0_1())
        .context("open exact source through current Viewer")?;

    let mut projection_kind_counts = BTreeMap::<String, usize>::new();
    for instance in &visual.projected_instances {
        *projection_kind_counts
            .entry(format!("{:?}", instance.scene_instance.projection_kind))
            .or_default() += 1;
    }

    let master_diagnostic_codes = visual
        .document
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code.starts_with("viewer.master."))
        .map(|diagnostic| diagnostic.code.clone())
        .collect::<Vec<_>>();

    let page_with_applied_master_count = roles
        .pages
        .iter()
        .filter(|page| page.applied_master_seq_num.is_some())
        .count();
    let applied_master_target_page_count = roles
        .pages
        .iter()
        .filter(|page| {
            page.applied_master_seq_num.is_some()
                && page.applied_master_raw_type == Some(0x43)
                && page.applied_master_field_id == Some(0x0d)
                && page.applied_master_block_type == Some(0x68)
        })
        .count();

    let receipt = json!({
        "schema": "chaptera.offscene-page-layer-census.v1",
        "label": label,
        "source_sha256": hash.to_string(),
        "source_bytes": bytes.len(),
        "raw_page_count": roles.confirmed_page_count,
        "page_with_applied_master_count": page_with_applied_master_count,
        "exact_page_to_page_master_count": applied_master_target_page_count,
        "master_relation_count": master_bridge.output.receipt.relation_count,
        "master_bridge_identity_parity": master_bridge.active_graph_identity_parity,
        "viewer_page_count": visual.document.pages.len(),
        "scene_surface_count": visual.scene.surfaces.len(),
        "scene_node_count": visual.scene.nodes.len(),
        "projected_instance_count": visual.projected_instances.len(),
        "projection_kind_counts": projection_kind_counts,
        "master_diagnostic_codes": master_diagnostic_codes,
        "claims": {
            "story_text_emitted": false,
            "raw_page_ids_emitted": false,
            "raw_contents_seq_nums_emitted": false,
            "source_bytes_emitted": false,
            "product_semantics_changed": false
        }
    });

    println!(
        "OFFSCENE_PAGE_LAYER_CENSUS {}",
        serde_json::to_string(&receipt)?
    );
    Ok(())
}

#[cfg(not(feature = "cmo-slot-compose"))]
fn main() {
    eprintln!("offscene_page_layer_probe requires --features cmo-slot-compose");
    std::process::exit(2);
}
