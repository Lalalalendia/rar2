//! Compile-isolated plain Story range transition laws.
//!
//! This crate is session-neutral by design. It must not depend on pub-editor,
//! pub-reader, layout/export/viewer/writer code, or EditorSession.

use pub_model::StoryId;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoryRangeTransitionErrorV1 {
    Stale { story_id: StoryId },
    Overflow { story_id: StoryId },
}

/// Canonical Story-state identity shared with services/editor-api/story_range_v1.py.
pub fn story_state_id_v1(story_id: StoryId, text: &str) -> String {
    let payload = serde_json::json!({
        "protocol_version": "chaptera.story-state.v1",
        "story_id": story_id.as_canonical().to_string(),
        "text": text,
    });
    let bytes =
        serde_json::to_vec(&payload).expect("canonical Story state JSON serialization cannot fail");
    let digest = Sha256::digest(bytes);
    let mut encoded = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;
        write!(&mut encoded, "{byte:02x}").expect("writing lowercase hex into String cannot fail");
    }
    format!("sha256:{encoded}")
}

fn scalar_byte_offset(text: &str, scalar_index: u32) -> Option<usize> {
    let target = usize::try_from(scalar_index).ok()?;
    if target == text.chars().count() {
        return Some(text.len());
    }
    text.char_indices().nth(target).map(|(offset, _)| offset)
}

pub fn replace_scalar_range_text_v1(
    text: &str,
    start_scalar: u32,
    end_scalar: u32,
    expected_before: &str,
    replacement_text: &str,
) -> Option<String> {
    if end_scalar < start_scalar {
        return None;
    }
    let start = scalar_byte_offset(text, start_scalar)?;
    let end = scalar_byte_offset(text, end_scalar)?;
    if text.get(start..end)? != expected_before {
        return None;
    }
    let mut after = String::with_capacity(
        text.len()
            .saturating_sub(end.saturating_sub(start))
            .saturating_add(replacement_text.len()),
    );
    after.push_str(&text[..start]);
    after.push_str(replacement_text);
    after.push_str(&text[end..]);
    Some(after)
}

#[allow(clippy::too_many_arguments)]
pub fn apply_story_range_forward_v1(
    current_text: &str,
    story_id: StoryId,
    start_scalar: u32,
    end_scalar: u32,
    expected_before: &str,
    replacement_text: &str,
    before_story_state_id: &str,
    after_story_state_id: &str,
) -> Result<String, StoryRangeTransitionErrorV1> {
    if story_state_id_v1(story_id, current_text) != before_story_state_id {
        return Err(StoryRangeTransitionErrorV1::Stale { story_id });
    }
    let after = replace_scalar_range_text_v1(
        current_text,
        start_scalar,
        end_scalar,
        expected_before,
        replacement_text,
    )
    .ok_or(StoryRangeTransitionErrorV1::Stale { story_id })?;
    if story_state_id_v1(story_id, &after) != after_story_state_id {
        return Err(StoryRangeTransitionErrorV1::Stale { story_id });
    }
    Ok(after)
}

pub fn apply_story_range_inverse_v1(
    current_text: &str,
    story_id: StoryId,
    start_scalar: u32,
    expected_before: &str,
    replacement_text: &str,
    before_story_state_id: &str,
    after_story_state_id: &str,
) -> Result<String, StoryRangeTransitionErrorV1> {
    if story_state_id_v1(story_id, current_text) != after_story_state_id {
        return Err(StoryRangeTransitionErrorV1::Stale { story_id });
    }
    let replacement_len = u32::try_from(replacement_text.chars().count())
        .map_err(|_| StoryRangeTransitionErrorV1::Overflow { story_id })?;
    let replacement_end = start_scalar
        .checked_add(replacement_len)
        .ok_or(StoryRangeTransitionErrorV1::Overflow { story_id })?;
    let before = replace_scalar_range_text_v1(
        current_text,
        start_scalar,
        replacement_end,
        replacement_text,
        expected_before,
    )
    .ok_or(StoryRangeTransitionErrorV1::Stale { story_id })?;
    if story_state_id_v1(story_id, &before) != before_story_state_id {
        return Err(StoryRangeTransitionErrorV1::Stale { story_id });
    }
    Ok(before)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pub_model::CanonicalId;

    fn story(byte: u8) -> StoryId {
        StoryId::from_canonical(CanonicalId::from_bytes([byte; 16]))
    }

    #[test]
    fn story_state_id_matches_cross_language_protocol_fixture() {
        assert_eq!(
            story_state_id_v1(story(0x40), "Hello world"),
            "sha256:627262487c605ad6629aa98f2eefe01ebda71a9da110a60b713de71f16626a0b"
        );
    }

    #[test]
    fn unicode_scalar_forward_inverse_is_exact() {
        let story_id = story(0x41);
        let before = "A💡Б";
        let after = "AXYБ";
        let before_state = story_state_id_v1(story_id, before);
        let after_state = story_state_id_v1(story_id, after);

        let applied = apply_story_range_forward_v1(
            before,
            story_id,
            1,
            2,
            "💡",
            "XY",
            &before_state,
            &after_state,
        )
        .expect("forward");
        assert_eq!(applied, after);

        let restored = apply_story_range_inverse_v1(
            &applied,
            story_id,
            1,
            "💡",
            "XY",
            &before_state,
            &after_state,
        )
        .expect("inverse");
        assert_eq!(restored, before);
    }

    #[test]
    fn forward_rejects_stale_state_or_expected_range() {
        let story_id = story(0x42);
        let before = "Hello world";
        let after = "Hello Chaptera";
        let before_state = story_state_id_v1(story_id, before);
        let after_state = story_state_id_v1(story_id, after);

        assert_eq!(
            apply_story_range_forward_v1(
                before,
                story_id,
                6,
                11,
                "world",
                "Chaptera",
                "sha256:stale",
                &after_state,
            ),
            Err(StoryRangeTransitionErrorV1::Stale { story_id })
        );
        assert_eq!(
            apply_story_range_forward_v1(
                before,
                story_id,
                6,
                11,
                "WORLD",
                "Chaptera",
                &before_state,
                &after_state,
            ),
            Err(StoryRangeTransitionErrorV1::Stale { story_id })
        );
    }

    #[test]
    fn inverse_rejects_wrong_after_state() {
        let story_id = story(0x43);
        let before = "Hello world";
        let after = "Hello Chaptera";
        let before_state = story_state_id_v1(story_id, before);

        assert_eq!(
            apply_story_range_inverse_v1(
                after,
                story_id,
                6,
                "world",
                "Chaptera",
                &before_state,
                "sha256:stale",
            ),
            Err(StoryRangeTransitionErrorV1::Stale { story_id })
        );
    }
}
