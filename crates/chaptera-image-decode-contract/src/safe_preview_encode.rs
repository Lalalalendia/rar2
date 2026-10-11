//! Deterministic, bounded *candidate* encoding from reference-decoded pixels.
//! No original PNG/JPEG container bytes are reused. This module is not a
//! browser-delivery permission: isolated execution and an independent host
//! admission/re-encode step remain mandatory.

use std::fmt;

use jpeg_encoder::{ColorType as JpegColorType, Encoder as JpegEncoder};
use png::{BitDepth, ColorType as PngColorType, Encoder as PngEncoder};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::{
    DECODE_CONTRACT_VERSION_V1, DecodeLimitsV1, DecodePolicyV1, DecodedImageV1,
    REFERENCE_DECODER_ID_V1, decode_image_v1,
};

pub const SAFE_PREVIEW_ENCODE_V1: &str = "chaptera.safe-preview-encode.v1";
pub const PNG_ENCODER_ID_V1: &str = "png:0.18.1/8bit/metadata-free";
pub const JPEG_ENCODER_ID_V1: &str = "jpeg-encoder:0.7.1/quality-ladder-v1/no-app-markers";
pub const JPEG_QUALITY_LADDER_V1: &[u8] = &[93, 88, 82, 75, 68, 60];
pub const PREVIEW_COLOR_DISPOSITION_V1: &str = "decoded-samples/no-icc-transform/partial-fidelity";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewCodecV1 {
    Png,
    Jpeg,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewEncodeLimitsV1 {
    pub max_dimension_px: u32,
    pub max_total_pixels: u64,
    pub max_decoded_bytes: usize,
    pub max_png_bytes: usize,
    pub max_jpeg_bytes: usize,
}

impl Default for PreviewEncodeLimitsV1 {
    fn default() -> Self {
        Self {
            max_dimension_px: 8192,
            max_total_pixels: 16_000_000,
            max_decoded_bytes: 48 * 1024 * 1024,
            max_png_bytes: 4 * 1024 * 1024,
            max_jpeg_bytes: 5 * 1024 * 1024,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewEncodeErrorV1 {
    pub code: &'static str,
}

impl fmt::Display for PreviewEncodeErrorV1 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code)
    }
}

impl std::error::Error for PreviewEncodeErrorV1 {}

fn reject(code: &'static str) -> PreviewEncodeErrorV1 {
    PreviewEncodeErrorV1 { code }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SafePreviewEncodeSpecV1 {
    pub protocol_version: &'static str,
    pub source_decode_contract: &'static str,
    pub source_decoder_id: &'static str,
    pub source_sample_sha256: String,
    pub source_color_disposition_sha256: String,
    pub container_mime: &'static str,
    pub encoder_id: &'static str,
    pub quality: Option<u8>,
    pub orientation_policy: &'static str,
    pub alpha_policy: &'static str,
    pub metadata_policy: &'static str,
    pub output_color_disposition: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SafePreviewEncodeReceiptV1 {
    pub protocol_version: &'static str,
    pub source_resource_sha256: String,
    pub source_sample_sha256: String,
    pub encode_spec_sha256: String,
    pub encoded_artifact_sha256: String,
    pub encoded_byte_len: usize,
    pub dimensions_px: [u32; 2],
    pub mime: &'static str,
    pub encoder_id: &'static str,
    pub quality: Option<u8>,
    pub alpha_preserved: bool,
    pub orientation_applied: bool,
    pub source_metadata_carried: bool,
    pub output_color_disposition: &'static str,
    pub visual_fidelity: &'static str,
    // A worker-produced candidate is NOT authorized for browser consumption.
    pub browser_delivery_authorized: bool,
}

#[derive(Debug, Clone)]
pub struct EncodedSafePreviewV1 {
    pub receipt: SafePreviewEncodeReceiptV1,
    /// Newly encoded bytes. Never insert into ReaderScene without independent
    /// host validation/derivative authority and normal scene byte quotas.
    pub bytes: Vec<u8>,
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn canonical_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn sample_layout(
    decoded: &DecodedImageV1,
    limits: &PreviewEncodeLimitsV1,
) -> Result<(u32, u32, PngColorType, Option<JpegColorType>), PreviewEncodeErrorV1> {
    if decoded.contract_version != DECODE_CONTRACT_VERSION_V1
        || decoded.decoder_id != REFERENCE_DECODER_ID_V1
        || !canonical_hash(&decoded.resource_sha256)
        || !canonical_hash(&decoded.sample_digest_sha256)
        || decoded.sample_digest_sha256 != sha256_hex(&decoded.samples)
        || decoded.frame_disposition != "single_static"
        || decoded.orientation_applied
        || decoded.decoded_precision_bits != 8
        || decoded.color_disposition_ref.is_empty()
    {
        return Err(reject("preview_source_decode_contract_invalid"));
    }

    let width = decoded.decoded_width_px;
    let height = decoded.decoded_height_px;
    let pixels = u64::from(width)
        .checked_mul(u64::from(height))
        .ok_or_else(|| reject("preview_pixel_arithmetic_overflow"))?;
    if width == 0
        || height == 0
        || width > limits.max_dimension_px
        || height > limits.max_dimension_px
        || pixels > limits.max_total_pixels
    {
        return Err(reject("preview_pixel_limit"));
    }

    let (channels, png_color, jpeg_color, alpha) = match decoded.decoded_sample_model.as_str() {
        "gray" => (
            1_u64,
            PngColorType::Grayscale,
            Some(JpegColorType::Luma),
            false,
        ),
        "rgb" => (3, PngColorType::Rgb, Some(JpegColorType::Rgb), false),
        "gray_alpha" => (2, PngColorType::GrayscaleAlpha, None, true),
        "rgba" => (4, PngColorType::Rgba, None, true),
        _ => return Err(reject("preview_sample_model_unsupported")),
    };
    let expected_len = pixels
        .checked_mul(channels)
        .and_then(|bytes| usize::try_from(bytes).ok())
        .ok_or_else(|| reject("preview_pixel_arithmetic_overflow"))?;
    if expected_len > limits.max_decoded_bytes
        || decoded.samples.len() != expected_len
        || decoded.decoded_byte_count != expected_len
        || decoded.alpha_present != alpha
        || (alpha && decoded.alpha_association != "straight_unassociated")
        || (!alpha && decoded.alpha_association != "none")
    {
        return Err(reject("preview_samples_invalid"));
    }
    Ok((width, height, png_color, jpeg_color))
}

fn encode_png(
    decoded: &DecodedImageV1,
    width: u32,
    height: u32,
    color: PngColorType,
) -> Result<Vec<u8>, PreviewEncodeErrorV1> {
    let mut bytes = Vec::new();
    {
        let mut encoder = PngEncoder::new(&mut bytes, width, height);
        encoder.set_color(color);
        encoder.set_depth(BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .map_err(|_| reject("preview_png_encode_failed"))?;
        writer
            .write_image_data(&decoded.samples)
            .map_err(|_| reject("preview_png_encode_failed"))?;
    }
    Ok(bytes)
}

fn encode_jpeg(
    decoded: &DecodedImageV1,
    width: u32,
    height: u32,
    color: JpegColorType,
    quality: u8,
) -> Result<Vec<u8>, PreviewEncodeErrorV1> {
    let w = u16::try_from(width).map_err(|_| reject("preview_pixel_limit"))?;
    let h = u16::try_from(height).map_err(|_| reject("preview_pixel_limit"))?;
    let mut bytes = Vec::new();
    let encoder = JpegEncoder::new(&mut bytes, quality);
    encoder
        .encode(&decoded.samples, w, h, color)
        .map_err(|_| reject("preview_jpeg_encode_failed"))?;
    Ok(bytes)
}

fn finalize_candidate(
    decoded: &DecodedImageV1,
    mime: &'static str,
    encoder_id: &'static str,
    quality: Option<u8>,
    bytes: Vec<u8>,
    limits: &PreviewEncodeLimitsV1,
) -> Result<EncodedSafePreviewV1, PreviewEncodeErrorV1> {
    let output_cap = if mime == "image/png" {
        limits.max_png_bytes
    } else {
        limits.max_jpeg_bytes
    };
    if bytes.is_empty() || bytes.len() > output_cap {
        return Err(reject("preview_encoded_bytes_limit"));
    }

    // Roundtrip a NEW container through the pinned reference decoder. It
    // cannot import arbitrary source EXIF/XMP/ICC/filename metadata, because
    // encoder inputs were pixel arrays only.
    let encoded_hash = sha256_hex(&bytes);
    let decoded_output = decode_image_v1(
        &bytes,
        mime,
        &encoded_hash,
        "safe-preview-untransformed-display-samples",
        &DecodePolicyV1::reference(),
        &DecodeLimitsV1 {
            max_encoded_bytes: output_cap,
            max_dimension_px: limits.max_dimension_px,
            max_total_pixels: limits.max_total_pixels,
            max_decoded_bytes: limits.max_decoded_bytes,
        },
    )
    .map_err(|_| reject("preview_encoded_verification_failed"))?;
    if decoded_output.decoded_width_px != decoded.decoded_width_px
        || decoded_output.decoded_height_px != decoded.decoded_height_px
        || decoded_output.decoded_precision_bits != 8
        || decoded_output.alpha_present != decoded.alpha_present
        || (mime == "image/png" && decoded_output.samples != decoded.samples)
    {
        return Err(reject("preview_encoded_verification_failed"));
    }

    let spec = SafePreviewEncodeSpecV1 {
        protocol_version: SAFE_PREVIEW_ENCODE_V1,
        source_decode_contract: DECODE_CONTRACT_VERSION_V1,
        source_decoder_id: REFERENCE_DECODER_ID_V1,
        source_sample_sha256: decoded.sample_digest_sha256.clone(),
        source_color_disposition_sha256: sha256_hex(decoded.color_disposition_ref.as_bytes()),
        container_mime: mime,
        encoder_id,
        quality,
        orientation_policy: "source-raster-unrotated;exif-stripped",
        alpha_policy: if decoded.alpha_present {
            "straight-alpha-preserved"
        } else {
            "opaque-no-alpha"
        },
        metadata_policy: "fresh-container-no-source-metadata",
        output_color_disposition: PREVIEW_COLOR_DISPOSITION_V1,
    };
    let spec_bytes =
        serde_json::to_vec(&spec).map_err(|_| reject("preview_encode_spec_invalid"))?;
    let spec_hash = sha256_hex(&spec_bytes);
    Ok(EncodedSafePreviewV1 {
        receipt: SafePreviewEncodeReceiptV1 {
            protocol_version: SAFE_PREVIEW_ENCODE_V1,
            source_resource_sha256: decoded.resource_sha256.clone(),
            source_sample_sha256: decoded.sample_digest_sha256.clone(),
            encode_spec_sha256: spec_hash,
            encoded_artifact_sha256: encoded_hash,
            encoded_byte_len: bytes.len(),
            dimensions_px: [decoded.decoded_width_px, decoded.decoded_height_px],
            mime,
            encoder_id,
            quality,
            alpha_preserved: decoded.alpha_present,
            orientation_applied: false,
            source_metadata_carried: false,
            output_color_disposition: PREVIEW_COLOR_DISPOSITION_V1,
            visual_fidelity: "partial_color_untransformed",
            browser_delivery_authorized: false,
        },
        bytes,
    })
}

/// A pure encoding component for a future isolated-worker derived-image
/// pipeline. It never accepts original source containers, and never grants
/// browser-delivery authority to its result.
pub fn encode_safe_preview_v1(
    decoded: &DecodedImageV1,
    codec: PreviewCodecV1,
    limits: &PreviewEncodeLimitsV1,
) -> Result<EncodedSafePreviewV1, PreviewEncodeErrorV1> {
    let (width, height, png_color, jpeg_color) = sample_layout(decoded, limits)?;
    match codec {
        PreviewCodecV1::Png => {
            let bytes = encode_png(decoded, width, height, png_color)?;
            finalize_candidate(decoded, "image/png", PNG_ENCODER_ID_V1, None, bytes, limits)
        }
        PreviewCodecV1::Jpeg => {
            let color = jpeg_color.ok_or_else(|| reject("preview_jpeg_alpha_unsupported"))?;
            for &quality in JPEG_QUALITY_LADDER_V1 {
                let bytes = encode_jpeg(decoded, width, height, color, quality)?;
                if bytes.len() <= limits.max_jpeg_bytes {
                    return finalize_candidate(
                        decoded,
                        "image/jpeg",
                        JPEG_ENCODER_ID_V1,
                        Some(quality),
                        bytes,
                        limits,
                    );
                }
            }
            Err(reject("preview_encoded_bytes_limit"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jpeg_encoder::{ColorType as JpegColor, Encoder as JpegEncoder};

    fn fixture_png(width: u32, height: u32, rgba: bool, pixels: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut encoder = PngEncoder::new(&mut out, width, height);
            encoder.set_color(if rgba {
                PngColorType::Rgba
            } else {
                PngColorType::Rgb
            });
            encoder.set_depth(BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(pixels).unwrap();
        }
        out
    }

    fn fixture_decoded(bytes: &[u8], mime: &str) -> DecodedImageV1 {
        decode_image_v1(
            bytes,
            mime,
            &sha256_hex(bytes),
            "sample-color-source",
            &DecodePolicyV1::reference(),
            &DecodeLimitsV1::default(),
        )
        .unwrap()
    }

    #[test]
    fn png_alpha_is_fresh_metadata_free_and_deterministic() {
        let input_pixels = [16, 32, 48, 100, 64, 80, 96, 200];
        let mut original = fixture_png(2, 1, true, &input_pixels);
        // Admitted original decoder may tolerate trailing bytes; never copy
        // those private source bytes into a new preview.
        original.extend_from_slice(b"source-private-exif-gps-canary");
        let decoded = fixture_decoded(&original, "image/png");
        let limits = PreviewEncodeLimitsV1::default();
        let a = encode_safe_preview_v1(&decoded, PreviewCodecV1::Png, &limits).unwrap();
        let b = encode_safe_preview_v1(&decoded, PreviewCodecV1::Png, &limits).unwrap();
        assert_eq!(a.bytes, b.bytes);
        assert_eq!(a.receipt, b.receipt);
        assert!(
            !a.bytes
                .windows(25)
                .any(|window| window == b"source-private-exif-gps-c")
        );
        assert_eq!(a.receipt.encoded_artifact_sha256, sha256_hex(&a.bytes));
        assert_ne!(
            a.receipt.source_resource_sha256,
            a.receipt.encoded_artifact_sha256
        );
        assert!(a.receipt.alpha_preserved);
        assert!(!a.receipt.browser_delivery_authorized);
        assert!(!a.receipt.source_metadata_carried);
        let clean = fixture_decoded(&a.bytes, "image/png");
        assert_eq!(clean.samples, input_pixels);
    }

    #[test]
    fn jpeg_quality_ladder_selects_first_fitting_size_and_changes_identity() {
        let mut pixels = Vec::new();
        for y in 0..128_u32 {
            for x in 0..128_u32 {
                pixels.extend_from_slice(&[
                    (x.wrapping_mul(97).wrapping_add(y.wrapping_mul(23)) % 256) as u8,
                    (x.wrapping_mul(13).wrapping_add(y.wrapping_mul(89)) % 256) as u8,
                    (x.wrapping_mul(51).wrapping_add(y.wrapping_mul(37)) % 256) as u8,
                ]);
            }
        }
        let original = fixture_png(128, 128, false, &pixels);
        let decoded = fixture_decoded(&original, "image/png");
        let first = encode_safe_preview_v1(
            &decoded,
            PreviewCodecV1::Jpeg,
            &PreviewEncodeLimitsV1::default(),
        )
        .unwrap();
        assert_eq!(first.receipt.quality, Some(93));
        let q88 = encode_jpeg(&decoded, 128, 128, JpegColorType::Rgb, 88).unwrap();
        assert!(first.bytes.len() > q88.len());
        let limits = PreviewEncodeLimitsV1 {
            max_jpeg_bytes: q88.len(),
            ..PreviewEncodeLimitsV1::default()
        };
        let smaller = encode_safe_preview_v1(&decoded, PreviewCodecV1::Jpeg, &limits).unwrap();
        assert!(smaller.receipt.quality.unwrap() < 93);
        assert!(smaller.bytes.len() <= limits.max_jpeg_bytes);
        assert_ne!(
            smaller.receipt.encode_spec_sha256,
            first.receipt.encode_spec_sha256
        );
        assert_ne!(
            smaller.receipt.encoded_artifact_sha256,
            first.receipt.encoded_artifact_sha256
        );
        assert_eq!(
            smaller.bytes,
            encode_safe_preview_v1(&decoded, PreviewCodecV1::Jpeg, &limits)
                .unwrap()
                .bytes
        );
        let impossible = PreviewEncodeLimitsV1 {
            max_jpeg_bytes: 8,
            ..limits
        };
        assert_eq!(
            encode_safe_preview_v1(&decoded, PreviewCodecV1::Jpeg, &impossible)
                .unwrap_err()
                .code,
            "preview_encoded_bytes_limit"
        );
    }

    #[test]
    fn jpeg_exif_is_not_copied_to_fresh_pixel_derived_container() {
        let mut original = Vec::new();
        let pixels = [5_u8, 70, 180, 12, 130, 230, 38, 90, 0, 200, 123, 25];
        {
            let mut encoder = JpegEncoder::new(&mut original, 90);
            encoder
                .add_exif_metadata(&[
                    73, 73, 42, 0, 8, 0, 0, 0, 1, 0, 18, 1, 3, 0, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0,
                    0,
                ])
                .unwrap();
            encoder.encode(&pixels, 2, 2, JpegColor::Rgb).unwrap();
        }
        let decoded = fixture_decoded(&original, "image/jpeg");
        assert_eq!(decoded.exif_orientation, Some(1));
        let preview = encode_safe_preview_v1(
            &decoded,
            PreviewCodecV1::Jpeg,
            &PreviewEncodeLimitsV1::default(),
        )
        .unwrap();
        assert!(!preview.bytes.windows(6).any(|w| w == b"Exif\0\0"));
        assert!(!preview.receipt.orientation_applied);
        assert!(!preview.receipt.source_metadata_carried);
        assert_eq!(
            fixture_decoded(&preview.bytes, "image/jpeg").decoded_width_px,
            2
        );
    }

    #[test]
    fn alpha_jpeg_corrupt_samples_and_bombs_reject_without_fallback() {
        let pixels = [1_u8, 2, 3, 4];
        let source = fixture_png(1, 1, true, &pixels);
        let rgba = fixture_decoded(&source, "image/png");
        let defaults = PreviewEncodeLimitsV1::default();
        assert_eq!(
            encode_safe_preview_v1(&rgba, PreviewCodecV1::Jpeg, &defaults)
                .unwrap_err()
                .code,
            "preview_jpeg_alpha_unsupported"
        );
        let mut forged = rgba.clone();
        forged.samples[0] ^= 1;
        assert_eq!(
            encode_safe_preview_v1(&forged, PreviewCodecV1::Png, &defaults)
                .unwrap_err()
                .code,
            "preview_source_decode_contract_invalid"
        );
        let mut forged = rgba.clone();
        forged.decoded_width_px = 100_000;
        assert_eq!(
            encode_safe_preview_v1(&forged, PreviewCodecV1::Png, &defaults)
                .unwrap_err()
                .code,
            "preview_pixel_limit"
        );
        let tiny = PreviewEncodeLimitsV1 {
            max_png_bytes: 10,
            ..defaults
        };
        assert_eq!(
            encode_safe_preview_v1(&rgba, PreviewCodecV1::Png, &tiny)
                .unwrap_err()
                .code,
            "preview_encoded_bytes_limit"
        );
    }
}
