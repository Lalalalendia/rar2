//! Desktop direct-text session event integration.
//!
//! Story/session law remains in text_session.rs. This module owns the ViewerApp
//! bridge for entry, pointer activation, exit, caret routing, text events and
//! keyboard events.

use super::{ViewerApp, text_session};
use eframe::egui;

impl ViewerApp {
    pub(super) fn enter_canvas_text_mode(
        &mut self,
        story_id: pub_editor::StoryId,
        frame_id: pub_editor::NodeId,
    ) {
        let outcome = self
            .editor
            .as_ref()
            .ok_or_else(|| "Editor session is unavailable.".to_owned())
            .and_then(|editor| text_session::enter_explicit_text_mode(editor, story_id, frame_id));
        match outcome {
            Ok(mode) => {
                self.text_mode = Some(mode);
                self.canvas_drag = None;
                self.canvas_resize = None;
                self.edit_status = Some(
                    "Text editing active on the canvas. Typing is committed through canonical Story range operations."
                        .to_owned(),
                );
            }
            Err(error) => {
                self.edit_status = Some(format!("Edit Text unavailable: {error}"));
            }
        }
    }

    pub(super) fn enter_canvas_text_mode_at_pointer(
        &mut self,
        story_id: pub_editor::StoryId,
        frame_id: pub_editor::NodeId,
        page_id: &str,
        point: pub_interaction::DocumentPoint,
    ) {
        let outcome = self
            .editor
            .as_ref()
            .ok_or_else(|| "Editor session is unavailable.".to_owned())
            .and_then(|editor| {
                text_session::enter_pointer_text_mode(
                    editor,
                    story_id,
                    frame_id,
                    page_id,
                    point.x.get(),
                    point.y.get(),
                )
            });
        match outcome {
            Ok(mode) => {
                self.text_mode = Some(mode);
                self.canvas_drag = None;
                self.canvas_resize = None;
                self.edit_status = Some(
                    "Text editing activated from the TextFrame interior. Typing is committed through canonical Story range operations."
                        .to_owned(),
                );
            }
            Err(error) => {
                self.edit_status = Some(format!("Text activation unavailable: {error}"));
            }
        }
    }

    pub(super) fn exit_canvas_text_mode(&mut self, trigger: &str) {
        let Some(mode) = self.text_mode.as_ref() else {
            return;
        };
        match text_session::exit_text_mode(mode, trigger) {
            Ok(()) => {
                self.text_mode = None;
                self.edit_status =
                    Some("Exited canvas text editing without creating an edit.".to_owned());
            }
            Err(error) => {
                self.edit_status = Some(format!("Could not exit canvas text editing: {error}"));
            }
        }
    }

    fn refresh_visual_text_projection_from_editor(&mut self) -> Result<(), String> {
        let editor = self
            .editor
            .as_ref()
            .ok_or_else(|| "Editor session is unavailable.".to_owned())?;
        let visual = self
            .visual
            .as_mut()
            .ok_or_else(|| "Viewer projection is unavailable.".to_owned())?;
        visual
            .refresh_text_projection_from_resolved(editor.graph())
            .map_err(|error| format!("refresh current Story projection: {error:#}"))
    }

    fn apply_canvas_text_input(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        let outcome = match (&mut self.editor, &mut self.text_mode) {
            (Some(editor), Some(mode)) => text_session::replace_external_text(editor, mode, text),
            _ => return,
        };
        match outcome {
            Ok(()) => match self.refresh_visual_text_projection_from_editor() {
                Ok(()) => self.finish_authoring_change(
                    "Typed on the canvas through one canonical ReplaceStoryRange operation.",
                ),
                Err(error) => {
                    self.edit_status = Some(format!(
                        "Text was committed, but the canvas projection could not be refreshed: {error}"
                    ));
                }
            },
            Err(error) => {
                self.edit_status = Some(format!("Canvas text input rejected: {error}"));
            }
        }
    }

    fn apply_canvas_text_keyboard(
        &mut self,
        command: chaptera_text_input_adapter::keyboard::KeyboardCommandV1,
    ) {
        let before_operations = self
            .editor
            .as_ref()
            .map(|editor| editor.operations().len())
            .unwrap_or(0);
        let outcome = match (&mut self.editor, &mut self.text_mode) {
            (Some(editor), Some(mode)) => {
                text_session::apply_keyboard_command(editor, mode, command)
            }
            _ => return,
        };
        match outcome {
            Ok(()) => {
                let after_operations = self
                    .editor
                    .as_ref()
                    .map(|editor| editor.operations().len())
                    .unwrap_or(before_operations);
                if after_operations > before_operations {
                    match self.refresh_visual_text_projection_from_editor() {
                        Ok(()) => self.finish_authoring_change(
                            "Canvas text keyboard edit committed through canonical Story range authority.",
                        ),
                        Err(error) => {
                            self.edit_status = Some(format!(
                                "Text keyboard edit was committed, but the canvas projection could not be refreshed: {error}"
                            ));
                        }
                    }
                }
            }
            Err(error) => {
                self.edit_status = Some(format!("Canvas text keyboard input rejected: {error}"));
            }
        }
    }

    pub(super) fn reposition_canvas_text_caret(
        &mut self,
        page_id: &str,
        point: pub_interaction::DocumentPoint,
    ) {
        let Some(mode) = self.text_mode.as_mut() else {
            return;
        };
        if let Err(error) =
            text_session::reposition_pointer(mode, page_id, point.x.get(), point.y.get())
        {
            self.edit_status = Some(format!("Canvas caret move rejected: {error}"));
        }
    }
    pub(super) fn process_canvas_text_input(&mut self, ctx: &egui::Context) -> bool {
        if self.text_mode.is_none() {
            return false;
        }
        if ctx.wants_keyboard_input() {
            self.exit_canvas_text_mode("focus_transfer");
            return true;
        }

        let events = ctx.input(|input| input.events.clone());
        for event in events {
            if self.text_mode.is_none() {
                break;
            }
            match event {
                egui::Event::Text(text) => self.apply_canvas_text_input(&text),
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => {
                    if key == egui::Key::Escape {
                        self.exit_canvas_text_mode("escape");
                        continue;
                    }

                    if (modifiers.ctrl || modifiers.command)
                        && !modifiers.alt
                        && !modifiers.shift
                        && key == egui::Key::A
                    {
                        if let Some(mode) = self.text_mode.as_mut() {
                            text_session::select_all(mode);
                        }
                        continue;
                    }

                    if self.process_story_object_keyboard(key, modifiers) {
                        continue;
                    }

                    if modifiers.ctrl || modifiers.command || modifiers.alt {
                        continue;
                    }
                    use chaptera_text_input_adapter::keyboard::KeyboardCommandV1;
                    let command = match (key, modifiers.shift) {
                        (egui::Key::ArrowLeft, false) => Some(KeyboardCommandV1::MovePrevious),
                        (egui::Key::ArrowRight, false) => Some(KeyboardCommandV1::MoveNext),
                        (egui::Key::ArrowLeft, true) => Some(KeyboardCommandV1::ExtendPrevious),
                        (egui::Key::ArrowRight, true) => Some(KeyboardCommandV1::ExtendNext),
                        (egui::Key::Backspace, _) => Some(KeyboardCommandV1::DeleteBackward),
                        (egui::Key::Delete, _) => Some(KeyboardCommandV1::DeleteForward),
                        _ => None,
                    };
                    if let Some(command) = command {
                        self.apply_canvas_text_keyboard(command);
                    }
                }
                _ => {}
            }
        }
        true
    }
}
