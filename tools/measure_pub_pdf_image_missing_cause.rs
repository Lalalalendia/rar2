use anyhow::{Context, Result, bail};
use pub_model::{NodeId, NodeKind, Sha256Digest};
use pub_reader::{
    PubAssetManifestDiagnostic, PubImageResourceDiagnostic, build_mature_0x2c_source_graph,
    build_pub_asset_manifest, build_pub_image_resource_catalog,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::io::Cursor;
use std::path::PathBuf;

const SCHEMA: &str = "chaptera.pub-pdf-image-missing-cause.v1";
const ESCHER_STREAM: &str = "/Escher/EscherStm";
const ESCHER_DELAY_STREAM: &str = "/Escher/EscherDelayStm";

fn source_hash(bytes: &[u8]) -> Sha256Digest {
    let digest = Sha256::digest(bytes);
    let mut raw = [0_u8; 32];
    raw.copy_from_slice(&digest);
    Sha256Digest::from_bytes(raw)
}

fn bump(map: &mut BTreeMap<String, usize>, key: impl Into<String>) {
    *map.entry(key.into()).or_default() += 1;
}

fn node_kind_name(kind: NodeKind) -> &'static str {
    match kind {
        NodeKind::Shape => "shape",
        NodeKind::TextFrame => "text_frame",
        NodeKind::ImageFrame => "image_frame",
        NodeKind::VectorPath => "vector_path",
        NodeKind::Group => "group",
        NodeKind::Connector => "connector",
        NodeKind::Table => "table",
        NodeKind::PlacedArtifact => "placed_artifact",
        NodeKind::Unsupported => "unsupported",
    }
}

fn manifest_diagnostic_code(diagnostic: &PubAssetManifestDiagnostic) -> (&'static str, u32) {
    match diagnostic {
        PubAssetManifestDiagnostic::MissingBStoreSlot { slot } => ("missing_bstore_slot", *slot),
        PubAssetManifestDiagnostic::EmptyBStoreSlot { slot } => ("empty_bstore_slot", *slot),
        PubAssetManifestDiagnostic::DelayedBlipUnresolved { slot, .. } => {
            ("delayed_blip_unresolved", *slot)
        }
        PubAssetManifestDiagnostic::EmbeddedBlipNotExtracted { slot } => {
            ("embedded_blip_not_extracted", *slot)
        }
        PubAssetManifestDiagnostic::StandardPayloadUnavailable { slot, .. } => {
            ("standard_payload_unavailable", *slot)
        }
    }
}

fn main() -> Result<()> {
    let mut args = env::args_os().skip(1);
    let source_path = PathBuf::from(
        args.next()
            .context("usage: image-missing-cause SOURCE.pub LOSS.json OUTPUT.json")?,
    );
    let loss_path = PathBuf::from(
        args.next()
            .context("usage: image-missing-cause SOURCE.pub LOSS.json OUTPUT.json")?,
    );
    let output_path = PathBuf::from(
        args.next()
            .context("usage: image-missing-cause SOURCE.pub LOSS.json OUTPUT.json")?,
    );
    if args.next().is_some() {
        bail!("image-missing-cause accepts exactly SOURCE.pub LOSS.json OUTPUT.json");
    }

    let bytes = fs::read(&source_path).with_context(|| format!("read {}", source_path.display()))?;
    let hash = source_hash(&bytes);
    let loss: Value = serde_json::from_slice(
        &fs::read(&loss_path).with_context(|| format!("read {}", loss_path.display()))?,
    )
    .context("parse fixed-PDF loss report")?;
    let loss_hash = loss
        .pointer("/conversion_profile/source/source_sha256")
        .and_then(Value::as_str)
        .context("loss report lacks source SHA-256")?;
    if loss_hash != hash.to_string() {
        bail!("loss report/source identity mismatch");
    }

    let source = build_mature_0x2c_source_graph(Cursor::new(bytes.as_slice()), hash)
        .context("build mature source graph for image-missing census")?;

    let escher = pub_cfb::read_stream_reader(Cursor::new(bytes.as_slice()), ESCHER_STREAM)
        .context("read Escher stream")?;
    let inventory =
        pub_cfb::inspect_reader(Cursor::new(bytes.as_slice())).context("inspect source CFB")?;
    let delayed = if inventory
        .entries
        .iter()
        .any(|entry| entry.path == ESCHER_DELAY_STREAM)
    {
        pub_cfb::read_stream_reader(Cursor::new(bytes.as_slice()), ESCHER_DELAY_STREAM)
            .context("read EscherDelay stream")?
    } else {
        Vec::new()
    };

    let manifest =
        build_pub_asset_manifest(&source.graph, &escher, &delayed).context("build asset manifest")?;
    let catalog = build_pub_image_resource_catalog(&source.graph, &manifest)
        .context("build exact image resource catalog")?;

    let mut manifest_diagnostics = BTreeMap::<u32, Vec<&'static str>>::new();
    for diagnostic in &manifest.diagnostics {
        let (code, slot) = manifest_diagnostic_code(diagnostic);
        manifest_diagnostics.entry(slot).or_default().push(code);
    }
    let mut catalog_nonexact_slots = BTreeSet::<u32>::new();
    for diagnostic in &catalog.diagnostics {
        match diagnostic {
            PubImageResourceDiagnostic::AssetPayloadNotExact { slot } => {
                catalog_nonexact_slots.insert(*slot);
            }
        }
    }
    let manifest_slots = manifest
        .assets
        .iter()
        .map(|asset| asset.slot)
        .collect::<BTreeSet<_>>();

    let report_nodes = loss
        .pointer("/pdf/nodes")
        .and_then(Value::as_array)
        .context("loss report lacks pdf.nodes")?;

    let mut missing = Vec::<NodeId>::new();
    for row in report_nodes {
        if row.get("code").and_then(Value::as_str) != Some("pdf.node.resource_missing") {
            continue;
        }
        missing.push(
            serde_json::from_value(
                row.get("origin")
                    .cloned()
                    .context("resource-missing node lacks origin")?,
            )
            .context("decode missing node identity")?,
        );
    }
    missing.sort();
    missing.dedup();

    let mut image_missing_count = 0usize;
    let mut cause_counts = BTreeMap::<String, usize>::new();
    let mut parent_topology_counts = BTreeMap::<String, usize>::new();
    let mut node_kind_counts = BTreeMap::<String, usize>::new();
    let mut feature_counts = BTreeMap::<String, usize>::new();
    let mut unique_slots = BTreeSet::<u32>::new();
    let mut graph_lookup_missing = 0usize;

    for node_id in missing {
        let Some(node) = source.graph.nodes.get(&node_id) else {
            graph_lookup_missing += 1;
            continue;
        };
        let Some(slot) = node.payload.image_slot else {
            continue;
        };
        image_missing_count += 1;
        unique_slots.insert(slot);
        bump(&mut node_kind_counts, node_kind_name(node.kind));
        if node.payload.legacy_ole.is_some() {
            bump(&mut feature_counts, "legacy_ole");
        }

        let direct_page_parent = source
            .graph
            .pages
            .keys()
            .any(|page_id| page_id.into_canonical() == node.header.parent_id);
        let parent_kind = if direct_page_parent {
            "direct_page".to_owned()
        } else {
            source
                .graph
                .nodes
                .get(&NodeId::from_canonical(node.header.parent_id))
                .map(|parent| format!("node_{}", node_kind_name(parent.kind)))
                .unwrap_or_else(|| "unresolved_parent".to_owned())
        };
        bump(&mut parent_topology_counts, parent_kind.clone());

        if !direct_page_parent {
            bump(
                &mut cause_counts,
                format!("non_page_parent_{parent_kind}_outside_exact_asset_use_index"),
            );
            continue;
        }

        if let Some(codes) = manifest_diagnostics.get(&slot) {
            if codes.is_empty() {
                bump(&mut cause_counts, "manifest_diagnostic_empty");
            } else {
                for code in codes {
                    bump(&mut cause_counts, *code);
                }
            }
            continue;
        }

        if !manifest_slots.contains(&slot) {
            bump(&mut cause_counts, "asset_manifest_slot_missing_without_diagnostic");
            continue;
        }

        if catalog_nonexact_slots.contains(&slot) {
            bump(&mut cause_counts, "asset_payload_not_exact");
            continue;
        }

        if catalog.node_resources.contains_key(&node_id) {
            bump(
                &mut cause_counts,
                "exact_catalog_resource_present_but_viewer_resource_missing",
            );
        } else {
            bump(
                &mut cause_counts,
                "exact_manifest_slot_but_node_resource_mapping_missing",
            );
        }
    }

    let receipt = json!({
        "schema": SCHEMA,
        "source_sha256": hash.to_string(),
        "pdf_resource_missing_node_count": report_nodes.iter().filter(|row| {
            row.get("code").and_then(Value::as_str) == Some("pdf.node.resource_missing")
        }).count(),
        "image_missing_node_count": image_missing_count,
        "unique_image_slot_count": unique_slots.len(),
        "cause_counts": cause_counts,
        "parent_topology_counts": parent_topology_counts,
        "node_kind_counts": node_kind_counts,
        "feature_counts": feature_counts,
        "resolved_graph_lookup_missing_count": graph_lookup_missing,
        "asset_manifest": {
            "slot_count": manifest.assets.len(),
            "diagnostic_count": manifest.diagnostics.len(),
            "exact_catalog_resource_count": catalog.resources.len(),
            "exact_catalog_use_count": catalog.node_resources.len(),
            "catalog_diagnostic_count": catalog.diagnostics.len(),
        },
        "claims": {
            "raw_node_ids_emitted": false,
            "raw_slot_ids_emitted": false,
            "source_offsets_emitted": false,
            "source_text_emitted": false,
            "image_bytes_emitted": false,
            "derived_preview_promoted_to_exact": false,
        }
    });

    fs::write(
        &output_path,
        serde_json::to_vec_pretty(&receipt).context("serialize image-missing cause receipt")?,
    )
    .with_context(|| format!("write {}", output_path.display()))?;
    Ok(())
}
