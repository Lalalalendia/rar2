//! Source-neutral resolved-graph -> bounded-authoring projection seam.
//!
//! This module is intentionally limited to the layout-authoring bridge used by
//! Viewer and current Editor/fixed-output consumers. It owns no Viewer paint,
//! table rendering, typography diagnostics, image recolor, or product-open
//! policy.

use crate::{ViewerGeometryDocument, ViewerPage};
use anyhow::{Context, Result, anyhow};
use pub_layout::{
    BoundedAuthoringSlice, BoundedNodeGeometryInput, BoundedTableInput, ResolvedSurface,
};
use pub_model::{Affine2D, Node, NodeId, NodeKind, PageId, Story, StoryFrame, StoryId};
use pub_reader::{PubResolvedGraph, PubResolvedNodePayload};
use std::collections::BTreeSet;

/// Transactionally refreshes only Viewer customer-page membership and surfaces
/// from an already-mutated resolved graph.
///
/// Page-role qualification remains external. The caller supplies the exact
/// admitted customer PageIds; this seam neither classifies raw Publisher pages
/// nor invents a second durable page list.
impl ViewerGeometryDocument {
    pub fn refresh_page_membership_from_resolved(
        &mut self,
        graph: &PubResolvedGraph,
        page_ids: &[PageId],
    ) -> Result<()> {
        if graph.source.source_hash != self.document.source.source_hash
            || graph.document.source_hash != self.document.source.source_hash
        {
            return Err(anyhow!(
                "Viewer page-membership refresh rejected a resolved graph with different source identity"
            ));
        }

        let (pages, surfaces) = viewer_page_membership_from_resolved(graph, page_ids)?;
        self.document.pages = pages;
        self.scene.surfaces = surfaces;
        Ok(())
    }
}

fn viewer_page_membership_from_resolved(
    graph: &PubResolvedGraph,
    page_ids: &[PageId],
) -> Result<(Vec<ViewerPage>, Vec<ResolvedSurface>)> {
    let document_membership = graph
        .document
        .pages
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    let mut seen = BTreeSet::new();
    let mut pages = Vec::with_capacity(page_ids.len());
    let mut surfaces = Vec::with_capacity(page_ids.len());

    for (zero_based, page_id) in page_ids.iter().copied().enumerate() {
        if !seen.insert(page_id) {
            return Err(anyhow!(
                "Viewer page-membership refresh received duplicate PageId {page_id:?}"
            ));
        }
        if !document_membership.contains(&page_id) {
            return Err(anyhow!(
                "Viewer page-membership refresh received PageId outside current document membership: {page_id:?}"
            ));
        }
        let page = graph
            .pages
            .get(&page_id)
            .with_context(|| format!("Viewer page-membership refresh missing page {page_id:?}"))?;
        let index = u32::try_from(zero_based + 1).context("Viewer page index exceeds u32")?;

        pages.push(ViewerPage {
            index,
            id: page_id,
            width_emu: page.size.width.get(),
            height_emu: page.size.height.get(),
        });
        surfaces.push(ResolvedSurface {
            origin: page_id,
            size: page.size,
            bleed: page.bleed,
            margins: page.margins,
        });
    }

    surfaces.sort_by_key(|surface| surface.origin);
    Ok((pages, surfaces))
}

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

/// Interactive text-session bridge: preserve every Story identity for
/// projection membership diagnostics, but materialize full Story payload only
/// for the requested Story. Fixed-output consumers keep using the full bridge.
pub fn bounded_authoring_slice_from_resolved_story_payload(
    graph: &PubResolvedGraph,
    story_id: StoryId,
) -> Result<BoundedAuthoringSlice> {
    bounded_authoring_slice_from_resolved_pages_with_story_payload_scope(
        graph,
        &graph.document.pages,
        Some(story_id),
    )
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

pub fn bounded_authoring_slice_from_resolved_pages(
    graph: &PubResolvedGraph,
    page_ids: &[PageId],
) -> Result<BoundedAuthoringSlice> {
    bounded_authoring_slice_from_resolved_pages_with_story_payload_scope(graph, page_ids, None)
}

fn bounded_authoring_slice_from_resolved_pages_with_story_payload_scope(
    graph: &PubResolvedGraph,
    page_ids: &[PageId],
    story_payload_scope: Option<StoryId>,
) -> Result<BoundedAuthoringSlice> {
    if let Some(story_id) = story_payload_scope {
        graph
            .stories
            .get(&story_id)
            .with_context(|| format!("layout projection missing requested Story {story_id:?}"))?;
    }
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

    let stories = graph
        .stories
        .values()
        .map(|story| match story_payload_scope {
            Some(story_id) if story.id != story_id => Story {
                id: story.id,
                text: String::new(),
                paragraphs: Vec::new(),
                runs: Vec::new(),
                fields: Vec::new(),
                hyperlinks: Vec::new(),
                source_refs: Vec::new(),
            },
            _ => story.clone(),
        })
        .collect();

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

#[cfg(test)]
mod page_membership_tests {
    use super::*;
    use pub_model::{
        CanonicalId, Document, LengthEmu, Page, Sha256Digest, Size2D, SourceDescriptor,
    };
    use std::collections::BTreeMap;

    fn page_id(byte: u8) -> PageId {
        PageId::from_canonical(CanonicalId::from_bytes([byte; 16]))
    }

    fn page(id: PageId, width: i64, height: i64) -> Page {
        Page {
            id,
            size: Size2D::new(LengthEmu::new(width), LengthEmu::new(height)),
            bleed: None,
            margins: None,
            children: Vec::new(),
            extensions: Vec::new(),
        }
    }

    fn graph(raw_pages: Vec<PageId>) -> PubResolvedGraph {
        let source_hash = Sha256Digest::from_bytes([0x7a; 32]);
        let pages = raw_pages
            .iter()
            .copied()
            .enumerate()
            .map(|(index, page_id)| {
                (
                    page_id,
                    page(
                        page_id,
                        1_000_000 + i64::try_from(index).unwrap() * 100_000,
                        2_000_000 + i64::try_from(index).unwrap() * 100_000,
                    ),
                )
            })
            .collect();

        PubResolvedGraph {
            cdm_version: "0.1".into(),
            resolver_version: pub_reader::PUB_RESOLVER_VERSION_V1.into(),
            source: SourceDescriptor {
                format: "pub".into(),
                format_version: Some("0x2c".into()),
                adapter_version: "pub-viewer/test".into(),
                source_hash,
            },
            document: Document {
                id: pub_model::DocumentId::from_canonical(CanonicalId::from_bytes([0x55; 16])),
                format_origin: "pub".into(),
                source_hash,
                pages: raw_pages,
                resources: Vec::new(),
                styles: Vec::new(),
            },
            pages,
            nodes: BTreeMap::new(),
            stories: BTreeMap::new(),
            paragraphs: BTreeMap::new(),
            text_runs: BTreeMap::new(),
            resources: BTreeMap::new(),
            styles: BTreeMap::new(),
            extensions: BTreeMap::new(),
        }
    }

    #[test]
    fn page_membership_projection_preserves_requested_order_and_exact_surface_geometry() {
        let first = page_id(1);
        let second = page_id(2);
        let graph = graph(vec![first, second]);

        let (pages, surfaces) =
            viewer_page_membership_from_resolved(&graph, &[second, first]).expect("projection");

        assert_eq!(
            pages.iter().map(|page| page.id).collect::<Vec<_>>(),
            vec![second, first]
        );
        assert_eq!(
            pages.iter().map(|page| page.index).collect::<Vec<_>>(),
            vec![1, 2]
        );
        assert_eq!(pages[0].width_emu, graph.pages[&second].size.width.get());
        assert_eq!(pages[0].height_emu, graph.pages[&second].size.height.get());

        assert_eq!(
            surfaces
                .iter()
                .map(|surface| surface.origin)
                .collect::<Vec<_>>(),
            vec![first, second]
        );
        assert_eq!(surfaces[0].size, graph.pages[&first].size);
        assert_eq!(surfaces[1].size, graph.pages[&second].size);
    }

    #[test]
    fn page_membership_projection_rejects_duplicates_and_nonmembers() {
        let first = page_id(1);
        let unknown = page_id(3);
        let graph = graph(vec![first]);

        assert!(viewer_page_membership_from_resolved(&graph, &[first, first]).is_err());
        assert!(viewer_page_membership_from_resolved(&graph, &[first, unknown]).is_err());
    }
}
