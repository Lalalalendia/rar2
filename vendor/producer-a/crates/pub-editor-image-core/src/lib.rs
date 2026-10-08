//! Compile-isolated image replacement/crop overlay transition laws.
//!
//! This crate is session-neutral by design. It must not depend on pub-editor,
//! pub-reader, export code, or EditorSession.

use pub_model::{NodeId, Sha256Digest};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageCropStateV1 {
    pub top_raw: Option<u32>,
    pub bottom_raw: Option<u32>,
    pub left_raw: Option<u32>,
    pub right_raw: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageReplacementTransitionErrorV1 {
    Stale { node_id: NodeId },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageCropTransitionErrorV1 {
    Stale { node_id: NodeId },
}

pub fn apply_image_replacement_forward_v1(
    replacements: &mut BTreeMap<NodeId, Sha256Digest>,
    node_id: NodeId,
    before_asset: Option<Sha256Digest>,
    after_asset: Sha256Digest,
) -> Result<(), ImageReplacementTransitionErrorV1> {
    if replacements.get(&node_id).copied() != before_asset {
        return Err(ImageReplacementTransitionErrorV1::Stale { node_id });
    }
    replacements.insert(node_id, after_asset);
    Ok(())
}

pub fn apply_image_replacement_inverse_v1(
    replacements: &mut BTreeMap<NodeId, Sha256Digest>,
    node_id: NodeId,
    before_asset: Option<Sha256Digest>,
    after_asset: Sha256Digest,
) -> Result<(), ImageReplacementTransitionErrorV1> {
    if replacements.get(&node_id).copied() != Some(after_asset) {
        return Err(ImageReplacementTransitionErrorV1::Stale { node_id });
    }
    if let Some(before_asset) = before_asset {
        replacements.insert(node_id, before_asset);
    } else {
        replacements.remove(&node_id);
    }
    Ok(())
}

pub fn effective_image_crop_state_v1(
    source_crop: Option<ImageCropStateV1>,
    overrides: &BTreeMap<NodeId, ImageCropStateV1>,
    node_id: NodeId,
) -> Option<ImageCropStateV1> {
    overrides.get(&node_id).copied().or(source_crop)
}

pub fn apply_image_crop_forward_v1(
    source_crop: Option<ImageCropStateV1>,
    overrides: &mut BTreeMap<NodeId, ImageCropStateV1>,
    node_id: NodeId,
    before: ImageCropStateV1,
    after: ImageCropStateV1,
) -> Result<(), ImageCropTransitionErrorV1> {
    if effective_image_crop_state_v1(source_crop, overrides, node_id) != Some(before) {
        return Err(ImageCropTransitionErrorV1::Stale { node_id });
    }
    overrides.insert(node_id, after);
    Ok(())
}

pub fn apply_image_crop_inverse_v1(
    source_crop: Option<ImageCropStateV1>,
    overrides: &mut BTreeMap<NodeId, ImageCropStateV1>,
    node_id: NodeId,
    before: ImageCropStateV1,
    after: ImageCropStateV1,
) -> Result<(), ImageCropTransitionErrorV1> {
    if effective_image_crop_state_v1(source_crop, overrides, node_id) != Some(after) {
        return Err(ImageCropTransitionErrorV1::Stale { node_id });
    }
    if source_crop == Some(before) {
        overrides.remove(&node_id);
    } else {
        overrides.insert(node_id, before);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pub_model::CanonicalId;

    fn node(byte: u8) -> NodeId {
        NodeId::from_canonical(CanonicalId::from_bytes([byte; 16]))
    }

    fn sha(byte: u8) -> Sha256Digest {
        Sha256Digest::from_bytes([byte; 32])
    }

    fn crop(value: u32) -> ImageCropStateV1 {
        ImageCropStateV1 {
            top_raw: Some(value),
            bottom_raw: Some(value + 1),
            left_raw: Some(value + 2),
            right_raw: Some(value + 3),
        }
    }

    #[test]
    fn crop_state_preserves_existing_wire_shape() {
        let value = serde_json::to_value(crop(7)).expect("serialize ImageCropStateV1");
        assert_eq!(value["top_raw"], 7);
        assert_eq!(value["bottom_raw"], 8);
        assert_eq!(value["left_raw"], 9);
        assert_eq!(value["right_raw"], 10);
        assert_eq!(value.as_object().expect("object").len(), 4);
    }

    #[test]
    fn replacement_forward_inverse_is_exact_and_stale_checked() {
        let node_id = node(0x21);
        let before = sha(0x11);
        let after = sha(0x22);
        let mut replacements = BTreeMap::from([(node_id, before)]);

        apply_image_replacement_forward_v1(&mut replacements, node_id, Some(before), after)
            .expect("forward");
        assert_eq!(replacements.get(&node_id), Some(&after));
        assert_eq!(
            apply_image_replacement_forward_v1(&mut replacements, node_id, Some(before), after,),
            Err(ImageReplacementTransitionErrorV1::Stale { node_id })
        );

        apply_image_replacement_inverse_v1(&mut replacements, node_id, Some(before), after)
            .expect("inverse");
        assert_eq!(replacements.get(&node_id), Some(&before));
    }

    #[test]
    fn replacement_inverse_removes_new_overlay() {
        let node_id = node(0x22);
        let after = sha(0x33);
        let mut replacements = BTreeMap::new();

        apply_image_replacement_forward_v1(&mut replacements, node_id, None, after)
            .expect("forward");
        apply_image_replacement_inverse_v1(&mut replacements, node_id, None, after)
            .expect("inverse");
        assert!(!replacements.contains_key(&node_id));
    }

    #[test]
    fn crop_inverse_restores_source_or_previous_overlay() {
        let node_id = node(0x23);
        let source = crop(10);
        let override_before = crop(20);
        let after = crop(30);
        let mut overrides = BTreeMap::new();

        assert_eq!(
            effective_image_crop_state_v1(Some(source), &overrides, node_id),
            Some(source)
        );
        apply_image_crop_forward_v1(Some(source), &mut overrides, node_id, source, after)
            .expect("source forward");
        apply_image_crop_inverse_v1(Some(source), &mut overrides, node_id, source, after)
            .expect("source inverse");
        assert!(!overrides.contains_key(&node_id));

        overrides.insert(node_id, override_before);
        apply_image_crop_forward_v1(
            Some(source),
            &mut overrides,
            node_id,
            override_before,
            after,
        )
        .expect("override forward");
        apply_image_crop_inverse_v1(
            Some(source),
            &mut overrides,
            node_id,
            override_before,
            after,
        )
        .expect("override inverse");
        assert_eq!(overrides.get(&node_id), Some(&override_before));
    }

    #[test]
    fn crop_transition_rejects_stale_state() {
        let node_id = node(0x24);
        let source = crop(1);
        let stale = crop(2);
        let after = crop(3);
        let mut overrides = BTreeMap::new();

        assert_eq!(
            apply_image_crop_forward_v1(Some(source), &mut overrides, node_id, stale, after,),
            Err(ImageCropTransitionErrorV1::Stale { node_id })
        );
    }
}
