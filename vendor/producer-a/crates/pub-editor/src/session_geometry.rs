//! Existing-node geometry session orchestration.
//!
//! This owner contains move/resize capability, canonical batch consumption,
//! and transition validation only. Paragraph formatting, authored duplication,
//! table mutation, image/text session logic, frame topology, and export remain
//! outside this module.

use super::*;
use pub_editor_geometry_core::{
    GeometryNodeSnapshotV1, MoveNodesTransitionErrorV1, ResizeNodesTransitionErrorV1,
    validate_move_nodes_transition_v1 as validate_move_nodes_transition_core_v1,
    validate_resize_nodes_transition_v1 as validate_resize_nodes_transition_core_v1,
};

impl EditorSession {
    fn has_table_track_extent_history_v1(&self, node_id: NodeId) -> bool {
        self.undo.iter().any(|operation| {
            matches!(
                operation,
                EditOperation::SetTableTrackExtent { history } if history.table_id == node_id
            )
        })
    }

    fn has_table_rowcol_history_v1(&self, node_id: NodeId) -> bool {
        self.undo.iter().any(|operation| {
            table_rowcol_history_v1(operation).is_some_and(|history| history.table_id == node_id)
        })
    }

    pub(super) fn has_node_resize_history_v1(&self, node_id: NodeId) -> bool {
        self.undo.iter().any(|operation| match operation {
            EditOperation::ResizeNode {
                node_id: resized, ..
            } => *resized == node_id,
            EditOperation::ResizeNodes { entries, .. } => {
                entries.iter().any(|entry| entry.node_id == node_id)
            }
            _ => false,
        })
    }

    pub fn can_move_node_to(
        &self,
        node_id: NodeId,
        x: LengthEmu,
        y: LengthEmu,
    ) -> Result<(), EditorError> {
        self.validate_source_identity()?;
        if self.has_table_track_extent_history_v1(node_id)
            || self.has_table_rowcol_history_v1(node_id)
        {
            return Err(EditorError::NodeMoveUnsupported { node_id });
        }

        let node = self
            .graph
            .nodes
            .get(&node_id)
            .ok_or(EditorError::NodeMoveUnsupported { node_id })?;
        if node.header.bounds.width.get() <= 0
            || node.header.bounds.height.get() <= 0
            || node.header.bounds.right().is_none()
            || node.header.bounds.bottom().is_none()
            || node.header.transform != pub_model::Affine2D::identity()
        {
            return Err(EditorError::NodeMoveUnsupported { node_id });
        }
        if !self
            .graph
            .pages
            .keys()
            .any(|page_id| page_id.into_canonical() == node.header.parent_id)
        {
            return Err(EditorError::NodeMoveUnsupported { node_id });
        }

        let candidate = RectEmu::new(x, y, node.header.bounds.width, node.header.bounds.height);
        if candidate.right().is_none() || candidate.bottom().is_none() {
            return Err(EditorError::NodeMoveOverflow { node_id });
        }

        Ok(())
    }

    pub fn move_node_to(
        &mut self,
        node_id: NodeId,
        x: LengthEmu,
        y: LengthEmu,
    ) -> Result<EditOperation, EditorError> {
        self.can_move_node_to(node_id, x, y)?;

        let before = self
            .graph
            .nodes
            .get(&node_id)
            .expect("capability check verified move node")
            .header
            .bounds;
        let after = RectEmu::new(x, y, before.width, before.height);
        if before == after {
            return Err(EditorError::NodeMoveNoChange { node_id });
        }

        let operation = EditOperation::MoveNode {
            node_id,
            before,
            after,
        };
        apply_forward(&mut self.graph, &operation)?;
        self.undo.push(operation.clone());
        self.redo.clear();
        self.validate_source_identity()?;
        Ok(operation)
    }

    /// Consume one already-authorized canonical MoveNodesV1 operation.
    ///
    /// Author-created provenance admission belongs to the source-neutral
    /// canonical authoring layer. This producer-side consumer deliberately
    /// does not infer provenance from Publisher source refs. It revalidates
    /// exact page ownership, stale before-state, translation-only geometry,
    /// canonical ordering/uniqueness and the existing bounded MoveNode
    /// capability before committing the whole batch as one history unit.
    pub(super) fn consume_canonical_move_nodes(
        &mut self,
        page_id: PageId,
        mut entries: Vec<MoveNodeBatchEntry>,
    ) -> Result<EditOperation, EditorError> {
        self.validate_source_identity()?;
        if entries.is_empty() {
            return Err(EditorError::MoveNodesEmpty);
        }
        if entries.len() > MAX_MOVE_NODES_V1 {
            return Err(EditorError::MoveNodesTooLarge {
                found: entries.len(),
            });
        }

        entries.sort_by_key(|entry| entry.node_id);
        for pair in entries.windows(2) {
            if pair[0].node_id == pair[1].node_id {
                return Err(EditorError::MoveNodesDuplicate {
                    node_id: pair[0].node_id,
                });
            }
        }

        let page_parent = page_id.into_canonical();
        for entry in &entries {
            let node =
                self.graph
                    .nodes
                    .get(&entry.node_id)
                    .ok_or(EditorError::NodeMoveUnsupported {
                        node_id: entry.node_id,
                    })?;
            if node.header.parent_id != page_parent {
                return Err(EditorError::MoveNodesPageMismatch {
                    node_id: entry.node_id,
                    page_id,
                });
            }
            if node.header.bounds != entry.before {
                return Err(EditorError::StaleNodeMove {
                    node_id: entry.node_id,
                });
            }
            if entry.before.width != entry.after.width || entry.before.height != entry.after.height
            {
                return Err(EditorError::MoveNodesSizeChanged {
                    node_id: entry.node_id,
                });
            }
            if entry.before == entry.after {
                return Err(EditorError::NodeMoveNoChange {
                    node_id: entry.node_id,
                });
            }
            self.can_move_node_to(entry.node_id, entry.after.x, entry.after.y)?;
        }

        let operation = EditOperation::MoveNodes { page_id, entries };
        apply_forward(&mut self.graph, &operation)?;
        self.undo.push(operation.clone());
        self.redo.clear();
        self.validate_source_identity()?;
        Ok(operation)
    }

    /// Consume one already-authorized canonical ResizeNodesV1 operation.
    ///
    /// Author-created provenance admission belongs to the source-neutral
    /// canonical authoring layer. The producer runtime revalidates only the
    /// persisted physical invariants required for atomic replay.
    pub(super) fn consume_canonical_resize_nodes(
        &mut self,
        page_id: PageId,
        mut entries: Vec<ResizeNodeBatchEntry>,
    ) -> Result<EditOperation, EditorError> {
        self.validate_source_identity()?;

        entries.sort_by_key(|entry| entry.node_id);
        for entry in &entries {
            if self.has_table_track_extent_history_v1(entry.node_id) {
                return Err(EditorError::NodeResizeUnsupported {
                    node_id: entry.node_id,
                });
            }
        }
        validate_resize_nodes_transition(&self.graph, page_id, &entries, true)?;

        let operation = EditOperation::ResizeNodes { page_id, entries };
        apply_forward(&mut self.graph, &operation)?;
        self.undo.push(operation.clone());
        self.redo.clear();
        self.validate_source_identity()?;
        Ok(operation)
    }

    pub fn can_resize_node(&self, node_id: NodeId) -> Result<(), EditorError> {
        self.validate_source_identity()?;
        if self.has_table_track_extent_history_v1(node_id)
            || self.has_table_rowcol_history_v1(node_id)
        {
            return Err(EditorError::NodeResizeUnsupported { node_id });
        }

        let node = self
            .graph
            .nodes
            .get(&node_id)
            .ok_or(EditorError::NodeResizeUnsupported { node_id })?;
        let before = node.header.bounds;
        if before.width.get() <= 0
            || before.height.get() <= 0
            || before.right().is_none()
            || before.bottom().is_none()
            || node.header.transform != pub_model::Affine2D::identity()
        {
            return Err(EditorError::NodeResizeUnsupported { node_id });
        }
        if !self
            .graph
            .pages
            .keys()
            .any(|page_id| page_id.into_canonical() == node.header.parent_id)
        {
            return Err(EditorError::NodeResizeUnsupported { node_id });
        }

        Ok(())
    }

    pub fn can_resize_node_to(&self, node_id: NodeId, bounds: RectEmu) -> Result<(), EditorError> {
        self.can_resize_node(node_id)?;

        let before = self
            .graph
            .nodes
            .get(&node_id)
            .expect("target capability check verified resize node")
            .header
            .bounds;
        if bounds.width.get() <= 0 || bounds.height.get() <= 0 {
            return Err(EditorError::NodeResizeNonPositive { node_id });
        }
        if bounds.right().is_none() || bounds.bottom().is_none() {
            return Err(EditorError::NodeResizeOverflow { node_id });
        }
        if before == bounds {
            return Err(EditorError::NodeResizeNoChange { node_id });
        }
        if before.width == bounds.width && before.height == bounds.height {
            return Err(EditorError::NodeResizeNoSizeChange { node_id });
        }

        Ok(())
    }

    pub fn resize_node_to(
        &mut self,
        node_id: NodeId,
        bounds: RectEmu,
    ) -> Result<EditOperation, EditorError> {
        self.can_resize_node_to(node_id, bounds)?;

        let before = self
            .graph
            .nodes
            .get(&node_id)
            .expect("capability check verified resize node")
            .header
            .bounds;
        let operation = EditOperation::ResizeNode {
            node_id,
            before,
            after: bounds,
        };
        apply_forward(&mut self.graph, &operation)?;
        self.undo.push(operation.clone());
        self.redo.clear();
        self.validate_source_identity()?;
        Ok(operation)
    }
}

fn geometry_node_snapshot_v1(
    graph: &PubResolvedGraph,
    node_id: NodeId,
) -> Option<GeometryNodeSnapshotV1> {
    let node = graph.nodes.get(&node_id)?;
    Some(GeometryNodeSnapshotV1 {
        node_id,
        parent_id: node.header.parent_id,
        bounds: node.header.bounds,
        transform: node.header.transform.clone(),
    })
}

fn move_transition_error_to_editor_v1(error: MoveNodesTransitionErrorV1) -> EditorError {
    match error {
        MoveNodesTransitionErrorV1::Empty => EditorError::MoveNodesEmpty,
        MoveNodesTransitionErrorV1::TooLarge { found } => EditorError::MoveNodesTooLarge { found },
        MoveNodesTransitionErrorV1::Duplicate { node_id } => {
            EditorError::MoveNodesDuplicate { node_id }
        }
        MoveNodesTransitionErrorV1::SizeChanged { node_id } => {
            EditorError::MoveNodesSizeChanged { node_id }
        }
        MoveNodesTransitionErrorV1::NoChange { node_id } => {
            EditorError::NodeMoveNoChange { node_id }
        }
        MoveNodesTransitionErrorV1::NodeUnsupported { node_id } => {
            EditorError::NodeMoveUnsupported { node_id }
        }
        MoveNodesTransitionErrorV1::PageMismatch { node_id, page_id } => {
            EditorError::MoveNodesPageMismatch { node_id, page_id }
        }
        MoveNodesTransitionErrorV1::Stale { node_id } => EditorError::StaleNodeMove { node_id },
    }
}

fn resize_transition_error_to_editor_v1(error: ResizeNodesTransitionErrorV1) -> EditorError {
    match error {
        ResizeNodesTransitionErrorV1::InvalidCount { found } => {
            EditorError::ResizeNodesInvalidCount { found }
        }
        ResizeNodesTransitionErrorV1::Duplicate { node_id } => {
            EditorError::ResizeNodesDuplicate { node_id }
        }
        ResizeNodesTransitionErrorV1::NotCanonical { node_id } => {
            EditorError::ResizeNodesNotCanonical { node_id }
        }
        ResizeNodesTransitionErrorV1::PageMismatch { node_id, page_id } => {
            EditorError::ResizeNodesPageMismatch { node_id, page_id }
        }
        ResizeNodesTransitionErrorV1::NodeUnsupported { node_id } => {
            EditorError::NodeResizeUnsupported { node_id }
        }
        ResizeNodesTransitionErrorV1::NonPositive { node_id } => {
            EditorError::NodeResizeNonPositive { node_id }
        }
        ResizeNodesTransitionErrorV1::Overflow { node_id } => {
            EditorError::NodeResizeOverflow { node_id }
        }
        ResizeNodesTransitionErrorV1::Stale { node_id } => EditorError::StaleNodeResize { node_id },
        ResizeNodesTransitionErrorV1::NoSizeChange => EditorError::ResizeNodesNoSizeChange,
    }
}

pub(super) fn validate_move_nodes_transition(
    graph: &PubResolvedGraph,
    page_id: PageId,
    entries: &[MoveNodeBatchEntry],
    forward: bool,
) -> Result<(), EditorError> {
    let nodes = entries
        .iter()
        .filter_map(|entry| geometry_node_snapshot_v1(graph, entry.node_id))
        .collect::<Vec<_>>();
    validate_move_nodes_transition_core_v1(page_id, entries, &nodes, forward)
        .map_err(move_transition_error_to_editor_v1)
}

pub(super) fn validate_resize_nodes_transition(
    graph: &PubResolvedGraph,
    page_id: PageId,
    entries: &[ResizeNodeBatchEntry],
    forward: bool,
) -> Result<(), EditorError> {
    let nodes = entries
        .iter()
        .filter_map(|entry| geometry_node_snapshot_v1(graph, entry.node_id))
        .collect::<Vec<_>>();
    validate_resize_nodes_transition_core_v1(page_id, entries, &nodes, forward)
        .map_err(resize_transition_error_to_editor_v1)
}
