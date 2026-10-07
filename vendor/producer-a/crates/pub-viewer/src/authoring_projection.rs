// CI routing control only: authoring projection owner must stay narrowly admitted.
//! Source-neutral resolved-graph -> bounded-authoring projection seam.
//!
//! This module is intentionally limited to the layout-authoring bridge used by
//! Viewer and current Editor/fixed-output consumers. It owns no Viewer paint,
//! table rendering, typography diagnostics, image recolor, or product-open
//! policy.

use anyhow::{Context, Result};
use pub_layout::{BoundedAuthoringSlice, BoundedNodeGeometryInput, BoundedTableInput};
use pub_model::{Affine2D, Node, NodeId, NodeKind, PageId, StoryFrame};
use pub_reader::{PubResolvedGraph, PubResolvedNodePayload};
use std::collections::BTreeSet;

/// Creates only the grounded semantic subset already accepted by pub-layout.
///
/// Viewer remains the semantic owner of this resolved-graph -> bounded-authoring
/// bridge. Desktop shaped-flow is the second concrete consumer, so the mapping
/// is public instead of being copied into another engine adapter.
pub fn bounded_authoring_slice_from_resolved(
    graph: &PubResolvedGraph,
) -> Result<BoundedAuthoringSlice> {
    bounded_authoring_slice_from_resolved_pages(graph, &graph.document.pages)
}

pub(super) fn legacy_noquill_structural_point_group_ids(
    graph: &PubResolvedGraph,
    page_ids: &[PageId],
) -> BTreeSet<NodeId> {
    let selected_pages = page_ids.iter().copied().collect::<BTreeSet<_>>();
    graph
        .nodes
        .values()
        .filter(|node| node.kind == NodeKind::Group)
        .filter(|node| selected_pages.contains(&PageId::from_canonical(node.header.parent_id)))
        .filter(|node| node.header.bounds.width.get() == 0 && node.header.bounds.height.get() == 0)
        .filter(|node| node.header.transform == Affine2D::identity())
        .map(|node| node.header.id)
        .collect()
}

fn legacy_noquill_image_page(
    graph: &PubResolvedGraph,
    node: &Node<PubResolvedNodePayload>,
    selected_pages: &BTreeSet<PageId>,
) -> Option<PageId> {
    if node.kind != NodeKind::ImageFrame {
        return None;
    }

    let mut current = node.header.parent_id;
    let mut seen = BTreeSet::new();
    loop {
        if !seen.insert(current) {
            return None;
        }

        let page_id = PageId::from_canonical(current);
        if graph.pages.contains_key(&page_id) {
            return selected_pages.contains(&page_id).then_some(page_id);
        }

        let parent = graph.nodes.get(&NodeId::from_canonical(current))?;
        if parent.kind != NodeKind::Group {
            return None;
        }
        current = parent.header.parent_id;
    }
}

pub(super) fn bounded_legacy_noquill_authoring_slice_from_resolved_pages(
    graph: &PubResolvedGraph,
    page_ids: &[PageId],
) -> Result<BoundedAuthoringSlice> {
    let mut authoring = bounded_authoring_slice_from_resolved_pages(graph, page_ids)?;
    let structural_point_groups = legacy_noquill_structural_point_group_ids(graph, page_ids);
    authoring
        .node_geometry
        .retain(|node| !structural_point_groups.contains(&node.node_id));

    let selected_pages = page_ids.iter().copied().collect::<BTreeSet<_>>();
    let mut projected_ids = authoring
        .node_geometry
        .iter()
        .map(|node| node.node_id)
        .collect::<BTreeSet<_>>();

    for node in graph.nodes.values() {
        if projected_ids.contains(&node.header.id) {
            continue;
        }
        let Some(page_id) = legacy_noquill_image_page(graph, node, &selected_pages) else {
            continue;
        };

        authoring.node_geometry.push(BoundedNodeGeometryInput {
            node_id: node.header.id,
            parent_origin: page_id.into_canonical(),
            bounds: node.header.bounds,
            transform: node.header.transform.clone(),
        });
        projected_ids.insert(node.header.id);
    }

    authoring.node_geometry.sort_by_key(|node| node.node_id);
    Ok(authoring)
}

pub(super) fn bounded_authoring_slice_from_resolved_pages(
    graph: &PubResolvedGraph,
    page_ids: &[PageId],
) -> Result<BoundedAuthoringSlice> {
    let pages = page_ids
        .iter()
        .map(|page_id| {
            graph
                .pages
                .get(page_id)
                .cloned()
                .with_context(|| format!("layout projection missing document page {page_id:?}"))
        })
        .collect::<Result<Vec<_>>>()?;

    let page_origins = page_ids
        .iter()
        .map(|page_id| page_id.into_canonical())
        .collect::<BTreeSet<_>>();

    let node_geometry = graph
        .nodes
        .values()
        .filter(|node| page_origins.contains(&node.header.parent_id))
        .map(|node| BoundedNodeGeometryInput {
            node_id: node.header.id,
            parent_origin: node.header.parent_id,
            bounds: node.header.bounds,
            transform: node.header.transform.clone(),
        })
        .collect();

    let stories = graph.stories.values().cloned().collect();

    let story_frames = graph
        .nodes
        .values()
        .filter(|node| page_origins.contains(&node.header.parent_id))
        .filter_map(|node| {
            let frame = node.payload.story_frame.as_ref()?;
            let story_id = frame.story_id?;
            Some(StoryFrame {
                story_id,
                frame_id: node.header.id,
                ordinal: frame.ordinal,
                previous: frame.previous_frame,
                next: frame.next_frame,
            })
        })
        .collect();

    let tables = graph
        .nodes
        .values()
        .filter(|node| page_origins.contains(&node.header.parent_id))
        .filter_map(|node| {
            let table = node.payload.table.as_ref()?.simple_table.as_ref()?.clone();
            Some(BoundedTableInput {
                node_id: node.header.id,
                table,
            })
        })
        .collect();

    Ok(BoundedAuthoringSlice {
        pages,
        node_geometry,
        stories,
        story_frames,
        tables,
        guides: Vec::new(),
        unknown_layout_state: Vec::new(),
    })
}
