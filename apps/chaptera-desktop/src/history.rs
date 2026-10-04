//! Canonical Desktop history commands and Ctrl/Cmd+Z/Y routing.
//!
//! Undo/Redo reuse the existing Editor authority and ViewerApp refresh path.
//! This module owns only the Desktop shell integration.

use super::{ViewerApp, reader_only_mode};
use eframe::egui;

impl ViewerApp {
    pub(super) fn apply_undo(&mut self) {
        let outcome = self
            .editor
            .as_mut()
            .expect("editor presence checked above")
            .undo();
        match outcome {
            Ok(_) => self.finish_authoring_change("Undo restored the previous authoring state."),
            Err(error) => {
                self.edit_status = Some(format!("Undo unavailable: {} ({})", error, error.code()));
            }
        }
    }

    pub(super) fn apply_redo(&mut self) {
        let outcome = self
            .editor
            .as_mut()
            .expect("editor presence checked above")
            .redo();
        match outcome {
            Ok(_) => self.finish_authoring_change("Redo restored the edited authoring state."),
            Err(error) => {
                self.edit_status = Some(format!("Redo unavailable: {} ({})", error, error.code()));
            }
        }
    }

    pub(super) fn process_global_history_shortcuts(&mut self, ctx: &egui::Context) {
        if reader_only_mode() || self.text_mode.is_some() || ctx.wants_keyboard_input() {
            return;
        }

        let (undo_pressed, redo_pressed) = ctx.input(|input| {
            let command = input.modifiers.ctrl || input.modifiers.command;
            let unmodified_command = command && !input.modifiers.alt && !input.modifiers.shift;
            (
                unmodified_command && input.key_pressed(egui::Key::Z),
                unmodified_command && input.key_pressed(egui::Key::Y),
            )
        });

        if undo_pressed {
            self.apply_undo();
        } else if redo_pressed {
            self.apply_redo();
        }
    }

}
