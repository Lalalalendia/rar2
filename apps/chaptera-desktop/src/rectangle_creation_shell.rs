//! Desktop Rectangle creation shell.
//!
//! Owns toolbar activation, pointer gesture routing, transient preview paint,
//! and durable CreateShape commit/selection binding. The gesture/state-machine
//! law remains in rectangle_creation.rs.

use super::{
    MoveTransaction, ResizeTransaction, ViewerApp, direct_page_local_instance_v1,
    rectangle_creation, render_backend,
};
use eframe::egui;

impl ViewerApp {
    pub(super) fn show_rectangle_tool_control(&mut self, ui: &mut egui::Ui, enabled: bool) {
        let rectangle_active = self.rectangle_creation.active();
        let rectangle_response = ui.add_enabled(
            enabled,
            egui::SelectableLabel::new(rectangle_active, "Rectangle"),
        );
        if rectangle_response.clicked() {
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
            } else {
                if self.text_box_creation.active() {
                    let _ = self.text_box_creation.deactivate_to_select();
                }
                if self.text_mode.is_some() {
                    self.exit_canvas_text_mode("rectangle_tool_activation");
                }
                match self.rectangle_creation.activate() {
                    Ok(()) => {
                        self.edit_status = Some(
                            "Rectangle tool active. Drag on the page to create one rectangle."
                                .to_owned(),
                        );
                    }
                    Err(error) => {
                        self.edit_status =
                            Some(format!("Rectangle tool could not activate: {error}"));
                    }
                }
            }
        }
    }

    pub(super) fn commit_rectangle_creation_release(
        &mut self,
        release: rectangle_creation::RectangleCreateReleaseV1,
    ) {
        let created_page_id = match release {
            rectangle_creation::RectangleCreateReleaseV1::Commit { page_id, .. } => Some(page_id),
            rectangle_creation::RectangleCreateReleaseV1::NoChange => None,
        };
        let outcome = match self.editor.as_mut() {
            Some(editor) => self.rectangle_creation.commit_release(editor, release),
            None => Err("Editor session is unavailable.".to_owned()),
        };
        match outcome {
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

#[allow(clippy::too_many_arguments)]
pub(super) fn process_primary_pointer_events(
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
    rectangle_release: &mut Option<rectangle_creation::RectangleCreateReleaseV1>,
    rectangle_error: &mut Option<String>,
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
        *rectangle_error = Some(format!("Rectangle draw could not start: {error}"));
    }

    if session.gesture_token.is_some()
        && primary_down
        && let Some(point) = pointer_document
        && let Err(error) = session.pointer_move(point)
    {
        let _ = session.cancel();
        *rectangle_error = Some(format!("Rectangle preview cancelled: {error}"));
    }

    if session.gesture_token.is_some() && primary_released {
        if let Some(point) = pointer_document {
            match session.pointer_up(point) {
                Ok(release) => *rectangle_release = Some(release),
                Err(error) => {
                    let _ = session.cancel();
                    *rectangle_error = Some(format!("Rectangle draw could not finish: {error}"));
                }
            }
        } else {
            let _ = session.cancel();
            *rectangle_error =
                Some("Rectangle draw ended outside the document coordinate boundary.".to_owned());
        }
    }
}

pub(super) fn process_drag_started(
    session: &mut rectangle_creation::RectangleCreateSessionV1,
    page_id: pub_editor::PageId,
    pointer_start: pub_interaction::DocumentPoint,
    pointer_current: pub_interaction::DocumentPoint,
    next_canvas_drag: &mut Option<MoveTransaction>,
    next_canvas_resize: &mut Option<ResizeTransaction>,
    rectangle_error: &mut Option<String>,
) -> bool {
    if !session.active() {
        return false;
    }

    *next_canvas_drag = None;
    *next_canvas_resize = None;
    let result = if session.gesture_token.is_none() {
        session
            .pointer_down(page_id, pointer_start, "rectangle-draw-v1".to_owned())
            .and_then(|()| session.pointer_move(pointer_current).map(|_| ()))
    } else {
        session.pointer_move(pointer_current).map(|_| ())
    };
    if let Err(error) = result {
        let _ = session.cancel();
        *rectangle_error = Some(format!("Rectangle draw could not start: {error}"));
    }
    true
}

pub(super) fn process_drag_stopped(
    session: &mut rectangle_creation::RectangleCreateSessionV1,
    pointer_document: Option<pub_interaction::DocumentPoint>,
    rectangle_release: &mut Option<rectangle_creation::RectangleCreateReleaseV1>,
    rectangle_error: &mut Option<String>,
) -> bool {
    if !session.active() || session.gesture_token.is_none() {
        return false;
    }

    if let Some(point) = pointer_document {
        match session.pointer_up(point) {
            Ok(release) => *rectangle_release = Some(release),
            Err(error) => {
                let _ = session.cancel();
                *rectangle_error = Some(format!("Rectangle draw could not finish: {error}"));
            }
        }
    } else {
        let _ = session.cancel();
        *rectangle_error =
            Some("Rectangle draw ended outside the document coordinate boundary.".to_owned());
    }
    true
}

pub(super) fn process_dragged(
    session: &mut rectangle_creation::RectangleCreateSessionV1,
    point: pub_interaction::DocumentPoint,
    rectangle_error: &mut Option<String>,
) -> bool {
    if !session.active() || session.gesture_token.is_none() {
        return false;
    }

    if let Err(error) = session.pointer_move(point) {
        let _ = session.cancel();
        *rectangle_error = Some(format!("Rectangle preview cancelled: {error}"));
    }
    true
}

pub(super) fn paint_preview(
    session: &rectangle_creation::RectangleCreateSessionV1,
    painter: &egui::Painter,
    page_id: pub_editor::PageId,
    page_rect: egui::Rect,
    scene_scale: f32,
) {
    if session.page_id == Some(page_id)
        && let Ok(rectangle_creation::RectangleCreatePreviewV1::Bounds(bounds)) = session.preview()
        && let Some(preview_rect) = render_backend::physical_rect_to_egui(
            page_rect,
            scene_scale,
            bounds.x.get(),
            bounds.y.get(),
            bounds.width.get(),
            bounds.height.get(),
        )
    {
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
}
