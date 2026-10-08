//! Desktop selection-keyboard event integration.
//!
//! Owns canvas shortcuts and the Story Alt+Arrow escape route. The existing
//! keyboard law and canonical editor/text-session authorities are reused.

use super::{ViewerApp, direct_scene_instance, reader_only_mode, selection_keyboard, text_session};
use chaptera_scene_instance::{ObjectMutationKindV1, admit_object_mutation_v1};
use eframe::egui;

impl ViewerApp {
    fn selected_direct_move_target(
        &self,
    ) -> Result<(pub_editor::NodeId, pub_editor::RectEmu), String> {
        if self.canvas_selection.len() != 1 {
            return Err("Object nudge requires exactly one selected object.".to_owned());
        }
        let selected_instance = self
            .canvas_selection
            .primary()
            .ok_or_else(|| "Select one object first.".to_owned())?;
        let visual = self
            .visual
            .as_ref()
            .ok_or_else(|| "Document scene is unavailable.".to_owned())?;
        let page = visual
            .document
            .pages
            .get(self.selected_page)
            .ok_or_else(|| "Selected page is unavailable.".to_owned())?;
        let editor = self
            .editor
            .as_ref()
            .ok_or_else(|| "Editor session is unavailable.".to_owned())?;
        let page_origin = page.id.into_canonical();
        let page_id_text = page.id.as_canonical().to_string();

        for scene_node in visual
            .scene
            .nodes
            .iter()
            .filter(|node| node.parent_origin == page_origin)
        {
            let Some(instance) = direct_scene_instance(editor, &page_id_text, scene_node.origin)
            else {
                continue;
            };
            if instance.instance_id != selected_instance {
                continue;
            }
            let admission = admit_object_mutation_v1(&instance, ObjectMutationKindV1::MoveNode);
            let origin_node_id = scene_node.origin.as_canonical().to_string();
            if !admission.admitted
                || admission.origin_node_id.as_deref() != Some(origin_node_id.as_str())
            {
                return Err("Selected visual instance is projected/read-only.".to_owned());
            }
            let authored = editor
                .graph()
                .nodes
                .get(&scene_node.origin)
                .ok_or_else(|| "Selected object has no authored node.".to_owned())?;
            let bounds = authored.header.bounds;
            editor
                .can_move_node_to(scene_node.origin, bounds.x, bounds.y)
                .map_err(|error| {
                    format!("Selected object cannot move: {} ({})", error, error.code())
                })?;
            return Ok((scene_node.origin, bounds));
        }

        Err("Selected visual instance is not a direct page-local object.".to_owned())
    }

    fn active_story_owns_selected_object(&self) -> bool {
        let Some(mode) = self.text_mode.as_ref() else {
            return false;
        };
        self.selected_direct_move_target()
            .is_ok_and(|(node_id, _)| node_id == mode.frame_id)
    }

    fn nudge_selected_object(&mut self, dx_emu: i64, dy_emu: i64, rebind_text: bool) {
        let outcome = self
            .selected_direct_move_target()
            .and_then(|(node_id, before)| {
                let x = before
                    .x
                    .checked_add(pub_editor::LengthEmu::new(dx_emu))
                    .ok_or_else(|| "Object nudge overflowed X geometry.".to_owned())?;
                let y = before
                    .y
                    .checked_add(pub_editor::LengthEmu::new(dy_emu))
                    .ok_or_else(|| "Object nudge overflowed Y geometry.".to_owned())?;
                let editor = self
                    .editor
                    .as_mut()
                    .ok_or_else(|| "Editor session is unavailable.".to_owned())?;
                let before_operations = editor.operations().len();
                editor
                    .can_move_node_to(node_id, x, y)
                    .map_err(|error| format!("Nudge unavailable: {} ({})", error, error.code()))?;
                editor
                    .move_node_to(node_id, x, y)
                    .map_err(|error| format!("Nudge rejected: {} ({})", error, error.code()))?;
                if editor.operations().len() != before_operations + 1 {
                    return Err(
                        "Object nudge must append exactly one MoveNode operation.".to_owned()
                    );
                }
                Ok(())
            });

        match outcome {
            Ok(()) => {
                self.finish_authoring_change(
                    "Nudged canvas object in the authoring session. One MoveNode operation was committed.",
                );
                if rebind_text
                    && let (Some(editor), Some(mode)) = (&self.editor, &mut self.text_mode)
                    && let Err(error) =
                        text_session::rebind_after_non_text_document_change(editor, mode)
                {
                    self.edit_status = Some(format!(
                        "Object nudge committed, but active Story authority could not rebind: {error}"
                    ));
                }
            }
            Err(error) => {
                self.edit_status = Some(error);
                self.sync_visual_geometry_from_editor();
            }
        }
    }

    fn select_all_current_page_objects(&mut self) {
        let Some(visual) = self.visual.as_ref() else {
            return;
        };
        let Some(editor) = self.editor.as_ref() else {
            return;
        };
        let Some(page) = visual.document.pages.get(self.selected_page) else {
            return;
        };
        let page_origin = page.id.into_canonical();
        let page_id_text = page.id.as_canonical().to_string();
        let selected = visual
            .scene
            .nodes
            .iter()
            .filter(|node| node.parent_origin == page_origin)
            .filter_map(|node| direct_scene_instance(editor, &page_id_text, node.origin))
            .map(|instance| instance.instance_id)
            .collect::<Vec<_>>();
        self.canvas_selection.replace_all(selected);
        self.canvas_drag = None;
        self.canvas_resize = None;
    }

    pub(super) fn process_canvas_object_keyboard(&mut self, ctx: &egui::Context) {
        if reader_only_mode() || self.text_mode.is_some() || ctx.wants_keyboard_input() {
            return;
        }

        let events = ctx.input(|input| input.events.clone());
        for event in events {
            let egui::Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } = event
            else {
                continue;
            };

            if key == egui::Key::Escape {
                if self.process_text_box_escape() {
                    continue;
                }
                if self.process_rectangle_escape() {
                } else if self.canvas_resize.take().is_some() || self.canvas_drag.take().is_some() {
                    self.edit_status = Some("Cancelled the active canvas gesture.".to_owned());
                } else if self.canvas_selection.len() > 0 {
                    self.canvas_selection.clear();
                    self.edit_status = Some("Cleared canvas selection.".to_owned());
                }
                continue;
            }

            if (modifiers.ctrl || modifiers.command)
                && !modifiers.alt
                && !modifiers.shift
                && key == egui::Key::A
            {
                self.select_all_current_page_objects();
                continue;
            }

            let Some(direction) = keyboard_arrow_direction(key) else {
                continue;
            };
            let movable = self.selected_direct_move_target().is_ok();
            let decision = selection_keyboard::route_arrow_v1(
                selection_keyboard::FocusOwnerV1::Canvas,
                direction,
                selection_keyboard::KeyModifiersV1 {
                    shift: modifiers.shift,
                    control_or_command: modifiers.ctrl || modifiers.command,
                    alt: modifiers.alt,
                },
                self.canvas_selection.len(),
                movable,
                false,
            );
            if decision.route == selection_keyboard::ArrowRouteV1::MoveObject {
                self.nudge_selected_object(decision.dx_emu, decision.dy_emu, false);
            }
        }
    }

    pub(super) fn process_story_object_keyboard(
        &mut self,
        key: egui::Key,
        modifiers: egui::Modifiers,
    ) -> bool {
        if modifiers.alt && !modifiers.shift && !modifiers.ctrl && !modifiers.command {
            if let Some(direction) = keyboard_arrow_direction(key) {
                let movable = self.selected_direct_move_target().is_ok();
                let decision = selection_keyboard::route_arrow_v1(
                    selection_keyboard::FocusOwnerV1::StoryText,
                    direction,
                    selection_keyboard::KeyModifiersV1 {
                        alt: true,
                        ..Default::default()
                    },
                    self.canvas_selection.len(),
                    movable,
                    self.active_story_owns_selected_object(),
                );
                if decision.route == selection_keyboard::ArrowRouteV1::MoveObject {
                    self.nudge_selected_object(decision.dx_emu, decision.dy_emu, true);
                }
            }
            return true;
        }
        false
    }
}

fn keyboard_arrow_direction(key: egui::Key) -> Option<selection_keyboard::ArrowDirectionV1> {
    match key {
        egui::Key::ArrowLeft => Some(selection_keyboard::ArrowDirectionV1::Left),
        egui::Key::ArrowRight => Some(selection_keyboard::ArrowDirectionV1::Right),
        egui::Key::ArrowUp => Some(selection_keyboard::ArrowDirectionV1::Up),
        egui::Key::ArrowDown => Some(selection_keyboard::ArrowDirectionV1::Down),
        _ => None,
    }
}
