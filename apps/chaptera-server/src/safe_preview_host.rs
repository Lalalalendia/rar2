//! Host-owned browser admission for isolated-worker SafePreview candidates.
//! Never forwards the worker's PNG/JPEG container: it independently decodes
//! bounded pixels and emits a fresh metadata-free PNG from the host encoder.
//! This module is an admission primitive, not an authorization to expose PUB.

use base64::{Engine as _, engine::general_purpose::STANDARD};
use chaptera_image_decode_contract::{
    DecodeLimitsV1, DecodePolicyV1, decode_image_v1,
    safe_preview_encode::{
        PreviewCodecV1, PreviewEncodeLimitsV1, SafePreviewEncodeReceiptV1,
        encode_safe_preview_v1,
    },
};
use sha2::{Digest, Sha256};

pub(crate) const MAX_BROWSER_PREVIEW_DATA_URL_BYTES: usize = 6 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HostAdmittedPreview {
    pub mime: &'static str,
    pub data_url: String,
    pub encoded_len: usize,
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Reject malformed, oversized or inconsistent worker receipts BEFORE host
/// decode, then independently decode and encode fresh bytes. In particular,
/// a worker-controlled `browser_delivery_authorized` flag is never authority.
pub(crate) fn admit_worker_preview(
    bytes: &[u8],
    receipt: &SafePreviewEncodeReceiptV1,
) -> Result<HostAdmittedPreview, &'static str> {
    let limits = PreviewEncodeLimitsV1::default();
    let input_cap = match receipt.mime {
        "image/png" => limits.max_png_bytes,
        "image/jpeg" => limits.max_jpeg_bytes,
        _ => return Err("preview_mime_denied"),
    };
    if receipt.protocol_version != chaptera_image_decode_contract::safe_preview_encode::SAFE_PREVIEW_ENCODE_V1
        || receipt.encoded_byte_len != bytes.len()
        || bytes.is_empty()
        || bytes.len() > input_cap
        || receipt.encoded_artifact_sha256 != sha256_hex(bytes)
        || receipt.source_metadata_carried
        || receipt.orientation_applied
        || receipt.dimensions_px.contains(&0)
        || receipt.dimensions_px.iter().any(|&n| n > limits.max_dimension_px)
        || u64::from(receipt.dimensions_px[0]) * u64::from(receipt.dimensions_px[1])
            > limits.max_total_pixels
    {
        return Err("preview_receipt_denied");
    }
    let decoded = decode_image_v1(
        bytes,
        receipt.mime,
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
    let fresh = encode_safe_preview_v1(&decoded, PreviewCodecV1::Png, &limits)
        .map_err(|_| "preview_host_reencode_rejected")?;
    let data_url = format!("data:image/png;base64,{}", STANDARD.encode(&fresh.bytes));
    if data_url.len() > MAX_BROWSER_PREVIEW_DATA_URL_BYTES {
        return Err("preview_browser_budget_exceeded");
    }
    Ok(HostAdmittedPreview {
        mime: "image/png",
        data_url,
        encoded_len: fresh.bytes.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chaptera_image_decode_contract::safe_preview_encode::SAFE_PREVIEW_ENCODE_V1;

    #[test]
    fn rejects_worker_preview_with_forged_identity_without_decoding() {
        let receipt = SafePreviewEncodeReceiptV1 {
            protocol_version: SAFE_PREVIEW_ENCODE_V1,
            source_resource_sha256: "a".repeat(64),
            source_sample_sha256: "b".repeat(64),
            encode_spec_sha256: "c".repeat(64),
            encoded_artifact_sha256: "d".repeat(64),
            encoded_byte_len: 3,
            dimensions_px: [1, 1],
            mime: "image/png",
            encoder_id: "untrusted",
            quality: None,
            alpha_preserved: false,
            orientation_applied: false,
            source_metadata_carried: false,
            output_color_disposition: "none",
            visual_fidelity: "partial_color_untransformed",
            browser_delivery_authorized: true,
        };
        assert_eq!(
            admit_worker_preview(b"bad", &receipt),
            Err("preview_receipt_denied")
        );
    }
}
