//! Viewer image placement and page-admission seam.
//!
//! This module owns source-neutral image DTOs plus crop/recolor placement and
//! selected-page admission. Image byte discovery, WMF/OLE rasterization, and
//! Leaf-control: image placement changes stay inside this owner boundary.
//! OfficeArt preview materialization remain outside this module.

use anyhow::Result;
use pub_model::{NodeId, PageId, ResourceId};
use pub_reader::{PubExplicitImageCropSource, PubExplicitImageRecolorSource, PubResolvedGraph};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const VIEWER_IMAGE_SOURCE_Q16_ONE: i64 = 1 << 16;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewerImageSourceWindowV1 {
    /// Normalized source-image viewport edges in signed Q16 units.
    ///
    /// Values may extend outside 0..1 for Publisher Fit/pan states. The
    /// backend clips the persisted source window against the real image
    /// domain instead of clamping the crop itself.
    pub left_q16: i64,
    pub top_q16: i64,
    pub right_q16: i64,
    pub bottom_q16: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewerImageRecolorV1 {
    pub target_rgb: [u8; 3],
    pub preserve_grays: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewerImagePlacementV1 {
    pub node_id: NodeId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_window: Option<ViewerImageSourceWindowV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_rotation_degrees: Option<i16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recolor: Option<ViewerImageRecolorV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewerEmbeddedImage {
    pub resource_id: ResourceId,
    pub mime: String,
    pub node_ids: Vec<NodeId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub placements: Vec<ViewerImagePlacementV1>,
    #[serde(skip)]
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) struct ViewerImagePageAdmissionStatsV1 {
    pub(super) dropped_node_uses: usize,
    pub(super) dropped_placements: usize,
    pub(super) dropped_resources: usize,
}

pub(super) fn retain_viewer_image_uses_for_selected_pages_v1<F>(
    images: &mut Vec<ViewerEmbeddedImage>,
    selected_pages: &BTreeSet<PageId>,
    mut page_for_node: F,
) -> ViewerImagePageAdmissionStatsV1
where
    F: FnMut(NodeId) -> Option<PageId>,
{
    let mut stats = ViewerImagePageAdmissionStatsV1::default();

    for image in images.iter_mut() {
        let node_use_count = image.node_ids.len();
        image.node_ids.retain(|node_id| {
            page_for_node(*node_id).is_none_or(|page_id| selected_pages.contains(&page_id))
        });
        stats.dropped_node_uses += node_use_count.saturating_sub(image.node_ids.len());

        let placement_count = image.placements.len();
        image.placements.retain(|placement| {
            page_for_node(placement.node_id).is_none_or(|page_id| selected_pages.contains(&page_id))
        });
        stats.dropped_placements += placement_count.saturating_sub(image.placements.len());
    }

    let resource_count = images.len();
    images.retain(|image| !image.node_ids.is_empty() || !image.placements.is_empty());
    stats.dropped_resources = resource_count.saturating_sub(images.len());
    stats
}

pub(super) fn resolved_page_for_node_v1(
    graph: &PubResolvedGraph,
    node_id: NodeId,
) -> Option<PageId> {
    let mut current = graph.nodes.get(&node_id)?.header.parent_id;
    let mut seen = BTreeSet::new();
    loop {
        if !seen.insert(current) {
            return None;
        }
        let page_id = PageId::from_canonical(current);
        if graph.pages.contains_key(&page_id) {
            return Some(page_id);
        }
        current = graph
            .nodes
            .get(&NodeId::from_canonical(current))?
            .header
            .parent_id;
    }
}

pub(super) fn viewer_image_source_window_v1(
    crop: Option<&PubExplicitImageCropSource>,
) -> Result<Option<ViewerImageSourceWindowV1>, &'static str> {
    let Some(crop) = crop else {
        return Ok(None);
    };
    if crop.ambiguous {
        return Err("ambiguous_crop_properties");
    }
    if crop.top_raw.is_none()
        && crop.bottom_raw.is_none()
        && crop.left_raw.is_none()
        && crop.right_raw.is_none()
    {
        return Ok(None);
    }

    let signed_q16 = |raw: Option<u32>| -> i64 { raw.map_or(0, |value| i64::from(value as i32)) };
    let left_q16 = signed_q16(crop.left_raw);
    let top_q16 = signed_q16(crop.top_raw);
    let right_q16 = VIEWER_IMAGE_SOURCE_Q16_ONE - signed_q16(crop.right_raw);
    let bottom_q16 = VIEWER_IMAGE_SOURCE_Q16_ONE - signed_q16(crop.bottom_raw);

    if right_q16 <= left_q16 || bottom_q16 <= top_q16 {
        return Err("non_positive_source_window");
    }
    if right_q16 <= 0
        || bottom_q16 <= 0
        || left_q16 >= VIEWER_IMAGE_SOURCE_Q16_ONE
        || top_q16 >= VIEWER_IMAGE_SOURCE_Q16_ONE
    {
        return Err("source_window_outside_image");
    }

    Ok(Some(ViewerImageSourceWindowV1 {
        left_q16,
        top_q16,
        right_q16,
        bottom_q16,
    }))
}

pub(super) fn viewer_image_recolor_v1(
    recolor: Option<&PubExplicitImageRecolorSource>,
) -> Option<ViewerImageRecolorV1> {
    recolor.map(|recolor| ViewerImageRecolorV1 {
        target_rgb: recolor.target_rgb,
        preserve_grays: recolor.preserve_grays,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pub_model::CanonicalId;
    use std::collections::BTreeMap;

    fn id(byte: u8) -> CanonicalId {
        CanonicalId::from_bytes([byte; 16])
    }

    #[test]
    fn image_uses_compose_only_with_known_page_exclusion() {
        let admitted = NodeId::from_canonical(id(61));
        let excluded = NodeId::from_canonical(id(62));
        let excluded_only = NodeId::from_canonical(id(63));
        let unresolved = NodeId::from_canonical(id(64));
        let selected_page = PageId::from_canonical(id(81));
        let excluded_page = PageId::from_canonical(id(82));
        let first_resource = ResourceId::from_canonical(id(71));
        let second_resource = ResourceId::from_canonical(id(72));
        let third_resource = ResourceId::from_canonical(id(73));
        let mut images = vec![
            ViewerEmbeddedImage {
                resource_id: first_resource,
                mime: "image/png".to_owned(),
                node_ids: vec![admitted, excluded],
                placements: vec![
                    ViewerImagePlacementV1 {
                        node_id: admitted,
                        source_window: None,
                        content_rotation_degrees: Some(90),
                        recolor: None,
                    },
                    ViewerImagePlacementV1 {
                        node_id: excluded,
                        source_window: None,
                        content_rotation_degrees: None,
                        recolor: Some(ViewerImageRecolorV1 {
                            target_rgb: [1, 2, 3],
                            preserve_grays: false,
                        }),
                    },
                ],
                bytes: vec![1, 2, 3],
            },
            ViewerEmbeddedImage {
                resource_id: second_resource,
                mime: "image/jpeg".to_owned(),
                node_ids: vec![excluded_only],
                placements: vec![ViewerImagePlacementV1 {
                    node_id: excluded_only,
                    source_window: None,
                    content_rotation_degrees: None,
                    recolor: None,
                }],
                bytes: vec![4, 5, 6],
            },
            ViewerEmbeddedImage {
                resource_id: third_resource,
                mime: "image/png".to_owned(),
                node_ids: vec![unresolved],
                placements: vec![ViewerImagePlacementV1 {
                    node_id: unresolved,
                    source_window: None,
                    content_rotation_degrees: None,
                    recolor: None,
                }],
                bytes: vec![7, 8, 9],
            },
        ];
        let selected_pages = BTreeSet::from([selected_page]);
        let page_by_node = BTreeMap::from([
            (admitted, selected_page),
            (excluded, excluded_page),
            (excluded_only, excluded_page),
        ]);

        let stats = retain_viewer_image_uses_for_selected_pages_v1(
            &mut images,
            &selected_pages,
            |node_id| page_by_node.get(&node_id).copied(),
        );

        assert_eq!(
            stats,
            ViewerImagePageAdmissionStatsV1 {
                dropped_node_uses: 2,
                dropped_placements: 2,
                dropped_resources: 1,
            }
        );
        assert_eq!(images.len(), 2);
        assert_eq!(images[0].resource_id, first_resource);
        assert_eq!(images[0].node_ids, vec![admitted]);
        assert_eq!(images[0].placements.len(), 1);
        assert_eq!(images[0].placements[0].node_id, admitted);
        assert_eq!(images[0].bytes, vec![1, 2, 3]);
        assert_eq!(images[1].resource_id, third_resource);
        assert_eq!(images[1].node_ids, vec![unresolved]);
        assert_eq!(images[1].placements.len(), 1);
        assert_eq!(images[1].placements[0].node_id, unresolved);
    }

    #[test]
    fn image_source_window_projects_signed_q16_crop_without_intrinsic_size_guessing() {
        let fill = PubExplicitImageCropSource {
            top_raw: Some(0x0000_5988),
            bottom_raw: Some(0x0000_5988),
            left_raw: Some(0),
            right_raw: Some(0),
            ambiguous: false,
        };
        assert_eq!(
            viewer_image_source_window_v1(Some(&fill)).expect("fill crop"),
            Some(ViewerImageSourceWindowV1 {
                left_q16: 0,
                top_q16: 0x5988,
                right_q16: VIEWER_IMAGE_SOURCE_Q16_ONE,
                bottom_q16: VIEWER_IMAGE_SOURCE_Q16_ONE - 0x5988,
            })
        );

        let fit = PubExplicitImageCropSource {
            top_raw: Some(0),
            bottom_raw: Some(0),
            left_raw: Some(0xFFFE_D618),
            right_raw: Some(0xFFFE_D618),
            ambiguous: false,
        };
        let fit_window = viewer_image_source_window_v1(Some(&fit))
            .expect("fit crop")
            .expect("fit source window");
        assert!(fit_window.left_q16 < 0);
        assert!(fit_window.right_q16 > VIEWER_IMAGE_SOURCE_Q16_ONE);
        assert_eq!(
            fit_window.right_q16 - VIEWER_IMAGE_SOURCE_Q16_ONE,
            -fit_window.left_q16
        );
    }

    #[test]
    fn image_source_window_fails_closed_on_ambiguous_or_empty_windows() {
        let ambiguous = PubExplicitImageCropSource {
            top_raw: Some(0),
            bottom_raw: None,
            left_raw: None,
            right_raw: None,
            ambiguous: true,
        };
        assert!(viewer_image_source_window_v1(Some(&ambiguous)).is_err());

        let collapsed = PubExplicitImageCropSource {
            top_raw: None,
            bottom_raw: None,
            left_raw: Some(40_000),
            right_raw: Some(40_000),
            ambiguous: false,
        };
        assert!(viewer_image_source_window_v1(Some(&collapsed)).is_err());
    }

    #[test]
    fn image_recolor_projects_exact_source_contract() {
        let source = PubExplicitImageRecolorSource {
            target_rgb: [12, 34, 56],
            preserve_grays: true,
        };
        assert_eq!(
            viewer_image_recolor_v1(Some(&source)),
            Some(ViewerImageRecolorV1 {
                target_rgb: [12, 34, 56],
                preserve_grays: true,
            })
        );
        assert_eq!(viewer_image_recolor_v1(None), None);
    }
}
