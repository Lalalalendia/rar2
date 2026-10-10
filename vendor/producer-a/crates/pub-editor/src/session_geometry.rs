//! Existing-node geometry session orchestration.
//!
//! This owner contains existing-node geometry plus bounded page/stack ordering
//! session orchestration. Paragraph formatting, authored duplication, table
//! mutation, image/text session logic, frame topology, and export remain outside.

use super::*;
use pub_editor_authoring_core::{
    DeleteAuthoredRectanglePageStateV1, DeleteAuthoredRectanglePageTransitionV1,
    DuplicateAuthoredRectanglePageStateV1, DuplicateAuthoredRectanglePageTransitionV1,
    DuplicateAuthoredRectanglesPageTransitionV1, MAX_DUPLICATED_AUTHORED_RECTANGLES_PAGE_V1,
    apply_delete_authored_rectangle_page_forward_v1,
    apply_delete_authored_rectangle_page_inverse_v1,
    apply_duplicate_authored_rectangle_page_forward_v1,
    apply_duplicate_authored_rectangle_page_inverse_v1,
    apply_duplicate_authored_rectangles_page_forward_v1,
    apply_duplicate_authored_rectangles_page_inverse_v1, plan_delete_authored_rectangle_page_v1,
    plan_duplicate_authored_rectangle_page_v1, plan_duplicate_authored_rectangles_page_v1,
};
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
        EditOperation::DeleteAuthoredRectanglePageV1 { transition } => {
            Some(transition.page.identity.page_id)
        }
        EditOperation::DuplicateAuthoredRectanglePageV1 { transition } => {
            Some(transition.page.destination_identity.page_id)
        }
        EditOperation::DuplicateAuthoredRectanglesPageV1 { transition } => {
            Some(transition.page.destination_identity.page_id)
        }
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
        EditOperation::DeleteAuthoredRectanglePageV1 { transition } => {
            let page_id = transition.page.identity.page_id;
            let current = stacks
                .get(&page_id)
                .cloned()
                .unwrap_or_else(|| AuthoredStackV1::empty(page_id));
            let after = apply_authored_stack_transition_forward_v1(&current, &transition.stack)
                .map_err(|_| EditorError::StaleAuthoredStack { page_id })?;
            install_authored_stack_in_map_v1(stacks, after);
        }
        EditOperation::DuplicateAuthoredRectanglePageV1 { transition } => {
            let page_id = transition.page.destination_identity.page_id;
            let current = stacks
                .get(&page_id)
                .cloned()
                .unwrap_or_else(|| AuthoredStackV1::empty(page_id));
            let after = apply_authored_stack_transition_forward_v1(&current, &transition.stack)
                .map_err(|_| EditorError::StaleAuthoredStack { page_id })?;
            install_authored_stack_in_map_v1(stacks, after);
        }
        EditOperation::DuplicateAuthoredRectanglesPageV1 { transition } => {
            let page_id = transition.page.destination_identity.page_id;
            let mut current = stacks
                .get(&page_id)
                .cloned()
                .unwrap_or_else(|| AuthoredStackV1::empty(page_id));
            for stack in &transition.stacks {
                current = apply_authored_stack_transition_forward_v1(&current, stack)
                    .map_err(|_| EditorError::StaleAuthoredStack { page_id })?;
            }
            install_authored_stack_in_map_v1(stacks, current);
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
            write!(
                formatter,
                "delete blank authored page is unsupported: {message}"
            )
        }
        EditorError::StalePageDelete => formatter.write_str(
            "current document/page membership no longer matches the delete-page precondition",
        ),
        _ => unreachable!("page-delete display helper receives only page-delete errors"),
    }
}

pub(super) fn duplicate_blank_page_error_to_editor_v1(
    error: DuplicateBlankPageErrorV1,
) -> EditorError {
    match error {
        DuplicateBlankPageErrorV1::BeforeStateMismatch
        | DuplicateBlankPageErrorV1::AfterStateMismatch
        | DuplicateBlankPageErrorV1::SourcePageStateMismatch
        | DuplicateBlankPageErrorV1::DestinationPageStateMismatch
        | DuplicateBlankPageErrorV1::InsertionSlotMismatch
        | DuplicateBlankPageErrorV1::CurrentCustomerOrderMismatch => {
            EditorError::StalePageDuplicate
        }
        other => EditorError::PageDuplicateUnsupported {
            message: format!("{other:?}"),
        },
    }
}

pub(super) fn display_page_duplicate_error_v1(
    error: &EditorError,
    formatter: &mut fmt::Formatter<'_>,
) -> fmt::Result {
    match error {
        EditorError::PageDuplicateUnsupported { message } => {
            write!(formatter, "duplicate blank page is unsupported: {message}")
        }
        EditorError::StalePageDuplicate => formatter.write_str(
            "current document/page membership no longer matches the duplicate-page precondition",
        ),
        _ => unreachable!("page-duplicate display helper receives only duplicate errors"),
    }
}

pub(super) fn insert_blank_page_after_error_to_editor_v1(
    error: InsertBlankPageAfterErrorV1,
) -> EditorError {
    match error {
        InsertBlankPageAfterErrorV1::BeforeStateMismatch
        | InsertBlankPageAfterErrorV1::AfterStateMismatch
        | InsertBlankPageAfterErrorV1::InsertionSlotMismatch
        | InsertBlankPageAfterErrorV1::CurrentCustomerOrderMismatch
        | InsertBlankPageAfterErrorV1::PageStateMismatch => EditorError::StalePageInsert,
        other => EditorError::PageInsertUnsupported {
            message: format!("{other:?}"),
        },
    }
}

pub(super) fn display_page_insert_error_v1(
    error: &EditorError,
    formatter: &mut fmt::Formatter<'_>,
) -> fmt::Result {
    match error {
        EditorError::PageInsertUnsupported { message } => {
            write!(
                formatter,
                "insert blank page after is unsupported: {message}"
            )
        }
        EditorError::StalePageInsert => formatter.write_str(
            "current document/page membership no longer matches insert-page preconditions",
        ),
        _ => unreachable!("insert-page display helper receives only insert-page errors"),
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
                EditOperation::DuplicateBlankPageV1 { transition } => Some((
                    transition.destination_identity.page_id,
                    transition.destination_identity,
                )),
                EditOperation::DuplicateAuthoredRectanglePageV1 { transition } => Some((
                    transition.page.destination_identity.page_id,
                    transition.page.destination_identity,
                )),
                EditOperation::DuplicateAuthoredRectanglesPageV1 { transition } => Some((
                    transition.page.destination_identity.page_id,
                    transition.page.destination_identity,
                )),
                EditOperation::InsertBlankPageAfterV1 { transition } => {
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
                EditOperation::DuplicateBlankPageV1 { transition } => {
                    active.push(transition.destination_identity.page_id);
                }
                EditOperation::DuplicateAuthoredRectanglePageV1 { transition } => {
                    active.push(transition.page.destination_identity.page_id);
                }
                EditOperation::DuplicateAuthoredRectanglesPageV1 { transition } => {
                    active.push(transition.page.destination_identity.page_id);
                }
                EditOperation::InsertBlankPageAfterV1 { transition } => {
                    active.push(transition.identity.page_id);
                }
                EditOperation::DeleteBlankAuthoredPageV1 { transition } => {
                    active.retain(|page_id| *page_id != transition.identity.page_id);
                }
                EditOperation::DeleteAuthoredRectanglePageV1 { transition } => {
                    active.retain(|id| *id != transition.page.identity.page_id);
                }
                _ => {}
            }
        }
        active
    }

    fn has_page_lifecycle_history_v1(&self, page_id: PageId) -> bool {
        self.undo
            .iter()
            .chain(self.redo.iter())
            .any(|operation| match operation {
                EditOperation::AppendBlankPageV1 { transition } => {
                    transition.identity.page_id == page_id
                }
                EditOperation::DeleteBlankAuthoredPageV1 { transition } => {
                    transition.identity.page_id == page_id
                }
                EditOperation::DeleteAuthoredRectanglePageV1 { transition } => {
                    transition.page.identity.page_id == page_id
                }
                EditOperation::DuplicateBlankPageV1 { transition } => {
                    transition.destination_identity.page_id == page_id
                }
                EditOperation::DuplicateAuthoredRectanglePageV1 { transition } => {
                    transition.page.destination_identity.page_id == page_id
                }
                EditOperation::DuplicateAuthoredRectanglesPageV1 { transition } => {
                    transition.page.destination_identity.page_id == page_id
                }
                EditOperation::InsertBlankPageAfterV1 { transition } => {
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

    /// Read-only admission for the exact same bounded deletion as the command.
    /// No EditorSession clone, revision, history or graph mutation per UI frame.
    pub fn can_delete_blank_authored_page_v1(
        &self,
        source_qualified_page_ids: &[PageId],
        page_id: PageId,
    ) -> bool {
        self.plan_delete_blank_authored_page_from_session_v1(source_qualified_page_ids, page_id)
            .is_ok()
    }

    fn plan_delete_blank_authored_page_from_session_v1(
        &self,
        source_qualified_page_ids: &[PageId],
        page_id: PageId,
    ) -> Result<DeleteBlankAuthoredPageTransitionV1, EditorError> {
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
            self.effective_customer_page_order_v1(source_qualified_page_ids)?;
        plan_delete_blank_authored_page_v1(
            self.graph.document.id,
            &self.graph.document.pages,
            &self.graph.pages,
            &current_customer_page_ids,
            identity,
        )
        .map_err(delete_blank_authored_page_error_to_editor_v1)
    }

    /// Read-only admission for an AuthorCreated customer Page containing
    /// exactly one independent AuthorCreated Rectangle.
    /// No revision or history is consumed by this query.
    pub fn can_delete_authored_rectangle_page_v1(
        &self,
        source_qualified_page_ids: &[PageId],
        page_id: PageId,
    ) -> bool {
        self.plan_delete_authored_rectangle_page_from_session_v1(source_qualified_page_ids, page_id)
            .is_ok()
    }

    /// Admission-only seam for the next versioned EditorSession operation.
    ///
    /// An authored Rectangle is an overlay, not a Page.children entry. The
    /// source-neutral planner deliberately cannot prove absence of resolved
    /// nodes, other authored entity types, or references to the overlay from
    /// the resolved graph. This runtime boundary MUST prove those facts.
    pub(super) fn plan_delete_authored_rectangle_page_from_session_v1(
        &self,
        source_qualified_page_ids: &[PageId],
        page_id: PageId,
    ) -> Result<DeleteAuthoredRectanglePageTransitionV1, EditorError> {
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
                message: "target PageId lacks active authored identity".to_owned(),
            })?;
        if !self.authored_customer_page_ids_v1().contains(&page_id) {
            return Err(EditorError::PageDeleteUnsupported {
                message: "target PageId is not active authored customer membership".to_owned(),
            });
        }

        let shapes_on_target = self
            .authored_shapes
            .iter()
            .filter_map(|(id, shape)| {
                (shape.page_id == page_id || shape.parent_id == page_id).then_some(*id)
            })
            .collect::<BTreeSet<_>>();
        // Other pages' authored lanes cannot reference a shape that we remove.
        let foreign_stack_reference = self.authored_stacks.iter().any(|(other_page, stack)| {
            *other_page != page_id && stack.members.iter().any(|id| shapes_on_target.contains(id))
        });
        // Graph descendants whose parent is an authored overlay may not reach
        // the Page via graph.nodes alone. Walk both the Page and overlay roots.
        let dependent_graph_node = self.graph.nodes.values().any(|node| {
            let mut parent = node.header.parent_id;
            let mut visited = BTreeSet::new();
            loop {
                if parent == page_id.into_canonical()
                    || shapes_on_target.contains(&NodeId::from_canonical(parent))
                    || !visited.insert(parent)
                {
                    return true;
                }
                let Some(ancestor) = self.graph.nodes.get(&NodeId::from_canonical(parent)) else {
                    return false;
                };
                parent = ancestor.header.parent_id;
            }
        });
        let foreign_or_unproven_membership = self.page_has_resolved_node_membership_v1(page_id)
            || dependent_graph_node
            || foreign_stack_reference
            || self
                .authored_lines
                .values()
                .any(|line| line.page_id == page_id || line.parent_id == page_id);
        let customer_pages = self.effective_customer_page_order_v1(source_qualified_page_ids)?;
        let state = DeleteAuthoredRectanglePageStateV1 {
            document_pages: self.graph.document.pages.clone(),
            pages: self.graph.pages.clone(),
            authored_shapes: self.authored_shapes.clone(),
            authored_stack: self.current_authored_stack_v1(page_id),
        };
        plan_delete_authored_rectangle_page_v1(
            self.graph.document.id,
            &state,
            &customer_pages,
            identity,
            foreign_or_unproven_membership,
        )
        .map_err(|error| EditorError::PageDeleteUnsupported {
            message: format!("atomic rectangle-page admission rejected: {error:?}"),
        })
    }

    /// Commit the Page, one Rectangle, and its authored stack as a single
    /// persistent EditOperation. Native Publisher bytes are never rewritten.
    pub fn delete_authored_rectangle_page_v1(
        &mut self,
        source_qualified_page_ids: Vec<PageId>,
        page_id: PageId,
    ) -> Result<EditOperation, EditorError> {
        let planned = self.plan_delete_authored_rectangle_page_from_session_v1(
            &source_qualified_page_ids,
            page_id,
        )?;
        self.consume_canonical_delete_authored_rectangle_page_v1(planned)
    }

    /// A replay-safe immutable-admission owner. External qualified source
    /// membership is recoverable from the recorded customer order by removing
    /// the active, history-proven authored customer identities.
    pub(super) fn consume_canonical_delete_authored_rectangle_page_v1(
        &mut self,
        expected: DeleteAuthoredRectanglePageTransitionV1,
    ) -> Result<EditOperation, EditorError> {
        let authored = self
            .authored_customer_page_ids_v1()
            .into_iter()
            .collect::<BTreeSet<_>>();
        let sources = expected
            .page
            .before_customer_page_ids
            .iter()
            .copied()
            .filter(|page_id| !authored.contains(page_id))
            .collect::<Vec<_>>();
        let planned = self.plan_delete_authored_rectangle_page_from_session_v1(
            &sources,
            expected.page.identity.page_id,
        )?;
        if planned != expected {
            return Err(EditorError::StalePageDelete);
        }
        let mut graph = self.graph.clone();
        let mut shapes = self.authored_shapes.clone();
        apply_authored_rectangle_page_history_candidate_v1(
            &mut graph,
            &mut shapes,
            self.current_authored_stack_v1(expected.page.identity.page_id),
            &expected,
            true,
        )?;
        // Publish all three authorities together only after a complete dry run.
        self.graph = graph;
        self.authored_shapes = shapes;
        self.install_authored_stack_v1(expected.stack.after.clone());
        let operation = EditOperation::DeleteAuthoredRectanglePageV1 {
            transition: expected,
        };
        self.undo.push(operation.clone());
        self.redo.clear();
        self.validate_source_identity()?;
        Ok(operation)
    }

    /// Non-mutating availability query. The allocated test identities never
    /// enter the history; no user-visible operation is synthesized by probing.
    pub fn can_duplicate_authored_rectangle_page_v1(
        &self,
        source_qualified_page_ids: &[PageId],
        source_page_id: PageId,
    ) -> bool {
        let destination = AuthoredPageIdentityV1 {
            page_id: PageId::from_canonical(pub_model::new_editor_canonical_id()),
            provenance: AuthoredEntityProvenanceV1::AuthorCreated,
        };
        let node_id = NodeId::from_canonical(pub_model::new_editor_canonical_id());
        self.plan_duplicate_authored_rectangle_page_from_session_v1(
            source_qualified_page_ids,
            source_page_id,
            destination,
            node_id,
        )
        .is_ok()
    }

    /// The existing graph and Story authorities must be proved absent before
    /// handing a "clean" signal to the source-neutral one-Rectangle planner.
    pub(super) fn plan_duplicate_authored_rectangle_page_from_session_v1(
        &self,
        source_qualified_page_ids: &[PageId],
        source_page_id: PageId,
        destination_identity: AuthoredPageIdentityV1,
        destination_node_id: NodeId,
    ) -> Result<DuplicateAuthoredRectanglePageTransitionV1, EditorError> {
        self.validate_source_identity()?;
        if self.project_identity.is_none() {
            return Err(EditorError::PageDuplicateUnsupported {
                message: "durable project identity is required".to_owned(),
            });
        }
        let source_identity = self
            .authored_page_identities_v1()
            .get(&source_page_id)
            .copied()
            .ok_or_else(|| EditorError::PageDuplicateUnsupported {
                message: "source Page is not an independently proven AuthorCreated Page".to_owned(),
            })?;
        if !self
            .authored_customer_page_ids_v1()
            .contains(&source_page_id)
        {
            return Err(EditorError::PageDuplicateUnsupported {
                message: "source Page is not active authored customer membership".to_owned(),
            });
        }
        if self.graph.pages.contains_key(&destination_identity.page_id)
            || self
                .authored_page_identities_v1()
                .contains_key(&destination_identity.page_id)
            || self.has_page_lifecycle_history_v1(destination_identity.page_id)
        {
            return Err(EditorError::AuthoredPageIdentityConflict {
                page_id: destination_identity.page_id,
            });
        }
        if self.graph.nodes.contains_key(&destination_node_id)
            || self.authored_shapes.contains_key(&destination_node_id)
            || self.authored_lines.contains_key(&destination_node_id)
            || self
                .authored_stacks
                .values()
                .any(|stack| stack.members.contains(&destination_node_id))
        {
            return Err(EditorError::PageDuplicateUnsupported {
                message:
                    "destination NodeId already belongs to an existing source or authored object"
                        .to_owned(),
            });
        }

        let source_shapes = self
            .authored_shapes
            .iter()
            .filter_map(|(id, shape)| {
                (shape.page_id == source_page_id || shape.parent_id == source_page_id)
                    .then_some(*id)
            })
            .collect::<BTreeSet<_>>();
        let foreign_stack_reference = self.authored_stacks.iter().any(|(page, stack)| {
            *page != source_page_id && stack.members.iter().any(|id| source_shapes.contains(id))
        });
        let dependent_graph_node = self.graph.nodes.values().any(|node| {
            let mut parent = node.header.parent_id;
            let mut visited = BTreeSet::new();
            loop {
                if parent == source_page_id.into_canonical()
                    || source_shapes.contains(&NodeId::from_canonical(parent))
                    || !visited.insert(parent)
                {
                    return true;
                }
                let Some(ancestor) = self.graph.nodes.get(&NodeId::from_canonical(parent)) else {
                    return false;
                };
                parent = ancestor.header.parent_id;
            }
        });
        let foreign_or_unproven_membership = self
            .page_has_resolved_node_membership_v1(source_page_id)
            || dependent_graph_node
            || foreign_stack_reference
            || self
                .authored_lines
                .values()
                .any(|line| line.page_id == source_page_id || line.parent_id == source_page_id);
        let customer_pages = self.effective_customer_page_order_v1(source_qualified_page_ids)?;
        let state = DuplicateAuthoredRectanglePageStateV1 {
            source_identity,
            document_pages: self.graph.document.pages.clone(),
            pages: self.graph.pages.clone(),
            authored_shapes: self.authored_shapes.clone(),
            source_stack: self.current_authored_stack_v1(source_page_id),
            destination_stack: self.current_authored_stack_v1(destination_identity.page_id),
        };
        plan_duplicate_authored_rectangle_page_v1(
            self.graph.document.id,
            &state,
            &customer_pages,
            source_page_id,
            destination_identity,
            destination_node_id,
            foreign_or_unproven_membership,
        )
        .map_err(|error| EditorError::PageDuplicateUnsupported {
            message: format!("atomic authored Rectangle Page admission rejected: {error:?}"),
        })
    }

    /// Exactly one history entry creates Page, Rectangle and destination stack.
    /// Source Publisher .pub bytes remain immutable; this is an EditorProject
    /// sidecar mutation, not native Publisher PageList/Oid/SPID allocation.
    pub fn duplicate_authored_rectangle_page_v1(
        &mut self,
        source_qualified_page_ids: Vec<PageId>,
        source_page_id: PageId,
        destination_identity: AuthoredPageIdentityV1,
        destination_node_id: NodeId,
    ) -> Result<EditOperation, EditorError> {
        let transition = self.plan_duplicate_authored_rectangle_page_from_session_v1(
            &source_qualified_page_ids,
            source_page_id,
            destination_identity,
            destination_node_id,
        )?;
        self.consume_canonical_duplicate_authored_rectangle_page_v1(transition)
    }

    pub(super) fn consume_canonical_duplicate_authored_rectangle_page_v1(
        &mut self,
        expected: DuplicateAuthoredRectanglePageTransitionV1,
    ) -> Result<EditOperation, EditorError> {
        let authored = self
            .authored_customer_page_ids_v1()
            .into_iter()
            .collect::<BTreeSet<_>>();
        let sources = expected
            .page
            .before_customer_page_ids
            .iter()
            .copied()
            .filter(|id| !authored.contains(id))
            .collect::<Vec<_>>();
        let planned = self.plan_duplicate_authored_rectangle_page_from_session_v1(
            &sources,
            expected.page.source_page_id,
            expected.page.destination_identity,
            expected.destination_shape.node_id,
        )?;
        if planned != expected {
            return Err(EditorError::StalePageDuplicate);
        }
        let mut graph = self.graph.clone();
        let mut shapes = self.authored_shapes.clone();
        apply_authored_rectangle_page_duplicate_history_candidate_v1(
            &mut graph,
            &mut shapes,
            self.current_authored_stack_v1(expected.page.source_page_id),
            self.current_authored_stack_v1(expected.page.destination_identity.page_id),
            self.authored_page_identities_v1()
                .get(&expected.page.source_page_id)
                .copied()
                .ok_or(EditorError::StalePageDuplicate)?,
            &expected,
            true,
        )?;
        self.graph = graph;
        self.authored_shapes = shapes;
        self.install_authored_stack_v1(expected.stack.after.clone());
        let operation = EditOperation::DuplicateAuthoredRectanglePageV1 {
            transition: Box::new(expected),
        };
        self.undo.push(operation.clone());
        self.redo.clear();
        self.validate_source_identity()?;
        Ok(operation)
    }

    /// Read-only admission for two to eight direct independent authored Rectangles.
    pub fn can_duplicate_authored_rectangles_page_v1(
        &self,
        source_qualified_page_ids: &[PageId],
        source_page_id: PageId,
    ) -> bool {
        let count = self.current_authored_stack_v1(source_page_id).members.len();
        if !(2..=MAX_DUPLICATED_AUTHORED_RECTANGLES_PAGE_V1).contains(&count) {
            return false;
        }
        let destination = AuthoredPageIdentityV1 {
            page_id: PageId::from_canonical(pub_model::new_editor_canonical_id()),
            provenance: AuthoredEntityProvenanceV1::AuthorCreated,
        };
        let destination_node_ids = (0..count)
            .map(|_| NodeId::from_canonical(pub_model::new_editor_canonical_id()))
            .collect::<Vec<_>>();
        self.plan_duplicate_authored_rectangles_page_from_session_v1(
            source_qualified_page_ids,
            source_page_id,
            destination,
            &destination_node_ids,
        )
        .is_ok()
    }

    pub(super) fn plan_duplicate_authored_rectangles_page_from_session_v1(
        &self,
        source_qualified_page_ids: &[PageId],
        source_page_id: PageId,
        destination_identity: AuthoredPageIdentityV1,
        destination_node_ids: &[NodeId],
    ) -> Result<DuplicateAuthoredRectanglesPageTransitionV1, EditorError> {
        self.validate_source_identity()?;
        if self.project_identity.is_none() {
            return Err(EditorError::PageDuplicateUnsupported {
                message: "durable project identity is required".to_owned(),
            });
        }
        let source_identity = self
            .authored_page_identities_v1()
            .get(&source_page_id)
            .copied()
            .ok_or_else(|| EditorError::PageDuplicateUnsupported {
                message: "source Page is not an independently proven AuthorCreated Page".to_owned(),
            })?;
        if !self
            .authored_customer_page_ids_v1()
            .contains(&source_page_id)
        {
            return Err(EditorError::PageDuplicateUnsupported {
                message: "source Page is not active authored customer membership".to_owned(),
            });
        }
        if self.graph.pages.contains_key(&destination_identity.page_id)
            || self
                .authored_page_identities_v1()
                .contains_key(&destination_identity.page_id)
            || self.has_page_lifecycle_history_v1(destination_identity.page_id)
        {
            return Err(EditorError::AuthoredPageIdentityConflict {
                page_id: destination_identity.page_id,
            });
        }
        let unique_destinations = destination_node_ids
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        if unique_destinations.len() != destination_node_ids.len() {
            return Err(EditorError::PageDuplicateUnsupported {
                message: "destination NodeIds must be unique".to_owned(),
            });
        }
        for destination_node_id in destination_node_ids {
            if self.graph.nodes.contains_key(destination_node_id)
                || self.authored_shapes.contains_key(destination_node_id)
                || self.authored_lines.contains_key(destination_node_id)
                || self
                    .authored_stacks
                    .values()
                    .any(|stack| stack.members.contains(destination_node_id))
            {
                return Err(EditorError::PageDuplicateUnsupported {
                    message:
                        "destination NodeId already belongs to an existing source or authored object"
                            .to_owned(),
                });
            }
        }

        let source_shapes = self
            .authored_shapes
            .iter()
            .filter_map(|(id, shape)| {
                (shape.page_id == source_page_id || shape.parent_id == source_page_id)
                    .then_some(*id)
            })
            .collect::<BTreeSet<_>>();
        let foreign_stack_reference = self.authored_stacks.iter().any(|(page, stack)| {
            *page != source_page_id && stack.members.iter().any(|id| source_shapes.contains(id))
        });
        let dependent_graph_node = self.graph.nodes.values().any(|node| {
            let mut parent = node.header.parent_id;
            let mut visited = BTreeSet::new();
            loop {
                if parent == source_page_id.into_canonical()
                    || source_shapes.contains(&NodeId::from_canonical(parent))
                    || !visited.insert(parent)
                {
                    return true;
                }
                let Some(ancestor) = self.graph.nodes.get(&NodeId::from_canonical(parent)) else {
                    return false;
                };
                parent = ancestor.header.parent_id;
            }
        });
        let foreign_or_unproven_membership = self
            .page_has_resolved_node_membership_v1(source_page_id)
            || dependent_graph_node
            || foreign_stack_reference
            || self
                .authored_lines
                .values()
                .any(|line| line.page_id == source_page_id || line.parent_id == source_page_id);
        let customer_pages = self.effective_customer_page_order_v1(source_qualified_page_ids)?;
        let state = DuplicateAuthoredRectanglePageStateV1 {
            source_identity,
            document_pages: self.graph.document.pages.clone(),
            pages: self.graph.pages.clone(),
            authored_shapes: self.authored_shapes.clone(),
            source_stack: self.current_authored_stack_v1(source_page_id),
            destination_stack: self.current_authored_stack_v1(destination_identity.page_id),
        };
        plan_duplicate_authored_rectangles_page_v1(
            self.graph.document.id,
            &state,
            &customer_pages,
            source_page_id,
            destination_identity,
            destination_node_ids,
            foreign_or_unproven_membership,
        )
        .map_err(|error| EditorError::PageDuplicateUnsupported {
            message: format!("atomic authored multi-Rectangle Page admission rejected: {error:?}"),
        })
    }

    /// Exactly one history entry creates Page, 2-8 Rectangles and destination stack.
    pub fn duplicate_authored_rectangles_page_v1(
        &mut self,
        source_qualified_page_ids: Vec<PageId>,
        source_page_id: PageId,
        destination_identity: AuthoredPageIdentityV1,
        destination_node_ids: Vec<NodeId>,
    ) -> Result<EditOperation, EditorError> {
        let transition = self.plan_duplicate_authored_rectangles_page_from_session_v1(
            &source_qualified_page_ids,
            source_page_id,
            destination_identity,
            &destination_node_ids,
        )?;
        self.consume_canonical_duplicate_authored_rectangles_page_v1(transition)
    }

    pub(super) fn consume_canonical_duplicate_authored_rectangles_page_v1(
        &mut self,
        expected: DuplicateAuthoredRectanglesPageTransitionV1,
    ) -> Result<EditOperation, EditorError> {
        let authored = self
            .authored_customer_page_ids_v1()
            .into_iter()
            .collect::<BTreeSet<_>>();
        let sources = expected
            .page
            .before_customer_page_ids
            .iter()
            .copied()
            .filter(|id| !authored.contains(id))
            .collect::<Vec<_>>();
        let destinations = expected
            .destination_shapes
            .iter()
            .map(|shape| shape.node_id)
            .collect::<Vec<_>>();
        let planned = self.plan_duplicate_authored_rectangles_page_from_session_v1(
            &sources,
            expected.page.source_page_id,
            expected.page.destination_identity,
            &destinations,
        )?;
        if planned != expected {
            return Err(EditorError::StalePageDuplicate);
        }
        let mut graph = self.graph.clone();
        let mut shapes = self.authored_shapes.clone();
        apply_authored_rectangles_page_duplicate_history_candidate_v1(
            &mut graph,
            &mut shapes,
            self.current_authored_stack_v1(expected.page.source_page_id),
            self.current_authored_stack_v1(expected.page.destination_identity.page_id),
            self.authored_page_identities_v1()
                .get(&expected.page.source_page_id)
                .copied()
                .ok_or(EditorError::StalePageDuplicate)?,
            &expected,
            true,
        )?;
        let final_stack = expected
            .stacks
            .last()
            .map(|transition| transition.after.clone())
            .ok_or_else(|| EditorError::PageDuplicateUnsupported {
                message: "multi-Rectangle transition has no destination stack hops".to_owned(),
            })?;
        self.graph = graph;
        self.authored_shapes = shapes;
        self.install_authored_stack_v1(final_stack);
        let operation = EditOperation::DuplicateAuthoredRectanglesPageV1 {
            transition: Box::new(expected),
        };
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
        let transition = self
            .plan_delete_blank_authored_page_from_session_v1(&source_qualified_page_ids, page_id)?;
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

    /// Duplicate one externally admitted blank page without object/Story cloning.
    /// Customer membership comes only from the source-qualified caller and active history.
    /// Read-only admission for the exact canonical DuplicateBlank source contract.
    /// A temporary identity validates the planner without creating a revision.
    pub fn can_duplicate_blank_page_v1(
        &self,
        source_qualified_page_ids: &[PageId],
        source_page_id: PageId,
    ) -> bool {
        let identity = AuthoredPageIdentityV1 {
            page_id: PageId::from_canonical(pub_model::new_editor_canonical_id()),
            provenance: AuthoredEntityProvenanceV1::AuthorCreated,
        };
        self.plan_duplicate_blank_page_from_session_v1(
            source_qualified_page_ids,
            source_page_id,
            identity,
        )
        .is_ok()
    }

    fn plan_duplicate_blank_page_from_session_v1(
        &self,
        source_qualified_page_ids: &[PageId],
        source_page_id: PageId,
        destination_identity: AuthoredPageIdentityV1,
    ) -> Result<DuplicateBlankPageTransitionV1, EditorError> {
        self.validate_source_identity()?;
        if self.project_identity.is_none() {
            return Err(EditorError::PageDuplicateUnsupported {
                message: "durable project identity is required".to_owned(),
            });
        }
        if validate_authored_page_identity_v1(&destination_identity).is_err() {
            return Err(EditorError::AuthoredPageIdentityInvalid {
                page_id: destination_identity.page_id,
            });
        }
        if self.graph.pages.contains_key(&destination_identity.page_id)
            || self
                .authored_page_identities_v1()
                .contains_key(&destination_identity.page_id)
            || self.has_page_lifecycle_history_v1(destination_identity.page_id)
        {
            return Err(EditorError::AuthoredPageIdentityConflict {
                page_id: destination_identity.page_id,
            });
        }
        if self.page_has_resolved_node_membership_v1(source_page_id)
            || !self
                .current_authored_stack_v1(source_page_id)
                .members
                .is_empty()
        {
            return Err(EditorError::PageDuplicateUnsupported {
                message: "source page owns resolved or authored content".to_owned(),
            });
        }
        let customer_page_ids = self.effective_customer_page_order_v1(source_qualified_page_ids)?;
        plan_duplicate_blank_page_v1(
            self.graph.document.id,
            &self.graph.document.pages,
            &self.graph.pages,
            &customer_page_ids,
            source_page_id,
            destination_identity,
        )
        .map_err(duplicate_blank_page_error_to_editor_v1)
    }

    pub fn duplicate_blank_page_v1(
        &mut self,
        source_qualified_page_ids: Vec<PageId>,
        source_page_id: PageId,
        destination_identity: AuthoredPageIdentityV1,
    ) -> Result<EditOperation, EditorError> {
        let transition = self.plan_duplicate_blank_page_from_session_v1(
            &source_qualified_page_ids,
            source_page_id,
            destination_identity,
        )?;
        self.consume_canonical_duplicate_blank_page_v1(transition)
    }

    pub fn insert_blank_page_after_v1(
        &mut self,
        source_qualified_page_ids: Vec<PageId>,
        anchor_page_id: PageId,
        identity: AuthoredPageIdentityV1,
        size: Size2D,
        bleed: Option<BoxEdges>,
        margins: Option<BoxEdges>,
    ) -> Result<EditOperation, EditorError> {
        self.validate_source_identity()?;
        if self.project_identity.is_none() {
            return Err(EditorError::PageInsertUnsupported {
                message: "durable project identity is required".to_owned(),
            });
        }
        if validate_authored_page_identity_v1(&identity).is_err() {
            return Err(EditorError::AuthoredPageIdentityInvalid {
                page_id: identity.page_id,
            });
        }
        if self.graph.pages.contains_key(&identity.page_id)
            || self
                .authored_page_identities_v1()
                .contains_key(&identity.page_id)
            || self.has_page_lifecycle_history_v1(identity.page_id)
        {
            return Err(EditorError::AuthoredPageIdentityConflict {
                page_id: identity.page_id,
            });
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
        let transition = plan_insert_blank_page_after_v1(
            self.graph.document.id,
            &self.graph.document.pages,
            &self.graph.pages,
            &current_customer_page_ids,
            anchor_page_id,
            identity,
            page,
        )
        .map_err(insert_blank_page_after_error_to_editor_v1)?;
        self.consume_canonical_insert_blank_page_after_v1(transition)
    }

    pub(super) fn consume_canonical_insert_blank_page_after_v1(
        &mut self,
        expected: InsertBlankPageAfterTransitionV1,
    ) -> Result<EditOperation, EditorError> {
        self.validate_source_identity()?;
        if self.project_identity.is_none() {
            return Err(EditorError::PageInsertUnsupported {
                message: "durable project identity is required".to_owned(),
            });
        }
        if self.graph.pages.contains_key(&expected.identity.page_id)
            || self
                .authored_page_identities_v1()
                .contains_key(&expected.identity.page_id)
            || self.has_page_lifecycle_history_v1(expected.identity.page_id)
        {
            return Err(EditorError::AuthoredPageIdentityConflict {
                page_id: expected.identity.page_id,
            });
        }
        let planned = plan_insert_blank_page_after_v1(
            self.graph.document.id,
            &self.graph.document.pages,
            &self.graph.pages,
            &expected.before_customer_page_ids,
            expected.anchor_page_id,
            expected.identity,
            expected.page.clone(),
        )
        .map_err(insert_blank_page_after_error_to_editor_v1)?;
        if planned != expected {
            return Err(EditorError::StalePageInsert);
        }
        let operation = EditOperation::InsertBlankPageAfterV1 {
            transition: expected,
        };
        apply_forward(&mut self.graph, &operation)?;
        self.undo.push(operation.clone());
        self.redo.clear();
        self.validate_source_identity()?;
        Ok(operation)
    }

    pub(super) fn consume_canonical_duplicate_blank_page_v1(
        &mut self,
        expected: DuplicateBlankPageTransitionV1,
    ) -> Result<EditOperation, EditorError> {
        self.validate_source_identity()?;
        if self.project_identity.is_none() {
            return Err(EditorError::PageDuplicateUnsupported {
                message: "durable project identity is required".to_owned(),
            });
        }
        if self.page_has_resolved_node_membership_v1(expected.source_page_id)
            || !self
                .current_authored_stack_v1(expected.source_page_id)
                .members
                .is_empty()
        {
            return Err(EditorError::PageDuplicateUnsupported {
                message: "source page owns resolved or authored content".to_owned(),
            });
        }
        if self
            .authored_page_identities_v1()
            .contains_key(&expected.destination_identity.page_id)
            || self.has_page_lifecycle_history_v1(expected.destination_identity.page_id)
        {
            return Err(EditorError::AuthoredPageIdentityConflict {
                page_id: expected.destination_identity.page_id,
            });
        }
        let planned = plan_duplicate_blank_page_v1(
            self.graph.document.id,
            &self.graph.document.pages,
            &self.graph.pages,
            &expected.before_customer_page_ids,
            expected.source_page_id,
            expected.destination_identity,
        )
        .map_err(duplicate_blank_page_error_to_editor_v1)?;
        if planned != expected {
            return Err(EditorError::StalePageDuplicate);
        }
        let operation = EditOperation::DuplicateBlankPageV1 {
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
        AuthoredEntityProvenanceV1, EDITOR_PROJECT_VERSION_V0_25, EDITOR_PROJECT_VERSION_V0_26,
        EDITOR_PROJECT_VERSION_V0_27, EDITOR_PROJECT_VERSION_V0_28, EditorProject,
        PubResolvedGraph,
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

    #[test]
    fn delete_blank_capability_is_read_only_and_rejects_authored_content() {
        let source = page_id("22222222-2222-4222-8222-222222222222");
        let identity = authored_identity();
        let mut session = EditorSession::new(source_graph(vec![source])).expect("session");
        session
            .append_blank_page_v1(
                vec![source],
                identity,
                Size2D::new(LengthEmu::new(2_000_000), LengthEmu::new(3_000_000)),
                None,
                None,
            )
            .expect("append blank authored page");

        let before_operations = session.operations().len();
        let before_pages = session.graph().document.pages.clone();
        assert!(session.can_delete_blank_authored_page_v1(&[source], identity.page_id));
        assert!(!session.can_delete_blank_authored_page_v1(&[source], source));
        assert_eq!(session.operations().len(), before_operations);
        assert_eq!(session.graph().document.pages, before_pages);

        let shape_id = NodeId::from_canonical(pub_model::new_editor_canonical_id());
        session
            .create_shape(
                shape_id,
                identity.page_id,
                RectEmu::new(
                    LengthEmu::new(100_000),
                    LengthEmu::new(100_000),
                    LengthEmu::new(400_000),
                    LengthEmu::new(300_000),
                ),
                crate::AuthoredShapePaintV1 {
                    fill: crate::AuthoredSolidFillV1 {
                        visible: true,
                        color: crate::Srgb8V1 {
                            r: 255,
                            g: 255,
                            b: 255,
                        },
                    },
                    stroke: crate::AuthoredSolidStrokeV1 {
                        visible: true,
                        color: crate::Srgb8V1 { r: 0, g: 0, b: 0 },
                        width_emu: 12_700,
                    },
                    provenance: AuthoredEntityProvenanceV1::AuthorCreated,
                },
            )
            .expect("create canonical content-bearing authored Rectangle");
        let content_operations = session.operations().len();
        assert!(!session.can_delete_blank_authored_page_v1(&[source], identity.page_id));
        assert!(
            session
                .delete_blank_authored_page_v1(vec![source], identity.page_id)
                .is_err()
        );
        assert_eq!(session.operations().len(), content_operations);
        assert!(session.graph().pages.contains_key(&identity.page_id));

        session.undo().expect("undo authored Rectangle");
        assert!(session.can_delete_blank_authored_page_v1(&[source], identity.page_id));
        session
            .delete_blank_authored_page_v1(vec![source], identity.page_id)
            .expect("delete after content removed");
        assert!(!session.can_delete_blank_authored_page_v1(&[source], identity.page_id));
    }

    #[test]
    fn authored_rectangle_page_delete_admission_is_read_only_and_fail_closed() {
        let source = page_id("22222222-2222-4222-8222-222222222222");
        let identity = authored_identity();
        let mut session = EditorSession::new(source_graph(vec![source])).expect("session");
        session
            .append_blank_page_v1(
                vec![source],
                identity,
                Size2D::new(LengthEmu::new(2_000_000), LengthEmu::new(3_000_000)),
                None,
                None,
            )
            .expect("append authored page");
        let shape_id = NodeId::from_canonical(pub_model::new_editor_canonical_id());
        session
            .create_shape(
                shape_id,
                identity.page_id,
                RectEmu::new(
                    LengthEmu::new(100_000),
                    LengthEmu::new(100_000),
                    LengthEmu::new(400_000),
                    LengthEmu::new(300_000),
                ),
                crate::AuthoredShapePaintV1 {
                    fill: crate::AuthoredSolidFillV1 {
                        visible: true,
                        color: crate::Srgb8V1 {
                            r: 255,
                            g: 255,
                            b: 255,
                        },
                    },
                    stroke: crate::AuthoredSolidStrokeV1 {
                        visible: true,
                        color: crate::Srgb8V1 { r: 0, g: 0, b: 0 },
                        width_emu: 12_700,
                    },
                    provenance: AuthoredEntityProvenanceV1::AuthorCreated,
                },
            )
            .expect("one authored Rectangle");
        let original_operations = session.operations().len();
        let original_graph_pages = session.graph().document.pages.clone();
        assert!(session.can_delete_authored_rectangle_page_v1(&[source], identity.page_id));
        let planned = session
            .plan_delete_authored_rectangle_page_from_session_v1(&[source], identity.page_id)
            .expect("admitted authored Rectangle page");
        assert_eq!(planned.shape_before.node_id, shape_id);
        assert_eq!(session.operations().len(), original_operations);
        assert_eq!(session.graph().document.pages, original_graph_pages);
        assert!(!session.can_delete_blank_authored_page_v1(&[source], identity.page_id));
        assert!(
            session
                .plan_delete_authored_rectangle_page_from_session_v1(&[source], source)
                .is_err(),
            "a source-backed Page is never admitted"
        );

        // A missing lane member must fail even when Page.children is empty.
        session
            .authored_stacks
            .get_mut(&identity.page_id)
            .expect("created authored lane")
            .members
            .clear();
        assert!(
            session
                .plan_delete_authored_rectangle_page_from_session_v1(&[source], identity.page_id,)
                .is_err()
        );
        assert_eq!(session.operations().len(), original_operations);
    }

    #[test]
    fn delete_authored_rectangle_page_v029_is_one_reversible_project_operation() {
        let source = page_id("22222222-2222-4222-8222-222222222222");
        let identity = authored_identity();
        let base = source_graph(vec![source]);
        let mut session = EditorSession::new(base.clone()).expect("session");
        let original_hash = session.source_hash();
        session
            .append_blank_page_v1(
                vec![source],
                identity,
                Size2D::new(LengthEmu::new(2_000_000), LengthEmu::new(3_000_000)),
                None,
                None,
            )
            .expect("append authored page");
        let node_id = NodeId::from_canonical(pub_model::new_editor_canonical_id());
        session
            .create_shape(
                node_id,
                identity.page_id,
                RectEmu::new(
                    LengthEmu::new(100_000),
                    LengthEmu::new(100_000),
                    LengthEmu::new(200_000),
                    LengthEmu::new(300_000),
                ),
                crate::AuthoredShapePaintV1 {
                    fill: crate::AuthoredSolidFillV1 {
                        visible: true,
                        color: crate::Srgb8V1 {
                            r: 255,
                            g: 255,
                            b: 255,
                        },
                    },
                    stroke: crate::AuthoredSolidStrokeV1 {
                        visible: true,
                        color: crate::Srgb8V1 { r: 0, g: 0, b: 0 },
                        width_emu: 12_700,
                    },
                    provenance: AuthoredEntityProvenanceV1::AuthorCreated,
                },
            )
            .expect("create one authored rectangle");
        let with_shape = session.graph().clone();
        let shape_before = session.authored_shapes[&node_id].clone();
        let stack_before = session.current_authored_stack_v1(identity.page_id);
        let history_before = session.operations().len();
        assert!(session.can_delete_authored_rectangle_page_v1(&[source], identity.page_id));

        let operation = session
            .delete_authored_rectangle_page_v1(vec![source], identity.page_id)
            .expect("single canonical delete");
        assert!(matches!(
            operation,
            EditOperation::DeleteAuthoredRectanglePageV1 { .. }
        ));
        assert_eq!(session.operations().len(), history_before + 1);
        assert_eq!(session.graph().document.pages, vec![source]);
        assert!(!session.graph().pages.contains_key(&identity.page_id));
        assert!(!session.authored_shapes.contains_key(&node_id));
        assert!(!session.authored_stacks.contains_key(&identity.page_id));
        assert_eq!(session.source_hash(), original_hash);

        let deleted_graph = session.graph().clone();
        let project = session.project();
        assert_eq!(project.schema_version, crate::EDITOR_PROJECT_VERSION_V0_29);
        let encoded = serde_json::to_vec(&project).expect("encode v0.29");
        let decoded: EditorProject = serde_json::from_slice(&encoded).expect("decode v0.29");
        let mut reopened = EditorSession::new(base.clone()).expect("fresh");
        reopened
            .apply_project(&decoded)
            .expect("exact project replay");
        assert_eq!(reopened.graph(), &deleted_graph);
        assert!(!reopened.authored_shapes.contains_key(&node_id));
        assert_eq!(reopened.operations(), decoded.operations.as_slice());
        assert_eq!(reopened.source_hash(), original_hash);

        session.undo().expect("single undo");
        assert_eq!(session.graph(), &with_shape);
        assert_eq!(session.authored_shapes.get(&node_id), Some(&shape_before));
        assert_eq!(
            session.current_authored_stack_v1(identity.page_id),
            stack_before
        );
        assert_eq!(session.operations().len(), history_before);
        session.redo().expect("single redo");
        assert_eq!(session.graph(), &deleted_graph);
        assert!(!session.authored_shapes.contains_key(&node_id));

        let mut forged_legacy = decoded.clone();
        forged_legacy.schema_version = crate::EDITOR_PROJECT_VERSION_V0_28.into();
        let mut legacy_reopen = EditorSession::new(base).expect("legacy fresh");
        assert!(matches!(
            legacy_reopen.apply_project(&forged_legacy),
            Err(crate::EditorProjectError::LegacyProjectCarriesDeleteAuthoredRectanglePageOperation { .. })
        ));
    }

    #[test]
    fn duplicate_authored_rectangle_page_v030_is_one_reversible_project_operation() {
        let source = page_id("22222222-2222-4222-8222-222222222222");
        let identity = authored_identity();
        let destination = AuthoredPageIdentityV1 {
            page_id: PageId::from_canonical(pub_model::new_editor_canonical_id()),
            provenance: AuthoredEntityProvenanceV1::AuthorCreated,
        };
        let source_node_id = NodeId::from_canonical(pub_model::new_editor_canonical_id());
        let destination_node_id = NodeId::from_canonical(pub_model::new_editor_canonical_id());
        let base = source_graph(vec![source]);
        let mut session = EditorSession::new(base.clone()).expect("session");
        let original_hash = session.source_hash();
        session
            .append_blank_page_v1(
                vec![source],
                identity,
                Size2D::new(LengthEmu::new(2_000_000), LengthEmu::new(3_000_000)),
                None,
                None,
            )
            .expect("append authored Page");
        session
            .create_shape(
                source_node_id,
                identity.page_id,
                RectEmu::new(
                    LengthEmu::new(100_000),
                    LengthEmu::new(150_000),
                    LengthEmu::new(200_000),
                    LengthEmu::new(300_000),
                ),
                crate::AuthoredShapePaintV1 {
                    fill: crate::AuthoredSolidFillV1 {
                        visible: true,
                        color: crate::Srgb8V1 {
                            r: 30,
                            g: 40,
                            b: 50,
                        },
                    },
                    stroke: crate::AuthoredSolidStrokeV1 {
                        visible: true,
                        color: crate::Srgb8V1 { r: 0, g: 0, b: 0 },
                        width_emu: 12_700,
                    },
                    provenance: AuthoredEntityProvenanceV1::AuthorCreated,
                },
            )
            .expect("authored Rectangle");
        let before_graph = session.graph().clone();
        let before_shape = session.authored_shapes[&source_node_id].clone();
        let before_stack = session.current_authored_stack_v1(identity.page_id);
        let history_len = session.operations().len();
        assert!(session.can_duplicate_authored_rectangle_page_v1(&[source], identity.page_id));
        assert_eq!(session.operations().len(), history_len);

        let operation = session
            .duplicate_authored_rectangle_page_v1(
                vec![source],
                identity.page_id,
                destination,
                destination_node_id,
            )
            .expect("one canonical Page and Rectangle duplication");
        assert!(matches!(
            operation,
            EditOperation::DuplicateAuthoredRectanglePageV1 { .. }
        ));
        assert_eq!(session.operations().len(), history_len + 1);
        assert_eq!(
            session.graph().document.pages,
            vec![source, identity.page_id, destination.page_id]
        );
        let copied = &session.authored_shapes[&destination_node_id];
        assert_eq!(copied.page_id, destination.page_id);
        assert_eq!(copied.parent_id, destination.page_id);
        assert_eq!(copied.bounds, before_shape.bounds);
        assert_eq!(copied.paint, before_shape.paint);
        assert_eq!(
            session
                .current_authored_stack_v1(destination.page_id)
                .members,
            vec![destination_node_id]
        );
        assert_eq!(
            session.authored_shapes.get(&source_node_id),
            Some(&before_shape)
        );
        assert_eq!(session.source_hash(), original_hash);

        let duplicated_graph = session.graph().clone();
        let project = session.project();
        assert_eq!(project.schema_version, crate::EDITOR_PROJECT_VERSION_V0_30);
        let encoded = serde_json::to_vec(&project).expect("encode v0.30");
        let decoded: EditorProject = serde_json::from_slice(&encoded).expect("decode v0.30");
        let mut reopened = EditorSession::new(base.clone()).expect("fresh session");
        reopened
            .apply_project(&decoded)
            .expect("fresh EditorProject replay");
        assert_eq!(reopened.graph(), &duplicated_graph);
        assert_eq!(reopened.authored_shapes, session.authored_shapes);
        assert_eq!(reopened.authored_stacks, session.authored_stacks);
        assert_eq!(reopened.operations(), decoded.operations.as_slice());
        assert_eq!(reopened.source_hash(), original_hash);

        session.undo().expect("one duplicate Undo");
        assert_eq!(session.graph(), &before_graph);
        assert_eq!(
            session.authored_shapes.get(&source_node_id),
            Some(&before_shape)
        );
        assert!(!session.authored_shapes.contains_key(&destination_node_id));
        assert!(!session.authored_stacks.contains_key(&destination.page_id));
        assert_eq!(
            session.current_authored_stack_v1(identity.page_id),
            before_stack
        );
        assert_eq!(session.operations().len(), history_len);
        session.redo().expect("same duplicate Redo");
        assert_eq!(session.graph(), &duplicated_graph);
        assert_eq!(session.authored_shapes, reopened.authored_shapes);
        assert_eq!(session.authored_stacks, reopened.authored_stacks);
        assert_eq!(session.source_hash(), original_hash);

        let mut forged_legacy = decoded.clone();
        forged_legacy.schema_version = crate::EDITOR_PROJECT_VERSION_V0_29.into();
        let mut legacy_reopen = EditorSession::new(base).expect("legacy fresh session");
        assert!(matches!(
            legacy_reopen.apply_project(&forged_legacy),
            Err(
                crate::EditorProjectError::LegacyProjectCarriesDuplicateAuthoredRectanglePageOperation {
                    ..
                }
            )
        ));
    }

    #[test]
    fn duplicate_authored_rectangles_page_v031_is_one_reversible_project_operation() {
        let source = page_id("22222222-2222-4222-8222-222222222222");
        let identity = authored_identity();
        let destination = AuthoredPageIdentityV1 {
            page_id: PageId::from_canonical(pub_model::new_editor_canonical_id()),
            provenance: AuthoredEntityProvenanceV1::AuthorCreated,
        };
        let source_node_a = NodeId::from_canonical(pub_model::new_editor_canonical_id());
        let source_node_b = NodeId::from_canonical(pub_model::new_editor_canonical_id());
        let destination_node_a = NodeId::from_canonical(pub_model::new_editor_canonical_id());
        let destination_node_b = NodeId::from_canonical(pub_model::new_editor_canonical_id());
        let base = source_graph(vec![source]);
        let mut session = EditorSession::new(base.clone()).expect("session");
        let original_hash = session.source_hash();
        session
            .append_blank_page_v1(
                vec![source],
                identity,
                Size2D::new(LengthEmu::new(2_000_000), LengthEmu::new(3_000_000)),
                None,
                None,
            )
            .expect("append authored Page");
        let paint_a = crate::AuthoredShapePaintV1 {
            fill: crate::AuthoredSolidFillV1 {
                visible: true,
                color: crate::Srgb8V1 {
                    r: 30,
                    g: 40,
                    b: 50,
                },
            },
            stroke: crate::AuthoredSolidStrokeV1 {
                visible: true,
                color: crate::Srgb8V1 { r: 0, g: 0, b: 0 },
                width_emu: 12_700,
            },
            provenance: AuthoredEntityProvenanceV1::AuthorCreated,
        };
        let paint_b = crate::AuthoredShapePaintV1 {
            fill: crate::AuthoredSolidFillV1 {
                visible: true,
                color: crate::Srgb8V1 {
                    r: 90,
                    g: 80,
                    b: 70,
                },
            },
            stroke: crate::AuthoredSolidStrokeV1 {
                visible: true,
                color: crate::Srgb8V1 { r: 5, g: 6, b: 7 },
                width_emu: 25_400,
            },
            provenance: AuthoredEntityProvenanceV1::AuthorCreated,
        };
        session
            .create_shape(
                source_node_a,
                identity.page_id,
                RectEmu::new(
                    LengthEmu::new(100_000),
                    LengthEmu::new(150_000),
                    LengthEmu::new(200_000),
                    LengthEmu::new(300_000),
                ),
                paint_a,
            )
            .expect("first authored Rectangle");
        assert!(
            !session.can_duplicate_authored_rectangles_page_v1(&[source], identity.page_id),
            "one Rectangle stays on the v0.30 command"
        );
        session
            .create_shape(
                source_node_b,
                identity.page_id,
                RectEmu::new(
                    LengthEmu::new(500_000),
                    LengthEmu::new(550_000),
                    LengthEmu::new(250_000),
                    LengthEmu::new(350_000),
                ),
                paint_b,
            )
            .expect("second authored Rectangle");
        let before_graph = session.graph().clone();
        let before_shapes = session.authored_shapes.clone();
        let before_stack = session.current_authored_stack_v1(identity.page_id);
        let history_len = session.operations().len();
        assert!(session.can_duplicate_authored_rectangles_page_v1(&[source], identity.page_id));
        assert_eq!(session.operations().len(), history_len);

        let operation = session
            .duplicate_authored_rectangles_page_v1(
                vec![source],
                identity.page_id,
                destination,
                vec![destination_node_a, destination_node_b],
            )
            .expect("one canonical Page and two-Rectangle duplication");
        assert!(matches!(
            operation,
            EditOperation::DuplicateAuthoredRectanglesPageV1 { .. }
        ));
        assert_eq!(session.operations().len(), history_len + 1);
        assert_eq!(
            session.graph().document.pages,
            vec![source, identity.page_id, destination.page_id]
        );
        assert_eq!(
            session
                .current_authored_stack_v1(destination.page_id)
                .members,
            vec![destination_node_a, destination_node_b]
        );
        for (source_id, destination_id) in [
            (source_node_a, destination_node_a),
            (source_node_b, destination_node_b),
        ] {
            let source_shape = &before_shapes[&source_id];
            let copied = &session.authored_shapes[&destination_id];
            assert_eq!(copied.page_id, destination.page_id);
            assert_eq!(copied.parent_id, destination.page_id);
            assert_eq!(copied.bounds, source_shape.bounds);
            assert_eq!(copied.paint, source_shape.paint);
        }
        assert_eq!(session.source_hash(), original_hash);

        let duplicated_graph = session.graph().clone();
        let duplicated_shapes = session.authored_shapes.clone();
        let duplicated_stacks = session.authored_stacks.clone();
        let project = session.project();
        assert_eq!(project.schema_version, crate::EDITOR_PROJECT_VERSION_V0_31);
        let encoded = serde_json::to_vec(&project).expect("encode v0.31");
        let decoded: EditorProject = serde_json::from_slice(&encoded).expect("decode v0.31");
        let mut reopened = EditorSession::new(base.clone()).expect("fresh session");
        reopened
            .apply_project(&decoded)
            .expect("fresh v0.31 EditorProject replay");
        assert_eq!(reopened.graph(), &duplicated_graph);
        assert_eq!(reopened.authored_shapes, duplicated_shapes);
        assert_eq!(reopened.authored_stacks, duplicated_stacks);
        assert_eq!(reopened.operations(), decoded.operations.as_slice());
        assert_eq!(reopened.source_hash(), original_hash);

        session.undo().expect("one multi-Rectangle duplicate Undo");
        assert_eq!(session.graph(), &before_graph);
        assert_eq!(session.authored_shapes, before_shapes);
        assert_eq!(
            session.current_authored_stack_v1(identity.page_id),
            before_stack
        );
        assert!(!session.authored_stacks.contains_key(&destination.page_id));
        assert_eq!(session.operations().len(), history_len);
        session.redo().expect("same multi-Rectangle duplicate Redo");
        assert_eq!(session.graph(), &duplicated_graph);
        assert_eq!(session.authored_shapes, duplicated_shapes);
        assert_eq!(session.authored_stacks, duplicated_stacks);

        let mut forged_legacy = decoded;
        forged_legacy.schema_version = crate::EDITOR_PROJECT_VERSION_V0_30.into();
        let mut legacy_reopen = EditorSession::new(base).expect("legacy fresh session");
        assert!(matches!(
            legacy_reopen.apply_project(&forged_legacy),
            Err(
                crate::EditorProjectError::LegacyProjectCarriesDuplicateAuthoredRectanglesPageOperation {
                    ..
                }
            )
        ));
    }

    #[test]
    fn duplicate_authored_rectangle_page_rejects_source_and_node_collisions_without_history() {
        let source = page_id("22222222-2222-4222-8222-222222222222");
        let identity = authored_identity();
        let destination = AuthoredPageIdentityV1 {
            page_id: PageId::from_canonical(pub_model::new_editor_canonical_id()),
            provenance: AuthoredEntityProvenanceV1::AuthorCreated,
        };
        let source_node_id = NodeId::from_canonical(pub_model::new_editor_canonical_id());
        let mut session = EditorSession::new(source_graph(vec![source])).expect("session");
        assert!(!session.can_duplicate_authored_rectangle_page_v1(&[source], source));
        assert!(
            session
                .duplicate_authored_rectangle_page_v1(
                    vec![source],
                    source,
                    destination,
                    source_node_id
                )
                .is_err()
        );
        assert!(session.operations().is_empty());

        session
            .append_blank_page_v1(
                vec![source],
                identity,
                Size2D::new(LengthEmu::new(2_000_000), LengthEmu::new(3_000_000)),
                None,
                None,
            )
            .expect("append authored Page");
        session
            .create_shape(
                source_node_id,
                identity.page_id,
                RectEmu::new(
                    LengthEmu::new(100_000),
                    LengthEmu::new(150_000),
                    LengthEmu::new(200_000),
                    LengthEmu::new(300_000),
                ),
                crate::AuthoredShapePaintV1 {
                    fill: crate::AuthoredSolidFillV1 {
                        visible: true,
                        color: crate::Srgb8V1 { r: 0, g: 0, b: 0 },
                    },
                    stroke: crate::AuthoredSolidStrokeV1 {
                        visible: true,
                        color: crate::Srgb8V1 { r: 0, g: 0, b: 0 },
                        width_emu: 12_700,
                    },
                    provenance: AuthoredEntityProvenanceV1::AuthorCreated,
                },
            )
            .expect("authored Rectangle");
        let before_operations = session.operations().len();
        let before_pages = session.graph().document.pages.clone();
        assert!(
            session
                .duplicate_authored_rectangle_page_v1(
                    vec![source],
                    identity.page_id,
                    destination,
                    source_node_id
                )
                .is_err(),
            "a cloned object must never alias the source NodeId"
        );
        assert_eq!(session.operations().len(), before_operations);
        assert_eq!(session.graph().document.pages, before_pages);

        session
            .authored_stacks
            .get_mut(&identity.page_id)
            .expect("source lane")
            .members
            .clear();
        assert!(!session.can_duplicate_authored_rectangle_page_v1(&[source], identity.page_id));
        assert_eq!(session.operations().len(), before_operations);
        assert_eq!(session.graph().document.pages, before_pages);
    }

    #[test]
    fn rectangle_page_delete_rejects_tampered_runtime_without_history() {
        let source = page_id("22222222-2222-4222-8222-222222222222");
        let identity = authored_identity();
        let mut session = EditorSession::new(source_graph(vec![source])).expect("session");
        session
            .append_blank_page_v1(
                vec![source],
                identity,
                Size2D::new(LengthEmu::new(2_000_000), LengthEmu::new(3_000_000)),
                None,
                None,
            )
            .expect("append");
        let node_id = NodeId::from_canonical(pub_model::new_editor_canonical_id());
        session
            .create_shape(
                node_id,
                identity.page_id,
                RectEmu::new(
                    LengthEmu::new(100_000),
                    LengthEmu::new(100_000),
                    LengthEmu::new(200_000),
                    LengthEmu::new(300_000),
                ),
                crate::AuthoredShapePaintV1 {
                    fill: crate::AuthoredSolidFillV1 {
                        visible: true,
                        color: crate::Srgb8V1 { r: 0, g: 0, b: 0 },
                    },
                    stroke: crate::AuthoredSolidStrokeV1 {
                        visible: true,
                        color: crate::Srgb8V1 { r: 0, g: 0, b: 0 },
                        width_emu: 12_700,
                    },
                    provenance: AuthoredEntityProvenanceV1::AuthorCreated,
                },
            )
            .expect("create shape");
        let before = session.operations().len();
        // Two contents cannot be silently reduced to one candidate; a second
        // AuthorCreated rectangle is an unsupported cascade.
        let second = NodeId::from_canonical(pub_model::new_editor_canonical_id());
        let mut extra = session.authored_shapes[&node_id].clone();
        extra.node_id = second;
        session.authored_shapes.insert(second, extra);
        assert!(!session.can_delete_authored_rectangle_page_v1(&[source], identity.page_id));
        assert!(
            session
                .delete_authored_rectangle_page_v1(vec![source], identity.page_id)
                .is_err()
        );
        assert_eq!(session.operations().len(), before);
        assert!(session.graph().pages.contains_key(&identity.page_id));
    }

    #[test]
    fn duplicate_blank_capability_uses_canonical_source_admission_without_revision() {
        let source = page_id("22222222-2222-4222-8222-222222222222");
        let mut session = EditorSession::new(source_graph(vec![source])).expect("session");
        let original_pages = session.graph().document.pages.clone();
        let operations_before = session.operations().len();

        assert!(session.can_duplicate_blank_page_v1(&[source], source));
        assert!(!session.can_duplicate_blank_page_v1(&[], source));
        assert_eq!(session.operations().len(), operations_before);
        assert_eq!(session.graph().document.pages, original_pages);

        let child = NodeId::from_canonical(pub_model::new_editor_canonical_id());
        session
            .graph
            .pages
            .get_mut(&source)
            .expect("blank source Page")
            .children
            .push(child);
        assert!(
            !session.can_duplicate_blank_page_v1(&[source], source),
            "nonblank source Page must fail before UI exposure"
        );
        session
            .graph
            .pages
            .get_mut(&source)
            .expect("source Page")
            .children
            .clear();
        assert!(session.can_duplicate_blank_page_v1(&[source], source));
        assert_eq!(session.operations().len(), operations_before);
        assert_eq!(session.graph().document.pages, original_pages);
    }

    #[test]
    fn append_then_duplicate_reopen_allows_exact_delete_blank() {
        let source = page_id("22222222-2222-4222-8222-222222222222");
        let appended = authored_identity();
        let duplicated = AuthoredPageIdentityV1 {
            page_id: page_id("01890f4f-1234-7abc-8def-0123456789ac"),
            provenance: AuthoredEntityProvenanceV1::AuthorCreated,
        };
        let graph = source_graph(vec![source]);
        let mut session = EditorSession::new(graph.clone()).expect("session");
        session
            .append_blank_page_v1(
                vec![source],
                appended,
                Size2D::new(LengthEmu::new(2_000_000), LengthEmu::new(3_000_000)),
                None,
                None,
            )
            .expect("append blank source");
        session
            .duplicate_blank_page_v1(vec![source], appended.page_id, duplicated)
            .expect("duplicate appended blank customer page");
        session.undo().expect("undo duplicate");
        session.redo().expect("redo same PageId");
        let project = session.project();
        let mut reopened = EditorSession::new(graph).expect("fresh Editor");
        reopened
            .apply_project(&project)
            .expect("replay two lifecycle operations");
        assert_eq!(reopened.operations().len(), 2);
        assert_eq!(
            reopened
                .effective_customer_page_order_v1(&[source])
                .expect("canonical customer order"),
            vec![source, appended.page_id, duplicated.page_id]
        );
        assert!(
            reopened.can_delete_blank_authored_page_v1(&[source], duplicated.page_id),
            "duplicate of appended blank page must remain DeleteBlank-admissible after project replay"
        );
        reopened
            .delete_blank_authored_page_v1(vec![source], duplicated.page_id)
            .expect("delete exact duplicate after replay");
        assert_eq!(
            reopened.graph().document.pages,
            vec![source, appended.page_id]
        );
    }

    #[test]
    fn insert_blank_after_contentful_customer_replays_one_exact_page() {
        let master = page_id("11111111-1111-4111-8111-111111111111");
        let a = page_id("22222222-2222-4222-8222-222222222222");
        let service = page_id("33333333-3333-4333-8333-333333333333");
        let b = page_id("44444444-4444-4444-8444-444444444444");
        let carrier = page_id("55555555-5555-4555-8555-555555555555");
        let identity = authored_identity();
        let graph = source_graph(vec![master, a, service, b, carrier]);
        let mut session = EditorSession::new(graph.clone()).expect("session");
        let source_hash = session.source_hash();
        let shape_id = NodeId::from_canonical(pub_model::new_editor_canonical_id());
        session
            .create_shape(
                shape_id,
                a,
                RectEmu::new(
                    LengthEmu::new(100_000),
                    LengthEmu::new(100_000),
                    LengthEmu::new(300_000),
                    LengthEmu::new(300_000),
                ),
                crate::AuthoredShapePaintV1 {
                    fill: crate::AuthoredSolidFillV1 {
                        visible: true,
                        color: crate::Srgb8V1 {
                            r: 255,
                            g: 255,
                            b: 255,
                        },
                    },
                    stroke: crate::AuthoredSolidStrokeV1 {
                        visible: true,
                        color: crate::Srgb8V1 { r: 0, g: 0, b: 0 },
                        width_emu: 12_700,
                    },
                    provenance: AuthoredEntityProvenanceV1::AuthorCreated,
                },
            )
            .expect("create canonical content on first customer page");
        let original_content = session.authored_shapes[&shape_id].clone();
        session
            .insert_blank_page_after_v1(
                vec![a, b],
                a,
                identity,
                Size2D::new(LengthEmu::new(2_000_000), LengthEmu::new(3_000_000)),
                None,
                None,
            )
            .expect("insert blank after populated customer");
        let ordered = vec![master, a, identity.page_id, service, b, carrier];
        assert_eq!(session.graph().document.pages, ordered);
        assert_eq!(
            session
                .effective_customer_page_order_v1(&[a, b])
                .expect("membership"),
            vec![a, identity.page_id, b]
        );
        assert_eq!(session.operations().len(), 2);
        assert_eq!(session.authored_shapes[&shape_id], original_content);
        assert!(session.graph().pages[&identity.page_id].children.is_empty());
        assert!(
            session.graph().pages[&identity.page_id]
                .extensions
                .is_empty()
        );
        assert_eq!(session.source_hash(), source_hash);

        session.undo().expect("undo insert");
        assert_eq!(session.graph().document.pages, graph.document.pages);
        assert_eq!(session.authored_shapes[&shape_id], original_content);
        assert!(
            session
                .insert_blank_page_after_v1(
                    vec![a, b],
                    a,
                    identity,
                    Size2D::new(LengthEmu::new(2_000_000), LengthEmu::new(3_000_000)),
                    None,
                    None,
                )
                .is_err(),
            "undone PageId cannot be recycled by a new insert operation"
        );
        session.redo().expect("redo insert");
        assert_eq!(session.graph().document.pages, ordered);

        let project = session.project();
        assert_eq!(project.schema_version, EDITOR_PROJECT_VERSION_V0_28);
        let serialized = serde_json::to_vec(&project).expect("encode v0.28");
        let decoded: EditorProject = serde_json::from_slice(&serialized).expect("decode v0.28");
        let mut reopened = EditorSession::new(graph.clone()).expect("fresh session");
        reopened
            .apply_project(&decoded)
            .expect("canonical insert replay");
        assert_eq!(reopened.operations(), decoded.operations.as_slice());
        assert_eq!(reopened.graph().document.pages, ordered);
        assert_eq!(reopened.authored_shapes[&shape_id], original_content);
        assert_eq!(reopened.source_hash(), source_hash);

        let mut legacy = decoded.clone();
        legacy.schema_version = EDITOR_PROJECT_VERSION_V0_27.to_owned();
        let mut rejected = EditorSession::new(graph).expect("legacy negative");
        assert!(matches!(
            rejected.apply_project(&legacy),
            Err(EditorProjectError::LegacyProjectCarriesInsertBlankPageOperation { .. })
        ));
        reopened
            .delete_blank_authored_page_v1(vec![a, b], identity.page_id)
            .expect("inserted empty page is deletable");
        assert_eq!(
            reopened.graph().document.pages,
            vec![master, a, service, b, carrier]
        );
        assert_eq!(reopened.authored_shapes[&shape_id], original_content);
    }

    #[test]
    fn duplicate_blank_page_preserves_raw_slots_and_replays_exact_identity() {
        let master = page_id("11111111-1111-4111-8111-111111111111");
        let source = page_id("22222222-2222-4222-8222-222222222222");
        let service = page_id("33333333-3333-4333-8333-333333333333");
        let later = page_id("44444444-4444-4444-8444-444444444444");
        let carrier = page_id("55555555-5555-4555-8555-555555555555");
        let identity = authored_identity();
        let graph = source_graph(vec![master, source, service, later, carrier]);
        let mut session = EditorSession::new(graph.clone()).expect("session");
        let source_hash = session.source_hash();

        session
            .duplicate_blank_page_v1(vec![source, later], source, identity)
            .expect("duplicate empty source customer page");
        let expected = vec![master, source, identity.page_id, service, later, carrier];
        assert_eq!(session.graph().document.pages, expected);
        assert_eq!(
            session
                .effective_customer_page_order_v1(&[source, later])
                .expect("effective order"),
            vec![source, identity.page_id, later]
        );
        assert_eq!(
            session.graph().pages[&identity.page_id].size,
            graph.pages[&source].size
        );
        assert_eq!(session.source_hash(), source_hash);

        session.undo().expect("undo duplicate");
        assert_eq!(session.graph().document.pages, graph.document.pages);
        session.redo().expect("redo duplicate");
        assert_eq!(session.graph().document.pages, expected);

        let project = session.project();
        assert_eq!(project.schema_version, EDITOR_PROJECT_VERSION_V0_27);
        let bytes = serde_json::to_vec(&project).expect("serialize project");
        let decoded: EditorProject = serde_json::from_slice(&bytes).expect("deserialize project");
        let mut reopened = EditorSession::new(graph).expect("fresh editor");
        reopened.apply_project(&decoded).expect("replay duplicate");
        assert_eq!(reopened.operations(), decoded.operations.as_slice());
        assert_eq!(reopened.graph().document.pages, expected);

        reopened
            .delete_blank_authored_page_v1(vec![source, later], identity.page_id)
            .expect("duplicate is eligible for DeleteBlank");
        assert_eq!(
            reopened.graph().document.pages,
            vec![master, source, service, later, carrier]
        );
        assert_eq!(reopened.source_hash(), source_hash);
        assert!(
            reopened
                .duplicate_blank_page_v1(vec![source, later], source, identity)
                .is_err()
        );
    }

    #[test]
    fn duplicated_blank_page_reaches_idml_and_odg_in_source_order() {
        let source = page_id("22222222-2222-4222-8222-222222222222");
        let identity = authored_identity();
        let mut session = EditorSession::new(source_graph(vec![source])).expect("session");
        session
            .duplicate_blank_page_v1(vec![source], source, identity)
            .expect("duplicate one source-admitted blank customer page");

        assert_eq!(
            session.graph().document.pages,
            vec![source, identity.page_id]
        );
        assert_eq!(
            session.graph().pages[&identity.page_id].size,
            session.graph().pages[&source].size
        );
        assert_eq!(
            session.graph().pages[&identity.page_id].bleed,
            session.graph().pages[&source].bleed
        );
        assert_eq!(
            session.graph().pages[&identity.page_id].margins,
            session.graph().pages[&source].margins
        );
        assert!(session.graph().pages[&identity.page_id].children.is_empty());

        let source_hex = page_hex(source);
        let duplicate_hex = page_hex(identity.page_id);
        let idml = session
            .export_editable(
                crate::EditorEditableTarget::Idml,
                "duplicate-blank-page-idml",
            )
            .expect("export duplicate to IDML");
        let designmap = read_zip_text(&idml.bytes, "designmap.xml");
        let source_spread = format!("Spreads/Spread_usp{source_hex}.xml");
        let duplicate_spread = format!("Spreads/Spread_usp{duplicate_hex}.xml");
        assert!(
            designmap.find(&source_spread).expect("source IDML spread")
                < designmap
                    .find(&duplicate_spread)
                    .expect("duplicate IDML spread"),
            "duplicate must follow source in IDML designmap"
        );
        let duplicate_xml = read_zip_text(&idml.bytes, &duplicate_spread);
        assert!(
            duplicate_xml.contains(&format!(
                r#"<Page Self="up{duplicate_hex}" GeometricBounds="0 0 144 72""#
            )),
            "IDML duplicate must preserve source physical page extent"
        );

        let odg = session
            .export_editable(crate::EditorEditableTarget::Odg, "duplicate-blank-page-odg")
            .expect("export duplicate to ODG");
        let content = read_zip_text(&odg.bytes, "content.xml");
        assert_eq!(content.matches("<draw:page ").count(), 2);
        assert!(
            content
                .find(&format!(r#"draw:name="Page_{source_hex}""#))
                .expect("source ODG page")
                < content
                    .find(&format!(r#"draw:name="Page_{duplicate_hex}""#))
                    .expect("duplicate ODG page"),
            "duplicate must follow source in ODG"
        );

        let styles = read_zip_text(&odg.bytes, "styles.xml");
        let duplicate_layout = format!(r#"<style:page-layout style:name="PM_{duplicate_hex}">"#);
        let start = styles
            .find(&duplicate_layout)
            .expect("duplicate ODG page layout");
        let tail = &styles[start..];
        let end = tail
            .find("</style:page-layout>")
            .expect("ODG page layout end");
        let layout = &tail[..end];
        assert!(layout.contains(r#"fo:page-width="72pt" fo:page-height="144pt""#));
    }

    #[test]
    fn delete_blank_authored_page_roundtrips_undo_redo_and_project_replay() {
        let source = page_id("22222222-2222-4222-8222-222222222222");
        let identity = authored_identity();
        let graph = source_graph(vec![source]);
        let mut session = EditorSession::new(graph.clone()).expect("session");
        let source_hash_before = session.source_hash();

        session
            .append_blank_page_v1(
                vec![source],
                identity,
                Size2D::new(LengthEmu::new(2_000_000), LengthEmu::new(3_000_000)),
                None,
                None,
            )
            .expect("append blank page");
        let after_append_pages = session.graph().document.pages.clone();
        let after_append_page = session
            .graph()
            .pages
            .get(&identity.page_id)
            .cloned()
            .expect("appended page");

        session
            .delete_blank_authored_page_v1(vec![source], identity.page_id)
            .expect("delete blank authored page");

        assert_eq!(session.graph().document.pages, vec![source]);
        assert!(!session.graph().pages.contains_key(&identity.page_id));
        assert_eq!(
            session
                .effective_customer_page_order_v1(&[source])
                .expect("effective after delete"),
            vec![source]
        );
        assert_eq!(session.source_hash(), source_hash_before);

        session.undo().expect("undo delete");
        assert_eq!(session.graph().document.pages, after_append_pages);
        assert_eq!(
            session.graph().pages.get(&identity.page_id),
            Some(&after_append_page)
        );
        assert_eq!(
            session
                .effective_customer_page_order_v1(&[source])
                .expect("effective after undo"),
            vec![source, identity.page_id]
        );

        session.redo().expect("redo delete");
        assert_eq!(session.graph().document.pages, vec![source]);
        assert!(!session.graph().pages.contains_key(&identity.page_id));

        let project = session.project();
        assert_eq!(project.schema_version, EDITOR_PROJECT_VERSION_V0_26);
        let bytes = serde_json::to_vec(&project).expect("serialize project");
        let decoded: EditorProject = serde_json::from_slice(&bytes).expect("deserialize project");

        let mut reopened = EditorSession::new(graph).expect("fresh session");
        reopened
            .apply_project(&decoded)
            .expect("replay delete project");
        assert_eq!(reopened.operations(), decoded.operations.as_slice());
        assert_eq!(reopened.graph().document.pages, vec![source]);
        assert!(!reopened.graph().pages.contains_key(&identity.page_id));
        assert_eq!(
            reopened
                .effective_customer_page_order_v1(&[source])
                .expect("replayed effective order"),
            vec![source]
        );
        assert_eq!(reopened.source_hash(), source_hash_before);
    }

    #[test]
    fn delete_blank_authored_page_blocks_same_page_id_resurrection() {
        let source = page_id("22222222-2222-4222-8222-222222222222");
        let identity = authored_identity();
        let mut session = EditorSession::new(source_graph(vec![source])).expect("session");

        session
            .append_blank_page_v1(
                vec![source],
                identity,
                Size2D::new(LengthEmu::new(2_000_000), LengthEmu::new(3_000_000)),
                None,
                None,
            )
            .expect("append");
        session
            .delete_blank_authored_page_v1(vec![source], identity.page_id)
            .expect("delete");

        assert_eq!(
            session.append_blank_page_v1(
                vec![source],
                identity,
                Size2D::new(LengthEmu::new(2_000_000), LengthEmu::new(3_000_000)),
                None,
                None,
            ),
            Err(EditorError::AuthoredPageIdentityConflict {
                page_id: identity.page_id,
            })
        );
        assert_eq!(session.operations().len(), 2);
    }

    #[test]
    fn delete_blank_authored_page_rejects_source_backed_and_final_customer_membership() {
        let source = page_id("22222222-2222-4222-8222-222222222222");
        let identity = authored_identity();
        let mut session = EditorSession::new(source_graph(vec![source])).expect("session");

        assert!(matches!(
            session.delete_blank_authored_page_v1(vec![source], source),
            Err(EditorError::PageDeleteUnsupported { .. })
        ));
        assert!(session.operations().is_empty());

        session
            .append_blank_page_v1(
                vec![source],
                identity,
                Size2D::new(LengthEmu::new(2_000_000), LengthEmu::new(3_000_000)),
                None,
                None,
            )
            .expect("append");
        assert!(matches!(
            session.delete_blank_authored_page_v1(Vec::new(), identity.page_id),
            Err(EditorError::PageDeleteUnsupported { .. })
        ));
        assert_eq!(session.operations().len(), 1);
        assert!(session.graph().pages.contains_key(&identity.page_id));
    }

    #[test]
    fn deleted_blank_page_is_absent_from_idml_and_odg_packages() {
        let source = page_id("22222222-2222-4222-8222-222222222222");
        let identity = authored_identity();
        let mut session = EditorSession::new(source_graph(vec![source])).expect("session");
        session
            .append_blank_page_v1(
                vec![source],
                identity,
                Size2D::new(
                    LengthEmu::new(200 * pub_model::EMU_PER_POINT),
                    LengthEmu::new(300 * pub_model::EMU_PER_POINT),
                ),
                None,
                None,
            )
            .expect("append");
        session
            .delete_blank_authored_page_v1(vec![source], identity.page_id)
            .expect("delete");

        let source_hex = page_hex(source);
        let deleted_hex = page_hex(identity.page_id);

        let idml = session
            .export_editable(crate::EditorEditableTarget::Idml, "delete-blank-page-idml")
            .expect("export IDML after delete");
        let designmap = read_zip_text(&idml.bytes, "designmap.xml");
        assert!(designmap.contains(&format!("Spreads/Spread_usp{source_hex}.xml")));
        assert!(!designmap.contains(&format!("Spreads/Spread_usp{deleted_hex}.xml")));

        let odg = session
            .export_editable(crate::EditorEditableTarget::Odg, "delete-blank-page-odg")
            .expect("export ODG after delete");
        let content = read_zip_text(&odg.bytes, "content.xml");
        let styles = read_zip_text(&odg.bytes, "styles.xml");
        assert!(content.contains(&format!("draw:name=\"Page_{source_hex}\"")));
        assert!(!content.contains(&format!("draw:name=\"Page_{deleted_hex}\"")));
        assert!(!styles.contains(&format!("style:name=\"PM_{deleted_hex}\"")));
    }
}

/// Atomically update the three candidate authorities. Forward requires the
/// caller's fresh live EditorSession admission; the pure planner's external
/// membership flag is false only after that proof. Inverse checks exact state.
pub(super) fn apply_authored_rectangle_page_history_candidate_v1(
    graph: &mut PubResolvedGraph,
    shapes: &mut BTreeMap<NodeId, AuthoredShapeRuntimeV1>,
    stack: AuthoredStackV1,
    transition: &DeleteAuthoredRectanglePageTransitionV1,
    forward: bool,
) -> Result<(), EditorError> {
    let mut candidate = DeleteAuthoredRectanglePageStateV1 {
        document_pages: graph.document.pages.clone(),
        pages: graph.pages.clone(),
        authored_shapes: shapes.clone(),
        authored_stack: stack,
    };
    let result = if forward {
        apply_delete_authored_rectangle_page_forward_v1(
            graph.document.id,
            &mut candidate,
            &transition.page.before_customer_page_ids,
            false,
            transition,
        )
    } else {
        apply_delete_authored_rectangle_page_inverse_v1(
            graph.document.id,
            &mut candidate,
            &transition.page.after_customer_page_ids,
            transition,
        )
    };
    result.map_err(|error| EditorError::PageDeleteUnsupported {
        message: format!("atomic rectangle-page transition rejected: {error:?}"),
    })?;
    graph.document.pages = candidate.document_pages;
    graph.pages = candidate.pages;
    *shapes = candidate.authored_shapes;
    Ok(())
}

/// Apply the same pure candidate against independently held Page + shape
/// authorities. Authored-stack history is owned by EditorSession's lane map.
pub(super) fn apply_authored_rectangle_page_duplicate_history_candidate_v1(
    graph: &mut PubResolvedGraph,
    shapes: &mut BTreeMap<NodeId, AuthoredShapeRuntimeV1>,
    source_stack: AuthoredStackV1,
    destination_stack: AuthoredStackV1,
    source_identity: AuthoredPageIdentityV1,
    transition: &DuplicateAuthoredRectanglePageTransitionV1,
    forward: bool,
) -> Result<(), EditorError> {
    let mut candidate = DuplicateAuthoredRectanglePageStateV1 {
        source_identity,
        document_pages: graph.document.pages.clone(),
        pages: graph.pages.clone(),
        authored_shapes: shapes.clone(),
        source_stack,
        destination_stack,
    };
    let result = if forward {
        apply_duplicate_authored_rectangle_page_forward_v1(
            graph.document.id,
            &mut candidate,
            &transition.page.before_customer_page_ids,
            false,
            transition,
        )
    } else {
        apply_duplicate_authored_rectangle_page_inverse_v1(
            graph.document.id,
            &mut candidate,
            &transition.page.after_customer_page_ids,
            transition,
        )
    };
    result.map_err(|error| EditorError::PageDuplicateUnsupported {
        message: format!("atomic authored Rectangle Page transition rejected: {error:?}"),
    })?;
    graph.document.pages = candidate.document_pages;
    graph.pages = candidate.pages;
    *shapes = candidate.authored_shapes;
    Ok(())
}

pub(super) fn apply_authored_rectangles_page_duplicate_history_candidate_v1(
    graph: &mut PubResolvedGraph,
    shapes: &mut BTreeMap<NodeId, AuthoredShapeRuntimeV1>,
    source_stack: AuthoredStackV1,
    destination_stack: AuthoredStackV1,
    source_identity: AuthoredPageIdentityV1,
    transition: &DuplicateAuthoredRectanglesPageTransitionV1,
    forward: bool,
) -> Result<(), EditorError> {
    let mut candidate = DuplicateAuthoredRectanglePageStateV1 {
        source_identity,
        document_pages: graph.document.pages.clone(),
        pages: graph.pages.clone(),
        authored_shapes: shapes.clone(),
        source_stack,
        destination_stack,
    };
    let result = if forward {
        apply_duplicate_authored_rectangles_page_forward_v1(
            graph.document.id,
            &mut candidate,
            &transition.page.before_customer_page_ids,
            false,
            transition,
        )
    } else {
        apply_duplicate_authored_rectangles_page_inverse_v1(
            graph.document.id,
            &mut candidate,
            &transition.page.after_customer_page_ids,
            transition,
        )
    };
    result.map_err(|error| EditorError::PageDuplicateUnsupported {
        message: format!("atomic authored multi-Rectangle Page transition rejected: {error:?}"),
    })?;
    graph.document.pages = candidate.document_pages;
    graph.pages = candidate.pages;
    *shapes = candidate.authored_shapes;
    Ok(())
}

// Canonical authored overlay replay helpers, extracted unchanged from lib.rs.
pub(super) fn authored_shape_from_operation(
    operation: &EditOperation,
) -> Option<AuthoredShapeRuntimeV1> {
    match operation {
        EditOperation::CreateShape {
            node_id,
            page_id,
            parent_id,
            shape_kind,
            bounds,
            transform,
            paint,
            provenance,
        } => Some(AuthoredShapeRuntimeV1 {
            node_id: *node_id,
            page_id: *page_id,
            parent_id: *parent_id,
            shape_kind: *shape_kind,
            bounds: *bounds,
            transform: *transform,
            paint: paint.clone(),
            provenance: *provenance,
        }),
        _ => None,
    }
}

pub(super) fn authored_line_from_operation(
    operation: &EditOperation,
) -> Option<AuthoredLineRuntimeV1> {
    match operation {
        EditOperation::CreateLine {
            node_id,
            page_id,
            parent_id,
            geometry,
            stroke,
            provenance,
        } => Some(AuthoredLineRuntimeV1 {
            node_id: *node_id,
            page_id: *page_id,
            parent_id: *parent_id,
            geometry: *geometry,
            stroke: stroke.clone(),
            provenance: *provenance,
        }),
        _ => None,
    }
}

pub(super) fn apply_authored_line_inverse(
    authored_lines: &mut BTreeMap<NodeId, AuthoredLineRuntimeV1>,
    operation: &EditOperation,
) -> Result<(), EditorError> {
    let line = authored_line_from_operation(operation)
        .expect("CreateLine inverse receives CreateLine operation");
    if authored_lines.get(&line.node_id) != Some(&line) {
        return Err(EditorError::CreateLineIdCollision {
            node_id: line.node_id,
        });
    }
    authored_lines.remove(&line.node_id);
    Ok(())
}

pub(super) fn apply_authored_shape_inverse(
    authored_shapes: &mut BTreeMap<NodeId, AuthoredShapeRuntimeV1>,
    operation: &EditOperation,
) -> Result<(), EditorError> {
    let shape = authored_shape_from_operation(operation)
        .expect("CreateShape inverse receives CreateShape operation");
    if authored_shapes.get(&shape.node_id) != Some(&shape) {
        return Err(EditorError::CreateShapeIdCollision {
            node_id: shape.node_id,
        });
    }
    authored_shapes.remove(&shape.node_id);
    Ok(())
}

pub(super) fn apply_authored_shape_delete_forward(
    authored_shapes: &mut BTreeMap<NodeId, AuthoredShapeRuntimeV1>,
    operation: &EditOperation,
) -> Result<(), EditorError> {
    let EditOperation::DeleteNode {
        node_id,
        page_id,
        before,
        before_state_id,
    } = operation
    else {
        unreachable!("DeleteNode forward receives DeleteNode operation")
    };

    if before.node_id != *node_id || before.page_id != *page_id || before.parent_id != *page_id {
        return Err(EditorError::NodeDeletePageMismatch {
            node_id: *node_id,
            page_id: *page_id,
        });
    }
    if authored_shape_state_id_v1(before) != *before_state_id
        || authored_shapes.get(node_id) != Some(before)
    {
        return Err(EditorError::StaleNodeDelete { node_id: *node_id });
    }
    authored_shapes.remove(node_id);
    Ok(())
}

pub(super) fn apply_authored_shape_delete_inverse(
    authored_shapes: &mut BTreeMap<NodeId, AuthoredShapeRuntimeV1>,
    operation: &EditOperation,
) -> Result<(), EditorError> {
    let EditOperation::DeleteNode {
        node_id,
        page_id,
        before,
        before_state_id,
    } = operation
    else {
        unreachable!("DeleteNode inverse receives DeleteNode operation")
    };

    if before.node_id != *node_id || before.page_id != *page_id || before.parent_id != *page_id {
        return Err(EditorError::NodeDeletePageMismatch {
            node_id: *node_id,
            page_id: *page_id,
        });
    }
    if authored_shape_state_id_v1(before) != *before_state_id
        || authored_shapes.contains_key(node_id)
    {
        return Err(EditorError::StaleNodeDelete { node_id: *node_id });
    }
    authored_shapes.insert(*node_id, before.clone());
    Ok(())
}

// Composite authored Page history lives alongside canonical Page admission.
impl EditorSession {
    pub(super) fn undo_delete_rectangle_page_candidate_v1(
        &self,
        graph: &mut PubResolvedGraph,
        shapes: &mut BTreeMap<NodeId, AuthoredShapeRuntimeV1>,
        transition: &DeleteAuthoredRectanglePageTransitionV1,
    ) -> Result<(), EditorError> {
        apply_authored_rectangle_page_history_candidate_v1(
            graph,
            shapes,
            self.current_authored_stack_v1(transition.page.identity.page_id),
            transition,
            false,
        )
    }

    pub(super) fn redo_delete_rectangle_page_candidate_v1(
        &self,
        graph: &mut PubResolvedGraph,
        shapes: &mut BTreeMap<NodeId, AuthoredShapeRuntimeV1>,
        transition: &DeleteAuthoredRectanglePageTransitionV1,
    ) -> Result<(), EditorError> {
        let authored = self
            .authored_customer_page_ids_v1()
            .into_iter()
            .collect::<BTreeSet<_>>();
        let sources = transition
            .page
            .before_customer_page_ids
            .iter()
            .copied()
            .filter(|id| !authored.contains(id))
            .collect::<Vec<_>>();
        let fresh = self.plan_delete_authored_rectangle_page_from_session_v1(
            &sources,
            transition.page.identity.page_id,
        )?;
        if fresh != *transition {
            return Err(EditorError::StalePageDelete);
        }
        apply_authored_rectangle_page_history_candidate_v1(
            graph,
            shapes,
            self.current_authored_stack_v1(transition.page.identity.page_id),
            transition,
            true,
        )
    }

    pub(super) fn undo_duplicate_rectangle_page_candidate_v1(
        &self,
        graph: &mut PubResolvedGraph,
        shapes: &mut BTreeMap<NodeId, AuthoredShapeRuntimeV1>,
        transition: &DuplicateAuthoredRectanglePageTransitionV1,
    ) -> Result<(), EditorError> {
        let source_id = transition.page.source_page_id;
        apply_authored_rectangle_page_duplicate_history_candidate_v1(
            graph,
            shapes,
            self.current_authored_stack_v1(source_id),
            self.current_authored_stack_v1(transition.page.destination_identity.page_id),
            self.authored_page_identities_v1()
                .get(&source_id)
                .copied()
                .ok_or(EditorError::StalePageDuplicate)?,
            transition,
            false,
        )
    }

    pub(super) fn redo_duplicate_rectangle_page_candidate_v1(
        &self,
        graph: &mut PubResolvedGraph,
        shapes: &mut BTreeMap<NodeId, AuthoredShapeRuntimeV1>,
        transition: &DuplicateAuthoredRectanglePageTransitionV1,
    ) -> Result<(), EditorError> {
        // The redo entry has been popped: no revision or new identity is
        // allocated while revalidating the exact historical transition.
        let authored = self
            .authored_customer_page_ids_v1()
            .into_iter()
            .collect::<BTreeSet<_>>();
        let sources = transition
            .page
            .before_customer_page_ids
            .iter()
            .copied()
            .filter(|id| !authored.contains(id))
            .collect::<Vec<_>>();
        let fresh = self.plan_duplicate_authored_rectangle_page_from_session_v1(
            &sources,
            transition.page.source_page_id,
            transition.page.destination_identity,
            transition.destination_shape.node_id,
        )?;
        if fresh != *transition {
            return Err(EditorError::StalePageDuplicate);
        }
        apply_authored_rectangle_page_duplicate_history_candidate_v1(
            graph,
            shapes,
            self.current_authored_stack_v1(transition.page.source_page_id),
            self.current_authored_stack_v1(transition.page.destination_identity.page_id),
            self.authored_page_identities_v1()
                .get(&transition.page.source_page_id)
                .copied()
                .ok_or(EditorError::StalePageDuplicate)?,
            transition,
            true,
        )
    }

    pub(super) fn undo_duplicate_rectangles_page_candidate_v1(
        &self,
        graph: &mut PubResolvedGraph,
        shapes: &mut BTreeMap<NodeId, AuthoredShapeRuntimeV1>,
        transition: &DuplicateAuthoredRectanglesPageTransitionV1,
    ) -> Result<(), EditorError> {
        let source_id = transition.page.source_page_id;
        apply_authored_rectangles_page_duplicate_history_candidate_v1(
            graph,
            shapes,
            self.current_authored_stack_v1(source_id),
            self.current_authored_stack_v1(transition.page.destination_identity.page_id),
            self.authored_page_identities_v1()
                .get(&source_id)
                .copied()
                .ok_or(EditorError::StalePageDuplicate)?,
            transition,
            false,
        )
    }

    pub(super) fn redo_duplicate_rectangles_page_candidate_v1(
        &self,
        graph: &mut PubResolvedGraph,
        shapes: &mut BTreeMap<NodeId, AuthoredShapeRuntimeV1>,
        transition: &DuplicateAuthoredRectanglesPageTransitionV1,
    ) -> Result<(), EditorError> {
        let authored = self
            .authored_customer_page_ids_v1()
            .into_iter()
            .collect::<BTreeSet<_>>();
        let sources = transition
            .page
            .before_customer_page_ids
            .iter()
            .copied()
            .filter(|id| !authored.contains(id))
            .collect::<Vec<_>>();
        let destinations = transition
            .destination_shapes
            .iter()
            .map(|shape| shape.node_id)
            .collect::<Vec<_>>();
        let fresh = self.plan_duplicate_authored_rectangles_page_from_session_v1(
            &sources,
            transition.page.source_page_id,
            transition.page.destination_identity,
            &destinations,
        )?;
        if fresh != *transition {
            return Err(EditorError::StalePageDuplicate);
        }
        apply_authored_rectangles_page_duplicate_history_candidate_v1(
            graph,
            shapes,
            self.current_authored_stack_v1(transition.page.source_page_id),
            self.current_authored_stack_v1(transition.page.destination_identity.page_id),
            self.authored_page_identities_v1()
                .get(&transition.page.source_page_id)
                .copied()
                .ok_or(EditorError::StalePageDuplicate)?,
            transition,
            true,
        )
    }
}
