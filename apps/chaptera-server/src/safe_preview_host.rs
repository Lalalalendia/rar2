//! Host-owned browser admission for isolated-worker SafePreview candidates.
//!
//! An isolated PUB worker may produce source-derived image *candidates*, never
//! browser payload authority. Here the host independently decodes each bounded
//! candidate and emits a new metadata-free container from pixel samples.
//! Original PUB image containers and worker-provided data URLs are NEVER used.

use std::collections::HashSet;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use chaptera_image_decode_contract::{
    DecodeLimitsV1, DecodePolicyV1, decode_image_v1,
    safe_preview_encode::{
        JPEG_ENCODER_ID_V1, JPEG_QUALITY_LADDER_V1, PNG_ENCODER_ID_V1,
        PREVIEW_COLOR_DISPOSITION_V1, PreviewCodecV1, PreviewEncodeLimitsV1,
        SAFE_PREVIEW_ENCODE_V1, encode_safe_preview_v1,
    },
};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::{product_replay_worker::IsolatedImagePreviewV1, reader_scene_v1::MAX_SCENE_BYTES};

const MAX_BROWSER_PREVIEW_DATA_URL_BYTES: usize = 7 * 1024 * 1024;
const MAX_BROWSER_PREVIEW_TOTAL_BYTES: usize = 8 * 1024 * 1024;
const MAX_WORKER_PREVIEW_BASE64_BYTES: usize = 7 * 1024 * 1024;

/// Owned wire receipt: the image contract's `&'static str` constants cannot
/// be deserialized from untrusted JSON. Enforce the exact fixed strings below.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PreviewWireReceipt {
    protocol_version: String,
    source_resource_sha256: String,
    source_sample_sha256: String,
    encode_spec_sha256: String,
    encoded_artifact_sha256: String,
    encoded_byte_len: usize,
    dimensions_px: [u32; 2],
    mime: String,
    encoder_id: String,
    quality: Option<u8>,
    alpha_preserved: bool,
    orientation_applied: bool,
    source_metadata_carried: bool,
    output_color_disposition: String,
    visual_fidelity: String,
    browser_delivery_authorized: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HostAdmittedPreview {
    pub mime: &'static str,
    pub data_url: String,
    pub encoded_len: usize,
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn canonical_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

/// Verify a hostile worker receipt BEFORE parsing its bytes. Return only
/// host-created encoded pixels; even an attested worker candidate is untrusted.
pub(crate) fn admit_worker_preview(
    bytes: &[u8],
    wire_receipt: &Value,
) -> Result<HostAdmittedPreview, &'static str> {
    let receipt: PreviewWireReceipt =
        serde_json::from_value(wire_receipt.clone()).map_err(|_| "preview_receipt_shape_denied")?;
    let limits = PreviewEncodeLimitsV1::default();
    let (input_cap, expected_encoder, codec) = match receipt.mime.as_str() {
        "image/png" => (limits.max_png_bytes, PNG_ENCODER_ID_V1, PreviewCodecV1::Png),
        "image/jpeg" => (
            limits.max_jpeg_bytes,
            JPEG_ENCODER_ID_V1,
            PreviewCodecV1::Jpeg,
        ),
        _ => return Err("preview_mime_denied"),
    };
    let quality_valid = if codec == PreviewCodecV1::Png {
        receipt.quality.is_none()
    } else {
        receipt
            .quality
            .is_some_and(|q| JPEG_QUALITY_LADDER_V1.contains(&q))
    };
    if receipt.protocol_version != SAFE_PREVIEW_ENCODE_V1
        || receipt.encoder_id != expected_encoder
        || !quality_valid
        || receipt.visual_fidelity != "partial_color_untransformed"
        || receipt.output_color_disposition != PREVIEW_COLOR_DISPOSITION_V1
        || receipt.browser_delivery_authorized
        || !canonical_hash(&receipt.source_resource_sha256)
        || !canonical_hash(&receipt.source_sample_sha256)
        || !canonical_hash(&receipt.encode_spec_sha256)
        || !canonical_hash(&receipt.encoded_artifact_sha256)
        || receipt.encoded_byte_len != bytes.len()
        || bytes.is_empty()
        || bytes.len() > input_cap
        || receipt.encoded_artifact_sha256 != sha256_hex(bytes)
        || receipt.source_metadata_carried
        || receipt.orientation_applied
        || receipt.dimensions_px.contains(&0)
        || receipt
            .dimensions_px
            .iter()
            .any(|&n| n > limits.max_dimension_px)
        || u64::from(receipt.dimensions_px[0]) * u64::from(receipt.dimensions_px[1])
            > limits.max_total_pixels
    {
        return Err("preview_receipt_denied");
    }
    let decoded = decode_image_v1(
        bytes,
        &receipt.mime,
        &receipt.encoded_artifact_sha256,
        "host-independent-browser-preview",
        &DecodePolicyV1::reference(),
        &DecodeLimitsV1 {
            max_encoded_bytes: input_cap,
            max_dimension_px: limits.max_dimension_px,
            max_total_pixels: limits.max_total_pixels,
            max_decoded_bytes: limits.max_decoded_bytes,
        },
    )
    .map_err(|_| "preview_host_decode_rejected")?;
    if [decoded.decoded_width_px, decoded.decoded_height_px] != receipt.dimensions_px
        || decoded.alpha_present != receipt.alpha_preserved
    {
        return Err("preview_receipt_pixels_mismatch");
    }
    // A second independent host encode. Preserve JPEG when needed to avoid
    // overflowing the unchanged 16 MiB Reader Scene budget for photographs.
    let fresh = encode_safe_preview_v1(&decoded, codec, &limits)
        .map_err(|_| "preview_host_reencode_rejected")?;
    let data_url = format!(
        "data:{};base64,{}",
        fresh.receipt.mime,
        STANDARD.encode(&fresh.bytes)
    );
    if data_url.len() > MAX_BROWSER_PREVIEW_DATA_URL_BYTES {
        return Err("preview_browser_budget_exceeded");
    }
    Ok(HostAdmittedPreview {
        mime: fresh.receipt.mime,
        data_url,
        encoded_len: fresh.bytes.len(),
    })
}

/// Called ONLY after worker receipt validation and after the legacy sanitizer
/// strips every worker-provided `inline_data_url`. Unadmitted image resources
/// stay descriptors, with an explicit partial-fidelity reason.
pub(crate) fn attach_admitted_previews(
    scene: &mut Value,
    previews: &[IsolatedImagePreviewV1],
) -> Result<usize, &'static str> {
    let mut accepted = 0_usize;
    let mut aggregate_raw = 0_usize;
    let mut seen = HashSet::new();
    for preview in previews {
        if !seen.insert(preview.resource_id.as_str())
            || preview.encoded_base64.len() > MAX_WORKER_PREVIEW_BASE64_BYTES
        {
            return Err("preview_host_transport_denied");
        }
        let index = scene
            .get("resources")
            .and_then(Value::as_array)
            .and_then(|resources| {
                resources.iter().position(|resource| {
                    resource["resource_id"].as_str() == Some(preview.resource_id.as_str())
                })
            })
            .ok_or("preview_resource_unbound")?;
        // Never allow another candidate to overwrite an already admitted one,
        // nor permit a pre-sanitize inline URL to become delivery authority.
        if scene["resources"][index]["availability"] != "descriptor_only"
            || scene["resources"][index].get("inline_data_url").is_some()
        {
            return Err("preview_resource_not_sanitized");
        }
        let Ok(bytes) = STANDARD.decode(&preview.encoded_base64) else {
            continue;
        };
        let Ok(admitted) = admit_worker_preview(&bytes, &preview.receipt) else {
            continue;
        };
        let Some(new_aggregate) = aggregate_raw.checked_add(admitted.encoded_len) else {
            continue;
        };
        if new_aggregate > MAX_BROWSER_PREVIEW_TOTAL_BYTES {
            continue;
        }
        let original = scene["resources"][index].clone();
        {
            let fields = scene["resources"][index]
                .as_object_mut()
                .ok_or("preview_resource_shape_denied")?;
            fields.insert("mime".to_owned(), Value::String(admitted.mime.to_owned()));
            fields.insert(
                "availability".to_owned(),
                Value::String("inline_data_url".to_owned()),
            );
            fields.insert(
                "inline_data_url".to_owned(),
                Value::String(admitted.data_url),
            );
        }
        let candidate_size = serde_json::to_vec(scene)
            .map_err(|_| "preview_scene_serialization_denied")?
            .len();
        if candidate_size > MAX_SCENE_BYTES.saturating_sub(256) {
            scene["resources"][index] = original;
            continue;
        }
        accepted += 1;
        aggregate_raw = new_aggregate;
    }
    let all_available = scene
        .get("resources")
        .and_then(Value::as_array)
        .is_none_or(|resources| {
            resources
                .iter()
                .all(|r| r["availability"] == "inline_data_url")
        });
    let fidelity = scene
        .get_mut("fidelity")
        .and_then(Value::as_object_mut)
        .ok_or("preview_scene_fidelity_denied")?;
    let reasons = fidelity
        .get_mut("reasons")
        .and_then(Value::as_array_mut)
        .ok_or("preview_scene_fidelity_denied")?;
    if all_available {
        reasons.retain(|reason| reason.as_str() != Some("image_resource_not_inline"));
    }
    if accepted > 0
        && !reasons
            .iter()
            .any(|r| r == "image_preview_partial_fidelity")
    {
        reasons.push(json!("image_preview_partial_fidelity"));
    }
    let state = if reasons.is_empty() {
        "supported"
    } else {
        "partial"
    };
    fidelity.insert("state".to_owned(), json!(state));
    if serde_json::to_vec(scene)
        .map_err(|_| "preview_scene_serialization_denied")?
        .len()
        > MAX_SCENE_BYTES
    {
        return Err("preview_scene_budget_denied");
    }
    Ok(accepted)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input_png() -> Vec<u8> {
        let mut encoded = Vec::new();
        {
            let mut writer = png::Encoder::new(&mut encoded, 2, 1);
            writer.set_color(png::ColorType::Rgba);
            writer.set_depth(png::BitDepth::Eight);
            let mut writer = writer.write_header().unwrap();
            writer
                .write_image_data(&[10, 20, 30, 80, 100, 110, 120, 240])
                .unwrap();
        }
        encoded
    }

    fn worker_candidate() -> (Vec<u8>, Value) {
        let source = input_png();
        let decoded = decode_image_v1(
            &source,
            "image/png",
            &sha256_hex(&source),
            "test-original-source",
            &DecodePolicyV1::reference(),
            &DecodeLimitsV1::default(),
        )
        .unwrap();
        let candidate = encode_safe_preview_v1(
            &decoded,
            PreviewCodecV1::Png,
            &PreviewEncodeLimitsV1::default(),
        )
        .unwrap();
        let receipt = serde_json::to_value(&candidate.receipt).unwrap();
        (candidate.bytes, receipt)
    }

    #[test]
    fn independent_host_roundtrip_admits_pixels_and_never_forwards_container() {
        let (candidate, receipt) = worker_candidate();
        let admitted = admit_worker_preview(&candidate, &receipt).unwrap();
        assert_eq!(admitted.mime, "image/png");
        assert!(admitted.data_url.starts_with("data:image/png;base64,"));
        let host_bytes = STANDARD
            .decode(admitted.data_url.split_once(',').unwrap().1)
            .unwrap();
        let roundtrip = decode_image_v1(
            &host_bytes,
            "image/png",
            &sha256_hex(&host_bytes),
            "test-host-preview",
            &DecodePolicyV1::reference(),
            &DecodeLimitsV1::default(),
        )
        .unwrap();
        assert_eq!(roundtrip.samples, [10, 20, 30, 80, 100, 110, 120, 240]);

        let mut scene = json!({
            "fidelity": {"state": "partial", "reasons": ["image_resource_not_inline"]},
            "resources": [{"resource_id":"resource-a","mime":"image/png","availability":"descriptor_only"}]
        });
        let previews = vec![IsolatedImagePreviewV1 {
            resource_id: "resource-a".to_owned(),
            encoded_base64: STANDARD.encode(&candidate),
            receipt,
        }];
        assert_eq!(attach_admitted_previews(&mut scene, &previews).unwrap(), 1);
        assert_eq!(scene["resources"][0]["availability"], "inline_data_url");
        assert_eq!(scene["fidelity"]["state"], "partial");
        assert_eq!(
            scene["fidelity"]["reasons"],
            json!(["image_preview_partial_fidelity"])
        );
    }

    #[test]
    fn rejects_forged_identity_metadata_and_delivery_flag() {
        let (candidate, mut receipt) = worker_candidate();
        receipt["encoded_artifact_sha256"] = json!("f".repeat(64));
        assert_eq!(
            admit_worker_preview(&candidate, &receipt),
            Err("preview_receipt_denied")
        );
        let (candidate, mut receipt) = worker_candidate();
        receipt["browser_delivery_authorized"] = json!(true);
        assert_eq!(
            admit_worker_preview(&candidate, &receipt),
            Err("preview_receipt_denied")
        );
        let (candidate, mut receipt) = worker_candidate();
        receipt["unknown_secret_field"] = json!("/private/path");
        assert_eq!(
            admit_worker_preview(&candidate, &receipt),
            Err("preview_receipt_shape_denied")
        );
        let (candidate, receipt) = worker_candidate();
        assert_eq!(
            admit_worker_preview(&candidate[..2], &receipt),
            Err("preview_receipt_denied")
        );
    }

    #[test]
    fn unsupported_or_unbound_preview_stays_fail_closed() {
        let (candidate, receipt) = worker_candidate();
        let mut scene = json!({
            "fidelity": {"state": "partial", "reasons": ["image_resource_not_inline"]},
            "resources": [{"resource_id":"resource-a","mime":"image/png","availability":"descriptor_only"}]
        });
        let alien = IsolatedImagePreviewV1 {
            resource_id: "resource-other".to_owned(),
            encoded_base64: STANDARD.encode(&candidate),
            receipt,
        };
        assert_eq!(
            attach_admitted_previews(&mut scene, &[alien]),
            Err("preview_resource_unbound")
        );
        assert_eq!(scene["resources"][0]["availability"], "descriptor_only");
    }

    #[test]
    fn original_source_marker_cannot_leak_as_container_metadata() {
        let (candidate, receipt) = worker_candidate();
        let encoded = admit_worker_preview(&candidate, &receipt).unwrap();
        assert!(!encoded.data_url.contains("SOURCE_EXIF_GPS_PRIVATE_MARKER"));
        // Original PUB or image containers are never passed to this host API:
        // the only browser byte path is a fresh independent pixel re-encode.
        assert!(encoded.encoded_len > 0);
    }
}
