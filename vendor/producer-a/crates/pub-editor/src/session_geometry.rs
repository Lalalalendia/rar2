//! Existing-node geometry session orchestration.
//!
//! This owner contains existing-node geometry plus bounded page/stack ordering
//! session orchestration. Paragraph formatting, authored duplication, table
//! mutation, image/text session logic, frame topology, and export remain outside.

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
    pub(super) fn current_authored_stack_v1(&self, page_id: PageId) -> AuthoredStackV1 {
        self.authored_stacks
            .get(&page_id)
            .cloned()
            .unwrap_or_else(|| AuthoredStackV1::empty(page_id))
    }

    pub(super) fn install_authored_stack_v1(&mut self, stack: AuthoredStackV1) {
        if stack.members.is_empty() {
            self.authored_stacks.remove(&stack.page_id);
        } else {
            self.authored_stacks.insert(stack.page_id, stack);
        }
    }

    /// Return current order for one externally-qualified customer-page set.
    ///
    /// Membership is owned by an existing page-role authority. Editor owns
    /// only durable order and refuses to classify raw PAGE records here.
    pub fn current_qualified_page_order_v1(
        &self,
        qualified_page_ids: &[PageId],
    ) -> Result<Vec<PageId>, EditorError> {
        self.validate_source_identity()?;
        for page_id in qualified_page_ids {
            if !self.graph.pages.contains_key(page_id) {
                return Err(EditorError::PageOrderUnsupported {
                    message: format!(
                        "qualified page {} is absent from the current semantic page map",
                        page_id.as_canonical()
                    ),
                });
            }
        }
        qualified_page_order_v1(&self.graph.document.pages, qualified_page_ids)
            .map_err(page_order_error_to_editor_v1)
    }

    pub fn reorder_pages_v1(
        &mut self,
        expected_before: Vec<PageId>,
        requested_after: Vec<PageId>,
    ) -> Result<EditOperation, EditorError> {
        self.validate_source_identity()?;
        if self.project_identity.is_none() {
            return Err(EditorError::PageOrderUnsupported {
                message: "durable project identity is required".to_owned(),
            });
        }
        for page_id in &expected_before {
            if !self.graph.pages.contains_key(page_id) {
                return Err(EditorError::PageOrderUnsupported {
                    message: format!(
                        "qualified page {} is absent from the current semantic page map",
                        page_id.as_canonical()
                    ),
                });
            }
        }

        let transition = plan_page_order_transition_v1(
            self.graph.document.id,
            &self.graph.document.pages,
            &expected_before,
            &requested_after,
        )
        .map_err(page_order_error_to_editor_v1)?;
        let operation = EditOperation::ReorderPagesV1 { transition };
        apply_forward(&mut self.graph, &operation)?;
        self.undo.push(operation.clone());
        self.redo.clear();
        self.validate_source_identity()?;
        Ok(operation)
    }

    pub fn reorder_authored_stack(
        &mut self,
        page_id: PageId,
        node_id: NodeId,
        mode: AuthoredStackReorderModeV1,
    ) -> Result<EditOperation, EditorError> {
        self.validate_source_identity()?;
        if self.project_identity.is_none() {
            return Err(EditorError::AuthoredStackReorderUnsupported { node_id });
        }
        let shape = self
            .authored_shapes
            .get(&node_id)
            .ok_or(EditorError::AuthoredStackReorderUnsupported { node_id })?;
        if shape.page_id != page_id
            || shape.parent_id != page_id
            || validate_authored_shape_runtime_v1(shape).is_err()
        {
            return Err(EditorError::AuthoredStackReorderUnsupported { node_id });
        }

        let before = self.current_authored_stack_v1(page_id);
        let transition = plan_reorder_authored_stack_v1(&before, node_id, mode).map_err(
            |error| match error {
                AuthoredStackReorderErrorV1::NoChange { .. } => {
                    EditorError::AuthoredStackReorderNoChange { node_id }
                }
                AuthoredStackReorderErrorV1::MissingMember { .. }
                | AuthoredStackReorderErrorV1::PageMismatch
                | AuthoredStackReorderErrorV1::InvalidStack => {
                    EditorError::AuthoredStackReorderUnsupported { node_id }
                }
                AuthoredStackReorderErrorV1::BeforeStateMismatch
                | AuthoredStackReorderErrorV1::AfterStateMismatch
                | AuthoredStackReorderErrorV1::TransitionMismatch => {
                    EditorError::StaleAuthoredStack { page_id }
                }
            },
        )?;
        self.consume_canonical_reorder_authored_stack(EditOperation::ReorderAuthoredStack {
            transition,
        })
    }

    pub(super) fn consume_canonical_reorder_authored_stack(
        &mut self,
        operation: EditOperation,
    ) -> Result<EditOperation, EditorError> {
        self.validate_source_identity()?;
        let EditOperation::ReorderAuthoredStack { transition } = &operation else {
            unreachable!("consume_canonical_reorder_authored_stack receives ReorderAuthoredStack")
        };
        let shape = self.authored_shapes.get(&transition.node_id).ok_or(
            EditorError::AuthoredStackReorderUnsupported {
                node_id: transition.node_id,
            },
        )?;
        if shape.page_id != transition.page_id || shape.parent_id != transition.page_id {
            return Err(EditorError::AuthoredStackReorderUnsupported {
                node_id: transition.node_id,
            });
        }
        let current = self.current_authored_stack_v1(transition.page_id);
        let after =
            apply_authored_stack_reorder_forward_v1(&current, transition).map_err(|_| {
                EditorError::StaleAuthoredStack {
                    page_id: transition.page_id,
                }
            })?;
        self.install_authored_stack_v1(after);
        self.undo.push(operation.clone());
        self.redo.clear();
        self.validate_source_identity()?;
        Ok(operation)
    }
}

pub(super) fn page_order_error_to_editor_v1(error: PageOrderErrorV1) -> EditorError {
    match error {
        PageOrderErrorV1::NoChange => EditorError::PageOrderNoChange,
        PageOrderErrorV1::CurrentOrderMismatch
        | PageOrderErrorV1::BeforeStateMismatch
        | PageOrderErrorV1::AfterStateMismatch => EditorError::StalePageOrder,
        other => EditorError::PageOrderUnsupported {
            message: other.to_string(),
        },
    }
}

pub(super) fn authored_stack_operation_page_id_v1(operation: &EditOperation) -> Option<PageId> {
    match operation {
        EditOperation::CreateShape { page_id, .. }
        | EditOperation::CreateLine { page_id, .. }
        | EditOperation::DeleteNode { page_id, .. } => Some(*page_id),
        EditOperation::CreateTable { table } => Some(table.page_id),
        EditOperation::ReorderAuthoredStack { transition } => Some(transition.page_id),
        _ => None,
    }
}

fn install_authored_stack_in_map_v1(
    stacks: &mut BTreeMap<PageId, AuthoredStackV1>,
    stack: AuthoredStackV1,
) {
    if stack.members.is_empty() {
        stacks.remove(&stack.page_id);
    } else {
        stacks.insert(stack.page_id, stack);
    }
}

pub(super) fn apply_authored_stack_history_forward_v1(
    stacks: &mut BTreeMap<PageId, AuthoredStackV1>,
    operation: &EditOperation,
) -> Result<(), EditorError> {
    match operation {
        EditOperation::CreateShape { page_id, .. } => {
            let shape = authored_shape_from_operation(operation)
                .expect("CreateShape reconstructs authored shape");
            let before = stacks
                .get(page_id)
                .cloned()
                .unwrap_or_else(|| AuthoredStackV1::empty(*page_id));
            let transition = plan_create_shape_append_v1(&before, &shape)
                .map_err(|_| EditorError::StaleAuthoredStack { page_id: *page_id })?;
            let after = apply_authored_stack_transition_forward_v1(&before, &transition)
                .map_err(|_| EditorError::StaleAuthoredStack { page_id: *page_id })?;
            install_authored_stack_in_map_v1(stacks, after);
        }
        EditOperation::CreateLine { page_id, .. } => {
            let line = authored_line_from_operation(operation)
                .expect("CreateLine reconstructs authored line");
            let before = stacks
                .get(page_id)
                .cloned()
                .unwrap_or_else(|| AuthoredStackV1::empty(*page_id));
            let transition = plan_create_line_append_v1(&before, &line)
                .map_err(|_| EditorError::StaleAuthoredStack { page_id: *page_id })?;
            let after = apply_authored_stack_transition_forward_v1(&before, &transition)
                .map_err(|_| EditorError::StaleAuthoredStack { page_id: *page_id })?;
            install_authored_stack_in_map_v1(stacks, after);
        }
        EditOperation::CreateTable { table } => {
            let before = stacks
                .get(&table.page_id)
                .cloned()
                .unwrap_or_else(|| AuthoredStackV1::empty(table.page_id));
            let transition = plan_create_table_append_v1(&before, table.node_id, table.page_id)
                .map_err(|_| EditorError::StaleAuthoredStack {
                    page_id: table.page_id,
                })?;
            let after =
                apply_authored_stack_transition_forward_v1(&before, &transition).map_err(|_| {
                    EditorError::StaleAuthoredStack {
                        page_id: table.page_id,
                    }
                })?;
            install_authored_stack_in_map_v1(stacks, after);
        }
        EditOperation::DeleteNode {
            page_id, before, ..
        } => {
            let stack = stacks
                .get(page_id)
                .cloned()
                .unwrap_or_else(|| AuthoredStackV1::empty(*page_id));
            let transition = plan_delete_shape_remove_v1(&stack, before)
                .map_err(|_| EditorError::StaleAuthoredStack { page_id: *page_id })?;
            let after = apply_authored_stack_transition_forward_v1(&stack, &transition)
                .map_err(|_| EditorError::StaleAuthoredStack { page_id: *page_id })?;
            install_authored_stack_in_map_v1(stacks, after);
        }
        EditOperation::ReorderAuthoredStack { transition } => {
            let current = stacks
                .get(&transition.page_id)
                .cloned()
                .unwrap_or_else(|| AuthoredStackV1::empty(transition.page_id));
            let after =
                apply_authored_stack_reorder_forward_v1(&current, transition).map_err(|_| {
                    EditorError::StaleAuthoredStack {
                        page_id: transition.page_id,
                    }
                })?;
            install_authored_stack_in_map_v1(stacks, after);
        }
        _ => {}
    }
    Ok(())
}

pub(super) fn derive_authored_stacks_from_operations_v1(
    operations: &[EditOperation],
) -> Result<BTreeMap<PageId, AuthoredStackV1>, EditorError> {
    let mut stacks = BTreeMap::new();
    for operation in operations {
        apply_authored_stack_history_forward_v1(&mut stacks, operation)?;
    }
    Ok(stacks)
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

pub(super) fn append_blank_page_error_to_editor_v1(error: AppendBlankPageErrorV1) -> EditorError {
    match error {
        AppendBlankPageErrorV1::IdentityCollision { page_id } => {
            EditorError::AuthoredPageIdentityConflict { page_id }
        }
        AppendBlankPageErrorV1::BeforeStateMismatch
        | AppendBlankPageErrorV1::AfterStateMismatch
        | AppendBlankPageErrorV1::CurrentCustomerOrderMismatch
        | AppendBlankPageErrorV1::PageStateMismatch => EditorError::StalePageAppend,
        other => EditorError::PageAppendUnsupported {
            message: format!("{other:?}"),
        },
    }
}

pub(super) fn display_page_append_error_v1(
    error: &EditorError,
    formatter: &mut fmt::Formatter<'_>,
) -> fmt::Result {
    match error {
        EditorError::PageAppendUnsupported { message } => {
            write!(formatter, "append blank page is unsupported: {message}")
        }
        EditorError::StalePageAppend => formatter.write_str(
            "current document/page membership no longer matches the append-page precondition",
        ),
        _ => unreachable!("page-append display helper receives only page-append errors"),
    }
}

pub(super) fn delete_blank_authored_page_error_to_editor_v1(
    error: DeleteBlankAuthoredPageErrorV1,
) -> EditorError {
    match error {
        DeleteBlankAuthoredPageErrorV1::BeforeStateMismatch
        | DeleteBlankAuthoredPageErrorV1::AfterStateMismatch
        | DeleteBlankAuthoredPageErrorV1::PageStateMismatch
        | DeleteBlankAuthoredPageErrorV1::RemovalSlotMismatch
        | DeleteBlankAuthoredPageErrorV1::CurrentCustomerOrderMismatch => {
            EditorError::StalePageDelete
        }
        other => EditorError::PageDeleteUnsupported {
            message: format!("{other:?}"),
        },
    }
}

pub(super) fn display_page_delete_error_v1(
    error: &EditorError,
    formatter: &mut fmt::Formatter<'_>,
) -> fmt::Result {
    match error {
        EditorError::PageDeleteUnsupported { message } => {
            write!(formatter, "delete blank authored page is unsupported: {message}")
        }
        EditorError::StalePageDelete => formatter.write_str(
            "current document/page membership no longer matches the delete-page precondition",
        ),
        _ => unreachable!("page-delete display helper receives only page-delete errors"),
    }
}

// Authored Page identity is an ordering-adjacent durable history primitive.
impl EditorSession {
    pub fn authored_page_identities_v1(&self) -> BTreeMap<PageId, AuthoredPageIdentityV1> {
        self.undo
            .iter()
            .filter_map(|operation| match operation {
                EditOperation::RegisterAuthoredPageIdentityV1 { identity } => {
                    Some((identity.page_id, *identity))
                }
                EditOperation::AppendBlankPageV1 { transition } => {
                    Some((transition.identity.page_id, transition.identity))
                }
                _ => None,
            })
            .collect()
    }

    pub fn authored_customer_page_ids_v1(&self) -> Vec<PageId> {
        let mut active = Vec::<PageId>::new();
        for operation in &self.undo {
            match operation {
                EditOperation::AppendBlankPageV1 { transition } => {
                    active.push(transition.identity.page_id);
                }
                EditOperation::DeleteBlankAuthoredPageV1 { transition } => {
                    active.retain(|page_id| *page_id != transition.identity.page_id);
                }
                _ => {}
            }
        }
        active
    }

    fn has_page_lifecycle_history_v1(&self, page_id: PageId) -> bool {
        self.undo.iter().chain(self.redo.iter()).any(|operation| match operation {
            EditOperation::AppendBlankPageV1 { transition } => {
                transition.identity.page_id == page_id
            }
            EditOperation::DeleteBlankAuthoredPageV1 { transition } => {
                transition.identity.page_id == page_id
            }
            _ => false,
        })
    }

    fn page_has_resolved_node_membership_v1(&self, page_id: PageId) -> bool {
        let target = page_id.into_canonical();
        self.graph.nodes.values().any(|node| {
            let mut current = node.header.parent_id;
            let mut seen = BTreeSet::new();
            loop {
                if current == target {
                    return true;
                }
                if !seen.insert(current) {
                    return true;
                }
                let node_id = NodeId::from_canonical(current);
                let Some(parent) = self.graph.nodes.get(&node_id) else {
                    return false;
                };
                current = parent.header.parent_id;
            }
        })
    }

    pub fn effective_customer_page_order_v1(
        &self,
        source_qualified_page_ids: &[PageId],
    ) -> Result<Vec<PageId>, EditorError> {
        self.validate_source_identity()?;
        let mut combined = Vec::with_capacity(
            source_qualified_page_ids.len() + self.authored_customer_page_ids_v1().len(),
        );
        let mut seen = BTreeSet::new();
        for page_id in source_qualified_page_ids
            .iter()
            .copied()
            .chain(self.authored_customer_page_ids_v1())
        {
            if !seen.insert(page_id) {
                return Err(EditorError::PageAppendUnsupported {
                    message: format!(
                        "effective customer membership repeats PageId {}",
                        page_id.as_canonical()
                    ),
                });
            }
            if !self.graph.pages.contains_key(&page_id) {
                return Err(EditorError::PageAppendUnsupported {
                    message: format!(
                        "effective customer PageId {} is absent from the current semantic page map",
                        page_id.as_canonical()
                    ),
                });
            }
            combined.push(page_id);
        }
        qualified_page_order_v1(&self.graph.document.pages, &combined)
            .map_err(page_order_error_to_editor_v1)
    }

    pub fn register_authored_page_identity_v1(
        &mut self,
        identity: AuthoredPageIdentityV1,
    ) -> Result<EditOperation, EditorError> {
        self.validate_source_identity()?;
        if validate_authored_page_identity_v1(&identity).is_err() {
            return Err(EditorError::AuthoredPageIdentityInvalid {
                page_id: identity.page_id,
            });
        }

        if self.graph.pages.contains_key(&identity.page_id)
            || self
                .authored_page_identities_v1()
                .contains_key(&identity.page_id)
        {
            return Err(EditorError::AuthoredPageIdentityConflict {
                page_id: identity.page_id,
            });
        }

        let operation = EditOperation::RegisterAuthoredPageIdentityV1 { identity };
        self.undo.push(operation.clone());
        self.redo.clear();
        self.validate_source_identity()?;
        Ok(operation)
    }

    pub fn append_blank_page_v1(
        &mut self,
        source_qualified_page_ids: Vec<PageId>,
        identity: AuthoredPageIdentityV1,
        size: Size2D,
        bleed: Option<BoxEdges>,
        margins: Option<BoxEdges>,
    ) -> Result<EditOperation, EditorError> {
        self.validate_source_identity()?;
        if self.project_identity.is_none() {
            return Err(EditorError::PageAppendUnsupported {
                message: "durable project identity is required".to_owned(),
            });
        }
        if validate_authored_page_identity_v1(&identity).is_err() {
            return Err(EditorError::AuthoredPageIdentityInvalid {
                page_id: identity.page_id,
            });
        }
        if self.graph.pages.contains_key(&identity.page_id) {
            return Err(EditorError::AuthoredPageIdentityConflict {
                page_id: identity.page_id,
            });
        }
        if self.has_page_lifecycle_history_v1(identity.page_id) {
            return Err(EditorError::AuthoredPageIdentityConflict {
                page_id: identity.page_id,
            });
        }
        if let Some(existing) = self.authored_page_identities_v1().get(&identity.page_id) {
            if *existing != identity {
                return Err(EditorError::AuthoredPageIdentityConflict {
                    page_id: identity.page_id,
                });
            }
        }

        let current_customer_page_ids =
            self.effective_customer_page_order_v1(&source_qualified_page_ids)?;
        let page = Page {
            id: identity.page_id,
            size,
            bleed,
            margins,
            children: Vec::new(),
            extensions: Vec::new(),
        };
        let existing_page_ids = self.graph.pages.keys().copied().collect::<BTreeSet<_>>();
        let transition = plan_append_blank_page_v1(
            self.graph.document.id,
            &self.graph.document.pages,
            &existing_page_ids,
            &current_customer_page_ids,
            identity,
            page,
        )
        .map_err(append_blank_page_error_to_editor_v1)?;
        self.consume_canonical_append_blank_page_v1(transition)
    }

    pub(super) fn consume_canonical_append_blank_page_v1(
        &mut self,
        expected: AppendBlankPageTransitionV1,
    ) -> Result<EditOperation, EditorError> {
        self.validate_source_identity()?;
        if self.project_identity.is_none() {
            return Err(EditorError::PageAppendUnsupported {
                message: "durable project identity is required".to_owned(),
            });
        }
        if self.graph.pages.contains_key(&expected.identity.page_id) {
            return Err(EditorError::AuthoredPageIdentityConflict {
                page_id: expected.identity.page_id,
            });
        }
        if let Some(existing) = self
            .authored_page_identities_v1()
            .get(&expected.identity.page_id)
        {
            if *existing != expected.identity {
                return Err(EditorError::AuthoredPageIdentityConflict {
                    page_id: expected.identity.page_id,
                });
            }
        }

        let existing_page_ids = self.graph.pages.keys().copied().collect::<BTreeSet<_>>();
        let planned = plan_append_blank_page_v1(
            self.graph.document.id,
            &self.graph.document.pages,
            &existing_page_ids,
            &expected.before_customer_page_ids,
            expected.identity,
            expected.page.clone(),
        )
        .map_err(append_blank_page_error_to_editor_v1)?;
        if planned != expected {
            return Err(EditorError::StalePageAppend);
        }

        let operation = EditOperation::AppendBlankPageV1 {
            transition: expected,
        };
        apply_forward(&mut self.graph, &operation)?;
        self.undo.push(operation.clone());
        self.redo.clear();
        self.validate_source_identity()?;
        Ok(operation)
    }

    pub fn delete_blank_authored_page_v1(
        &mut self,
        source_qualified_page_ids: Vec<PageId>,
        page_id: PageId,
    ) -> Result<EditOperation, EditorError> {
        self.validate_source_identity()?;
        if self.project_identity.is_none() {
            return Err(EditorError::PageDeleteUnsupported {
                message: "durable project identity is required".to_owned(),
            });
        }
        let identity = self
            .authored_page_identities_v1()
            .get(&page_id)
            .copied()
            .ok_or_else(|| EditorError::PageDeleteUnsupported {
                message: format!(
                    "target PageId {} has no authored identity history",
                    page_id.as_canonical()
                ),
            })?;
        if !self.authored_customer_page_ids_v1().contains(&page_id) {
            return Err(EditorError::PageDeleteUnsupported {
                message: format!(
                    "target PageId {} is not active authored customer membership",
                    page_id.as_canonical()
                ),
            });
        }
        if self.page_has_resolved_node_membership_v1(page_id)
            || !self.current_authored_stack_v1(page_id).members.is_empty()
        {
            return Err(EditorError::PageDeleteUnsupported {
                message: format!(
                    "target PageId {} is not empty across resolved/authored membership",
                    page_id.as_canonical()
                ),
            });
        }
        let current_customer_page_ids =
            self.effective_customer_page_order_v1(&source_qualified_page_ids)?;
        let transition = plan_delete_blank_authored_page_v1(
            self.graph.document.id,
            &self.graph.document.pages,
            &self.graph.pages,
            &current_customer_page_ids,
            identity,
        )
        .map_err(delete_blank_authored_page_error_to_editor_v1)?;
        self.consume_canonical_delete_blank_authored_page_v1(transition)
    }

    pub(super) fn consume_canonical_delete_blank_authored_page_v1(
        &mut self,
        expected: DeleteBlankAuthoredPageTransitionV1,
    ) -> Result<EditOperation, EditorError> {
        self.validate_source_identity()?;
        if self.project_identity.is_none() {
            return Err(EditorError::PageDeleteUnsupported {
                message: "durable project identity is required".to_owned(),
            });
        }
        if self.page_has_resolved_node_membership_v1(expected.identity.page_id)
            || !self
                .current_authored_stack_v1(expected.identity.page_id)
                .members
                .is_empty()
        {
            return Err(EditorError::PageDeleteUnsupported {
                message: "target page is not empty across resolved/authored membership".to_owned(),
            });
        }
        let planned = plan_delete_blank_authored_page_v1(
            self.graph.document.id,
            &self.graph.document.pages,
            &self.graph.pages,
            &expected.before_customer_page_ids,
            expected.identity,
        )
        .map_err(delete_blank_authored_page_error_to_editor_v1)?;
        if planned != expected {
            return Err(EditorError::StalePageDelete);
        }
        let operation = EditOperation::DeleteBlankAuthoredPageV1 {
            transition: expected,
        };
        apply_forward(&mut self.graph, &operation)?;
        self.undo.push(operation.clone());
        self.redo.clear();
        self.validate_source_identity()?;
        Ok(operation)
    }
}

#[cfg(test)]
mod authored_page_identity_tests {
    use super::*;
    use crate::{
        AuthoredEntityProvenanceV1, EDITOR_PROJECT_VERSION_V0_24, EditorProject, PubResolvedGraph,
    };
    use pub_model::{Document, LengthEmu, Page, Sha256Digest, Size2D, SourceDescriptor};

    fn source_hash() -> Sha256Digest {
        Sha256Digest::from_bytes([0x5a; 32])
    }

    fn authored_page_id() -> PageId {
        serde_json::from_str("\"01890f4f-1234-7abc-8def-0123456789ab\"")
            .expect("valid UUIDv7 PageId")
    }

    fn source_graph(source_pages: Vec<PageId>) -> PubResolvedGraph {
        let hash = source_hash();
        let pages = source_pages
            .iter()
            .copied()
            .map(|page_id| {
                (
                    page_id,
                    Page {
                        id: page_id,
                        size: Size2D::new(LengthEmu::new(914_400), LengthEmu::new(914_400)),
                        bleed: None,
                        margins: None,
                        children: Vec::new(),
                        extensions: Vec::new(),
                    },
                )
            })
            .collect();

        PubResolvedGraph {
            cdm_version: "0.1".into(),
            resolver_version: pub_reader::PUB_RESOLVER_VERSION_V1.into(),
            source: SourceDescriptor {
                format: "pub".into(),
                format_version: Some("0x2c".into()),
                adapter_version: "pub-rs/test".into(),
                source_hash: hash,
            },
            document: Document {
                id: serde_json::from_str("\"33000000-0000-4000-8000-000000000001\"")
                    .expect("document id"),
                format_origin: "pub".into(),
                source_hash: hash,
                pages: source_pages,
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

    fn identity() -> AuthoredPageIdentityV1 {
        AuthoredPageIdentityV1 {
            page_id: authored_page_id(),
            provenance: AuthoredEntityProvenanceV1::AuthorCreated,
        }
    }

    #[test]
    fn registered_page_identity_is_history_derived_and_source_graph_immutable() {
        let graph = source_graph(Vec::new());
        let source_before = graph.clone();
        let mut session = EditorSession::new(graph).expect("session");

        let operation = session
            .register_authored_page_identity_v1(identity())
            .expect("register authored Page identity");

        assert!(matches!(
            operation,
            EditOperation::RegisterAuthoredPageIdentityV1 { .. }
        ));
        assert_eq!(session.graph(), &source_before);
        assert_eq!(
            session
                .authored_page_identities_v1()
                .get(&authored_page_id()),
            Some(&identity())
        );

        session.undo().expect("undo identity registration");
        assert!(session.authored_page_identities_v1().is_empty());
        assert_eq!(session.graph(), &source_before);

        session.redo().expect("redo identity registration");
        assert_eq!(
            session
                .authored_page_identities_v1()
                .get(&authored_page_id()),
            Some(&identity())
        );
        assert_eq!(session.graph(), &source_before);
    }

    #[test]
    fn authored_page_identity_project_replays_exact_id_and_provenance() {
        let graph = source_graph(Vec::new());
        let mut session = EditorSession::new(graph.clone()).expect("session");
        session
            .register_authored_page_identity_v1(identity())
            .expect("register identity");

        let project = session.project();
        assert_eq!(project.schema_version, EDITOR_PROJECT_VERSION_V0_24);
        let encoded = serde_json::to_vec(&project).expect("serialize project");
        let decoded: EditorProject = serde_json::from_slice(&encoded).expect("deserialize project");

        let mut reopened = EditorSession::new(graph).expect("fresh session");
        reopened.apply_project(&decoded).expect("replay identity");

        assert_eq!(reopened.operations(), decoded.operations.as_slice());
        assert_eq!(
            reopened
                .authored_page_identities_v1()
                .get(&authored_page_id()),
            Some(&identity())
        );
    }

    #[test]
    fn authored_page_identity_rejects_source_page_collision() {
        let page_id = authored_page_id();
        let mut session = EditorSession::new(source_graph(vec![page_id])).expect("session");

        assert_eq!(
            session.register_authored_page_identity_v1(identity()),
            Err(EditorError::AuthoredPageIdentityConflict { page_id })
        );
        assert!(session.operations().is_empty());
    }

    #[test]
    fn authored_page_identity_rejects_duplicate_history_identity() {
        let page_id = authored_page_id();
        let mut session = EditorSession::new(source_graph(Vec::new())).expect("session");
        session
            .register_authored_page_identity_v1(identity())
            .expect("first registration");

        assert_eq!(
            session.register_authored_page_identity_v1(identity()),
            Err(EditorError::AuthoredPageIdentityConflict { page_id })
        );
        assert_eq!(session.operations().len(), 1);
    }
}

#[cfg(test)]
mod authored_page_append_tests {
    use super::*;
    use crate::{
        AuthoredEntityProvenanceV1, EDITOR_PROJECT_VERSION_V0_25, EditorProject, PubResolvedGraph,
    };
    use pub_model::{Document, Sha256Digest, SourceDescriptor};

    fn source_hash() -> Sha256Digest {
        Sha256Digest::from_bytes([0x6b; 32])
    }

    fn page_id(value: &str) -> PageId {
        serde_json::from_str(&format!("\"{value}\"")).expect("valid PageId")
    }

    fn authored_identity() -> AuthoredPageIdentityV1 {
        AuthoredPageIdentityV1 {
            page_id: page_id("01890f4f-1234-7abc-8def-0123456789ab"),
            provenance: AuthoredEntityProvenanceV1::AuthorCreated,
        }
    }

    fn source_page(page_id: PageId) -> Page {
        Page {
            id: page_id,
            size: Size2D::new(LengthEmu::new(914_400), LengthEmu::new(1_828_800)),
            bleed: None,
            margins: None,
            children: Vec::new(),
            extensions: Vec::new(),
        }
    }

    fn source_graph(raw_pages: Vec<PageId>) -> PubResolvedGraph {
        let hash = source_hash();
        let pages = raw_pages
            .iter()
            .copied()
            .map(|page_id| (page_id, source_page(page_id)))
            .collect();

        PubResolvedGraph {
            cdm_version: "0.1".into(),
            resolver_version: pub_reader::PUB_RESOLVER_VERSION_V1.into(),
            source: SourceDescriptor {
                format: "pub".into(),
                format_version: Some("0x2c".into()),
                adapter_version: "pub-rs/test".into(),
                source_hash: hash,
            },
            document: Document {
                id: serde_json::from_str("\"33000000-0000-4000-8000-000000000001\"")
                    .expect("document id"),
                format_origin: "pub".into(),
                source_hash: hash,
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
    fn append_blank_page_preserves_raw_carriers_and_roundtrips_undo_redo() {
        let master = page_id("11111111-1111-4111-8111-111111111111");
        let a = page_id("22222222-2222-4222-8222-222222222222");
        let service = page_id("33333333-3333-4333-8333-333333333333");
        let b = page_id("44444444-4444-4444-8444-444444444444");
        let carrier = page_id("55555555-5555-4555-8555-555555555555");
        let identity = authored_identity();
        let mut session = EditorSession::new(source_graph(vec![master, a, service, b, carrier]))
            .expect("session");
        let source_hash_before = session.source_hash();

        session
            .append_blank_page_v1(
                vec![a, b],
                identity,
                Size2D::new(LengthEmu::new(2_000_000), LengthEmu::new(3_000_000)),
                None,
                None,
            )
            .expect("append");

        assert_eq!(
            session.graph().document.pages,
            vec![master, a, service, b, identity.page_id, carrier]
        );
        assert!(session.graph().pages.contains_key(&identity.page_id));
        assert_eq!(
            session
                .effective_customer_page_order_v1(&[a, b])
                .expect("effective"),
            vec![a, b, identity.page_id]
        );
        assert_eq!(session.source_hash(), source_hash_before);

        session.undo().expect("undo");
        assert_eq!(
            session.graph().document.pages,
            vec![master, a, service, b, carrier]
        );
        assert!(!session.graph().pages.contains_key(&identity.page_id));
        assert_eq!(
            session
                .effective_customer_page_order_v1(&[a, b])
                .expect("effective"),
            vec![a, b]
        );

        session.redo().expect("redo");
        assert_eq!(
            session.graph().document.pages,
            vec![master, a, service, b, identity.page_id, carrier]
        );
        assert!(session.graph().pages.contains_key(&identity.page_id));
    }

    #[test]
    fn standalone_identity_is_not_membership_and_can_be_reused_by_append() {
        let a = page_id("22222222-2222-4222-8222-222222222222");
        let identity = authored_identity();
        let mut session = EditorSession::new(source_graph(vec![a])).expect("session");

        session
            .register_authored_page_identity_v1(identity)
            .expect("register identity");
        assert_eq!(
            session
                .effective_customer_page_order_v1(&[a])
                .expect("effective"),
            vec![a]
        );

        session
            .append_blank_page_v1(
                vec![a],
                identity,
                Size2D::new(LengthEmu::new(2_000_000), LengthEmu::new(3_000_000)),
                None,
                None,
            )
            .expect("append using pre-registered identity");
        assert_eq!(
            session
                .effective_customer_page_order_v1(&[a])
                .expect("effective"),
            vec![a, identity.page_id]
        );

        session.undo().expect("undo append only");
        assert!(
            session
                .authored_page_identities_v1()
                .contains_key(&identity.page_id)
        );
        assert_eq!(
            session
                .effective_customer_page_order_v1(&[a])
                .expect("effective"),
            vec![a]
        );
    }

    #[test]
    fn append_project_replays_exact_membership_geometry_and_identity() {
        let a = page_id("22222222-2222-4222-8222-222222222222");
        let identity = authored_identity();
        let graph = source_graph(vec![a]);
        let mut session = EditorSession::new(graph.clone()).expect("session");
        session
            .append_blank_page_v1(
                vec![a],
                identity,
                Size2D::new(LengthEmu::new(2_000_000), LengthEmu::new(3_000_000)),
                None,
                None,
            )
            .expect("append");

        let project = session.project();
        assert_eq!(project.schema_version, EDITOR_PROJECT_VERSION_V0_25);
        let bytes = serde_json::to_vec(&project).expect("serialize");
        let decoded: EditorProject = serde_json::from_slice(&bytes).expect("deserialize");

        let mut reopened = EditorSession::new(graph).expect("fresh session");
        reopened.apply_project(&decoded).expect("replay");
        assert_eq!(reopened.operations(), decoded.operations.as_slice());
        assert_eq!(
            reopened.graph().document.pages,
            session.graph().document.pages
        );
        assert_eq!(
            reopened.graph().pages.get(&identity.page_id),
            session.graph().pages.get(&identity.page_id)
        );
        assert_eq!(
            reopened
                .effective_customer_page_order_v1(&[a])
                .expect("effective"),
            vec![a, identity.page_id]
        );
    }

    #[test]
    fn append_blank_page_rejects_zero_customer_input() {
        let identity = authored_identity();
        let mut session = EditorSession::new(source_graph(Vec::new())).expect("session");
        assert!(
            session
                .append_blank_page_v1(
                    Vec::new(),
                    identity,
                    Size2D::new(LengthEmu::new(2_000_000), LengthEmu::new(3_000_000)),
                    None,
                    None,
                )
                .is_err()
        );
        assert!(session.operations().is_empty());
    }

    fn page_hex(page_id: PageId) -> String {
        page_id.as_canonical().to_string().replace('-', "")
    }

    fn read_zip_text(bytes: &[u8], path: &str) -> String {
        use std::io::Read as _;

        let mut archive =
            zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("open exported ZIP");
        let mut entry = archive
            .by_name(path)
            .expect("expected exported package part");
        let mut text = String::new();
        entry
            .read_to_string(&mut text)
            .expect("read exported XML part");
        text
    }

    #[test]
    fn appended_blank_page_reaches_idml_and_odg_with_order_and_geometry() {
        let source = page_id("22222222-2222-4222-8222-222222222222");
        let identity = authored_identity();
        let mut session = EditorSession::new(source_graph(vec![source])).expect("session");
        let appended_size = Size2D::new(
            LengthEmu::new(200 * pub_model::EMU_PER_POINT),
            LengthEmu::new(300 * pub_model::EMU_PER_POINT),
        );

        session
            .append_blank_page_v1(vec![source], identity, appended_size, None, None)
            .expect("append blank page");

        let source_hex = page_hex(source);
        let appended_hex = page_hex(identity.page_id);

        let idml = session
            .export_editable(crate::EditorEditableTarget::Idml, "append-blank-page-idml")
            .expect("export appended page to IDML");
        let designmap = read_zip_text(&idml.bytes, "designmap.xml");
        let source_spread = format!("Spreads/Spread_usp{source_hex}.xml");
        let appended_spread = format!("Spreads/Spread_usp{appended_hex}.xml");
        assert!(
            designmap
                .find(&source_spread)
                .expect("source spread in designmap")
                < designmap
                    .find(&appended_spread)
                    .expect("appended spread in designmap"),
            "IDML designmap must preserve canonical page order"
        );
        let appended_spread_xml = read_zip_text(&idml.bytes, &appended_spread);
        assert!(appended_spread_xml.contains(&format!(
            "<Page Self=\"up{appended_hex}\" GeometricBounds=\"0 0 300 200\""
        )));

        let odg = session
            .export_editable(crate::EditorEditableTarget::Odg, "append-blank-page-odg")
            .expect("export appended page to ODG");
        let content = read_zip_text(&odg.bytes, "content.xml");
        let source_page = format!("draw:name=\"Page_{source_hex}\"");
        let appended_page = format!("draw:name=\"Page_{appended_hex}\"");
        assert_eq!(content.matches("<draw:page ").count(), 2);
        assert!(
            content.find(&source_page).expect("source ODG page")
                < content.find(&appended_page).expect("appended ODG page"),
            "ODG content.xml must preserve canonical page order"
        );

        let styles = read_zip_text(&odg.bytes, "styles.xml");
        let appended_layout = format!("<style:page-layout style:name=\"PM_{appended_hex}\">");
        let start = styles
            .find(&appended_layout)
            .expect("appended ODG page layout");
        let tail = &styles[start..];
        let end = tail
            .find("</style:page-layout>")
            .expect("appended ODG page layout end");
        let layout = &tail[..end];
        assert!(layout.contains("fo:page-width=\"200pt\" fo:page-height=\"300pt\""));
    }
}
