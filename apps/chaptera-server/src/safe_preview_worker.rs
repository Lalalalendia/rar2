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
const MAX_PREVIEW_DECODE_INPUT_BYTES: usize = 64 * 1024 * 1024;
const MAX_PREVIEW_DECODED_WORK_BYTES: usize = 96 * 1024 * 1024;

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Only source-neutral counts. Never source hashes, image IDs, paths or bytes.
#[derive(Debug, Default, serde::Serialize)]
pub(crate) struct PreviewWorkerCensusV1 {
    pub image_count: usize,
    pub source_exact_count: usize,
    pub independently_verified_count: usize,
    pub scene_resource_count: usize,
    pub eligible_resources: usize,
    pub supported_mime: usize,
    pub verified_sha: usize,
    pub decode_attempted: usize,
    pub decode_succeeded: usize,
    pub encode_succeeded: usize,
    pub output_budget_admitted: usize,
    pub emitted: usize,
}

/// Bounded best-effort derived images. An undecodable or unrepresentable
/// image stays descriptor-only. There is no fallback to source containers.
pub(crate) fn derive_isolated_image_previews(
    geometry: &ViewerGeometryDocument,
    scene: &ReaderSceneV1,
    stage: &mut PreviewWorkerCensusV1,
) -> Vec<IsolatedImagePreviewV1> {
    stage.image_count = geometry.images.len();
    stage.source_exact_count = geometry.images.iter().filter(|i| i.source_exact).count();
    stage.independently_verified_count = geometry
        .images
        .iter()
        .filter(|i| i.verified_source_sha256.is_some())
        .count();
    stage.scene_resource_count = scene.resources.len();
    let approved_ids: HashSet<&str> = scene
        .resources
        .iter()
        .map(|resource| resource.resource_id.as_str())
        .collect();
    let mut previews = Vec::new();
    let mut used_bytes = 0_usize;
    let mut attempted_images = 0_usize;
    let mut total_decode_input = 0_usize;
    let mut total_decoded_work = 0_usize;
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
        stage.eligible_resources += 1;
        let mime = match mime {
            "image/png" => "image/png",
            "image/jpeg" | "image/jpg" => "image/jpeg",
            _ => return,
        };
        stage.supported_mime += 1;
        let source_sha = sha256_hex(bytes);
        if verified_sha.is_some_and(|hash| hash != source_sha) {
            return;
        }
        stage.verified_sha += 1;
        // A malicious PUB could contain arbitrarily many small compressed
        // raster resources. Bound unsuccessful decode attempts as well as
        // successful preview output so the Reader remains responsive.
        let Some(next_input_total) = total_decode_input.checked_add(bytes.len()) else {
            return;
        };
        if attempted_images >= MAX_WORKER_PREVIEW_ITEMS
            || next_input_total > MAX_PREVIEW_DECODE_INPUT_BYTES
            || total_decoded_work >= MAX_PREVIEW_DECODED_WORK_BYTES
        {
            return;
        }
        attempted_images += 1;
        stage.decode_attempted += 1;
        total_decode_input = next_input_total;
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
        stage.decode_succeeded += 1;
        let Some(next_work_total) = total_decoded_work.checked_add(decoded.decoded_byte_count)
        else {
            return;
        };
        if next_work_total > MAX_PREVIEW_DECODED_WORK_BYTES {
            return;
        }
        total_decoded_work = next_work_total;
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
        stage.encode_succeeded += 1;
        let Some(total) = used_bytes.checked_add(candidate.bytes.len()) else {
            return;
        };
        if total > MAX_WORKER_PREVIEW_RAW_BYTES {
            return;
        }
        stage.output_budget_admitted += 1;
        let Ok(receipt) = serde_json::to_value(&candidate.receipt) else {
            return;
        };
        used_bytes = total;
        previews.push(IsolatedImagePreviewV1 {
            resource_id: id,
            encoded_base64: STANDARD.encode(&candidate.bytes),
            receipt,
        });
        stage.emitted += 1;
    };

    for image in &geometry.images {
        // Exact original PUB images require the Reader's independently verified
        // source-resource hash. A raw image container without that authority
        // must remain descriptor-only, even when its bytes are decodable.
        if image.source_exact && image.verified_source_sha256.is_none() {
            continue;
        }
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
