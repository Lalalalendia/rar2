//! Desktop adapter from the bounded image decode contract to egui textures.
//!
//! Decoder policy stays in `chaptera-image-decode-contract`. This module owns only
//! admission of decoded samples into the desktop RGBA8 texture representation.

use chaptera_image_decode_contract::{
    DecodeLimitsV1, DecodePolicyV1, DecodedImageV1, decode_image_v1,
};
use eframe::egui;
use pub_model::Sha256Digest;
use pub_viewer::ViewerEmbeddedImage;
use sha2::{Digest, Sha256};
use std::fmt;

pub const DESKTOP_COLOR_DISPOSITION_REF_V1: &str = "chaptera.desktop.egui.rgba8.v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopImageDecodeDiagnostic {
    pub resource_key: String,
    pub mime: String,
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopImageDecodeError {
    pub code: String,
    pub detail: String,
}

impl fmt::Display for DesktopImageDecodeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.detail)
    }
}

impl std::error::Error for DesktopImageDecodeError {}

#[derive(Debug, Clone)]
pub struct AdmittedTextureImage {
    pub color_image: egui::ColorImage,
    pub cache_identity_sha256: String,
}

pub fn exact_sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

// The Reader manifest validated the exact extracted source bytes already.
// This function must NOT hash those bytes to invent its own expected value.
// The single actual-byte integrity hash remains in decode_image_v1.
//
// Non-exact WMF/OLE preview PNGs have no independent source-PNG hash; keep
// their previous bounded compatibility path instead of promoting preview
// identity into exact-image authority.
fn expected_viewer_image_sha256_with_v1(
    source_exact: bool,
    verified_source_sha256: Option<Sha256Digest>,
    bytes: &[u8],
    derive_preview_sha256: impl FnOnce(&[u8]) -> String,
) -> Result<String, DesktopImageDecodeError> {
    match (source_exact, verified_source_sha256) {
        (true, Some(digest)) => Ok(digest.to_string()),
        (true, None) => Err(DesktopImageDecodeError {
            code: "missing_verified_source_image_hash".to_owned(),
            detail: "source-exact Viewer image has no independently verified Reader SHA-256"
                .to_owned(),
        }),
        (false, Some(_)) => Err(DesktopImageDecodeError {
            code: "preview_claims_verified_source_hash".to_owned(),
            detail: "derived Viewer preview cannot carry exact source-image hash authority"
                .to_owned(),
        }),
        (false, None) => Ok(derive_preview_sha256(bytes)),
    }
}

pub fn decode_viewer_embedded_texture_v1(
    image: &ViewerEmbeddedImage,
) -> Result<AdmittedTextureImage, DesktopImageDecodeError> {
    let expected_sha256 = expected_viewer_image_sha256_with_v1(
        image.source_exact,
        image.verified_source_sha256,
        &image.bytes,
        exact_sha256_hex,
    )?;
    decode_texture_image_v1(&image.bytes, &image.mime, &expected_sha256)
}

pub fn decode_texture_image_v1(
    bytes: &[u8],
    mime: &str,
    expected_sha256: &str,
) -> Result<AdmittedTextureImage, DesktopImageDecodeError> {
    let decoded = decode_image_v1(
        bytes,
        mime,
        expected_sha256,
        DESKTOP_COLOR_DISPOSITION_REF_V1,
        &DecodePolicyV1::reference(),
        &DecodeLimitsV1::default(),
    )
    .map_err(|error| DesktopImageDecodeError {
        code: error.code.to_owned(),
        detail: error.detail,
    })?;

    admit_decoded_image_v1(&decoded)
}

pub fn diagnostic_for(
    resource_key: impl Into<String>,
    mime: impl Into<String>,
    error: &DesktopImageDecodeError,
) -> DesktopImageDecodeDiagnostic {
    DesktopImageDecodeDiagnostic {
        resource_key: resource_key.into(),
        mime: mime.into(),
        code: error.code.clone(),
        message: error.detail.clone(),
    }
}

fn admit_decoded_image_v1(
    decoded: &DecodedImageV1,
) -> Result<AdmittedTextureImage, DesktopImageDecodeError> {
    if decoded.decoded_precision_bits != 8 {
        return Err(DesktopImageDecodeError {
            code: "desktop_precision_unsupported".to_owned(),
            detail: format!(
                "egui texture adapter admits only 8-bit decoded samples; found {}-bit",
                decoded.decoded_precision_bits
            ),
        });
    }
    if decoded.orientation_class != "normal" || decoded.orientation_applied {
        return Err(DesktopImageDecodeError {
            code: "desktop_orientation_unsupported".to_owned(),
            detail: format!(
                "desktop texture adapter does not silently apply orientation {:?}",
                decoded.orientation_class
            ),
        });
    }

    let width = usize::try_from(decoded.decoded_width_px).map_err(|_| DesktopImageDecodeError {
        code: "desktop_dimension_overflow".to_owned(),
        detail: "decoded width does not fit desktop address space".to_owned(),
    })?;
    let height =
        usize::try_from(decoded.decoded_height_px).map_err(|_| DesktopImageDecodeError {
            code: "desktop_dimension_overflow".to_owned(),
            detail: "decoded height does not fit desktop address space".to_owned(),
        })?;
    let pixels = width
        .checked_mul(height)
        .ok_or_else(|| DesktopImageDecodeError {
            code: "desktop_dimension_overflow".to_owned(),
            detail: "decoded pixel count overflows desktop address space".to_owned(),
        })?;

    let channels = match decoded.decoded_sample_model.as_str() {
        "gray" => 1,
        "gray_alpha" => 2,
        "rgb" => 3,
        "rgba" => 4,
        other => {
            return Err(DesktopImageDecodeError {
                code: "desktop_sample_model_unsupported".to_owned(),
                detail: format!(
                    "decoded sample model {other:?} is not admitted by the egui adapter"
                ),
            });
        }
    };
    let expected_len = pixels
        .checked_mul(channels)
        .ok_or_else(|| DesktopImageDecodeError {
            code: "desktop_sample_length_overflow".to_owned(),
            detail: "decoded sample length overflows desktop address space".to_owned(),
        })?;
    if decoded.samples.len() != expected_len {
        return Err(DesktopImageDecodeError {
            code: "desktop_sample_length_mismatch".to_owned(),
            detail: format!(
                "decoded sample length {} does not match {}x{}x{} = {}",
                decoded.samples.len(),
                width,
                height,
                channels,
                expected_len
            ),
        });
    }

    let rgba_len = pixels
        .checked_mul(4)
        .ok_or_else(|| DesktopImageDecodeError {
            code: "desktop_sample_length_overflow".to_owned(),
            detail: "RGBA texture length overflows desktop address space".to_owned(),
        })?;
    let mut rgba = Vec::with_capacity(rgba_len);
    match decoded.decoded_sample_model.as_str() {
        "gray" => {
            for &gray in &decoded.samples {
                rgba.extend_from_slice(&[gray, gray, gray, 255]);
            }
        }
        "gray_alpha" => {
            for sample in decoded.samples.chunks_exact(2) {
                rgba.extend_from_slice(&[sample[0], sample[0], sample[0], sample[1]]);
            }
        }
        "rgb" => {
            for sample in decoded.samples.chunks_exact(3) {
                rgba.extend_from_slice(&[sample[0], sample[1], sample[2], 255]);
            }
        }
        "rgba" => rgba.extend_from_slice(&decoded.samples),
        _ => unreachable!("sample model was admitted above"),
    }

    Ok(AdmittedTextureImage {
        color_image: egui::ColorImage::from_rgba_unmultiplied([width, height], &rgba),
        cache_identity_sha256: decoded.cache_identity_sha256.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decoded(model: &str, precision: u8, orientation: &str, samples: Vec<u8>) -> DecodedImageV1 {
        DecodedImageV1 {
            contract_version: "test".to_owned(),
            resource_sha256: "ab".repeat(32),
            mime_type: "image/png".to_owned(),
            codec_family: "png".to_owned(),
            encoded_width_px: 2,
            encoded_height_px: 1,
            decoded_width_px: 2,
            decoded_height_px: 1,
            source_sample_model: model.to_owned(),
            decoded_sample_model: model.to_owned(),
            source_precision_bits: precision,
            decoded_precision_bits: precision,
            alpha_present: matches!(model, "gray_alpha" | "rgba"),
            alpha_association: if matches!(model, "gray_alpha" | "rgba") {
                "straight_unassociated"
            } else {
                "none"
            }
            .to_owned(),
            orientation_class: orientation.to_owned(),
            exif_orientation: None,
            orientation_applied: false,
            frame_disposition: "single_static".to_owned(),
            coding_process: "test".to_owned(),
            decoder_id: "test".to_owned(),
            color_disposition_ref: DESKTOP_COLOR_DISPOSITION_REF_V1.to_owned(),
            decoded_byte_count: samples.len(),
            sample_digest_sha256: "cd".repeat(32),
            cache_identity_sha256: "ef".repeat(32),
            samples,
        }
    }

    #[test]
    fn rgb_and_gray_alpha_expand_deterministically_to_rgba8() {
        let rgb = admit_decoded_image_v1(&decoded("rgb", 8, "normal", vec![1, 2, 3, 4, 5, 6]))
            .expect("RGB admitted");
        assert_eq!(
            rgb.color_image.pixels.as_slice(),
            &[
                egui::Color32::from_rgba_unmultiplied(1, 2, 3, 255),
                egui::Color32::from_rgba_unmultiplied(4, 5, 6, 255),
            ]
        );

        let gray_alpha =
            admit_decoded_image_v1(&decoded("gray_alpha", 8, "normal", vec![7, 8, 9, 10]))
                .expect("gray-alpha admitted");
        assert_eq!(
            gray_alpha.color_image.pixels.as_slice(),
            &[
                egui::Color32::from_rgba_unmultiplied(7, 7, 7, 8),
                egui::Color32::from_rgba_unmultiplied(9, 9, 9, 10),
            ]
        );
    }

    #[test]
    fn non_eight_bit_samples_fail_closed() {
        let error = admit_decoded_image_v1(&decoded("rgb", 16, "normal", vec![0; 12]))
            .expect_err("16-bit samples must not be silently truncated");
        assert_eq!(error.code, "desktop_precision_unsupported");
    }

    #[test]
    fn non_normal_orientation_fails_closed() {
        let error =
            admit_decoded_image_v1(&decoded("rgb", 8, "metadata_only_non_normal", vec![0; 6]))
                .expect_err("orientation metadata must not be silently ignored");
        assert_eq!(error.code, "desktop_orientation_unsupported");
    }

    #[test]
    fn exact_viewer_image_reuses_reader_hash_without_a_predecode_sha256_pass() {
        use std::cell::Cell;

        let actual_bytes = b"not the manifest resource";
        let verified = Sha256Digest::from_bytes([0x22; 32]);
        let extra_hash_passes = Cell::new(0);
        let expected =
            expected_viewer_image_sha256_with_v1(true, Some(verified), actual_bytes, |bytes| {
                extra_hash_passes.set(extra_hash_passes.get() + 1);
                exact_sha256_hex(bytes)
            })
            .expect("source-exact must carry Reader proof");
        assert_eq!(extra_hash_passes.get(), 0, "no self-issued predecode hash");
        assert_eq!(expected, "22".repeat(32));

        let mismatch = decode_texture_image_v1(actual_bytes, "image/png", &expected)
            .expect_err("decoder must still hash actual bytes and fail closed");
        assert_eq!(mismatch.code, "resource_hash_mismatch");

        let missing = expected_viewer_image_sha256_with_v1(true, None, actual_bytes, |_| {
            panic!("missing trusted SHA must not hash/accept source bytes")
        })
        .expect_err("no Reader authority means no exact admission");
        assert_eq!(missing.code, "missing_verified_source_image_hash");

        let bad_preview =
            expected_viewer_image_sha256_with_v1(false, Some(verified), actual_bytes, |_| {
                panic!("preview cannot gain exact source authority")
            })
            .expect_err("preview cannot impersonate a validated source resource");
        assert_eq!(bad_preview.code, "preview_claims_verified_source_hash");

        let preview_hash_passes = Cell::new(0);
        let preview = expected_viewer_image_sha256_with_v1(false, None, actual_bytes, |bytes| {
            preview_hash_passes.set(preview_hash_passes.get() + 1);
            exact_sha256_hex(bytes)
        })
        .expect("legacy preview uses derived PNG identity only");
        assert_eq!(preview_hash_passes.get(), 1);
        assert_eq!(preview, exact_sha256_hex(actual_bytes));
    }

    #[test]
    fn sample_length_mismatch_fails_closed() {
        let error = admit_decoded_image_v1(&decoded("rgba", 8, "normal", vec![0; 7]))
            .expect_err("truncated sample buffer must fail closed");
        assert_eq!(error.code, "desktop_sample_length_mismatch");
    }
}
