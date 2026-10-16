//! Evidence-bounded Publisher 2/95/97 character-style sidecar.
//!
//! Raw CHPX bytes remain inside pub-reader; consumers must not promote
//! unproven legacy palette slots to RGB or assume a physical font face.

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PubLegacyUnderlineStyle {
    None,
    Single,
    WordsOnly,
    Double,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PubLegacyCharacterStyleRun {
    pub story_id: StoryId,
    pub story_scalar_start: u32,
    pub story_scalar_end: u32,
    pub bold: bool,
    pub italic: bool,
    pub small_caps: bool,
    pub all_caps: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_font_index: Option<u8>,
    /// Exact point size represented in half-points when the bounded CHPX shape
    /// independently grounds the size-delta interpretation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_size_half_points: Option<i16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline_shift_half_points: Option<i8>,
    /// Publisher legacy palette slot. This is deliberately not promoted to RGB
    /// here because palette resolution is a separate authority.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub legacy_color_index: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub underline: Option<PubLegacyUnderlineStyle>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub letter_spacing_eighth_points: Option<i16>,
    /// Exact compact CHPX property bytes retained at the source adapter layer.
    /// Product Viewer DTOs intentionally do not expose this raw payload.
    pub raw_payload: Vec<u8>,
    pub source_refs: Vec<SourceRef>,
}

