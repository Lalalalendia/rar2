use anyhow::{Context, Result};
use pub_model::{
    NodeId, ResourceId, Sha256Digest, SourceDerivedIdInput, derive_source_canonical_id,
};
use pub_reader::{
    LEGACY_OLE_WMF_PREVIEW_RASTERIZER_V1, PubBorderArtSlotV1, PubResolvedGraph,
    rasterize_wmf_preview, read_mature_0x2c_borderart_assets_from_pub_bytes_v1,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

use crate::{ViewerDiagnostic, ViewerDiagnosticSeverity, encode_wmf_preview_png};

const BORDERART_PREVIEW_SIDE_PX: u32 = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewerDecorativeBorderSlotV1 {
    TopLeft,
    Top,
    TopRight,
    Right,
    BottomRight,
    Bottom,
    BottomLeft,
    Left,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewerDecorativeBorderResourceV1 {
    pub resource_id: ResourceId,
    pub mime: String,
    pub source_wmf_sha256: String,
    #[serde(skip)]
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewerDecorativeBorderSlotRefV1 {
    pub slot: ViewerDecorativeBorderSlotV1,
    pub resource_id: ResourceId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewerDecorativeBorderV1 {
    pub node_id: NodeId,
    pub name: String,
    pub corner_extent_emu: u32,
    pub horizontal_extent_emu: u32,
    pub vertical_extent_emu: u32,
    pub resources: Vec<ViewerDecorativeBorderResourceV1>,
    pub slots: Vec<ViewerDecorativeBorderSlotRefV1>,
}

fn viewer_slot(slot: PubBorderArtSlotV1) -> ViewerDecorativeBorderSlotV1 {
    match slot {
        PubBorderArtSlotV1::TopLeft => ViewerDecorativeBorderSlotV1::TopLeft,
        PubBorderArtSlotV1::Top => ViewerDecorativeBorderSlotV1::Top,
        PubBorderArtSlotV1::TopRight => ViewerDecorativeBorderSlotV1::TopRight,
        PubBorderArtSlotV1::Right => ViewerDecorativeBorderSlotV1::Right,
        PubBorderArtSlotV1::BottomRight => ViewerDecorativeBorderSlotV1::BottomRight,
        PubBorderArtSlotV1::Bottom => ViewerDecorativeBorderSlotV1::Bottom,
        PubBorderArtSlotV1::BottomLeft => ViewerDecorativeBorderSlotV1::BottomLeft,
        PubBorderArtSlotV1::Left => ViewerDecorativeBorderSlotV1::Left,
    }
}

fn borderart_resource_id(
    source_hash: &Sha256Digest,
    ordinal: u32,
    pool_index: u8,
    sha256: &str,
) -> Result<ResourceId> {
    let key = format!(
        "borderart/catalog-{ordinal}/pool-{pool_index}/wmf-sha256-{sha256}/{LEGACY_OLE_WMF_PREVIEW_RASTERIZER_V1}"
    );
    let canonical = derive_source_canonical_id(SourceDerivedIdInput {
        source_hash,
        adapter_id: "pub-viewer",
        source_object_key: &key,
        semantic_role: "viewer.decorative-border-preview-v1",
    })
    .map_err(|error| anyhow::anyhow!("derive BorderArt resource identity: {error:?}"))?;
    Ok(ResourceId::from_canonical(canonical))
}

fn seq_to_node_map(
    graph: &PubResolvedGraph,
    diagnostics: &mut Vec<ViewerDiagnostic>,
) -> BTreeMap<u32, NodeId> {
    let mut map = BTreeMap::new();
    let mut ambiguous = BTreeSet::new();

    for node in graph.nodes.values() {
        let seq = node.payload.contents_seq_num;
        if let Some(existing) = map.insert(seq, node.header.id) {
            if existing != node.header.id {
                ambiguous.insert(seq);
            }
        }
    }

    for seq in ambiguous {
        map.remove(&seq);
        diagnostics.push(ViewerDiagnostic {
            code: "viewer.borderart.shape_identity_ambiguous".to_owned(),
            severity: ViewerDiagnosticSeverity::FidelityWarning,
            message: format!(
                "A persisted decorative-border shape reference uses source object {seq}, but that source identity maps to multiple Viewer nodes."
            ),
        });
    }

    map
}

pub(crate) fn viewer_decorative_borders_v1(
    pub_bytes: &[u8],
    source_hash: &Sha256Digest,
    graph: &PubResolvedGraph,
) -> (Vec<ViewerDecorativeBorderV1>, Vec<ViewerDiagnostic>) {
    let mut diagnostics = Vec::new();
    let read = match read_mature_0x2c_borderart_assets_from_pub_bytes_v1(pub_bytes) {
        Ok(read) => read,
        Err(error) => {
            diagnostics.push(ViewerDiagnostic {
                code: "viewer.borderart.read_unavailable".to_owned(),
                severity: ViewerDiagnosticSeverity::FidelityWarning,
                message: format!(
                    "Persisted decorative-border metadata could not be projected safely: {error}"
                ),
            });
            return (Vec::new(), diagnostics);
        }
    };

    for diagnostic in &read.diagnostics {
        diagnostics.push(ViewerDiagnostic {
            code: format!("viewer.{}", diagnostic.code),
            severity: ViewerDiagnosticSeverity::FidelityWarning,
            message: diagnostic.detail.clone(),
        });
    }

    if read.shape_uses.is_empty() {
        return (Vec::new(), diagnostics);
    }

    let node_by_seq = seq_to_node_map(graph, &mut diagnostics);
    let entry_by_ordinal = read
        .entries
        .iter()
        .map(|entry| (entry.ordinal, entry))
        .collect::<BTreeMap<_, _>>();

    let mut seen_nodes = BTreeSet::new();
    let mut out = Vec::new();

    for shape_use in &read.shape_uses {
        let Some(entry) = entry_by_ordinal.get(&u32::from(shape_use.fbid)).copied() else {
            diagnostics.push(ViewerDiagnostic {
                code: "viewer.borderart.catalog_entry_unavailable".to_owned(),
                severity: ViewerDiagnosticSeverity::FidelityWarning,
                message: format!(
                    "Decorative-border source object {} selects catalog ordinal {}, but that catalog entry was not safely materialized.",
                    shape_use.contents_seq_num, shape_use.fbid
                ),
            });
            continue;
        };
        let Some(node_id) = node_by_seq.get(&shape_use.contents_seq_num).copied() else {
            diagnostics.push(ViewerDiagnostic {
                code: "viewer.borderart.shape_identity_unavailable".to_owned(),
                severity: ViewerDiagnosticSeverity::FidelityWarning,
                message: format!(
                    "Decorative-border source object {} could not be joined to one Viewer node.",
                    shape_use.contents_seq_num
                ),
            });
            continue;
        };
        if !seen_nodes.insert(node_id) {
            diagnostics.push(ViewerDiagnostic {
                code: "viewer.borderart.duplicate_shape_use".to_owned(),
                severity: ViewerDiagnosticSeverity::FidelityWarning,
                message: format!(
                    "Viewer node {} has more than one persisted decorative-border use and was rejected.",
                    node_id.as_canonical()
                ),
            });
            continue;
        }

        let mut resource_ids = BTreeMap::new();
        let mut resources = Vec::new();
        let mut resource_failed = false;
        for resource in &entry.resources {
            match borderart_resource_id(
                source_hash,
                entry.ordinal,
                resource.pool_index,
                &resource.sha256,
            ) {
                Ok(resource_id) => {
                    let preview = match rasterize_wmf_preview(
                        &resource.bytes,
                        BORDERART_PREVIEW_SIDE_PX,
                        BORDERART_PREVIEW_SIDE_PX,
                    ) {
                        Ok(preview) => preview,
                        Err(error) => {
                            diagnostics.push(ViewerDiagnostic {
                                code: "viewer.borderart.resource_raster_unsupported".to_owned(),
                                severity: ViewerDiagnosticSeverity::FidelityWarning,
                                message: format!(
                                    "A validated decorative-border vector resource could not be rasterized by the bounded Viewer profile: {error}"
                                ),
                            });
                            resource_failed = true;
                            break;
                        }
                    };
                    let png = match encode_wmf_preview_png(&preview) {
                        Ok(png) => png,
                        Err(error) => {
                            diagnostics.push(ViewerDiagnostic {
                                code: "viewer.borderart.resource_encode_unavailable".to_owned(),
                                severity: ViewerDiagnosticSeverity::FidelityWarning,
                                message: format!(
                                    "A bounded decorative-border vector preview could not be encoded for Viewer transport: {error}"
                                ),
                            });
                            resource_failed = true;
                            break;
                        }
                    };
                    resource_ids.insert(resource.pool_index, resource_id);
                    resources.push(ViewerDecorativeBorderResourceV1 {
                        resource_id,
                        mime: "image/png".to_owned(),
                        source_wmf_sha256: resource.sha256.clone(),
                        bytes: png,
                    });
                }
                Err(error) => {
                    diagnostics.push(ViewerDiagnostic {
                        code: "viewer.borderart.resource_identity_unavailable".to_owned(),
                        severity: ViewerDiagnosticSeverity::FidelityWarning,
                        message: error.to_string(),
                    });
                    resource_failed = true;
                    break;
                }
            }
        }
        if resource_failed {
            continue;
        }

        let mut slots = Vec::with_capacity(entry.slots.len());
        let mut slot_failed = false;
        for slot in &entry.slots {
            let Some(resource_id) = resource_ids.get(&slot.resource_pool_index).copied() else {
                diagnostics.push(ViewerDiagnostic {
                    code: "viewer.borderart.slot_resource_unavailable".to_owned(),
                    severity: ViewerDiagnosticSeverity::FidelityWarning,
                    message: format!(
                        "Decorative-border catalog {:?} has a semantic slot pointing outside its admitted resource pool.",
                        entry.name
                    ),
                });
                slot_failed = true;
                break;
            };
            slots.push(ViewerDecorativeBorderSlotRefV1 {
                slot: viewer_slot(slot.slot),
                resource_id,
            });
        }
        if slot_failed || slots.len() != 8 {
            continue;
        }

        out.push(ViewerDecorativeBorderV1 {
            node_id,
            name: entry.name.clone(),
            corner_extent_emu: entry.corner_extent_emu,
            horizontal_extent_emu: entry.horizontal_extent_emu,
            vertical_extent_emu: entry.vertical_extent_emu,
            resources,
            slots,
        });
    }

    if !out.is_empty() {
        diagnostics.push(ViewerDiagnostic {
            code: "viewer.borderart.source_decoration_preserved".to_owned(),
            severity: ViewerDiagnosticSeverity::Info,
            message: format!(
                "{} source-backed decorative border(s) preserve exact catalog identity, geometry and directional WMF resources separately from ordinary shape Line state; admitted vector resources are transported as bounded PNG previews.",
                out.len()
            ),
        });
    }

    (out, diagnostics)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_slot_mapping_is_explicit_and_stable() {
        assert_eq!(
            viewer_slot(PubBorderArtSlotV1::TopLeft),
            ViewerDecorativeBorderSlotV1::TopLeft
        );
        assert_eq!(
            viewer_slot(PubBorderArtSlotV1::BottomRight),
            ViewerDecorativeBorderSlotV1::BottomRight
        );
        assert_eq!(
            viewer_slot(PubBorderArtSlotV1::Left),
            ViewerDecorativeBorderSlotV1::Left
        );
    }
}
