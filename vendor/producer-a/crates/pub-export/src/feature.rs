//! Canonical target-neutral semantic feature vocabulary.
//!
//! A feature key names source-neutral authoring/export semantics only. Merely
//! defining a key never grants support to any target. TargetCapabilityManifest
//! remains the sole authority for Preserved/Approximated/etc. dispositions.

pub const PAGE_GEOMETRY: &str = "page.geometry";
pub const PAGE_OBJECT_ORDER: &str = "page.object_order";
pub const STORY_TEXT: &str = "story.text";
pub const STORY_LINKED_FRAMES: &str = "story.linked_frames";
pub const STORY_SHARED_IDENTITY: &str = "story.shared_identity";
pub const TABLE_STRUCTURE: &str = "table.structure";
pub const TABLE_STYLE: &str = "table.style";
pub const NODE_UNSUPPORTED: &str = "node.unsupported";

pub const IMAGE_BYTES: &str = "image.bytes";
pub const IMAGE_FRAME_GEOMETRY: &str = "image.frame_geometry";
pub const IMAGE_CONTENT_TRANSFORM: &str = "image.content_transform";

pub const OBJECT_GROUP_STRUCTURE: &str = "object.group_structure";
pub const PAGE_MASTER_RELATION: &str = "page.master_relation";
pub const LAYOUT_GUIDE_GRID: &str = "layout.guide_grid";

pub const ESTABLISHED_V0_1: &[&str] = &[
    PAGE_GEOMETRY,
    PAGE_OBJECT_ORDER,
    STORY_TEXT,
    STORY_LINKED_FRAMES,
    STORY_SHARED_IDENTITY,
    TABLE_STRUCTURE,
    TABLE_STYLE,
    NODE_UNSUPPORTED,
    IMAGE_BYTES,
    IMAGE_FRAME_GEOMETRY,
    IMAGE_CONTENT_TRANSFORM,
    OBJECT_GROUP_STRUCTURE,
    PAGE_MASTER_RELATION,
    LAYOUT_GUIDE_GRID,
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        CapabilityLevel, SemanticFeatureRequest, TargetCapabilityManifest, TargetProfile,
        plan_export,
    };
    use std::collections::{BTreeMap, BTreeSet};

    #[test]
    fn established_vocabulary_is_unique_and_stable() {
        let unique = ESTABLISHED_V0_1.iter().copied().collect::<BTreeSet<_>>();
        assert_eq!(unique.len(), ESTABLISHED_V0_1.len());
        assert_eq!(
            ESTABLISHED_V0_1,
            &[
                "page.geometry",
                "page.object_order",
                "story.text",
                "story.linked_frames",
                "story.shared_identity",
                "table.structure",
                "table.style",
                "node.unsupported",
                "image.bytes",
                "image.frame_geometry",
                "image.content_transform",
                "object.group_structure",
                "page.master_relation",
                "layout.guide_grid",
            ],
        );
    }

    #[test]
    fn new_vocabulary_keys_do_not_grant_target_support() {
        let manifest = TargetCapabilityManifest {
            target: TargetProfile {
                format: "empty".into(),
                adapter_version: "empty-v0.1".into(),
                profile: "empty".into(),
                schema_fence: None,
            },
            features: BTreeMap::new(),
        };
        let requests = [
            OBJECT_GROUP_STRUCTURE,
            PAGE_MASTER_RELATION,
            LAYOUT_GUIDE_GRID,
        ]
        .into_iter()
        .map(|feature| SemanticFeatureRequest {
            feature: feature.into(),
            origin: None,
            property_path: None,
            require_preserved: false,
        })
        .collect();

        let plan = plan_export(&manifest, requests);
        assert_eq!(plan.features.len(), 3);
        assert!(
            plan.features
                .iter()
                .all(|feature| feature.disposition == CapabilityLevel::Unsupported)
        );
        assert_eq!(plan.losses.len(), 3);
        assert!(plan.blockers.is_empty());
    }
}
