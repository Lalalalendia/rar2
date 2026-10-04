//! Desktop Duplicate Rectangle shell ownership.
//!
//! The pub-editor duplicate runtime remains the semantic authority. This module
//! owns selection admission and the one-operation Desktop duplicate binding.

use crate::*;

impl ViewerApp {
    pub(super) fn selected_authored_rectangle_target(
        &self,
    ) -> Result<(pub_editor::NodeId, pub_editor::PageId), String> {
        if self.canvas_selection.len() != 1 {
            return Err("Duplicate requires exactly one selected authored Rectangle.".to_owned());
        }
        let selected_instance = self
            .canvas_selection
            .primary()
            .ok_or_else(|| "Select one authored Rectangle first.".to_owned())?;
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
        let stack = editor
            .authored_stack(page.id)
            .ok_or_else(|| "Selected page is unavailable in the authoring session.".to_owned())?;
        let page_id_text = page.id.as_canonical().to_string();
    
        for node_id in stack.members {
            let Some(shape) = editor.authored_shape(node_id) else {
                continue;
            };
            if shape.page_id != page.id || shape.parent_id != page.id {
                continue;
            }
            let instance =
                direct_page_local_instance_v1(&node_id.as_canonical().to_string(), &page_id_text)
                    .map_err(|error| format!("Duplicate selection identity is invalid: {error}"))?;
            if instance.instance_id != selected_instance {
                continue;
            }
            editor
                .can_duplicate_authored_rectangle(node_id)
                .map_err(|error| format!("Duplicate is unavailable: {error}"))?;
            return Ok((node_id, page.id));
        }
    
        Err("Selected visual instance is not an admitted authored Rectangle.".to_owned())
    }
    
    pub(super) fn duplicate_selected_authored_rectangle(&mut self) -> Result<pub_editor::NodeId, String> {
        let (source_node_id, page_id) = self.selected_authored_rectangle_target()?;
        let destination_node_id =
            pub_editor::NodeId::from_canonical(pub_model::new_editor_canonical_id());
        let editor = self
            .editor
            .as_mut()
            .ok_or_else(|| "Editor session is unavailable.".to_owned())?;
        let before_operations = editor.operations().len();
        let operation = editor
            .duplicate_authored_rectangle(
                source_node_id,
                destination_node_id,
                pub_editor::DUPLICATE_PLACEMENT_POLICY_V1,
            )
            .map_err(|error| format!("Duplicate rejected: {error}"))?;
        if editor.operations().len() != before_operations + 1 {
            return Err("Duplicate must append exactly one CreateShape operation.".to_owned());
        }
        if !matches!(
            operation,
            pub_editor::EditOperation::CreateShape { node_id, .. }
                if node_id == destination_node_id
        ) {
            return Err(
                "Duplicate must persist as one canonical CreateShape operation.".to_owned(),
            );
        }
    
        self.finish_authoring_change(
            "Duplicated authored Rectangle. One CreateShape operation was committed.",
        );
        let instance = direct_page_local_instance_v1(
            &destination_node_id.as_canonical().to_string(),
            &page_id.as_canonical().to_string(),
        )
        .map_err(|error| {
            format!("Duplicate committed, but durable selection could not bind: {error}")
        })?;
        self.canvas_selection.select_only(instance.instance_id);
        Ok(destination_node_id)
    }
}
