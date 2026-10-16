//! Source-neutral, read-only legacy CHPX style ranges.
//! Rendering and exact physical font selection are deliberately out of scope.

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewerLegacyUnderlineStyle {
    None,
    Single,
    WordsOnly,
    Double,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewerLegacyCharacterStyleRun {
    pub story_id: StoryId,
    pub scalar_start: u32,
    pub scalar_end: u32,
    pub bold: bool,
    pub italic: bool,
    pub small_caps: bool,
    pub all_caps: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_font_index: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_size_half_points: Option<i16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline_shift_half_points: Option<i8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub legacy_color_index: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub underline: Option<ViewerLegacyUnderlineStyle>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub letter_spacing_eighth_points: Option<i16>,
    pub source_story_text_sha256: Sha256Digest,
}


fn viewer_legacy_character_style_runs(
    source: &PubSourceGraphBuild,
    graph: &PubResolvedGraph,
) -> Vec<ViewerLegacyCharacterStyleRun> {
    source
        .legacy_character_style_runs
        .iter()
        .filter_map(|run| {
            let story = graph.stories.get(&run.story_id)?;
            let scalar_len = u32::try_from(story.text.chars().count()).ok()?;
            if run.story_scalar_start > run.story_scalar_end || run.story_scalar_end > scalar_len {
                return None;
            }
            Some(ViewerLegacyCharacterStyleRun {
                story_id: run.story_id,
                scalar_start: run.story_scalar_start,
                scalar_end: run.story_scalar_end,
                bold: run.bold,
                italic: run.italic,
                small_caps: run.small_caps,
                all_caps: run.all_caps,
                source_font_index: run.source_font_index,
                text_size_half_points: run.text_size_half_points,
                baseline_shift_half_points: run.baseline_shift_half_points,
                legacy_color_index: run.legacy_color_index,
                underline: run.underline.map(|value| match value {
                    PubLegacyUnderlineStyle::None => ViewerLegacyUnderlineStyle::None,
                    PubLegacyUnderlineStyle::Single => ViewerLegacyUnderlineStyle::Single,
                    PubLegacyUnderlineStyle::WordsOnly => ViewerLegacyUnderlineStyle::WordsOnly,
                    PubLegacyUnderlineStyle::Double => ViewerLegacyUnderlineStyle::Double,
                }),
                letter_spacing_eighth_points: run.letter_spacing_eighth_points,
                source_story_text_sha256: viewer_story_text_sha256(&story.text),
            })
        })
        .collect()
}

