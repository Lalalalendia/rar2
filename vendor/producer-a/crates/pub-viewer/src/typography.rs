use pub_model::{Sha256Digest, StoryId};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

// Source typography DTO and exact source-bound Quill index provenance.
// Kept outside pub-viewer/lib.rs to preserve source-fanout and monolith limits.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewerTypographyBooleanV1 {
    pub local_toggle: bool,
    pub inherited_value: bool,
    pub effective_value: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewerTypographyRun {
    pub story_id: StoryId,
    pub scalar_start: u32,
    pub scalar_end: u32,
    pub source_font_name: String,
    /// Exact Quill font ordinal when parsed from a source typography run.
    /// This is a source identity, not an admitted physical font resource.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_font_index: Option<u32>,
    pub text_size_emu: u32,
    pub font_inherited: bool,
    pub size_inherited: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color_rgb: Option<[u8; 3]>,
    #[serde(default)]
    pub color_inherited: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bold: Option<ViewerTypographyBooleanV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub italic: Option<ViewerTypographyBooleanV1>,
    pub source_story_text_sha256: Sha256Digest,
}

#[cfg(test)]
mod viewer_source_font_index_provenance_tests {
    use super::ViewerTypographyRun;

    #[test]
    fn viewer_typography_direct_index_roundtrip_v1() {
        let mut source = serde_json::json!({
            "story_id": "15613e56-726e-5ae7-8c54-ec876c9bcfda",
            "scalar_start": 0,
            "scalar_end": 22,
            "source_font_name": "Rockwell Condensed",
            "source_font_index": 18,
            "text_size_emu": 228600,
            "font_inherited": false,
            "size_inherited": true,
            "source_story_text_sha256":
                "58f537aade9b4a15df4537c5010aee92092642f048d38099696af46c3bb5daa6"
        });
        let current: ViewerTypographyRun = serde_json::from_value(source.clone())
            .expect("source-bound exact Quill typography run");
        assert_eq!(current.source_font_index, Some(18));
        let stored = serde_json::to_value(&current).expect("serialize Viewer typography");
        assert_eq!(stored["source_font_index"], 18);
        assert_eq!(stored["source_font_name"], "Rockwell Condensed");
        // Old persisted Viewer receipts remain parseable, but contain no
        // independent direct-index evidence. Never fill from a family name.
        source
            .as_object_mut()
            .expect("json object")
            .remove("source_font_index");
        let legacy: ViewerTypographyRun = serde_json::from_value(source)
            .expect("old Viewer receipt without direct source Quill index");
        assert_eq!(legacy.source_font_index, None);
        assert!(
            serde_json::to_value(&legacy)
                .expect("serialize old record")
                .get("source_font_index")
                .is_none()
        );
    }
}

impl ViewerTypographyRun {
    pub fn applies_to_story_text(&self, text: &str) -> bool {
        self.source_story_text_sha256 == viewer_story_text_sha256(text)
    }
}

pub fn viewer_story_text_sha256(text: &str) -> Sha256Digest {
    let digest = Sha256::digest(text.as_bytes());
    let mut bytes = [0_u8; 32];
    bytes.copy_from_slice(&digest);
    Sha256Digest::from_bytes(bytes)
}
