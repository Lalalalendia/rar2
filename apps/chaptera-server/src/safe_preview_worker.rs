//! Optional image-derivative production inside the confined PUB worker.
//!
//! This code runs only after the source and project are verified and filesystem
//! access is denied by the sandbox. Every emitted candidate is still untrusted
//! by the host; never promote it directly into a browser scene.

use std::collections::HashSet;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use chaptera_image_decode_contract::{
    DecodeLimitsV1, DecodePolicyV1, decode_image_v1,
    safe_preview_encode::{PreviewCodecV1, PreviewEncodeLimitsV1, encode_safe_preview_v1},
};
use pub_viewer::ViewerGeometryDocument;
use sha2::{Digest, Sha256};

use crate::{product_replay_worker::IsolatedImagePreviewV1, reader_scene_v1::ReaderSceneV1};

pub(crate) const MAX_WORKER_PREVIEW_ITEMS: usize = 64;
pub(crate) const MAX_WORKER_PREVIEW_RAW_BYTES: usize = 7 * 1024 * 1024;
pub(crate) const MAX_SOURCE_IMAGE_BYTES_FOR_PREVIEW: usize = 16 * 1024 * 1024;

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Bounded best-effort derived images. An undecodable or unrepresentable
/// image stays descriptor-only. There is no fallback to source containers.
pub(crate) fn derive_isolated_image_previews(
    geometry: &ViewerGeometryDocument,
    scene: &ReaderSceneV1,
) -> Vec<IsolatedImagePreviewV1> {
    let approved_ids: HashSet<&str> = scene
        .resources
        .iter()
        .map(|resource| resource.resource_id.as_str())
        .collect();
    let mut previews = Vec::new();
    let mut used_bytes = 0_usize;
    let mut emitted = HashSet::new();

    let mut try_add = |id: String, mime: &str, bytes: &[u8], verified_sha: Option<String>| {
        if previews.len() >= MAX_WORKER_PREVIEW_ITEMS
            || !approved_ids.contains(id.as_str())
            || !emitted.insert(id.clone())
            || bytes.is_empty()
            || bytes.len() > MAX_SOURCE_IMAGE_BYTES_FOR_PREVIEW
        {
            return;
        }
        let mime = match mime {
            "image/png" => "image/png",
            "image/jpeg" | "image/jpg" => "image/jpeg",
            _ => return,
        };
        let source_sha = sha256_hex(bytes);
        if verified_sha.is_some_and(|hash| hash != source_sha) {
            return;
        }
        let limits = PreviewEncodeLimitsV1::default();
        let decoded = match decode_image_v1(
            bytes,
            mime,
            &source_sha,
            "isolated-reader-image-pixels",
            &DecodePolicyV1::reference(),
            &DecodeLimitsV1 {
                max_encoded_bytes: MAX_SOURCE_IMAGE_BYTES_FOR_PREVIEW,
                max_dimension_px: limits.max_dimension_px,
                max_total_pixels: limits.max_total_pixels,
                max_decoded_bytes: limits.max_decoded_bytes,
            },
        ) {
            Ok(decoded) => decoded,
            Err(_) => return,
        };
        let codec = if mime == "image/jpeg" && !decoded.alpha_present {
            PreviewCodecV1::Jpeg
        } else {
            PreviewCodecV1::Png
        };
        let candidate = encode_safe_preview_v1(&decoded, codec, &limits).or_else(|_| {
            if decoded.alpha_present || codec == PreviewCodecV1::Jpeg {
                return Err(
                    chaptera_image_decode_contract::safe_preview_encode::PreviewEncodeErrorV1 {
                        code: "preview_unsupported",
                    },
                );
            }
            encode_safe_preview_v1(&decoded, PreviewCodecV1::Jpeg, &limits)
        });
        let Ok(candidate) = candidate else {
            return;
        };
        let Some(total) = used_bytes.checked_add(candidate.bytes.len()) else {
            return;
        };
        if total > MAX_WORKER_PREVIEW_RAW_BYTES {
            return;
        }
        let Ok(receipt) = serde_json::to_value(&candidate.receipt) else {
            return;
        };
        used_bytes = total;
        previews.push(IsolatedImagePreviewV1 {
            resource_id: id,
            encoded_base64: STANDARD.encode(&candidate.bytes),
            receipt,
        });
    };

    for image in &geometry.images {
        try_add(
            image.resource_id.as_canonical().to_string(),
            &image.mime,
            &image.bytes,
            image
                .verified_source_sha256
                .as_ref()
                .map(ToString::to_string),
        );
    }
    for resource in &geometry.decorative_border_resources {
        try_add(
            resource.resource_id.as_canonical().to_string(),
            &resource.mime,
            &resource.bytes,
            None,
        );
    }
    previews
}
