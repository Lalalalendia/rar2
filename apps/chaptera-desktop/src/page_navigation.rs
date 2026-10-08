//! Canonical Desktop page navigation and Publisher-compatible page shortcut routing.
//!
//! This module owns page-index transitions plus Ctrl/Cmd+PageUp/PageDown routing.
//! Page UI surfaces call the same canonical transition method.

use super::{ViewerApp, reader_only_mode, supporter};
use eframe::egui;

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
