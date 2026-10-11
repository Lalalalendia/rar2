//! Source-backed derived EMF BI_RGB preview integration owned outside Viewer monolith.
use super::*;
use pub_reader::mature_emf::{
    MATURE_OFFICEART_EMF_BITMAP_PREVIEW_SOURCE_V1,
    build_mature_0x2c_emf_bitmap_preview_bundle_from_bytes,
};

fn mature_officeart_emf_bitmap_preview_resource_id(
    source_hash: &Sha256Digest,
    slot: u32,
    rgba: &[u8],
) -> Result<ResourceId> {
    let rgba_sha256 = Sha256::digest(rgba)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let source_object_key = format!(
        "mature-officeart-emf-birgb-preview/slot-{slot}/rgba-sha256-{rgba_sha256}/{}",
        MATURE_OFFICEART_EMF_BITMAP_PREVIEW_SOURCE_V1,
    );
    let canonical = derive_source_canonical_id(SourceDerivedIdInput {
        source_hash,
        adapter_id: "pub-viewer",
        source_object_key: &source_object_key,
        semantic_role: "viewer.mature-officeart-emf-bitmap-preview-v1",
    })
    .map_err(|error| anyhow!("derive mature EMF bitmap preview identity: {error:?}"))?;
    Ok(ResourceId::from_canonical(canonical))
}

/// EMF bitmap is a derived *layer*, not full metafile playback. Even strictly
/// validated pixels omit EMF vector commands, transforms, clipping and effects.
pub(super) fn viewer_mature_officeart_emf_bitmap_preview_images(
    bytes: &[u8],
    source_hash: &Sha256Digest,
    source: &PubSourceGraphBuild,
    resolved: &PubResolvedGraph,
    scene: &BoundedResolvedScene,
    diagnostics: &mut Vec<ViewerDiagnostic>,
) -> Vec<ViewerEmbeddedImage> {
    let bundle = match build_mature_0x2c_emf_bitmap_preview_bundle_from_bytes(bytes, &source.graph)
    {
        Ok(bundle) => bundle,
        Err(_) => {
            diagnostics.push(ViewerDiagnostic {
                    code: "viewer.mature_officeart_emf.bitmap_source_unavailable".to_owned(),
                    severity: ViewerDiagnosticSeverity::FidelityWarning,
                    message: "Source-backed OfficeArt EMF bitmap-preview admission could not be strictly validated.".to_owned(),
                });
            return Vec::new();
        }
    };
    if bundle.rejected_source_count > 0 {
        diagnostics.push(ViewerDiagnostic {
            code: "viewer.mature_officeart_emf.bitmap_source_rejected".to_owned(),
            severity: ViewerDiagnosticSeverity::FidelityWarning,
            message: format!(
                "{} mature EMF source(s) rejected by the bounded bitmap-only preview profile.",
                bundle.rejected_source_count,
            ),
        });
    }
    if bundle.vector_only_count > 0 {
        diagnostics.push(ViewerDiagnostic {
            code: "viewer.mature_officeart_emf.vector_not_rendered".to_owned(),
            severity: ViewerDiagnosticSeverity::FidelityWarning,
            message: format!(
                "{} mature EMF source(s) contained no supported bitmap layer; vector playback is not implemented.",
                bundle.vector_only_count,
            ),
        });
    }
    let renderable_node_ids = scene
        .nodes
        .iter()
        .map(|node| node.origin)
        .collect::<BTreeSet<_>>();
    let mut images = Vec::new();
    for candidate in bundle.sources {
        let mut node_ids = candidate
            .uses
            .iter()
            .map(|usage| usage.node_id)
            .filter(|node_id| renderable_node_ids.contains(node_id))
            .filter(|node_id| {
                source
                    .graph
                    .nodes
                    .get(node_id)
                    .and_then(|node| node.payload.image_slot)
                    == Some(candidate.slot)
            })
            .collect::<Vec<_>>();
        node_ids.sort();
        node_ids.dedup();
        if node_ids.is_empty() {
            diagnostics.push(ViewerDiagnostic {
                code: "viewer.mature_officeart_emf.source_use_not_in_scene".to_owned(),
                severity: ViewerDiagnosticSeverity::FidelityWarning,
                message:
                    "Strictly validated EMF bitmap has no matching admitted physical image frame."
                        .to_owned(),
            });
            continue;
        }

        let preview = WmfPreviewRgba {
            width: candidate.width,
            height: candidate.height,
            rgba: candidate.rgba,
        };
        let png = match encode_wmf_preview_png(&preview) {
            Ok(png) => png,
            Err(_) => {
                diagnostics.push(ViewerDiagnostic {
                    code: "viewer.mature_officeart_emf.bitmap_encode_unavailable".to_owned(),
                    severity: ViewerDiagnosticSeverity::FidelityWarning,
                    message:
                        "Strictly validated EMF bitmap layer exceeds bounded PNG preview encoding."
                            .to_owned(),
                });
                continue;
            }
        };
        let resource_id = match mature_officeart_emf_bitmap_preview_resource_id(
            source_hash,
            candidate.slot,
            &preview.rgba,
        ) {
            Ok(id) => id,
            Err(_) => {
                diagnostics.push(ViewerDiagnostic {
                    code: "viewer.mature_officeart_emf.preview_identity_unavailable".to_owned(),
                    severity: ViewerDiagnosticSeverity::FidelityWarning,
                    message:
                        "Derived EMF bitmap layer has no deterministic Viewer resource identity."
                            .to_owned(),
                });
                continue;
            }
        };

        let mut placements = Vec::new();
        for node_id in &node_ids {
            let resolved_node = resolved.nodes.get(node_id);
            let source_window = match resolved_node.map(|node| {
                viewer_image_source_window_v1(node.payload.explicit_image_crop.as_ref())
            }) {
                Some(Ok(window)) => window,
                Some(Err(_)) => {
                    diagnostics.push(ViewerDiagnostic {
                        code: "viewer.mature_officeart_emf.crop_partial".to_owned(),
                        severity: ViewerDiagnosticSeverity::FidelityWarning,
                        message: "EMF derived bitmap source crop could not be projected; full bitmap shown as partial preview.".to_owned(),
                    });
                    None
                }
                None => None,
            };
            let content_rotation_degrees = resolved_node
                .and_then(|node| node.payload.explicit_image_cardinal_rotation_degrees);
            let recolor = source.graph.nodes.get(node_id).and_then(|node| {
                viewer_image_recolor_v1(node.payload.explicit_image_recolor.as_ref())
            });
            if source_window.is_some() || content_rotation_degrees.is_some() || recolor.is_some() {
                placements.push(ViewerImagePlacementV1 {
                    node_id: *node_id,
                    source_window,
                    content_rotation_degrees,
                    recolor,
                });
            }
        }

        // The PNG is derived and intentionally carries source_exact=false.
        images.push(ViewerEmbeddedImage::preview_png(
            resource_id,
            node_ids,
            placements,
            png,
        ));
        diagnostics.push(ViewerDiagnostic {
            code: "viewer.mature_officeart_emf.bitmap_layer_partial".to_owned(),
            severity: ViewerDiagnosticSeverity::FidelityWarning,
            message: format!(
                "A source-backed EMF BI_RGB bitmap layer was decoded, but complete metafile playback is unsupported ({} other EMR records). Vector overlays, clipping and transforms may differ; keep Partial/Needs review.",
                candidate.other_emr_count,
            ),
        });
    }
    images
}
