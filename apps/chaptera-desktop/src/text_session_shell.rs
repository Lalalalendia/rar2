//! Desktop canvas text entry, input, focus, caret and widget integration.
//! The existing text_session module remains the Story/session semantic authority.

use super::{SceneHitEntry, ViewerApp, text_session};
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

    pub(super) fn finish_canvas_text_frame(&mut self, ctx: &egui::Context) {
        if self.text_mode.is_some() && ctx.wants_keyboard_input() {
            self.exit_canvas_text_mode("explicit_exit");
        }
    }
}

pub(super) struct CanvasTextPointerRequest {
    pub(super) canvas_hit: Option<String>,
    pub(super) text_pointer_request: Option<(String, pub_interaction::DocumentPoint)>,
    pub(super) text_activation_request: Option<(
        pub_editor::StoryId,
        pub_editor::NodeId,
        String,
        pub_interaction::DocumentPoint,
    )>,
    pub(super) text_exit_request: bool,
}

pub(super) fn canvas_text_pointer_request(
    mode: Option<&text_session::DesktopTextMode>,
    topmost: Option<&SceneHitEntry>,
    visual: &super::ViewerGeometryDocument,
    editor: Option<&pub_editor::EditorSession>,
    page_id_text: &str,
    point: pub_interaction::DocumentPoint,
) -> CanvasTextPointerRequest {
    let mut request = CanvasTextPointerRequest {
        canvas_hit: None,
        text_pointer_request: None,
        text_activation_request: None,
        text_exit_request: false,
    };
    if let Some(mode) = mode {
        match topmost {
            Some(hit) if hit.node_id == mode.frame_id => {
                request.canvas_hit = Some(hit.instance_id.clone());
                request.text_pointer_request = Some((page_id_text.to_owned(), point));
            }
            Some(hit) => {
                request.text_exit_request = true;
                request.canvas_hit = Some(hit.instance_id.clone());
            }
            None => {
                request.text_exit_request = true;
                request.canvas_hit = None;
            }
        }
    } else if let Some(hit) = topmost {
        request.canvas_hit = Some(hit.instance_id.clone());
        if strict_document_rect_interior(&hit.bounds, point)
            && let Some(fragment) = visual
                .text_fragments
                .iter()
                .find(|fragment| fragment.frame_id == hit.node_id)
            && editor.is_some_and(|editor| editor.can_replace_story_text(fragment.story_id).is_ok())
        {
            request.text_activation_request = Some((
                fragment.story_id,
                fragment.frame_id,
                page_id_text.to_owned(),
                point,
            ));
        }
    } else {
        request.canvas_hit = None;
    }
    request
}

pub(super) fn paint_canvas_text_caret(
    mode: Option<&text_session::DesktopTextMode>,
    painter: &egui::Painter,
    page_rect: egui::Rect,
    scene_scale: f32,
    page_id_text: &str,
) {
    if let Some(mode) = mode
        && let Some(stop) = text_session::focus_caret(mode)
        && stop.page_id == page_id_text
    {
        let x = page_rect.left() + stop.page_x_emu as f32 * scene_scale;
        let y_top = page_rect.top() + stop.page_y_top_emu as f32 * scene_scale;
        let y_bottom = page_rect.top() + stop.page_y_bottom_emu as f32 * scene_scale;
        painter.line_segment(
            [egui::pos2(x, y_top), egui::pos2(x, y_bottom)],
            egui::Stroke::new(2.0_f32, egui::Color32::from_rgb(232, 126, 36)),
        );
    }
}

pub(super) fn show_canvas_edit_text_button(
    ui: &mut egui::Ui,
    text_mode_active: bool,
    visual: &super::ViewerGeometryDocument,
    editor: Option<&pub_editor::EditorSession>,
    selected_node_id: pub_editor::NodeId,
    selected_rect: egui::Rect,
    page_rect: egui::Rect,
) -> Option<(pub_editor::StoryId, pub_editor::NodeId)> {
    if !text_mode_active
        && let Some(fragment) = visual
            .text_fragments
            .iter()
            .find(|fragment| fragment.frame_id == selected_node_id)
        && editor.is_some_and(|editor| editor.can_replace_story_text(fragment.story_id).is_ok())
    {
        let button_width = 76.0_f32;
        let button_height = 22.0_f32;
        let button_min = egui::pos2(
            selected_rect.left(),
            (selected_rect.top() - button_height - 4.0_f32).max(page_rect.top() + 2.0_f32),
        );
        let button_rect =
            egui::Rect::from_min_size(button_min, egui::vec2(button_width, button_height));
        if ui
            .put(button_rect, egui::Button::new("Edit Text"))
            .clicked()
        {
            return Some((fragment.story_id, selected_node_id));
        }
    }
    None
}

pub(super) fn strict_document_rect_interior(
    bounds: &pub_editor::RectEmu,
    point: pub_interaction::DocumentPoint,
) -> bool {
    let x = i128::from(point.x.get());
    let y = i128::from(point.y.get());
    let left = i128::from(bounds.x.get());
    let top = i128::from(bounds.y.get());
    let right = left + i128::from(bounds.width.get());
    let bottom = top + i128::from(bounds.height.get());
    x > left && x < right && y > top && y < bottom
}

#[cfg(test)]
mod tests {
    use super::strict_document_rect_interior;

    #[test]
    fn strict_text_activation_interior_excludes_exact_frame_boundary() {
        let bounds = pub_editor::RectEmu::new(
            pub_editor::LengthEmu::new(100),
            pub_editor::LengthEmu::new(200),
            pub_editor::LengthEmu::new(300),
            pub_editor::LengthEmu::new(400),
        );
        assert!(strict_document_rect_interior(
            &bounds,
            pub_interaction::DocumentPoint::new(
                pub_editor::LengthEmu::new(250),
                pub_editor::LengthEmu::new(400),
            ),
        ));
        assert!(!strict_document_rect_interior(
            &bounds,
            pub_interaction::DocumentPoint::new(
                pub_editor::LengthEmu::new(100),
                pub_editor::LengthEmu::new(400),
            ),
        ));
        assert!(!strict_document_rect_interior(
            &bounds,
            pub_interaction::DocumentPoint::new(
                pub_editor::LengthEmu::new(400),
                pub_editor::LengthEmu::new(400),
            ),
        ));
    }
}
