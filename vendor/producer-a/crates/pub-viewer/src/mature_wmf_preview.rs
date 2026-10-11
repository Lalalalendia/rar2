//! Mature OfficeArt WMF bounded derived preview and deterministic source identity.
//! Extracted from the Viewer monolith; retains the exact original admission logic.
use super::*;

fn mature_officeart_wmf_preview_resource_id(
    source_hash: &Sha256Digest,
    slot: u32,
    wmf_bytes: &[u8],
) -> Result<ResourceId> {
    let digest = Sha256::digest(wmf_bytes);
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut wmf_sha256 = String::with_capacity(digest.len() * 2);
    for byte in digest {
        wmf_sha256.push(char::from(HEX[usize::from(byte >> 4)]));
        wmf_sha256.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    let source_object_key = format!(
        "mature-officeart-wmf-preview/slot-{slot}/wmf-sha256-{wmf_sha256}/{}/{}",
        MATURE_OFFICEART_WMF_PREVIEW_SOURCE_V1, LEGACY_OLE_WMF_PREVIEW_RASTERIZER_V1,
    );
    let canonical = derive_source_canonical_id(SourceDerivedIdInput {
        source_hash,
        adapter_id: "pub-viewer",
        source_object_key: &source_object_key,
        semantic_role: "viewer.mature-officeart-wmf-preview-v1",
    })
    .map_err(|error| anyhow!("derive mature OfficeArt WMF preview identity: {error:?}"))?;
    Ok(ResourceId::from_canonical(canonical))
}

pub(super) fn viewer_mature_officeart_wmf_preview_images(
    bytes: &[u8],
    source_hash: &Sha256Digest,
    source: &PubSourceGraphBuild,
    resolved: &PubResolvedGraph,
    scene: &BoundedResolvedScene,
    diagnostics: &mut Vec<ViewerDiagnostic>,
) -> Vec<ViewerEmbeddedImage> {
    let bundle = match build_mature_0x2c_wmf_preview_bundle_from_bytes(bytes, &source.graph) {
        Ok(bundle) => bundle,
        Err(_) => {
            diagnostics.push(ViewerDiagnostic {
                code: "viewer.mature_officeart_wmf.preview_source_unavailable".to_owned(),
                severity: ViewerDiagnosticSeverity::FidelityWarning,
                message:
                    "Source-backed mature OfficeArt WMF images could not be materialized through the bounded preview bridge."
                        .to_owned(),
            });
            return Vec::new();
        }
    };

    if bundle.rejected_source_count > 0 {
        diagnostics.push(ViewerDiagnostic {
            code: "viewer.mature_officeart_wmf.preview_source_rejected".to_owned(),
            severity: ViewerDiagnosticSeverity::FidelityWarning,
            message: format!(
                "{} mature OfficeArt WMF source(s) remain outside the bounded preview profile.",
                bundle.rejected_source_count
            ),
        });
    }

    let renderable_node_ids = scene
        .nodes
        .iter()
        .map(|node| node.origin)
        .collect::<BTreeSet<_>>();
    let mut images = Vec::new();

    for preview_source in bundle.sources {
        let mut node_ids = preview_source
            .uses
            .iter()
            .map(|usage| usage.node_id)
            .filter(|node_id| renderable_node_ids.contains(node_id))
            .collect::<Vec<_>>();
        node_ids.sort();
        node_ids.dedup();
        if node_ids.is_empty() {
            continue;
        }

        let preview = match rasterize_wmf_preview(
            &preview_source.wmf_bytes,
            preview_source.width_hint,
            preview_source.height_hint,
        ) {
            Ok(preview) => preview,
            Err(_) => {
                diagnostics.push(ViewerDiagnostic {
                    code: "viewer.mature_officeart_wmf.preview_raster_unsupported".to_owned(),
                    severity: ViewerDiagnosticSeverity::FidelityWarning,
                    message:
                        "A structurally valid mature OfficeArt WMF is outside the existing bounded Viewer raster profile."
                            .to_owned(),
                });
                continue;
            }
        };
        let png = match encode_wmf_preview_png(&preview) {
            Ok(png) => png,
            Err(_) => {
                diagnostics.push(ViewerDiagnostic {
                    code: "viewer.mature_officeart_wmf.preview_encode_unavailable".to_owned(),
                    severity: ViewerDiagnosticSeverity::FidelityWarning,
                    message:
                        "A bounded mature OfficeArt WMF preview could not be encoded as a Viewer image resource."
                            .to_owned(),
                });
                continue;
            }
        };
        let resource_id = match mature_officeart_wmf_preview_resource_id(
            source_hash,
            preview_source.slot,
            &preview_source.wmf_bytes,
        ) {
            Ok(resource_id) => resource_id,
            Err(_) => {
                diagnostics.push(ViewerDiagnostic {
                    code: "viewer.mature_officeart_wmf.preview_identity_unavailable".to_owned(),
                    severity: ViewerDiagnosticSeverity::FidelityWarning,
                    message:
                        "A bounded mature OfficeArt WMF preview could not receive a deterministic Viewer resource identity."
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
                Some(Ok(source_window)) => source_window,
                Some(Err(reason)) => {
                    diagnostics.push(ViewerDiagnostic {
                        code: "viewer.image.crop_partial".to_owned(),
                        severity: ViewerDiagnosticSeverity::FidelityWarning,
                        message: format!(
                            "Image crop for node {} is present but cannot be projected exactly ({reason}); the bounded WMF preview remains available as the full-image fallback.",
                            node_id.as_canonical()
                        ),
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

        images.push(ViewerEmbeddedImage::preview_png(
            resource_id,
            node_ids,
            placements,
            png,
        ));
    }

    if !images.is_empty() {
        diagnostics.push(ViewerDiagnostic {
            code: "viewer.mature_officeart_wmf.preview_applied".to_owned(),
            severity: ViewerDiagnosticSeverity::Info,
            message: format!(
                "{} bounded source-backed mature OfficeArt WMF preview resource(s) admitted through the shared Viewer image path.",
                images.len()
            ),
        });
    }

    images
}

#[cfg(test)]
mod mature_officeart_wmf_preview_identity_tests {
    use super::*;

    #[test]
    fn preview_resource_identity_is_source_slot_and_wmf_bound() {
        let source_hash = Sha256Digest::from_bytes([0x11; 32]);
        let first = mature_officeart_wmf_preview_resource_id(&source_hash, 3, b"normalized-wmf-a")
            .expect("preview resource id");
        let repeated =
            mature_officeart_wmf_preview_resource_id(&source_hash, 3, b"normalized-wmf-a")
                .expect("repeat preview resource id");
        let other_slot =
            mature_officeart_wmf_preview_resource_id(&source_hash, 4, b"normalized-wmf-a")
                .expect("other slot preview resource id");
        let other_wmf =
            mature_officeart_wmf_preview_resource_id(&source_hash, 3, b"normalized-wmf-b")
                .expect("other WMF preview resource id");

        assert_eq!(first, repeated);
        assert_ne!(first, other_slot);
        assert_ne!(first, other_wmf);
    }
}
