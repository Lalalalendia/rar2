//! Canonical Desktop page navigation and customer-page reorder routing.
//!
//! Navigation changes only the current view. Page reorder delegates durable
//! order to EditorSession::reorder_pages_v1 and mirrors that canonical order
//! back into the Viewer by stable PageId.

use super::{ViewerApp, reader_only_mode, supporter};
use eframe::egui;
use pub_editor::PageId;
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
