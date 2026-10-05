//! Product-owned admission/presentation seam for editing one canonical Story
//! through an already validated linked TextFrame chain.
//!
//! Topology authority remains in pub-editor/pub-model. This module only
//! translates recovered frame membership into bounded Desktop UI state.

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LinkedStoryUiStateV1 {
    pub(crate) frame_count: usize,
    pub(crate) chain_membership: Option<String>,
}

pub(crate) fn state_for_validated_story_v1(
    frame_ordinals: &[u32],
) -> Result<LinkedStoryUiStateV1, &'static str> {
    if frame_ordinals.is_empty() {
        return Err("this Story has no proven placed TextFrame");
    }

    let chain_membership = if frame_ordinals.len() > 1 {
        let mut ordinals = frame_ordinals.to_vec();
        ordinals.sort_unstable();
        Some(
            ordinals
                .into_iter()
                .map(|ordinal| (u64::from(ordinal) + 1).to_string())
                .collect::<Vec<_>>()
                .join(" → "),
        )
    } else {
        None
    };

    Ok(LinkedStoryUiStateV1 {
        frame_count: frame_ordinals.len(),
        chain_membership,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validated_story_ui_state_is_fail_closed_without_a_placed_frame() {
        assert_eq!(
            state_for_validated_story_v1(&[]),
            Err("this Story has no proven placed TextFrame")
        );
    }

    #[test]
    fn single_frame_story_needs_no_chain_affordance() {
        let state = state_for_validated_story_v1(&[0]).expect("single frame is placed");
        assert_eq!(state.frame_count, 1);
        assert_eq!(state.chain_membership, None);
    }

    #[test]
    fn linked_story_membership_is_ordered_by_recovered_ordinal() {
        let state = state_for_validated_story_v1(&[2, 0, 1]).expect("linked frames are placed");
        assert_eq!(state.frame_count, 3);
        assert_eq!(state.chain_membership.as_deref(), Some("1 → 2 → 3"));
    }
}
