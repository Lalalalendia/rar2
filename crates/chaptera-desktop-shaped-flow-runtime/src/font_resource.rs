use super::DesktopShapedFlowRuntimeError;
use pub_layout::font_fingerprint_sha256;
use pub_model::LengthEmu;

#[derive(Debug, Clone, Copy)]
pub struct ExplicitDesktopFontResourceV1<'a> {
    pub resource_id: &'a str,
    pub expected_sha256: &'a str,
    pub face_index: u32,
    pub font_size_emu: LengthEmu,
    pub line_height_emu: LengthEmu,
    pub bytes: &'a [u8],
}

pub fn validate_explicit_font_resource_v1(
    font: &ExplicitDesktopFontResourceV1<'_>,
) -> Result<String, DesktopShapedFlowRuntimeError> {
    if font.resource_id.is_empty() {
        return Err(DesktopShapedFlowRuntimeError::new(
            "font_resource_missing",
            "explicit Desktop font resource_id is required",
        ));
    }
    if font.bytes.is_empty() {
        return Err(DesktopShapedFlowRuntimeError::new(
            "font_resource_missing",
            "explicit Desktop font bytes are required",
        ));
    }
    if font.font_size_emu.get() <= 0 {
        return Err(DesktopShapedFlowRuntimeError::new(
            "invalid_font_size",
            "font_size_emu must be positive",
        ));
    }
    if font.line_height_emu.get() <= 0 {
        return Err(DesktopShapedFlowRuntimeError::new(
            "invalid_line_height",
            "line_height_emu must be positive",
        ));
    }

    let actual = font_fingerprint_sha256(font.bytes);
    if font.expected_sha256.is_empty() || actual != font.expected_sha256 {
        return Err(DesktopShapedFlowRuntimeError::new(
            "font_fingerprint_mismatch",
            format!(
                "explicit Desktop font fingerprint mismatch: expected={} actual={actual}",
                font.expected_sha256
            ),
        ));
    }
    Ok(actual)
}
