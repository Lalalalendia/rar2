//! Desktop Text Box creation shell integration.
//!
//! TextBoxCreateSessionV1 remains the semantic interaction owner. This module
//! owns only Desktop toolbar, pointer, preview, Escape and commit wiring.

use super::{ViewerApp, direct_page_local_instance_v1, render_backend, text_box_creation};
use eframe::egui;

#[derive(Default)]
pub(super) struct TextBoxFrameOutcome {
    release: Option<text_box_creation::TextBoxCreateReleaseV1>,
    error: Option<String>,
}

impl ViewerApp {
    pub(super) fn show_text_box_tool_control(
        &mut self,
        ui: &mut egui::Ui,
        editor_available: bool,
    ) {
        let text_box_active = self.text_box_creation.active();
        let text_box_response = ui.add_enabled(
            editor_available && self.visual.is_some(),
            egui::SelectableLabel::new(text_box_active, "Text Box"),
        );
        if !text_box_response.clicked() {
            return;
        }

        self.canvas_drag = None;
        self.canvas_resize = None;
        if text_box_active {
            match self.text_box_creation.deactivate_to_select() {
                Ok(()) => {
                    self.edit_status = Some("Text Box tool deactivated.".to_owned());
                }
                Err(error) => {
                    self.edit_status =
                        Some(format!("Text Box tool could not deactivate: {error}"));
                }
            }
            return;
        }

        if self.rectangle_creation.active() {
            let _ = self.rectangle_creation.deactivate_to_select();
        }
        if self.text_mode.is_some() {
            self.exit_canvas_text_mode("textbox_tool_activation");
        }
        match self.text_box_creation.activate() {
            Ok(()) => {
                self.edit_status = Some(
                    "Text Box tool active. Drag on the page to create one empty Story.".to_owned(),
                );
            }
            Err(error) => {
                self.edit_status = Some(format!("Text Box tool could not activate: {error}"));
            }
        }
    }

    pub(super) fn process_text_box_escape(&mut self) -> bool {
        if !self.text_box_creation.active() {
            return false;
        }

        if self.text_box_creation.gesture_token.is_some() {
            match self.text_box_creation.cancel() {
                Ok(()) => {
                    self.edit_status = Some("Cancelled the Text Box draw gesture.".to_owned());
                }
                Err(error) => {
                    self.edit_status = Some(format!("Text Box gesture cancel failed: {error}"));
                }
            }
        } else {
            match self.text_box_creation.deactivate_to_select() {
                Ok(()) => {
                    self.edit_status = Some("Text Box tool deactivated.".to_owned());
                }
                Err(error) => {
                    self.edit_status =
                        Some(format!("Text Box tool could not deactivate: {error}"));
                }
            }
        }
        true
    }

    pub(super) fn finish_text_box_frame(&mut self, outcome: TextBoxFrameOutcome) {
        if let Some(error) = outcome.error {
            self.edit_status = Some(error);
        }
        let Some(release) = outcome.release else {
            return;
        };

        let result = match self.editor.as_mut() {
            Some(editor) => self.text_box_creation.commit_release(editor, release),
            None => Err("Editor session is unavailable.".to_owned()),
        };
        match result {
            Ok(Some(created)) => {
                self.finish_authoring_change(
                    "Created Text Box in the authoring session. One CreateTextBox operation was committed.",
                );
                match direct_page_local_instance_v1(
                    &created.node_id.as_canonical().to_string(),
                    &created.page_id.as_canonical().to_string(),
                ) {
                    Ok(instance) => self.canvas_selection.select_only(instance.instance_id),
                    Err(error) => {
                        self.edit_status = Some(format!(
                            "Text Box was created, but durable selection could not bind: {error}"
                        ));
                    }
                }
                self.enter_canvas_text_mode(created.story_id, created.node_id);
            }
            Ok(None) => {}
            Err(error) => {
                self.edit_status = Some(error);
            }
        }
    }
}

pub(super) fn process_text_box_primary_pointer(
    session: &mut text_box_creation::TextBoxCreateSessionV1,
    reader_only: bool,
    text_mode_active: bool,
    page_id: pub_editor::PageId,
    page_rect: egui::Rect,
    press_screen: Option<egui::Pos2>,
    press_document: Option<pub_interaction::DocumentPoint>,
    pointer_document: Option<pub_interaction::DocumentPoint>,
    primary_pressed: bool,
    primary_down: bool,
    primary_released: bool,
    outcome: &mut TextBoxFrameOutcome,
) -> bool {
    if reader_only || text_mode_active || !session.active() {
        return false;
    }

    if session.gesture_token.is_none()
        && primary_pressed
        && let (Some(pointer_start_screen), Some(pointer_start)) = (press_screen, press_document)
        && page_rect.contains(pointer_start_screen)
        && let Err(error) =
            session.pointer_down(page_id, pointer_start, "textbox-draw-v1".to_owned())
    {
        outcome.error = Some(format!("Text Box draw could not start: {error}"));
    }

    if session.gesture_token.is_some()
        && primary_down
        && let Some(point) = pointer_document
        && let Err(error) = session.pointer_move(point)
    {
        let _ = session.cancel();
        outcome.error = Some(format!("Text Box preview cancelled: {error}"));
    }

    if session.gesture_token.is_some() && primary_released {
        finish_text_box_pointer(session, pointer_document, outcome);
    }
    true
}

pub(super) fn process_text_box_drag_started(
    session: &mut text_box_creation::TextBoxCreateSessionV1,
    page_id: pub_editor::PageId,
    pointer_start: pub_interaction::DocumentPoint,
    pointer_current: pub_interaction::DocumentPoint,
    outcome: &mut TextBoxFrameOutcome,
) -> bool {
    if !session.active() {
        return false;
    }

    let result = if session.gesture_token.is_none() {
        session
            .pointer_down(page_id, pointer_start, "textbox-draw-v1".to_owned())
            .and_then(|()| session.pointer_move(pointer_current).map(|_| ()))
    } else {
        session.pointer_move(pointer_current).map(|_| ())
    };
    if let Err(error) = result {
        let _ = session.cancel();
        outcome.error = Some(format!("Text Box draw could not start: {error}"));
    }
    true
}

pub(super) fn process_text_box_drag_stopped(
    session: &mut text_box_creation::TextBoxCreateSessionV1,
    pointer_document: Option<pub_interaction::DocumentPoint>,
    outcome: &mut TextBoxFrameOutcome,
) -> bool {
    if !session.active() || session.gesture_token.is_none() {
        return false;
    }
    finish_text_box_pointer(session, pointer_document, outcome);
    true
}

pub(super) fn process_text_box_dragged(
    session: &mut text_box_creation::TextBoxCreateSessionV1,
    point: pub_interaction::DocumentPoint,
    outcome: &mut TextBoxFrameOutcome,
) -> bool {
    if !session.active() || session.gesture_token.is_none() {
        return false;
    }

    if let Err(error) = session.pointer_move(point) {
        let _ = session.cancel();
        outcome.error = Some(format!("Text Box preview cancelled: {error}"));
    }
    true
}

pub(super) fn paint_text_box_preview(
    session: &text_box_creation::TextBoxCreateSessionV1,
    painter: &egui::Painter,
    page_id: pub_editor::PageId,
    page_rect: egui::Rect,
    scene_scale: f32,
) {
    if session.page_id != Some(page_id) {
        return;
    }
    let Ok(text_box_creation::TextBoxCreatePreviewV1::Bounds(bounds)) = session.preview() else {
        return;
    };
    let Some(preview_rect) = render_backend::physical_rect_to_egui(
        page_rect,
        scene_scale,
        bounds.x.get(),
        bounds.y.get(),
        bounds.width.get(),
        bounds.height.get(),
    ) else {
        return;
    };

    painter.rect_stroke(
        preview_rect,
        0,
        egui::Stroke::new(1.5_f32, egui::Color32::from_rgb(232, 126, 36)),
        egui::StrokeKind::Inside,
    );
}

fn finish_text_box_pointer(
    session: &mut text_box_creation::TextBoxCreateSessionV1,
    pointer_document: Option<pub_interaction::DocumentPoint>,
    outcome: &mut TextBoxFrameOutcome,
) {
    if let Some(point) = pointer_document {
        match session.pointer_up(point) {
            Ok(release) => outcome.release = Some(release),
            Err(error) => {
                let _ = session.cancel();
                outcome.error = Some(format!("Text Box draw could not finish: {error}"));
            }
        }
    } else {
        let _ = session.cancel();
        outcome.error =
            Some("Text Box draw ended outside the document coordinate boundary.".to_owned());
    }
}
