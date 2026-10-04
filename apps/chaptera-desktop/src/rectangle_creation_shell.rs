//! Desktop Rectangle creation shell integration.
//!
//! RectangleCreateSessionV1 remains the semantic interaction owner. This
//! module owns only Desktop toolbar, pointer, preview, Escape and commit wiring.

use super::{ViewerApp, direct_page_local_instance_v1, rectangle_creation, render_backend};
use eframe::egui;

#[derive(Default)]
pub(super) struct RectangleFrameOutcome {
    release: Option<rectangle_creation::RectangleCreateReleaseV1>,
    error: Option<String>,
}

pub(super) fn rectangle_tool_inactive(
    session: &rectangle_creation::RectangleCreateSessionV1,
) -> bool {
    !session.active()
}

impl ViewerApp {
    pub(super) fn show_rectangle_tool_control(
        &mut self,
        ui: &mut egui::Ui,
        editor_available: bool,
    ) {
        let rectangle_active = self.rectangle_creation.active();
        let rectangle_response = ui.add_enabled(
            editor_available && self.visual.is_some(),
            egui::SelectableLabel::new(rectangle_active, "Rectangle"),
        );
        if !rectangle_response.clicked() {
            return;
        }

        self.canvas_drag = None;
        self.canvas_resize = None;
        if rectangle_active {
            match self.rectangle_creation.deactivate_to_select() {
                Ok(()) => {
                    self.edit_status = Some("Rectangle tool deactivated.".to_owned());
                }
                Err(error) => {
                    self.edit_status =
                        Some(format!("Rectangle tool could not deactivate: {error}"));
                }
            }
            return;
        }

        if self.text_box_creation.active() {
            let _ = self.text_box_creation.deactivate_to_select();
        }
        if self.text_mode.is_some() {
            self.exit_canvas_text_mode("rectangle_tool_activation");
        }
        match self.rectangle_creation.activate() {
            Ok(()) => {
                self.edit_status = Some(
                    "Rectangle tool active. Drag on the page to create one rectangle.".to_owned(),
                );
            }
            Err(error) => {
                self.edit_status = Some(format!("Rectangle tool could not activate: {error}"));
            }
        }
    }

    pub(super) fn deactivate_rectangle_for_other_tool(&mut self) {
        if self.rectangle_creation.active() {
            let _ = self.rectangle_creation.deactivate_to_select();
        }
    }

    pub(super) fn process_rectangle_escape(&mut self) -> bool {
        if !self.rectangle_creation.active() {
            return false;
        }

        if self.rectangle_creation.gesture_token.is_some() {
            match self.rectangle_creation.cancel() {
                Ok(()) => {
                    self.edit_status = Some("Cancelled the Rectangle draw gesture.".to_owned());
                }
                Err(error) => {
                    self.edit_status = Some(format!("Rectangle gesture cancel failed: {error}"));
                }
            }
        } else {
            match self.rectangle_creation.deactivate_to_select() {
                Ok(()) => {
                    self.edit_status = Some("Rectangle tool deactivated.".to_owned());
                }
                Err(error) => {
                    self.edit_status =
                        Some(format!("Rectangle tool could not deactivate: {error}"));
                }
            }
        }
        true
    }

    pub(super) fn finish_rectangle_frame(&mut self, outcome: RectangleFrameOutcome) {
        if let Some(error) = outcome.error {
            self.edit_status = Some(error);
        }
        let Some(release) = outcome.release else {
            return;
        };

        let created_page_id = match release {
            rectangle_creation::RectangleCreateReleaseV1::Commit { page_id, .. } => Some(page_id),
            rectangle_creation::RectangleCreateReleaseV1::NoChange => None,
        };
        let result = match self.editor.as_mut() {
            Some(editor) => self.rectangle_creation.commit_release(editor, release),
            None => Err("Editor session is unavailable.".to_owned()),
        };
        match result {
            Ok(Some(node_id)) => {
                self.finish_authoring_change(
                    "Created Rectangle in the authoring session. One CreateShape operation was committed.",
                );
                if let Some(page_id) = created_page_id {
                    match direct_page_local_instance_v1(
                        &node_id.as_canonical().to_string(),
                        &page_id.as_canonical().to_string(),
                    ) {
                        Ok(instance) => self.canvas_selection.select_only(instance.instance_id),
                        Err(error) => {
                            self.edit_status = Some(format!(
                                "Rectangle was created, but durable selection could not bind: {error}"
                            ));
                        }
                    }
                }
            }
            Ok(None) => {}
            Err(error) => {
                self.edit_status = Some(error);
            }
        }
    }
}

pub(super) fn process_rectangle_primary_pointer(
    session: &mut rectangle_creation::RectangleCreateSessionV1,
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
    outcome: &mut RectangleFrameOutcome,
) {
    if reader_only || text_mode_active || !session.active() {
        return;
    }

    if session.gesture_token.is_none()
        && primary_pressed
        && let (Some(pointer_start_screen), Some(pointer_start)) = (press_screen, press_document)
        && page_rect.contains(pointer_start_screen)
        && let Err(error) =
            session.pointer_down(page_id, pointer_start, "rectangle-draw-v1".to_owned())
    {
        outcome.error = Some(format!("Rectangle draw could not start: {error}"));
    }

    if session.gesture_token.is_some()
        && primary_down
        && let Some(point) = pointer_document
        && let Err(error) = session.pointer_move(point)
    {
        let _ = session.cancel();
        outcome.error = Some(format!("Rectangle preview cancelled: {error}"));
    }

    if session.gesture_token.is_some() && primary_released {
        finish_rectangle_pointer(session, pointer_document, outcome);
    }
}

pub(super) fn process_rectangle_drag_started(
    session: &mut rectangle_creation::RectangleCreateSessionV1,
    page_id: pub_editor::PageId,
    pointer_start: pub_interaction::DocumentPoint,
    pointer_current: pub_interaction::DocumentPoint,
    outcome: &mut RectangleFrameOutcome,
) -> bool {
    if !session.active() {
        return false;
    }

    let result = if session.gesture_token.is_none() {
        session
            .pointer_down(page_id, pointer_start, "rectangle-draw-v1".to_owned())
            .and_then(|()| session.pointer_move(pointer_current).map(|_| ()))
    } else {
        session.pointer_move(pointer_current).map(|_| ())
    };
    if let Err(error) = result {
        let _ = session.cancel();
        outcome.error = Some(format!("Rectangle draw could not start: {error}"));
    }
    true
}

pub(super) fn process_rectangle_drag_stopped(
    session: &mut rectangle_creation::RectangleCreateSessionV1,
    pointer_document: Option<pub_interaction::DocumentPoint>,
    outcome: &mut RectangleFrameOutcome,
) -> bool {
    if !session.active() || session.gesture_token.is_none() {
        return false;
    }
    finish_rectangle_pointer(session, pointer_document, outcome);
    true
}

pub(super) fn process_rectangle_dragged(
    session: &mut rectangle_creation::RectangleCreateSessionV1,
    point: pub_interaction::DocumentPoint,
    outcome: &mut RectangleFrameOutcome,
) -> bool {
    if !session.active() || session.gesture_token.is_none() {
        return false;
    }

    if let Err(error) = session.pointer_move(point) {
        let _ = session.cancel();
        outcome.error = Some(format!("Rectangle preview cancelled: {error}"));
    }
    true
}

pub(super) fn paint_rectangle_preview(
    session: &rectangle_creation::RectangleCreateSessionV1,
    painter: &egui::Painter,
    page_id: pub_editor::PageId,
    page_rect: egui::Rect,
    scene_scale: f32,
) {
    if session.page_id != Some(page_id) {
        return;
    }
    let Ok(rectangle_creation::RectangleCreatePreviewV1::Bounds(bounds)) = session.preview() else {
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

    painter.rect_filled(
        preview_rect,
        0,
        egui::Color32::from_rgba_unmultiplied(255, 255, 255, 48),
    );
    painter.rect_stroke(
        preview_rect,
        0,
        egui::Stroke::new(1.5_f32, egui::Color32::BLACK),
        egui::StrokeKind::Inside,
    );
}

fn finish_rectangle_pointer(
    session: &mut rectangle_creation::RectangleCreateSessionV1,
    pointer_document: Option<pub_interaction::DocumentPoint>,
    outcome: &mut RectangleFrameOutcome,
) {
    if let Some(point) = pointer_document {
        match session.pointer_up(point) {
            Ok(release) => outcome.release = Some(release),
            Err(error) => {
                let _ = session.cancel();
                outcome.error = Some(format!("Rectangle draw could not finish: {error}"));
            }
        }
    } else {
        let _ = session.cancel();
        outcome.error =
            Some("Rectangle draw ended outside the document coordinate boundary.".to_owned());
    }
}
