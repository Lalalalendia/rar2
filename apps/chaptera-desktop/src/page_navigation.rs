//! Canonical Desktop page navigation and customer-page reorder routing.
//!
//! Navigation changes only the current view. Page reorder delegates durable
//! order to EditorSession::reorder_pages_v1 and mirrors that canonical order
//! back into the Viewer by stable PageId.

use super::{ViewerApp, reader_only_mode, supporter};
use eframe::egui;
use pub_editor::{AuthoredEntityProvenanceV1, AuthoredPageIdentityV1, PageId};
use std::collections::BTreeMap;

impl ViewerApp {
    pub(super) fn navigate_to_page_index(&mut self, index: usize) -> bool {
        let page_count = self
            .visual
            .as_ref()
            .map(|visual| visual.document.pages.len())
            .unwrap_or(0);
        if index >= page_count || index == self.selected_page {
            return false;
        }

        self.selected_page = index;
        self.canvas_selection.clear();
        self.canvas_drag = None;
        self.canvas_resize = None;
        self.supporter_value
            .observe(supporter::ValueEvent::PageNavigated { page_index: index });
        true
    }

    pub(super) fn sync_visual_page_membership_from_editor(&mut self) -> Result<bool, String> {
        let Some(editor) = self.editor.as_ref() else {
            return Ok(false);
        };
        let Some(visual) = self.visual.as_ref() else {
            return Ok(false);
        };

        let selected_page_id = visual
            .document
            .pages
            .get(self.selected_page)
            .map(|page| page.id);
        let current_page_ids = visual
            .document
            .pages
            .iter()
            .map(|page| page.id)
            .collect::<Vec<_>>();
        let effective_page_ids = editor
            .effective_customer_page_order_v1(&self.source_customer_page_ids)
            .map_err(|error| {
                format!(
                    "Page membership projection is unavailable: {} ({})",
                    error,
                    error.code()
                )
            })?;
        if current_page_ids == effective_page_ids {
            return Ok(false);
        }

        {
            let editor = self
                .editor
                .as_ref()
                .expect("Editor presence validated before page-membership sync");
            let visual = self
                .visual
                .as_mut()
                .expect("Viewer presence validated before page-membership sync");
            visual
                .refresh_page_membership_from_resolved(editor.graph(), &effective_page_ids)
                .map_err(|error| error.to_string())?;
        }

        let next_index = selected_page_id
            .and_then(|page_id| {
                self.visual.as_ref().and_then(|visual| {
                    visual
                        .document
                        .pages
                        .iter()
                        .position(|page| page.id == page_id)
                })
            })
            .unwrap_or_else(|| {
                if effective_page_ids.is_empty() {
                    0
                } else {
                    self.selected_page.min(effective_page_ids.len() - 1)
                }
            });
        let next_selected_page_id = self
            .visual
            .as_ref()
            .and_then(|visual| visual.document.pages.get(next_index))
            .map(|page| page.id);

        self.selected_page = next_index;
        if selected_page_id != next_selected_page_id {
            self.canvas_selection.clear();
            self.canvas_drag = None;
            self.canvas_resize = None;
        }
        Ok(true)
    }

    pub(super) fn sync_visual_page_order_from_editor(&mut self) {
        let Some(editor) = self.editor.as_ref() else {
            return;
        };
        let Some(visual) = self.visual.as_ref() else {
            return;
        };
        let selected_page_id = visual
            .document
            .pages
            .get(self.selected_page)
            .map(|page| page.id);
        let qualified_page_ids = visual
            .document
            .pages
            .iter()
            .map(|page| page.id)
            .collect::<Vec<_>>();
        let Ok(page_order) = editor.current_qualified_page_order_v1(&qualified_page_ids) else {
            return;
        };
        if page_order.len() != qualified_page_ids.len() {
            return;
        }
        let rank = page_order
            .iter()
            .copied()
            .enumerate()
            .map(|(index, page_id)| (page_id, index))
            .collect::<BTreeMap<PageId, usize>>();
        if rank.len() != visual.document.pages.len()
            || visual
                .document
                .pages
                .iter()
                .any(|page| !rank.contains_key(&page.id))
        {
            return;
        }

        let visual = self
            .visual
            .as_mut()
            .expect("Viewer presence validated before page-order sync");
        visual
            .document
            .pages
            .sort_by_key(|page| rank.get(&page.id).copied().unwrap_or(usize::MAX));
        for (index, page) in visual.document.pages.iter_mut().enumerate() {
            page.index = u32::try_from(index + 1).unwrap_or(u32::MAX);
        }
        if let Some(selected_page_id) = selected_page_id
            && let Some(index) = visual
                .document
                .pages
                .iter()
                .position(|page| page.id == selected_page_id)
        {
            self.selected_page = index;
        }
    }

    fn page_append_capability_v1(&self) -> bool {
        let Some(editor) = self.editor.as_ref() else {
            return false;
        };
        let Ok(effective_page_ids) =
            editor.effective_customer_page_order_v1(&self.source_customer_page_ids)
        else {
            return false;
        };
        let Some(last_page_id) = effective_page_ids.last() else {
            return false;
        };
        editor
            .graph()
            .pages
            .get(last_page_id)
            .is_some_and(|page| page.size.is_positive())
    }

    fn append_blank_page_at_end_v1(&mut self) -> Result<PageId, String> {
        let (operation_count_before, page_size) = {
            let editor = self
                .editor
                .as_ref()
                .ok_or_else(|| "Editor session is unavailable.".to_owned())?;
            let effective_page_ids = editor
                .effective_customer_page_order_v1(&self.source_customer_page_ids)
                .map_err(|error| {
                    format!("Page append is unavailable: {} ({})", error, error.code())
                })?;
            let last_page_id = effective_page_ids.last().copied().ok_or_else(|| {
                "Page append requires at least one admitted customer page.".to_owned()
            })?;
            let page_size = editor
                .graph()
                .pages
                .get(&last_page_id)
                .ok_or_else(|| "Last admitted customer page is unavailable.".to_owned())?
                .size;
            (editor.operations().len(), page_size)
        };

        let uuid = uuid::Uuid::now_v7();
        let identity = AuthoredPageIdentityV1 {
            page_id: PageId::from_canonical(pub_model::CanonicalId::from_bytes(*uuid.as_bytes())),
            provenance: AuthoredEntityProvenanceV1::AuthorCreated,
        };

        let mut candidate = self
            .editor
            .as_ref()
            .expect("Editor presence validated before page append")
            .clone();
        candidate
            .append_blank_page_v1(
                self.source_customer_page_ids.clone(),
                identity,
                page_size,
                None,
                None,
            )
            .map_err(|error| format!("Page append rejected: {} ({})", error, error.code()))?;
        if candidate.operations().len() != operation_count_before + 1
            || !matches!(
                candidate.operations().last(),
                Some(pub_editor::EditOperation::AppendBlankPageV1 { transition })
                    if transition.identity == identity
            )
        {
            return Err(
                "Add Page must append exactly one canonical lifecycle operation.".to_owned(),
            );
        }

        let effective_page_ids = candidate
            .effective_customer_page_order_v1(&self.source_customer_page_ids)
            .map_err(|error| {
                format!(
                    "Page append projection is unavailable before commit: {} ({})",
                    error,
                    error.code()
                )
            })?;
        let mut visual_candidate = self
            .visual
            .as_ref()
            .ok_or_else(|| "Document page projection is unavailable.".to_owned())?
            .clone();
        visual_candidate
            .refresh_page_membership_from_resolved(candidate.graph(), &effective_page_ids)
            .map_err(|error| format!("Page append projection rejected before commit: {error}"))?;
        if !visual_candidate
            .document
            .pages
            .iter()
            .any(|page| page.id == identity.page_id)
        {
            return Err(
                "Page append projection did not contain the new canonical PageId.".to_owned(),
            );
        }

        self.editor = Some(candidate);
        self.visual = Some(visual_candidate);
        self.finish_authoring_change(
            "Appended one blank customer page at publication end. Source PUB bytes were not written.",
        );

        let new_index = self
            .visual
            .as_ref()
            .and_then(|visual| {
                visual
                    .document
                    .pages
                    .iter()
                    .position(|page| page.id == identity.page_id)
            })
            .ok_or_else(|| {
                "Page append committed, but Viewer membership refresh failed closed.".to_owned()
            })?;
        self.navigate_to_page_index(new_index);
        Ok(identity.page_id)
    }

    fn page_duplicate_blank_capability_v1(&self) -> bool {
        let Some(editor) = self.editor.as_ref() else {
            return false;
        };
        let Some(visual) = self.visual.as_ref() else {
            return false;
        };
        let Some(source_page_id) = visual
            .document
            .pages
            .get(self.selected_page)
            .map(|page| page.id)
        else {
            return false;
        };
        editor.can_duplicate_blank_page_v1(&self.source_customer_page_ids, source_page_id)
    }

    fn duplicate_selected_blank_page_v1(&mut self) -> Result<PageId, String> {
        let (operations_before, source_page_id, before_page_ids, source_index) = {
            let editor = self
                .editor
                .as_ref()
                .ok_or_else(|| "Editor session is unavailable.".to_owned())?;
            let visual = self
                .visual
                .as_ref()
                .ok_or_else(|| "Document page projection is unavailable.".to_owned())?;
            let source_page_id = visual
                .document
                .pages
                .get(self.selected_page)
                .map(|page| page.id)
                .ok_or_else(|| "Selected source PageId is unavailable.".to_owned())?;
            let before_page_ids = editor
                .effective_customer_page_order_v1(&self.source_customer_page_ids)
                .map_err(|error| {
                    format!(
                        "Page duplicate is unavailable: {} ({})",
                        error,
                        error.code()
                    )
                })?;
            let source_index = before_page_ids
                .iter()
                .position(|page_id| *page_id == source_page_id)
                .ok_or_else(|| {
                    "Selected source PageId is absent from customer membership.".to_owned()
                })?;
            (
                editor.operations().len(),
                source_page_id,
                before_page_ids,
                source_index,
            )
        };

        let uuid = uuid::Uuid::now_v7();
        let identity = AuthoredPageIdentityV1 {
            page_id: PageId::from_canonical(pub_model::CanonicalId::from_bytes(*uuid.as_bytes())),
            provenance: AuthoredEntityProvenanceV1::AuthorCreated,
        };
        let mut candidate = self
            .editor
            .as_ref()
            .expect("Editor presence validated before page duplicate")
            .clone();
        candidate
            .duplicate_blank_page_v1(
                self.source_customer_page_ids.clone(),
                source_page_id,
                identity,
            )
            .map_err(|error| format!("Page duplicate rejected: {} ({})", error, error.code()))?;
        if candidate.operations().len() != operations_before + 1
            || !matches!(
                candidate.operations().last(),
                Some(pub_editor::EditOperation::DuplicateBlankPageV1 { transition })
                    if transition.source_page_id == source_page_id
                        && transition.destination_identity == identity
            )
        {
            return Err(
                "Duplicate Blank Page must append exactly one canonical lifecycle operation."
                    .to_owned(),
            );
        }

        let after_page_ids = candidate
            .effective_customer_page_order_v1(&self.source_customer_page_ids)
            .map_err(|error| {
                format!(
                    "Page duplicate membership is unavailable before commit: {} ({})",
                    error,
                    error.code()
                )
            })?;
        let mut expected_page_ids = before_page_ids;
        expected_page_ids.insert(source_index + 1, identity.page_id);
        if after_page_ids != expected_page_ids {
            return Err(
                "Page duplicate did not insert the new PageId immediately after source.".to_owned(),
            );
        }

        let mut visual_candidate = self
            .visual
            .as_ref()
            .ok_or_else(|| "Document page projection is unavailable.".to_owned())?
            .clone();
        visual_candidate
            .refresh_page_membership_from_resolved(candidate.graph(), &after_page_ids)
            .map_err(|error| {
                format!("Page duplicate projection rejected before commit: {error}")
            })?;
        if visual_candidate
            .document
            .pages
            .iter()
            .map(|page| page.id)
            .collect::<Vec<_>>()
            != after_page_ids
            || !visual_candidate
                .scene
                .surfaces
                .iter()
                .any(|surface| surface.origin == identity.page_id)
        {
            return Err(
                "Page duplicate Viewer projection did not contain the exact canonical membership."
                    .to_owned(),
            );
        }
        let destination_index = source_index + 1;
        if visual_candidate
            .document
            .pages
            .get(destination_index)
            .map(|page| page.id)
            != Some(identity.page_id)
        {
            return Err("Page duplicate destination selection is unavailable.".to_owned());
        }

        self.editor = Some(candidate);
        self.visual = Some(visual_candidate);
        self.selected_page = destination_index;
        self.canvas_selection.clear();
        self.canvas_drag = None;
        self.canvas_resize = None;
        self.finish_authoring_change(
            "Duplicated one blank customer page with independent PageId. Source PUB bytes were not written.",
        );
        Ok(identity.page_id)
    }

    /// Read-only admission for exactly one independent AuthorCreated
    /// Rectangle on one active AuthorCreated customer Page.
    fn page_duplicate_rectangle_capability_v1(&self) -> bool {
        let (Some(editor), Some(visual)) = (self.editor.as_ref(), self.visual.as_ref()) else {
            return false;
        };
        let Some(source_page_id) = visual
            .document
            .pages
            .get(self.selected_page)
            .map(|page| page.id)
        else {
            return false;
        };
        editor.can_duplicate_authored_rectangle_page_v1(
            &self.source_customer_page_ids,
            source_page_id,
        )
    }

    /// One real GUI command -> one canonical v0.30 Page+Rectangle+stack
    /// history operation. Preflight the Viewer clone before any live commit.
    fn duplicate_selected_authored_rectangle_page_v1(&mut self) -> Result<PageId, String> {
        let editor = self
            .editor
            .as_ref()
            .ok_or_else(|| "Editor session is unavailable.".to_owned())?;
        let visual = self
            .visual
            .as_ref()
            .ok_or_else(|| "Document page projection is unavailable.".to_owned())?;
        let source_page_id = visual
            .document
            .pages
            .get(self.selected_page)
            .map(|page| page.id)
            .ok_or_else(|| "Selected source PageId is unavailable.".to_owned())?;
        let before_page_ids = editor
            .effective_customer_page_order_v1(&self.source_customer_page_ids)
            .map_err(|error| {
                format!(
                    "Rectangle-page duplicate is unavailable: {} ({})",
                    error,
                    error.code()
                )
            })?;
        let source_index = before_page_ids
            .iter()
            .position(|id| *id == source_page_id)
            .ok_or_else(|| "Selected PageId is not active customer membership.".to_owned())?;
        if !editor.can_duplicate_authored_rectangle_page_v1(
            &self.source_customer_page_ids,
            source_page_id,
        ) {
            return Err(
                "Only one independent AuthorCreated Rectangle Page can be duplicated.".to_owned(),
            );
        }
        let operations_before = editor.operations().len();
        let identity = AuthoredPageIdentityV1 {
            page_id: PageId::from_canonical(pub_model::new_editor_canonical_id()),
            provenance: AuthoredEntityProvenanceV1::AuthorCreated,
        };
        let destination_node_id =
            pub_editor::NodeId::from_canonical(pub_model::new_editor_canonical_id());
        let mut candidate = editor.clone();
        candidate
            .duplicate_authored_rectangle_page_v1(
                self.source_customer_page_ids.clone(),
                source_page_id,
                identity,
                destination_node_id,
            )
            .map_err(|error| {
                format!(
                    "Rectangle-page duplicate rejected: {} ({})",
                    error,
                    error.code()
                )
            })?;

        let source_shape_id = match candidate.operations().last() {
            Some(pub_editor::EditOperation::DuplicateAuthoredRectanglePageV1 { transition })
                if candidate.operations().len() == operations_before + 1
                    && transition.page.source_page_id == source_page_id
                    && transition.page.destination_identity == identity
                    && transition.page.before_customer_page_ids == before_page_ids
                    && transition.destination_shape.node_id == destination_node_id
                    && transition.destination_shape.page_id == identity.page_id
                    && transition.destination_shape.parent_id == identity.page_id =>
            {
                transition.source_shape.node_id
            }
            _ => {
                return Err(
                    "Duplicate Rectangle Page must append exactly one canonical v0.30 operation."
                        .to_owned(),
                );
            }
        };
        let source_shape = editor
            .authored_shape(source_shape_id)
            .ok_or_else(|| "Source authored Rectangle disappeared before commit.".to_owned())?;
        let duplicated_shape = candidate
            .authored_shape(destination_node_id)
            .ok_or_else(|| "Duplicated Rectangle is absent from Editor authority.".to_owned())?;
        if duplicated_shape.bounds != source_shape.bounds
            || duplicated_shape.paint != source_shape.paint
            || candidate
                .authored_stack(identity.page_id)
                .is_none_or(|stack| {
                    stack.members.len() != 1 || stack.members[0] != destination_node_id
                })
        {
            return Err(
                "Duplicated Rectangle geometry, paint or stack differs from source.".to_owned(),
            );
        }
        let after_page_ids = candidate
            .effective_customer_page_order_v1(&self.source_customer_page_ids)
            .map_err(|error| {
                format!(
                    "Rectangle-page membership unavailable: {} ({})",
                    error,
                    error.code()
                )
            })?;
        let mut expected_page_ids = before_page_ids;
        expected_page_ids.insert(source_index + 1, identity.page_id);
        if after_page_ids != expected_page_ids {
            return Err(
                "Rectangle-page duplicate did not insert immediately after source.".to_owned(),
            );
        }

        let mut visual_candidate = visual.clone();
        visual_candidate
            .refresh_page_membership_from_resolved(candidate.graph(), &after_page_ids)
            .map_err(|error| {
                format!("Rectangle-page Viewer projection rejected before commit: {error}")
            })?;
        let destination_index = source_index + 1;
        if visual_candidate
            .document
            .pages
            .iter()
            .map(|page| page.id)
            .collect::<Vec<_>>()
            != after_page_ids
            || visual_candidate
                .document
                .pages
                .get(destination_index)
                .map(|page| page.id)
                != Some(identity.page_id)
            || !visual_candidate
                .scene
                .surfaces
                .iter()
                .any(|surface| surface.origin == identity.page_id)
        {
            return Err(
                "Rectangle-page Viewer projection failed to preserve exact Page membership."
                    .to_owned(),
            );
        }

        self.editor = Some(candidate);
        self.visual = Some(visual_candidate);
        self.selected_page = destination_index;
        self.canvas_selection.clear();
        self.canvas_drag = None;
        self.canvas_resize = None;
        self.finish_authoring_change(
            "Duplicated one Chaptera-created Rectangle Page as a single Undo unit. Source PUB bytes were not written.",
        );
        Ok(identity.page_id)
    }

    /// Insert a new blank customer Page after the selected *stable* PageId.
    ///
    /// The selected page may already own content. Only its physical page
    /// metrics are copied; Node, Story, resource and extension identities are not.
    fn page_insert_after_capability_v1(&self) -> bool {
        let Some(editor) = self.editor.as_ref() else {
            return false;
        };
        let Some(visual) = self.visual.as_ref() else {
            return false;
        };
        let Some(anchor_page_id) = visual
            .document
            .pages
            .get(self.selected_page)
            .map(|page| page.id)
        else {
            return false;
        };
        let Ok(customer_page_ids) =
            editor.effective_customer_page_order_v1(&self.source_customer_page_ids)
        else {
            return false;
        };
        customer_page_ids.contains(&anchor_page_id)
            && editor
                .graph()
                .pages
                .get(&anchor_page_id)
                .is_some_and(|page| page.size.is_positive())
    }

    fn insert_selected_blank_page_after_v1(&mut self) -> Result<PageId, String> {
        let (operations_before, anchor_page_id, before_page_ids, anchor_index, anchor_page) = {
            let editor = self
                .editor
                .as_ref()
                .ok_or_else(|| "Editor session is unavailable.".to_owned())?;
            let visual = self
                .visual
                .as_ref()
                .ok_or_else(|| "Document page projection is unavailable.".to_owned())?;
            let anchor_page_id = visual
                .document
                .pages
                .get(self.selected_page)
                .map(|page| page.id)
                .ok_or_else(|| "Selected PageId is unavailable.".to_owned())?;
            let before_page_ids = editor
                .effective_customer_page_order_v1(&self.source_customer_page_ids)
                .map_err(|error| {
                    format!("Page insert is unavailable: {} ({})", error, error.code())
                })?;
            let anchor_index = before_page_ids
                .iter()
                .position(|page_id| *page_id == anchor_page_id)
                .ok_or_else(|| {
                    "Selected PageId is absent from admitted customer membership.".to_owned()
                })?;
            let anchor_page = editor
                .graph()
                .pages
                .get(&anchor_page_id)
                .ok_or_else(|| "Selected canonical Page is unavailable.".to_owned())?
                .clone();
            if !anchor_page.size.is_positive() {
                return Err("Selected canonical Page has invalid dimensions.".to_owned());
            }
            (
                editor.operations().len(),
                anchor_page_id,
                before_page_ids,
                anchor_index,
                anchor_page,
            )
        };

        let uuid = uuid::Uuid::now_v7();
        let identity = AuthoredPageIdentityV1 {
            page_id: PageId::from_canonical(pub_model::CanonicalId::from_bytes(*uuid.as_bytes())),
            provenance: AuthoredEntityProvenanceV1::AuthorCreated,
        };
        let mut candidate = self
            .editor
            .as_ref()
            .expect("Editor validated before page insert")
            .clone();
        candidate
            .insert_blank_page_after_v1(
                self.source_customer_page_ids.clone(),
                anchor_page_id,
                identity,
                anchor_page.size,
                anchor_page.bleed,
                anchor_page.margins,
            )
            .map_err(|error| format!("Page insert rejected: {} ({})", error, error.code()))?;
        if candidate.operations().len() != operations_before + 1
            || !matches!(
                candidate.operations().last(),
                Some(pub_editor::EditOperation::InsertBlankPageAfterV1 { transition })
                    if transition.anchor_page_id == anchor_page_id
                        && transition.identity == identity
                        && transition.before_customer_page_ids == before_page_ids
            )
        {
            return Err(
                "Insert Blank After Selected must append exactly one canonical lifecycle operation."
                    .to_owned(),
            );
        }
        if candidate.graph().pages.get(&anchor_page_id) != Some(&anchor_page) {
            return Err("Page insert changed its content-bearing anchor before commit.".to_owned());
        }
        let destination_page = candidate
            .graph()
            .pages
            .get(&identity.page_id)
            .ok_or_else(|| "Page insert did not create a destination Page.".to_owned())?;
        if destination_page.size != anchor_page.size
            || destination_page.bleed != anchor_page.bleed
            || destination_page.margins != anchor_page.margins
            || !destination_page.children.is_empty()
            || !destination_page.extensions.is_empty()
        {
            return Err("Page insert destination is not an independent blank Page.".to_owned());
        }

        let after_page_ids = candidate
            .effective_customer_page_order_v1(&self.source_customer_page_ids)
            .map_err(|error| {
                format!(
                    "Page insert membership is unavailable before commit: {} ({})",
                    error,
                    error.code()
                )
            })?;
        let destination_index = anchor_index + 1;
        let mut expected_page_ids = before_page_ids;
        expected_page_ids.insert(destination_index, identity.page_id);
        if after_page_ids != expected_page_ids {
            return Err("Page insert produced unexpected canonical customer order.".to_owned());
        }

        let mut visual_candidate = self
            .visual
            .as_ref()
            .ok_or_else(|| "Document page projection is unavailable.".to_owned())?
            .clone();
        visual_candidate
            .refresh_page_membership_from_resolved(candidate.graph(), &after_page_ids)
            .map_err(|error| format!("Page insert projection rejected before commit: {error}"))?;
        if visual_candidate
            .document
            .pages
            .iter()
            .map(|page| page.id)
            .collect::<Vec<_>>()
            != after_page_ids
            || visual_candidate
                .document
                .pages
                .get(destination_index)
                .map(|page| page.id)
                != Some(identity.page_id)
            || !visual_candidate
                .scene
                .surfaces
                .iter()
                .any(|surface| surface.origin == identity.page_id)
        {
            return Err(
                "Page insert Viewer projection lacks exact destination membership or surface."
                    .to_owned(),
            );
        }

        self.editor = Some(candidate);
        self.visual = Some(visual_candidate);
        self.selected_page = destination_index;
        self.canvas_selection.clear();
        self.canvas_drag = None;
        self.canvas_resize = None;
        self.finish_authoring_change(
            "Inserted one independent blank customer page immediately after selected PageId. Source PUB bytes were not written.",
        );
        Ok(identity.page_id)
    }

    fn page_delete_blank_capability_v1(&self) -> bool {
        let Some(editor) = self.editor.as_ref() else {
            return false;
        };
        let Some(visual) = self.visual.as_ref() else {
            return false;
        };
        let Some(selected_page_id) = visual
            .document
            .pages
            .get(self.selected_page)
            .map(|page| page.id)
        else {
            return false;
        };
        editor.can_delete_blank_authored_page_v1(&self.source_customer_page_ids, selected_page_id)
    }

    fn delete_selected_blank_authored_page_v1(&mut self) -> Result<PageId, String> {
        let (operation_count_before, selected_page_id, before_page_ids, deleted_index) = {
            let editor = self
                .editor
                .as_ref()
                .ok_or_else(|| "Editor session is unavailable.".to_owned())?;
            let visual = self
                .visual
                .as_ref()
                .ok_or_else(|| "Document page projection is unavailable.".to_owned())?;
            let selected_page_id = visual
                .document
                .pages
                .get(self.selected_page)
                .map(|page| page.id)
                .ok_or_else(|| "Selected page is unavailable.".to_owned())?;
            let before_page_ids = editor
                .effective_customer_page_order_v1(&self.source_customer_page_ids)
                .map_err(|error| {
                    format!("Page delete is unavailable: {} ({})", error, error.code())
                })?;
            let deleted_index = before_page_ids
                .iter()
                .position(|page_id| *page_id == selected_page_id)
                .ok_or_else(|| {
                    "Selected PageId is absent from effective customer membership.".to_owned()
                })?;
            (
                editor.operations().len(),
                selected_page_id,
                before_page_ids,
                deleted_index,
            )
        };

        let mut candidate = self
            .editor
            .as_ref()
            .expect("Editor presence validated before page delete")
            .clone();
        candidate
            .delete_blank_authored_page_v1(self.source_customer_page_ids.clone(), selected_page_id)
            .map_err(|error| format!("Page delete rejected: {} ({})", error, error.code()))?;
        if candidate.operations().len() != operation_count_before + 1
            || !matches!(
                candidate.operations().last(),
                Some(pub_editor::EditOperation::DeleteBlankAuthoredPageV1 { transition })
                    if transition.identity.page_id == selected_page_id
            )
        {
            return Err(
                "Delete Empty Page must append exactly one canonical lifecycle operation."
                    .to_owned(),
            );
        }

        let effective_page_ids = candidate
            .effective_customer_page_order_v1(&self.source_customer_page_ids)
            .map_err(|error| {
                format!(
                    "Page delete projection is unavailable before commit: {} ({})",
                    error,
                    error.code()
                )
            })?;
        if effective_page_ids.len() + 1 != before_page_ids.len()
            || effective_page_ids.contains(&selected_page_id)
        {
            return Err(
                "Page delete did not produce the expected effective customer membership."
                    .to_owned(),
            );
        }

        let mut visual_candidate = self
            .visual
            .as_ref()
            .ok_or_else(|| "Document page projection is unavailable.".to_owned())?
            .clone();
        visual_candidate
            .refresh_page_membership_from_resolved(candidate.graph(), &effective_page_ids)
            .map_err(|error| format!("Page delete projection rejected before commit: {error}"))?;
        if visual_candidate
            .document
            .pages
            .iter()
            .any(|page| page.id == selected_page_id)
            || visual_candidate
                .scene
                .surfaces
                .iter()
                .any(|surface| surface.origin == selected_page_id)
        {
            return Err(
                "Page delete projection still contains the deleted canonical PageId.".to_owned(),
            );
        }

        let fallback_index = if deleted_index == 0 {
            0
        } else {
            deleted_index - 1
        };
        let fallback_page_id = effective_page_ids
            .get(fallback_index)
            .copied()
            .ok_or_else(|| "Page delete produced no valid surviving selection.".to_owned())?;
        if visual_candidate
            .document
            .pages
            .get(fallback_index)
            .map(|page| page.id)
            != Some(fallback_page_id)
        {
            return Err(
                "Page delete projection did not preserve the canonical fallback page order."
                    .to_owned(),
            );
        }

        self.editor = Some(candidate);
        self.visual = Some(visual_candidate);
        self.selected_page = fallback_index;
        self.canvas_selection.clear();
        self.canvas_drag = None;
        self.canvas_resize = None;
        self.finish_authoring_change(
            "Deleted one empty Chaptera-created customer page. Source PUB bytes were not written.",
        );
        Ok(selected_page_id)
    }

    /// Read-only UI admission. The resolved graph and authored overlays are
    /// checked by EditorSession, never inferred from Page.children alone.
    fn page_delete_rectangle_capability_v1(&self) -> bool {
        let (Some(editor), Some(visual)) = (self.editor.as_ref(), self.visual.as_ref()) else {
            return false;
        };
        let Some(selected_page_id) = visual
            .document
            .pages
            .get(self.selected_page)
            .map(|page| page.id)
        else {
            return false;
        };
        editor
            .can_delete_authored_rectangle_page_v1(&self.source_customer_page_ids, selected_page_id)
    }

    /// One user command is one canonical v0.29 operation. Build Editor and
    /// Viewer candidates before touching any live editor or visible state.
    fn delete_selected_authored_rectangle_page_v1(&mut self) -> Result<PageId, String> {
        let editor = self
            .editor
            .as_ref()
            .ok_or_else(|| "Editor session is unavailable.".to_owned())?;
        let visual = self
            .visual
            .as_ref()
            .ok_or_else(|| "Document page projection is unavailable.".to_owned())?;
        let selected_page_id = visual
            .document
            .pages
            .get(self.selected_page)
            .map(|page| page.id)
            .ok_or_else(|| "Selected PageId is unavailable.".to_owned())?;
        let before_page_ids = editor
            .effective_customer_page_order_v1(&self.source_customer_page_ids)
            .map_err(|error| {
                format!(
                    "Rectangle-page delete is unavailable: {} ({})",
                    error,
                    error.code()
                )
            })?;
        let deleted_index = before_page_ids
            .iter()
            .position(|id| *id == selected_page_id)
            .ok_or_else(|| "Selected PageId is not an admitted customer page.".to_owned())?;
        let operations_before = editor.operations().len();
        if !editor
            .can_delete_authored_rectangle_page_v1(&self.source_customer_page_ids, selected_page_id)
        {
            return Err(
                "Selected Page is not an admitted single authored Rectangle page.".to_owned(),
            );
        }

        let mut candidate = editor.clone();
        candidate
            .delete_authored_rectangle_page_v1(
                self.source_customer_page_ids.clone(),
                selected_page_id,
            )
            .map_err(|error| {
                format!(
                    "Rectangle-page delete rejected: {} ({})",
                    error,
                    error.code()
                )
            })?;
        let removed_rectangle_id = match candidate.operations().last() {
            Some(pub_editor::EditOperation::DeleteAuthoredRectanglePageV1 { transition })
                if candidate.operations().len() == operations_before + 1
                    && transition.page.identity.page_id == selected_page_id
                    && transition.page.before_customer_page_ids == before_page_ids
                    && transition.shape_before.page_id == selected_page_id
                    && transition.shape_before.parent_id == selected_page_id =>
            {
                transition.shape_before.node_id
            }
            _ => {
                return Err(
                    "Delete Rectangle Page must append exactly one canonical v0.29 operation."
                        .to_owned(),
                );
            }
        };
        if candidate.graph().pages.contains_key(&selected_page_id)
            || candidate.authored_shape(removed_rectangle_id).is_some()
            || candidate.authored_stack(selected_page_id).is_some()
        {
            return Err("Rectangle-page delete left stale Editor membership.".to_owned());
        }
        let after_page_ids = candidate
            .effective_customer_page_order_v1(&self.source_customer_page_ids)
            .map_err(|error| {
                format!(
                    "Rectangle-page membership rejected: {} ({})",
                    error,
                    error.code()
                )
            })?;
        let mut expected_page_ids = before_page_ids;
        expected_page_ids.remove(deleted_index);
        if after_page_ids != expected_page_ids {
            return Err(
                "Rectangle-page delete changed unexpected customer membership or order.".to_owned(),
            );
        }

        let mut visual_candidate = visual.clone();
        visual_candidate
            .refresh_page_membership_from_resolved(candidate.graph(), &after_page_ids)
            .map_err(|error| {
                format!("Rectangle-page Viewer projection rejected before commit: {error}")
            })?;
        if visual_candidate
            .document
            .pages
            .iter()
            .map(|page| page.id)
            .collect::<Vec<_>>()
            != after_page_ids
            || visual_candidate
                .scene
                .surfaces
                .iter()
                .any(|surface| surface.origin == selected_page_id)
        {
            return Err(
                "Rectangle-page Viewer projection retained removed membership or page surface."
                    .to_owned(),
            );
        }
        let fallback_index = deleted_index.saturating_sub(1);
        let fallback_page_id = after_page_ids
            .get(fallback_index)
            .copied()
            .ok_or_else(|| "Rectangle-page delete has no surviving selection.".to_owned())?;
        if visual_candidate
            .document
            .pages
            .get(fallback_index)
            .map(|page| page.id)
            != Some(fallback_page_id)
        {
            return Err(
                "Rectangle-page Viewer projection has inconsistent fallback selection.".to_owned(),
            );
        }

        self.editor = Some(candidate);
        self.visual = Some(visual_candidate);
        self.selected_page = fallback_index;
        self.canvas_selection.clear();
        self.canvas_drag = None;
        self.canvas_resize = None;
        self.finish_authoring_change(
            "Deleted one Chaptera-created page with one authored Rectangle as a single Undo unit. Source PUB bytes were not written.",
        );
        Ok(selected_page_id)
    }

    /// Read-only UI admission for the bounded v0.32 Page + 2-8 Rectangle delete.
    fn page_delete_rectangles_capability_v1(&self) -> bool {
        let (Some(editor), Some(visual)) = (self.editor.as_ref(), self.visual.as_ref()) else {
            return false;
        };
        let Some(selected_page_id) = visual
            .document
            .pages
            .get(self.selected_page)
            .map(|page| page.id)
        else {
            return false;
        };
        editor.can_delete_authored_rectangles_page_v1(
            &self.source_customer_page_ids,
            selected_page_id,
        )
    }

    /// One real GUI command -> one canonical v0.32 Page + N-Rectangle delete.
    /// Editor and Viewer are both preflighted on clones before live commit.
    fn delete_selected_authored_rectangles_page_v1(&mut self) -> Result<PageId, String> {
        let editor = self
            .editor
            .as_ref()
            .ok_or_else(|| "Editor session is unavailable.".to_owned())?;
        let visual = self
            .visual
            .as_ref()
            .ok_or_else(|| "Document page projection is unavailable.".to_owned())?;
        let selected_page_id = visual
            .document
            .pages
            .get(self.selected_page)
            .map(|page| page.id)
            .ok_or_else(|| "Selected PageId is unavailable.".to_owned())?;
        let before_page_ids = editor
            .effective_customer_page_order_v1(&self.source_customer_page_ids)
            .map_err(|error| {
                format!(
                    "Multi-Rectangle Page delete is unavailable: {} ({})",
                    error,
                    error.code()
                )
            })?;
        let deleted_index = before_page_ids
            .iter()
            .position(|id| *id == selected_page_id)
            .ok_or_else(|| "Selected PageId is not an admitted customer page.".to_owned())?;
        if !editor.can_delete_authored_rectangles_page_v1(
            &self.source_customer_page_ids,
            selected_page_id,
        ) {
            return Err("Selected Page is not an admitted 2-8 authored Rectangle page.".to_owned());
        }
        let source_node_ids = editor
            .authored_stack(selected_page_id)
            .map(|stack| stack.members.clone())
            .ok_or_else(|| "Selected Page authored Rectangle stack is unavailable.".to_owned())?;
        if !(2..=pub_editor::MAX_DELETED_AUTHORED_RECTANGLES_PAGE_V1)
            .contains(&source_node_ids.len())
        {
            return Err("Selected Page Rectangle count is outside the v0.32 bound.".to_owned());
        }
        let operations_before = editor.operations().len();

        let mut candidate = editor.clone();
        candidate
            .delete_authored_rectangles_page_v1(
                self.source_customer_page_ids.clone(),
                selected_page_id,
            )
            .map_err(|error| {
                format!(
                    "Multi-Rectangle Page delete rejected: {} ({})",
                    error,
                    error.code()
                )
            })?;
        let removed_node_ids = match candidate.operations().last() {
            Some(pub_editor::EditOperation::DeleteAuthoredRectanglesPageV1 { transition })
                if candidate.operations().len() == operations_before + 1
                    && transition.page.identity.page_id == selected_page_id
                    && transition.page.before_customer_page_ids == before_page_ids
                    && transition.shapes_before.iter().all(|shape| {
                        shape.page_id == selected_page_id && shape.parent_id == selected_page_id
                    }) =>
            {
                transition
                    .shapes_before
                    .iter()
                    .map(|shape| shape.node_id)
                    .collect::<Vec<_>>()
            }
            _ => {
                return Err(
                    "Delete Multi-Rectangle Page must append exactly one canonical v0.32 operation."
                        .to_owned(),
                );
            }
        };
        if removed_node_ids != source_node_ids {
            return Err(
                "Deleted multi-Rectangle Page did not preserve exact source paint-stack order."
                    .to_owned(),
            );
        }
        if candidate.graph().pages.contains_key(&selected_page_id)
            || removed_node_ids
                .iter()
                .any(|node_id| candidate.authored_shape(*node_id).is_some())
            || candidate.authored_stack(selected_page_id).is_some()
        {
            return Err(
                "Multi-Rectangle Page delete left stale Editor Page, shape or stack membership."
                    .to_owned(),
            );
        }

        let after_page_ids = candidate
            .effective_customer_page_order_v1(&self.source_customer_page_ids)
            .map_err(|error| {
                format!(
                    "Multi-Rectangle Page membership rejected: {} ({})",
                    error,
                    error.code()
                )
            })?;
        let mut expected_page_ids = before_page_ids;
        expected_page_ids.remove(deleted_index);
        if after_page_ids != expected_page_ids {
            return Err(
                "Multi-Rectangle Page delete changed unexpected customer membership or order."
                    .to_owned(),
            );
        }

        let mut visual_candidate = visual.clone();
        visual_candidate
            .refresh_page_membership_from_resolved(candidate.graph(), &after_page_ids)
            .map_err(|error| {
                format!("Multi-Rectangle Page Viewer projection rejected before commit: {error}")
            })?;
        if visual_candidate
            .document
            .pages
            .iter()
            .map(|page| page.id)
            .collect::<Vec<_>>()
            != after_page_ids
            || visual_candidate
                .scene
                .surfaces
                .iter()
                .any(|surface| surface.origin == selected_page_id)
        {
            return Err(
                "Multi-Rectangle Page Viewer projection retained removed membership or surface."
                    .to_owned(),
            );
        }

        let fallback_index = deleted_index.saturating_sub(1);
        let fallback_page_id = after_page_ids
            .get(fallback_index)
            .copied()
            .ok_or_else(|| "Multi-Rectangle Page delete has no surviving selection.".to_owned())?;
        if visual_candidate
            .document
            .pages
            .get(fallback_index)
            .map(|page| page.id)
            != Some(fallback_page_id)
        {
            return Err(
                "Multi-Rectangle Page Viewer projection has inconsistent fallback selection."
                    .to_owned(),
            );
        }

        self.editor = Some(candidate);
        self.visual = Some(visual_candidate);
        self.selected_page = fallback_index;
        self.canvas_selection.clear();
        self.canvas_drag = None;
        self.canvas_resize = None;
        self.finish_authoring_change(
            "Deleted one Chaptera-created Page with 2-8 authored Rectangles as a single Undo unit. Source PUB bytes were not written.",
        );
        Ok(selected_page_id)
    }

    fn page_reorder_capabilities_v1(&self) -> (bool, bool) {
        let Some(editor) = self.editor.as_ref() else {
            return (false, false);
        };
        let Some(visual) = self.visual.as_ref() else {
            return (false, false);
        };
        let Some(selected_page_id) = visual
            .document
            .pages
            .get(self.selected_page)
            .map(|page| page.id)
        else {
            return (false, false);
        };
        let qualified_page_ids = visual
            .document
            .pages
            .iter()
            .map(|page| page.id)
            .collect::<Vec<_>>();
        let Ok(page_order) = editor.current_qualified_page_order_v1(&qualified_page_ids) else {
            return (false, false);
        };
        let Some(index) = page_order
            .iter()
            .position(|page_id| *page_id == selected_page_id)
        else {
            return (false, false);
        };
        (index > 0, index + 1 < page_order.len())
    }

    fn move_selected_page_v1(&mut self, delta: isize) -> Result<bool, String> {
        let (qualified_page_ids, selected_page_id) = {
            let visual = self
                .visual
                .as_ref()
                .ok_or_else(|| "Document page projection is unavailable.".to_owned())?;
            let selected_page_id = visual
                .document
                .pages
                .get(self.selected_page)
                .map(|page| page.id)
                .ok_or_else(|| "Selected page is unavailable.".to_owned())?;
            (
                visual
                    .document
                    .pages
                    .iter()
                    .map(|page| page.id)
                    .collect::<Vec<_>>(),
                selected_page_id,
            )
        };
        let editor = self
            .editor
            .as_ref()
            .ok_or_else(|| "Editor session is unavailable.".to_owned())?;
        let before = editor
            .current_qualified_page_order_v1(&qualified_page_ids)
            .map_err(|error| {
                format!("Page reorder is unavailable: {} ({})", error, error.code())
            })?;
        let current_index = before
            .iter()
            .position(|page_id| *page_id == selected_page_id)
            .ok_or_else(|| "Selected PageId is absent from canonical page order.".to_owned())?;
        let target_index = match delta {
            -1 => current_index.checked_sub(1),
            1 => current_index
                .checked_add(1)
                .filter(|target| *target < before.len()),
            _ => None,
        };
        let Some(target_index) = target_index else {
            return Ok(false);
        };

        let operation_count_before = editor.operations().len();
        let mut after = before.clone();
        after.swap(current_index, target_index);
        let mut candidate = editor.clone();
        candidate
            .reorder_pages_v1(before, after)
            .map_err(|error| format!("Page reorder rejected: {} ({})", error, error.code()))?;
        if candidate.operations().len() != operation_count_before + 1 {
            return Err("Page reorder must append exactly one authoring operation.".to_owned());
        }

        self.editor = Some(candidate);
        self.finish_authoring_change(
            "Moved the selected customer page in canonical publication order.",
        );
        Ok(true)
    }

    pub(super) fn show_page_reorder_controls(&mut self, ui: &mut egui::Ui) {
        let can_append = self.page_append_capability_v1();
        if ui
            .add_enabled(can_append, egui::Button::new("Add Page at End"))
            .clicked()
            && let Err(error) = self.append_blank_page_at_end_v1()
        {
            self.edit_status = Some(error);
        }

        let can_insert = self.page_insert_after_capability_v1();
        let insert_response =
            ui.add_enabled(can_insert, egui::Button::new("Insert Blank After Selected"));
        if insert_response.clicked()
            && let Err(error) = self.insert_selected_blank_page_after_v1()
        {
            self.edit_status = Some(error);
        }
        if !can_insert {
            insert_response.on_disabled_hover_text(
                "Select an admitted customer Page with valid dimensions. Existing content is preserved.",
            );
        }

        let can_delete = self.page_delete_blank_capability_v1();
        let delete_response = ui.add_enabled(can_delete, egui::Button::new("Delete Empty Page"));
        if delete_response.clicked()
            && let Err(error) = self.delete_selected_blank_authored_page_v1()
        {
            self.edit_status = Some(error);
        }
        if !can_delete {
            delete_response.on_disabled_hover_text(
                "Select an active Chaptera-created customer page. Content-bearing and source-backed pages fail closed.",
            );
        }

        let can_delete_rectangle = self.page_delete_rectangle_capability_v1();
        let rectangle_response = ui.add_enabled(
            can_delete_rectangle,
            egui::Button::new("Delete Rectangle Page"),
        );
        if rectangle_response.clicked()
            && let Err(error) = self.delete_selected_authored_rectangle_page_v1()
        {
            self.edit_status = Some(error);
        }
        if !can_delete_rectangle {
            rectangle_response.on_disabled_hover_text(
                "Select one active Chaptera-created page with exactly one direct authored Rectangle. Source-backed, linked and multi-object pages are unsupported.",
            );
        }

        let can_delete_rectangles = self.page_delete_rectangles_capability_v1();
        let rectangles_delete_response = ui.add_enabled(
            can_delete_rectangles,
            egui::Button::new("Delete Multi-Rectangle Page"),
        );
        if rectangles_delete_response.clicked()
            && let Err(error) = self.delete_selected_authored_rectangles_page_v1()
        {
            self.edit_status = Some(error);
        }
        if !can_delete_rectangles {
            rectangles_delete_response.on_disabled_hover_text(
                "Select one active Chaptera-created Page with 2-8 direct independent authored Rectangles. Imported, linked, mixed-object and larger Pages are unsupported.",
            );
        }

        let can_duplicate_rectangle = self.page_duplicate_rectangle_capability_v1();
        let rectangle_duplicate_response = ui.add_enabled(
            can_duplicate_rectangle,
            egui::Button::new("Duplicate Rectangle Page"),
        );
        if rectangle_duplicate_response.clicked()
            && let Err(error) = self.duplicate_selected_authored_rectangle_page_v1()
        {
            self.edit_status = Some(error);
        }
        if !can_duplicate_rectangle {
            rectangle_duplicate_response.on_disabled_hover_text(
                "Select an active Chaptera-created Page with exactly one direct independent authored Rectangle. Imported, linked and multi-object Pages are unsupported.",
            );
        }

        let can_duplicate = self.page_duplicate_blank_capability_v1();
        let duplicate_response =
            ui.add_enabled(can_duplicate, egui::Button::new("Duplicate Blank Page"));
        if duplicate_response.clicked()
            && let Err(error) = self.duplicate_selected_blank_page_v1()
        {
            self.edit_status = Some(error);
        }
        if !can_duplicate {
            duplicate_response.on_disabled_hover_text(
                "Select an admitted, truly empty customer page. Content-bearing pages fail closed.",
            );
        }

        let (can_move_up, can_move_down) = self.page_reorder_capabilities_v1();
        let mut command = None;
        ui.horizontal(|ui| {
            if ui
                .add_enabled(can_move_up, egui::Button::new("Move Page Up"))
                .clicked()
            {
                command = Some(-1);
            }
            if ui
                .add_enabled(can_move_down, egui::Button::new("Move Page Down"))
                .clicked()
            {
                command = Some(1);
            }
        });
        if let Some(delta) = command
            && let Err(error) = self.move_selected_page_v1(delta)
        {
            self.edit_status = Some(error);
        }
        ui.separator();
    }

    pub(super) fn capture_source_customer_page_ids_from_visual(&mut self) {
        self.source_customer_page_ids = self
            .visual
            .as_ref()
            .map(|visual| {
                visual
                    .document
                    .pages
                    .iter()
                    .map(|page| page.id)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
    }

    pub(super) fn finish_open_authoring_projection(&mut self) {
        let mut refresh_errors = Vec::new();
        if let Err(error) = self.sync_visual_page_membership_from_editor() {
            refresh_errors.push(format!("page membership: {error}"));
        }
        if let Err(error) = self.sync_visual_stories_from_editor() {
            refresh_errors.push(format!("text projection: {error}"));
        }
        if let Err(error) = self.sync_visual_created_text_boxes_from_editor() {
            refresh_errors.push(format!("created TextBox scene: {error}"));
        }
        self.sync_visual_geometry_from_editor();

        if !refresh_errors.is_empty() {
            self.edit_status = Some(format!(
                "Viewer authoring projection refresh failed closed: {}",
                refresh_errors.join("; ")
            ));
        }
    }

    pub(super) fn finish_authoring_change(&mut self, status: &str) {
        self.canvas_drag = None;
        self.canvas_resize = None;
        self.page_frame_cache.clear();

        let mut refresh_errors = Vec::new();
        if let Err(error) = self.sync_visual_page_membership_from_editor() {
            refresh_errors.push(format!("page membership: {error}"));
        }
        if let Err(error) = self.sync_visual_stories_from_editor() {
            refresh_errors.push(format!("text projection: {error}"));
        }
        if let Err(error) = self.sync_visual_created_text_boxes_from_editor() {
            refresh_errors.push(format!("created TextBox scene: {error}"));
        }
        self.sync_visual_geometry_from_editor();
        self.refresh_search();
        self.export_preview = None;
        self.project_status = Some("Editor project has unsaved changes.".to_owned());
        self.edit_status = Some(if refresh_errors.is_empty() {
            status.to_owned()
        } else {
            format!(
                "{status} Viewer refresh failed closed: {}",
                refresh_errors.join("; ")
            )
        });
    }

    pub(super) fn process_global_page_navigation_shortcuts(&mut self, ctx: &egui::Context) {
        if reader_only_mode() || self.text_mode.is_some() || ctx.wants_keyboard_input() {
            return;
        }

        let (previous_pressed, next_pressed) = ctx.input(|input| {
            let command = input.modifiers.ctrl || input.modifiers.command;
            let unmodified_command = command && !input.modifiers.alt && !input.modifiers.shift;
            (
                unmodified_command && input.key_pressed(egui::Key::PageUp),
                unmodified_command && input.key_pressed(egui::Key::PageDown),
            )
        });

        if previous_pressed {
            if let Some(index) = self.selected_page.checked_sub(1) {
                self.navigate_to_page_index(index);
            }
        } else if next_pressed && let Some(index) = self.selected_page.checked_add(1) {
            self.navigate_to_page_index(index);
        }
    }
}
