//! One-history-operation Undo/Redo consumers for content-bearing authored Pages.
//!
//! The Page/Rectangle pure transitions live in pub-editor-authoring-core.
//! Session admission is owned by session_geometry. This module exists so the
//! central pub-editor enum/history orchestration does not grow another large
//! inline implementation for a new composite Page operation.

use super::*;

impl EditorSession {
    pub(super) fn undo_delete_rectangle_page_candidate_v1(
        &self,
        graph: &mut PubResolvedGraph,
        shapes: &mut BTreeMap<NodeId, AuthoredShapeRuntimeV1>,
        transition: &DeleteAuthoredRectanglePageTransitionV1,
    ) -> Result<(), EditorError> {
        session_geometry::apply_authored_rectangle_page_history_candidate_v1(
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
        session_geometry::apply_authored_rectangle_page_history_candidate_v1(
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
        session_geometry::apply_authored_rectangle_page_duplicate_history_candidate_v1(
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
        session_geometry::apply_authored_rectangle_page_duplicate_history_candidate_v1(
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
