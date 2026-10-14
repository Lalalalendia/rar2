//! SafePreview: isolated Viewer pixels -> independently encoded host-owned image.
//!
//! Never forward original PUB image containers or arbitrary worker data URLs.
//! The worker is allowed to decode source images after syscall confinement;
//! the host only accepts bounded sample arrays and performs a fresh encode.
//! A preview is partial-fidelity display material, not source-exact evidence.

use std::collections::{BTreeMap, HashMap, HashSet};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use chaptera_image_decode_contract::{
    DecodeLimitsV1, DecodePolicyV1, DecodedImageV1, decode_image_v1,
    safe_preview_encode::{PreviewCodecV1, PreviewEncodeLimitsV1, encode_safe_preview_v1},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::reader_scene_v1::MAX_SCENE_BYTES;

const MAX_ORIGINAL_CONTAINER_BYTES: usize = 8 * 1024 * 1024;
const MAX_PREVIEW_SAMPLE_BYTES: usize = 40 * 1024 * 1024;
const MAX_PREVIEW_CANDIDATES: usize = 96;
// Fail closed on *attempted* decodes, not only successfully encoded previews.
// An adversarial PUB can contain many tiny malformed compressed resources.
const MAX_PREVIEW_DECODE_ATTEMPTS: usize = 64;
const MAX_PREVIEW_DECODE_INPUT_BYTES: usize = 64 * 1024 * 1024;
const MAX_PREVIEW_DECODED_WORK_BYTES: usize = 96 * 1024 * 1024;
const PREVIEW_COLOR_REASON: &str = "safe_preview_color_untransformed";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerSafePreviewSamplesV1 {
    pub resource_id: String,
    pub mime: String,
    /// Decoder receipt contains provenance and pixel layout but not samples.
    #[serde(rename = "source_decode")]
    pub decoded: DecodedImageV1,
    pub samples_base64: String,
    pub encode_spec_sha256: String,
    pub encoded_artifact_sha256: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WorkerSafePreviewAuditV1 {
    // Fixed reason codes and aggregate counts only. No raw paths, source
    // bytes, image IDs, pixels, user text, or provider credentials.
    pub input_resources: usize,
    pub input_encoded_byte_count: usize,
    pub eligible_mime_resources: usize,
    pub decoded_resources: usize,
    pub encoded_candidates: usize,
    pub candidate_encoded_bytes: usize,
    pub reject_codes: BTreeMap<String, usize>,
}

impl WorkerSafePreviewAuditV1 {
    fn reject(&mut self, reason: &str) {
        let reason = reason.chars().take(96).collect::<String>();
        *self.reject_codes.entry(reason).or_default() += 1;
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// The decoder returns only these fixed, source-neutral failure stages and
/// variants. Worker receipts never echo arbitrary parser messages.
fn safe_jpeg_decode_stage_code(detail: &str) -> &'static str {
    match detail {
        "jpeg_header_format" => "jpeg_header_invalid_format",
        "jpeg_header_unsupported" => "jpeg_header_unsupported",
        "jpeg_header_io_eof" => "jpeg_header_io_eof",
        "jpeg_header_io_invalid_data" => "jpeg_header_io_invalid_data",
        "jpeg_header_io_memory" => "jpeg_header_io_memory",
        "jpeg_header_io_interrupted" => "jpeg_header_io_interrupted",
        "jpeg_header_io_permission" => "jpeg_header_io_permission",
        "jpeg_header_io_other" => "jpeg_header_io_other",
        "jpeg_header_internal" => "jpeg_header_internal",
        "jpeg_pixels_format" => "jpeg_pixels_invalid_format",
        "jpeg_pixels_unsupported" => "jpeg_pixels_unsupported",
        "jpeg_pixels_io_eof" => "jpeg_pixels_io_eof",
        "jpeg_pixels_io_invalid_data" => "jpeg_pixels_io_invalid_data",
        "jpeg_pixels_io_memory" => "jpeg_pixels_io_memory",
        "jpeg_pixels_io_interrupted" => "jpeg_pixels_io_interrupted",
        "jpeg_pixels_io_permission" => "jpeg_pixels_io_permission",
        "jpeg_pixels_io_other" => "jpeg_pixels_io_other",
        "jpeg_pixels_internal" => "jpeg_pixels_internal",
        _ => "jpeg_stage_unknown",
    }
}

fn codec(mime: &str) -> Option<PreviewCodecV1> {
    match mime {
        "image/png" => Some(PreviewCodecV1::Png),
        "image/jpeg" => Some(PreviewCodecV1::Jpeg),
        _ => None,
    }
}

/// Runs only inside the already-confined PUB worker. Unsupported images
/// stay descriptor-only; failures are measured without exposing payloads.
pub(crate) fn collect_worker_preview_samples<'a>(
    resources: impl IntoIterator<Item = (String, &'a str, &'a [u8])>,
) -> Vec<WorkerSafePreviewSamplesV1> {
    collect_worker_preview_samples_with_audit(resources).0
}

/// Fail-closed image disposition evidence; never a browser-delivery grant.
/// The audit contains ONLY fixed error codes and aggregate resource counts.
pub(crate) fn collect_worker_preview_samples_with_audit<'a>(
    resources: impl IntoIterator<Item = (String, &'a str, &'a [u8])>,
) -> (Vec<WorkerSafePreviewSamplesV1>, WorkerSafePreviewAuditV1) {
    let mut candidates = Vec::new();
    let mut seen = HashSet::new();
    let mut remaining = MAX_PREVIEW_SAMPLE_BYTES;
    let mut attempted_decodes = 0usize;
    let mut total_decode_input = 0usize;
    let mut total_decoded_work = 0usize;
    let mut audit = WorkerSafePreviewAuditV1::default();

    for (resource_id, mime, original_bytes) in resources {
        audit.input_resources += 1;
        audit.input_encoded_byte_count = audit
            .input_encoded_byte_count
            .saturating_add(original_bytes.len());
        if candidates.len() >= MAX_PREVIEW_CANDIDATES {
            audit.reject("preview_candidate_count_limit");
            continue;
        }
        let Some(codec) = codec(mime) else {
            audit.reject("preview_mime_not_supported");
            continue;
        };
        audit.eligible_mime_resources += 1;
        if resource_id.is_empty() || resource_id.len() > 1024 || !seen.insert(resource_id.clone()) {
            audit.reject("preview_resource_identity_invalid");
            continue;
        }
        if original_bytes.is_empty() || original_bytes.len() > MAX_ORIGINAL_CONTAINER_BYTES {
            audit.reject("preview_original_container_limit");
            continue;
        }
        // Charge malformed inputs too. Successful-candidate and sample-output
        // caps do not otherwise prevent repeated expensive failed decodes.
        if attempted_decodes >= MAX_PREVIEW_DECODE_ATTEMPTS {
            audit.reject("preview_decode_attempt_budget");
            continue;
        }
        let Some(next_input_total) = total_decode_input.checked_add(original_bytes.len()) else {
            audit.reject("preview_decode_input_budget");
            continue;
        };
        if next_input_total > MAX_PREVIEW_DECODE_INPUT_BYTES {
            audit.reject("preview_decode_input_budget");
            continue;
        }
        if total_decoded_work >= MAX_PREVIEW_DECODED_WORK_BYTES {
            audit.reject("preview_decode_work_budget");
            continue;
        }
        attempted_decodes += 1;
        total_decode_input = next_input_total;
        let original_sha = sha256_hex(original_bytes);
        let decoded = match decode_image_v1(
            original_bytes,
            mime,
            &original_sha,
            "safe-preview-source-untransformed-v1",
            &DecodePolicyV1::reference(),
            &DecodeLimitsV1 {
                max_encoded_bytes: MAX_ORIGINAL_CONTAINER_BYTES,
                max_dimension_px: 8192,
                max_total_pixels: 16_000_000,
                // JPEG progressive DCT coefficients may exceed the final
                // RGB sample buffer. SK1 pinned 022 reference decode uses a
                // 64MiB scratch cap. Exported samples remain capped at 40MiB
                // for the entire document and encoded browser limits do not change.
                max_decoded_bytes: 64 * 1024 * 1024,
            },
        ) {
            Ok(decoded) => decoded,
            Err(error) => {
                if error.code == "jpeg_decode_error" {
                    audit.reject(safe_jpeg_decode_stage_code(&error.detail));
                } else {
                    audit.reject(error.code);
                }
                continue;
            }
        };
        let Some(next_work_total) = total_decoded_work.checked_add(decoded.decoded_byte_count)
        else {
            audit.reject("preview_decode_work_budget");
            continue;
        };
        if next_work_total > MAX_PREVIEW_DECODED_WORK_BYTES {
            audit.reject("preview_decode_work_budget");
            continue;
        }
        total_decoded_work = next_work_total;
        audit.decoded_resources += 1;
        if decoded.samples.len() > remaining {
            audit.reject("preview_aggregate_sample_bytes_limit");
            continue;
        }
        let preview =
            match encode_safe_preview_v1(&decoded, codec, &PreviewEncodeLimitsV1::default()) {
                Ok(preview) => preview,
                Err(error) => {
                    audit.reject(error.code);
                    continue;
                }
            };
        remaining -= decoded.samples.len();
        audit.candidate_encoded_bytes += preview.bytes.len();
        audit.encoded_candidates += 1;
        candidates.push(WorkerSafePreviewSamplesV1 {
            resource_id,
            mime: mime.to_owned(),
            samples_base64: BASE64.encode(&decoded.samples),
            decoded,
            encode_spec_sha256: preview.receipt.encode_spec_sha256,
            encoded_artifact_sha256: preview.receipt.encoded_artifact_sha256,
        });
    }
    (candidates, audit)
}

/// Runs in the host only after exact document/revision receipt validation and
/// the existing worker-image URL stripper. No original PNG/JPEG container is
/// parsed here: the host verifies raw samples and creates a fresh container.
/// Unknown or oversized resources stay explicitly unavailable.
pub(crate) fn admit_host_reencoded_previews(
    scene: &mut Value,
    candidates: &[WorkerSafePreviewSamplesV1],
) -> Result<(), &'static str> {
    if candidates.is_empty() {
        return Ok(());
    }
    if candidates.len() > MAX_PREVIEW_CANDIDATES {
        return Err("safe_preview_candidate_count_invalid");
    }
    let resources = scene
        .get("resources")
        .and_then(Value::as_array)
        .ok_or("safe_preview_scene_resources_invalid")?;
    let mut catalog = HashMap::new();
    for (index, resource) in resources.iter().enumerate() {
        let resource_id = resource
            .get("resource_id")
            .and_then(Value::as_str)
            .ok_or("safe_preview_resource_identity_invalid")?;
        let mime = resource
            .get("mime")
            .and_then(Value::as_str)
            .ok_or("safe_preview_resource_identity_invalid")?;
        if catalog
            .insert(resource_id.to_owned(), (index, mime.to_owned()))
            .is_some()
        {
            return Err("safe_preview_resource_duplicate");
        }
    }

    let nodes = scene
        .get("nodes")
        .and_then(Value::as_array)
        .ok_or("safe_preview_scene_nodes_invalid")?;
    let mut seen = HashSet::new();
    let mut total_decoded = 0usize;
    let mut admitted_urls = Vec::new();

    for candidate in candidates {
        let Some(&(index, ref expected_mime)) = catalog.get(&candidate.resource_id) else {
            return Err("safe_preview_resource_unbound");
        };
        if !seen.insert(candidate.resource_id.clone())
            || expected_mime != &candidate.mime
            || codec(&candidate.mime).is_none()
            || candidate.decoded.mime_type != candidate.mime
            || candidate.samples_base64.len() > MAX_PREVIEW_SAMPLE_BYTES.div_ceil(3) * 4
        {
            return Err("safe_preview_candidate_invalid");
        }
        let has_scene_use = nodes.iter().any(|node| {
            node.get("resource_id").and_then(Value::as_str) == Some(candidate.resource_id.as_str())
                || node
                    .get("decorative_border")
                    .and_then(|border| border.get("placements"))
                    .and_then(Value::as_array)
                    .is_some_and(|placements| {
                        placements.iter().any(|placement| {
                            placement.get("resource_id").and_then(Value::as_str)
                                == Some(candidate.resource_id.as_str())
                        })
                    })
        });
        if !has_scene_use {
            continue;
        }

        let samples = BASE64
            .decode(candidate.samples_base64.as_bytes())
            .map_err(|_| "safe_preview_samples_base64_invalid")?;
        total_decoded = total_decoded
            .checked_add(samples.len())
            .ok_or("safe_preview_samples_limit")?;
        if total_decoded > MAX_PREVIEW_SAMPLE_BYTES {
            return Err("safe_preview_samples_limit");
        }
        let mut decoded = candidate.decoded.clone();
        decoded.samples = samples;
        let preview = encode_safe_preview_v1(
            &decoded,
            codec(&candidate.mime).ok_or("safe_preview_candidate_invalid")?,
            &PreviewEncodeLimitsV1::default(),
        )
        .map_err(|_| "safe_preview_source_samples_invalid")?;
        if preview.receipt.encode_spec_sha256 != candidate.encode_spec_sha256
            || preview.receipt.encoded_artifact_sha256 != candidate.encoded_artifact_sha256
            || preview.receipt.source_resource_sha256 != decoded.resource_sha256
            || preview.receipt.source_sample_sha256 != decoded.sample_digest_sha256
            || preview.receipt.browser_delivery_authorized
        {
            return Err("safe_preview_receipt_mismatch");
        }
        // The hash is of the fresh host-encoded SafePreview derivative;
        // never reuse the source PUB container or an untrusted worker URL.
        let artifact_sha256 = preview.receipt.encoded_artifact_sha256.clone();
        let derivative_id = format!("safe-preview:{}", preview.receipt.encode_spec_sha256);
        let byte_len = preview.bytes.len();
        let url = format!(
            "data:{};base64,{}",
            preview.receipt.mime,
            BASE64.encode(&preview.bytes)
        );
        admitted_urls.push((index, url, artifact_sha256, derivative_id, byte_len));
    }

    let mut admitted_count = 0usize;
    for (index, url, artifact_sha256, derivative_id, byte_len) in admitted_urls {
        let old = scene["resources"][index].clone();
        scene["resources"][index]["availability"] = json!("inline_data_url");
        scene["resources"][index]["inline_data_url"] = json!(url);
        scene["resources"][index]["encoded_artifact_sha256"] = json!(artifact_sha256);
        scene["resources"][index]["derivative_id"] = json!(derivative_id);
        scene["resources"][index]["byte_len"] = json!(byte_len);
        let serialized =
            serde_json::to_vec(&*scene).map_err(|_| "safe_preview_scene_serialization_failed")?;
        // Leave headroom for the final partial-fidelity diagnostic.
        if serialized.len() > MAX_SCENE_BYTES.saturating_sub(256) {
            scene["resources"][index] = old;
        } else {
            admitted_count += 1;
        }
    }

    if admitted_count > 0 {
        let all_inline = scene["resources"].as_array().is_some_and(|resources| {
            resources
                .iter()
                .all(|resource| resource["inline_data_url"].is_string())
        });
        let reasons = scene["fidelity"]["reasons"]
            .as_array_mut()
            .ok_or("safe_preview_fidelity_invalid")?;
        if all_inline {
            reasons.retain(|value| value.as_str() != Some("image_resource_not_inline"));
        }
        if !reasons
            .iter()
            .any(|value| value.as_str() == Some(PREVIEW_COLOR_REASON))
        {
            reasons.push(json!(PREVIEW_COLOR_REASON));
        }
        scene["fidelity"]["state"] = json!("partial");
    }

    if serde_json::to_vec(&*scene)
        .map_err(|_| "safe_preview_scene_serialization_failed")?
        .len()
        > MAX_SCENE_BYTES
    {
        return Err("safe_preview_scene_bytes_limit");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jpeg_decoder_diagnostics_never_echo_untrusted_error_details() {
        assert_eq!(
            safe_jpeg_decode_stage_code("jpeg_header_format"),
            "jpeg_header_invalid_format"
        );
        assert_eq!(
            safe_jpeg_decode_stage_code("jpeg_pixels_format"),
            "jpeg_pixels_invalid_format"
        );
        assert_eq!(
            safe_jpeg_decode_stage_code("jpeg_pixels_io_eof"),
            "jpeg_pixels_io_eof"
        );
        assert_eq!(
            safe_jpeg_decode_stage_code("jpeg_pixels_internal"),
            "jpeg_pixels_internal"
        );
        // Even a forged decoder detail cannot appear in the receipt.
        assert_eq!(
            safe_jpeg_decode_stage_code("jpeg_pixels: MARKER-CANARY"),
            "jpeg_stage_unknown"
        );
    }

    fn source_png() -> Vec<u8> {
        let mut result = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut result, 2, 2);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            writer
                .write_image_data(&[255, 0, 0, 255, 0, 255, 0, 128, 0, 0, 255, 200, 1, 2, 3, 255])
                .unwrap();
        }
        result
    }

    fn scene_with_hostile_worker_url() -> Value {
        json!({
            "fidelity": {"state": "partial", "reasons": ["image_resource_not_inline"]},
            "nodes": [{"resource_id": "image-a"}],
            "resources": [{
                "resource_id": "image-a",
                "mime": "image/png",
                "availability": "descriptor_only"
            }]
        })
    }

    #[test]
    fn host_freshly_encodes_bounded_worker_pixels() {
        let original = source_png();
        let candidates = collect_worker_preview_samples(std::iter::once((
            "image-a".to_owned(),
            "image/png",
            original.as_slice(),
        )));
        assert_eq!(candidates.len(), 1);
        let mut scene = scene_with_hostile_worker_url();
        admit_host_reencoded_previews(&mut scene, &candidates).unwrap();
        assert_eq!(scene["resources"][0]["availability"], "inline_data_url");
        assert!(
            scene["resources"][0]["inline_data_url"]
                .as_str()
                .unwrap()
                .starts_with("data:image/png;base64,")
        );
        let data_url = scene["resources"][0]["inline_data_url"].as_str().unwrap();
        let encoded = BASE64.decode(data_url.split_once(',').unwrap().1).unwrap();
        let artifact_sha256 = sha256_hex(&encoded);
        assert_eq!(
            scene["resources"][0]["encoded_artifact_sha256"].as_str(),
            Some(artifact_sha256.as_str())
        );
        assert_eq!(
            scene["resources"][0]["encoded_artifact_sha256"].as_str(),
            Some(candidates[0].encoded_artifact_sha256.as_str())
        );
        let derivative = format!("safe-preview:{}", candidates[0].encode_spec_sha256);
        assert_eq!(
            scene["resources"][0]["derivative_id"].as_str(),
            Some(derivative.as_str())
        );
        assert_eq!(
            scene["resources"][0]["byte_len"].as_u64(),
            Some(encoded.len() as u64)
        );
        assert_eq!(scene["fidelity"]["state"], "partial");
        assert!(
            scene["fidelity"]["reasons"]
                .as_array()
                .unwrap()
                .iter()
                .any(|r| r == PREVIEW_COLOR_REASON)
        );
    }

    #[test]
    fn synthetic_adobe_cmyk_one_resource_four_frames_passes_worker_host_boundary() {
        // Generated 2x2 synthetic Adobe-CMYK JPEG (no ICC or user image bytes).
        // Reproduces the SBDC storage topology: one picture resource, four
        // separate page placements. It does NOT substitute for a real-PUB proof.
        let source = BASE64.decode("/9j/7gAOQWRvYmUAZAAAAAAA/9sAQwACAQECAQECAgICAgICAgMFAwMDAwMGBAQDBQcGBwcHBgcHCAkLCQgICggHBwoNCgoLDAwMDAcJDg8NDA4LDAwM/8AAFAgAAgACBEMRAE0RAFkRAEsRAP/EABQAAQAAAAAAAAAAAAAAAAAAAAn/xAAdEAABBQEBAQEAAAAAAAAAAAACAQMEBQYHCQgA/9oADgRDAE0AWQBLAAA/ADu9NvTb6SwXpJ9B0VF9B9wpaSl6Vo4FfXwN1aRosCO1aSQaZaaB9AbbABERAURBRERERE/G96E+hPfeCffXcMLhe4dgxeJxfQL6iz2eotlY11VRV0WxkMRoUSMy8LTEdlkAbbabEQAAERRERE/G96E+hPfeCffXcMLhe4dgxeJxfQL6iz2eotlY11VRV0WxkMRoUSMy8LTEdlkAbbabEQAAERRERE/MZ5k6y13vm38+Xt7Z2F1d3XNc5PsLCfIOTKnyHauMbrzrpqpuOGZERGSqpKqqqqq/v//Z").unwrap();
        let (candidates, audit) = collect_worker_preview_samples_with_audit(std::iter::once((
            "star-image".to_owned(),
            "image/jpeg",
            source.as_slice(),
        )));
        assert_eq!(audit.input_resources, 1);
        assert_eq!(audit.eligible_mime_resources, 1);
        assert_eq!(audit.decoded_resources, 1);
        assert_eq!(audit.encoded_candidates, 1);
        assert!(audit.reject_codes.is_empty());
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].decoded.source_sample_model, "jpeg_adobe_cmyk");
        assert_eq!(candidates[0].decoded.decoded_sample_model, "rgb");
        assert!(
            candidates[0]
                .decoded
                .color_disposition_ref
                .contains("adobe-cmyk0-unprofiled-rgb-v1")
        );

        let frames = (0..4)
            .map(|i| {
                json!({
                    "node_id": format!("picture-frame-{i}"),
                    "page_id": "page-1",
                    "kind": "picture_frame",
                    "resource_id": "star-image",
                    "bounds": {"x": i * 1_000, "y": (i % 2) * 2_000,
                               "width": 900, "height": 900}
                })
            })
            .collect::<Vec<_>>();
        let mut scene = json!({
            "fidelity": {"state": "partial", "reasons": ["image_resource_not_inline"]},
            "nodes": frames,
            "resources": [{
                "resource_id": "star-image",
                "mime": "image/jpeg",
                "availability": "descriptor_only"
            }]
        });
        let original_nodes = scene["nodes"].clone();
        admit_host_reencoded_previews(&mut scene, &candidates).unwrap();
        assert_eq!(scene["nodes"], original_nodes);
        assert_eq!(scene["nodes"].as_array().unwrap().len(), 4);
        assert!(
            scene["nodes"]
                .as_array()
                .unwrap()
                .iter()
                .all(|node| node["resource_id"] == "star-image")
        );
        assert_eq!(scene["resources"].as_array().unwrap().len(), 1);
        assert_eq!(scene["resources"][0]["availability"], "inline_data_url");
        let url = scene["resources"][0]["inline_data_url"].as_str().unwrap();
        assert!(url.starts_with("data:image/jpeg;base64,"));
        let derivative_bytes = BASE64.decode(url.split_once(',').unwrap().1).unwrap();
        assert_ne!(derivative_bytes, source);
        assert_eq!(
            scene["resources"][0]["encoded_artifact_sha256"],
            sha256_hex(&derivative_bytes)
        );
        assert!(
            scene["resources"][0]["derivative_id"]
                .as_str()
                .unwrap()
                .starts_with("safe-preview:")
        );
        assert_eq!(scene["fidelity"]["state"], "partial");
        assert!(
            scene["fidelity"]["reasons"]
                .as_array()
                .unwrap()
                .iter()
                .any(|reason| reason == PREVIEW_COLOR_REASON)
        );
        assert!(
            !scene["fidelity"]["reasons"]
                .as_array()
                .unwrap()
                .iter()
                .any(|reason| reason == "image_resource_not_inline")
        );
    }

    #[test]
    fn host_rejects_tampered_samples_and_cross_resource_replay() {
        let original = source_png();
        let candidates = collect_worker_preview_samples(std::iter::once((
            "image-a".to_owned(),
            "image/png",
            original.as_slice(),
        )));
        let mut tampered = candidates.clone();
        tampered[0].samples_base64 = BASE64.encode(b"not the validated pixels");
        assert!(
            admit_host_reencoded_previews(&mut scene_with_hostile_worker_url(), &tampered).is_err()
        );

        let mut replayed = candidates;
        replayed[0].resource_id = "image-from-other-document".to_owned();
        assert_eq!(
            admit_host_reencoded_previews(&mut scene_with_hostile_worker_url(), &replayed),
            Err("safe_preview_resource_unbound")
        );
    }

    #[test]
    fn malformed_images_consume_decode_attempt_budget() {
        let invalid_png: &[u8] = b"not a valid png";
        let (candidates, audit) = collect_worker_preview_samples_with_audit(
            (0..MAX_PREVIEW_DECODE_ATTEMPTS + 3)
                .map(|i| (format!("malformed-{i}"), "image/png", invalid_png)),
        );
        assert!(candidates.is_empty());
        assert_eq!(audit.decoded_resources, 0);
        assert_eq!(
            audit.reject_codes.get("preview_decode_attempt_budget"),
            Some(&3)
        );
    }

    #[test]
    fn malformed_images_consume_aggregate_encoded_input_budget() {
        // One source buffer is reused: no large fixture or source bytes leave
        // the confined worker. The ninth 8 MiB decode must not be attempted.
        let invalid_png = vec![0u8; MAX_ORIGINAL_CONTAINER_BYTES];
        let (candidates, audit) = collect_worker_preview_samples_with_audit(
            (0..MAX_PREVIEW_DECODE_INPUT_BYTES / MAX_ORIGINAL_CONTAINER_BYTES + 2).map(|i| {
                (
                    format!("large-malformed-{i}"),
                    "image/png",
                    invalid_png.as_slice(),
                )
            }),
        );
        assert!(candidates.is_empty());
        assert_eq!(audit.decoded_resources, 0);
        assert_eq!(
            audit.reject_codes.get("preview_decode_input_budget"),
            Some(&2)
        );
    }

    #[test]
    fn unsupported_worker_containers_never_produce_candidates() {
        let data = b"untrusted-container";
        assert!(
            collect_worker_preview_samples(std::iter::once((
                "image-a".to_owned(),
                "image/svg+xml",
                data.as_slice(),
            )))
            .is_empty()
        );
    }
}
