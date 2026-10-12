//! UI-only Arrange commands for one direct Chaptera-authored Rectangle.
//!
//! The EditorSession authored stack is the sole ordering and history authority.
//! Imported Publisher objects, Line/group objects and multi-selection stay out.

use crate::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AuthoredArrangeTargetV1 {
    node_id: pub_editor::NodeId,
    page_id: pub_editor::PageId,
}

impl ViewerApp {
    fn selected_authored_arrange_target_v1(&self) -> Result<AuthoredArrangeTargetV1, String> {
        if reader_only_mode() {
            return Err("Reader cannot reorder objects.".to_owned());
        }
        if self.canvas_selection.len() != 1 {
            return Err("Arrange requires exactly one selected Chaptera Rectangle.".to_owned());
        }
        if self.text_mode.is_some()
            || self.rectangle_creation.active()
            || self.text_box_creation.active()
            || self.canvas_drag.is_some()
            || self.canvas_resize.is_some()
        {
            return Err("Finish the current text or pointer tool before arranging.".to_owned());
        }
        let selected = self
            .canvas_selection
            .primary()
            .ok_or_else(|| "Select one Chaptera-authored Rectangle before arranging.".to_owned())?;
        let visual = self
            .visual
            .as_ref()
            .ok_or_else(|| "Arrange requires a current document scene.".to_owned())?;
        let page_id = visual
            .document
            .pages
            .get(self.selected_page)
            .ok_or_else(|| "Arrange requires a current visible page.".to_owned())?
            .id;
        let editor = self
            .editor
            .as_ref()
            .ok_or_else(|| "Arrange requires an editable Chaptera project.".to_owned())?;
        let stack = editor
            .authored_stack(page_id)
            .ok_or_else(|| "Selected page has no canonical authored stack.".to_owned())?;
        let page_key = page_id.as_canonical().to_string();
        for node_id in stack.members {
            let Some(shape) = editor.authored_shape(node_id) else {
                continue;
            };
            if shape.page_id != page_id
                || shape.parent_id != page_id
                || shape.shape_kind != pub_editor::AuthoredShapeKindV1::Rectangle
                || shape.provenance != pub_editor::AuthoredEntityProvenanceV1::AuthorCreated
                || shape.paint.provenance != pub_editor::AuthoredEntityProvenanceV1::AuthorCreated
                || pub_editor::validate_authored_shape_runtime_v1(shape).is_err()
            {
                continue;
            }
            let instance =
                direct_page_local_instance_v1(&node_id.as_canonical().to_string(), &page_key)
                    .map_err(|error| format!("Arrange direct-instance binding failed: {error}"))?;
            if instance.instance_id == selected {
                return Ok(AuthoredArrangeTargetV1 { node_id, page_id });
            }
        }
        Err("Imported/projected objects, groups and non-authored objects cannot be arranged. Only one direct Chaptera Rectangle is supported.".to_owned())
    }

    fn authored_arrange_admission_v1(
        &self,
        mode: pub_editor::AuthoredStackReorderModeV1,
    ) -> Result<AuthoredArrangeTargetV1, String> {
        let target = self.selected_authored_arrange_target_v1()?;
        let stack = self
            .editor
            .as_ref()
            .expect("admitted editor")
            .authored_stack(target.page_id)
            .expect("admitted canonical page");
        pub_editor::plan_reorder_authored_stack_v1(&stack, target.node_id, mode)
            .map_err(|error| format!("Arrange is unavailable: {error}"))?;
        Ok(target)
    }

    pub(super) fn arrange_selected_authored_shape_v1(
        &mut self,
        mode: pub_editor::AuthoredStackReorderModeV1,
    ) -> Result<(), String> {
        let target = self.authored_arrange_admission_v1(mode)?;
        let selected_instance = self
            .canvas_selection
            .primary()
            .expect("admitted selected instance")
            .to_owned();
        let editor = self.editor.as_mut().expect("admitted editor");
        let before = editor.operations().len();
        let operation = editor
            .reorder_authored_stack(target.page_id, target.node_id, mode)
            .map_err(|error| {
                format!(
                    "Arrange refused by EditorSession: {error} ({})",
                    error.code()
                )
            })?;
        if editor.operations().len() != before + 1
            || !matches!(
                operation,
                pub_editor::EditOperation::ReorderAuthoredStack { transition }
                    if transition.page_id == target.page_id
                    && transition.node_id == target.node_id
                    && transition.mode == mode
            )
        {
            return Err(
                "Arrange did not record exactly one canonical ReorderAuthoredStack operation."
                    .to_owned(),
            );
        }
        self.finish_authoring_change(
            "Reordered one Chaptera-authored Rectangle; imported Publisher stacking and source PUB remain unchanged.",
        );
        self.canvas_selection.select_only(selected_instance);
        Ok(())
    }

    pub(super) fn show_authored_arrange_controls_v1(&mut self, ui: &mut egui::Ui) {
        ui.add_space(10.0);
        ui.strong("Arrange · Chaptera objects only");
        let target = self.selected_authored_arrange_target_v1();
        if let Err(reason) = &target {
            ui.small(reason);
        }
        ui.horizontal_wrapped(|ui| {
            for (label, mode) in [
                (
                    "Bring Forward",
                    pub_editor::AuthoredStackReorderModeV1::StepForward,
                ),
                (
                    "Send Backward",
                    pub_editor::AuthoredStackReorderModeV1::StepBackward,
                ),
                (
                    "Bring to Front",
                    pub_editor::AuthoredStackReorderModeV1::ToFront,
                ),
                (
                    "Send to Back",
                    pub_editor::AuthoredStackReorderModeV1::ToBack,
                ),
            ] {
                let admission = self.authored_arrange_admission_v1(mode);
                let response = ui.add_enabled(admission.is_ok(), egui::Button::new(label));
                if response.clicked() {
                    if let Err(error) = self.arrange_selected_authored_shape_v1(mode) {
                        self.edit_status = Some(error);
                    }
                } else if let Err(reason) = admission {
                    response.on_disabled_hover_text(reason);
                }
            }
        });
        ui.small("One direct Chaptera-authored Rectangle per operation. Undo/Redo and Save Project preserve the new order.");
    }
}

impl ViewerApp {
    pub(super) fn show_object_edit_controls(&mut self, ui: &mut egui::Ui) {
        if self.canvas_selection.primary().is_none() {
            return;
        }

        ui.add_space(16.0);
        ui.heading("Object");
        ui.separator();
        self.show_authored_arrange_controls_v1(ui);
        ui.separator();

        match self.selected_direct_replace_image_target() {
            Ok(_) => {
                ui.small(
                    "Direct page-local image placement. PNG/JPEG replacement preserves source PUB bytes and is saved with the EditorProject.",
                );
                if ui.button("Replace image…").clicked() {
                    #[cfg(target_os = "windows")]
                    {
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("Images", &["png", "jpg", "jpeg"])
                            .pick_file()
                        {
                            if let Err(error) = self.replace_selected_image_from_path(&path) {
                                self.edit_status = Some(error);
                            }
                        }
                    }
                    #[cfg(not(target_os = "windows"))]
                    {
                        self.edit_status = Some(
                            "The native replacement picker is part of Windows Editor V0."
                                .to_owned(),
                        );
                    }
                }
            }
            Err(reason) => {
                let response = ui.add_enabled(false, egui::Button::new("Replace image…"));
                response.on_disabled_hover_text(&reason);
                ui.weak(reason);
            }
        }
    }
}
