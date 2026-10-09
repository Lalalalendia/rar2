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

        self.editor = Some(candidate);
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
