use chaptera_canvas_creation_interaction::{
    BoxDrawCommitStatusV1, BoxDrawPreviewStatusV1, BoxDrawTransactionV1, CanvasToolStateV1,
    PointEmuV1, RectEmuV1, activate_canvas_tool_v1, commit_box_draw_v1,
    default_canvas_tool_state_v1, end_pointer_gesture_v1, preview_box_draw_v1, select_tool_v1,
    start_box_draw_v1, start_pointer_gesture_v1, textbox_create_tool_v1, update_box_draw_v1,
    update_pointer_gesture_v1,
};
use pub_editor::{
    AuthoringTextPresetV1, EditorSession, LengthEmu, NodeId, PageId, RectEmu, StoryId,
};

#[derive(Debug, Clone)]
pub struct TextBoxCreateSessionV1 {
    pub tool_state: CanvasToolStateV1,
    pub gesture_token: Option<String>,
    pub page_id: Option<PageId>,
    pub draw: Option<BoxDrawTransactionV1>,
}

impl Default for TextBoxCreateSessionV1 {
    fn default() -> Self {
        Self {
            tool_state: default_canvas_tool_state_v1(),
            gesture_token: None,
            page_id: None,
            draw: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextBoxCreatePreviewV1 {
    None,
    Bounds(RectEmu),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextBoxCreateReleaseV1 {
    NoChange,
    Commit { page_id: PageId, bounds: RectEmu },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CreatedTextBoxV1 {
    pub node_id: NodeId,
    pub story_id: StoryId,
    pub page_id: PageId,
}

pub fn chaptera_text_box_preset_v1() -> AuthoringTextPresetV1 {
    AuthoringTextPresetV1 {
        resource_id: chaptera_desktop_fallback_font_resource::RESOURCE_ID.to_owned(),
        font_fingerprint_sha256:
            chaptera_desktop_fallback_font_resource::EXPECTED_SHA256.to_owned(),
        face_index: 0,
        font_size_emu: LengthEmu::new(chaptera_desktop_fallback_font_resource::FONT_SIZE_EMU),
        line_height_emu: LengthEmu::new(
            chaptera_desktop_fallback_font_resource::LINE_HEIGHT_EMU,
        ),
    }
}

fn point(point: pub_interaction::DocumentPoint) -> Result<PointEmuV1, String> {
    PointEmuV1::new(point.x.get(), point.y.get()).map_err(|error| error.to_string())
}

fn rect(bounds: RectEmuV1) -> RectEmu {
    RectEmu::new(
        LengthEmu::new(bounds.x),
        LengthEmu::new(bounds.y),
        LengthEmu::new(bounds.width),
        LengthEmu::new(bounds.height),
    )
}

impl TextBoxCreateSessionV1 {
    pub fn active(&self) -> bool {
        self.tool_state.active_tool == textbox_create_tool_v1()
    }

    pub fn activate(&mut self) -> Result<(), String> {
        let transition = activate_canvas_tool_v1(&self.tool_state, textbox_create_tool_v1())
            .map_err(|error| error.to_string())?;
        self.tool_state = transition.state;
        self.gesture_token = None;
        self.page_id = None;
        self.draw = None;
        Ok(())
    }

    pub fn deactivate_to_select(&mut self) -> Result<(), String> {
        let transition = activate_canvas_tool_v1(&self.tool_state, select_tool_v1())
            .map_err(|error| error.to_string())?;
        self.tool_state = transition.state;
        self.gesture_token = None;
        self.page_id = None;
        self.draw = None;
        Ok(())
    }

    pub fn pointer_down(
        &mut self,
        page_id: PageId,
        point_document: pub_interaction::DocumentPoint,
        gesture_token: String,
    ) -> Result<(), String> {
        if !self.active() {
            return Err("Text Box tool is not active.".to_owned());
        }
        if self.draw.is_some() || self.gesture_token.is_some() {
            return Err("Text Box gesture is already active.".to_owned());
        }
        let transition = start_pointer_gesture_v1(
            &self.tool_state,
            textbox_create_tool_v1(),
            gesture_token.clone(),
        )
        .map_err(|error| error.to_string())?;
        let draw = start_box_draw_v1(page_id.as_canonical().to_string(), point(point_document)?)
            .map_err(|error| error.to_string())?;

        self.tool_state = transition.state;
        self.gesture_token = Some(gesture_token);
        self.page_id = Some(page_id);
        self.draw = Some(draw);
        Ok(())
    }

    pub fn pointer_move(
        &mut self,
        point_document: pub_interaction::DocumentPoint,
    ) -> Result<TextBoxCreatePreviewV1, String> {
        let token = self
            .gesture_token
            .as_deref()
            .ok_or_else(|| "Text Box gesture is not active.".to_owned())?;
        let draw = self
            .draw
            .as_ref()
            .ok_or_else(|| "Text Box draw transaction is missing.".to_owned())?;

        let transition =
            update_pointer_gesture_v1(&self.tool_state, &textbox_create_tool_v1(), token)
                .map_err(|error| error.to_string())?;
        let draw =
            update_box_draw_v1(draw, point(point_document)?).map_err(|error| error.to_string())?;
        let preview = preview_box_draw_v1(&draw).map_err(|error| error.to_string())?;

        self.tool_state = transition.state;
        self.draw = Some(draw);

        Ok(match (preview.status, preview.bounds) {
            (BoxDrawPreviewStatusV1::Preview, Some(bounds)) => {
                TextBoxCreatePreviewV1::Bounds(rect(bounds))
            }
            _ => TextBoxCreatePreviewV1::None,
        })
    }

    pub fn pointer_up(
        &mut self,
        point_document: pub_interaction::DocumentPoint,
    ) -> Result<TextBoxCreateReleaseV1, String> {
        let preview = self.pointer_move(point_document)?;
        let token = self
            .gesture_token
            .clone()
            .ok_or_else(|| "Text Box gesture token is missing.".to_owned())?;
        let page_id = self
            .page_id
            .ok_or_else(|| "Text Box target page is missing.".to_owned())?;
        let draw = self
            .draw
            .as_ref()
            .ok_or_else(|| "Text Box draw transaction is missing.".to_owned())?;
        let commit = commit_box_draw_v1(draw).map_err(|error| error.to_string())?;
        let ended = end_pointer_gesture_v1(&self.tool_state, &textbox_create_tool_v1(), &token)
            .map_err(|error| error.to_string())?;

        self.tool_state = ended.state;
        self.gesture_token = None;
        self.page_id = None;
        self.draw = None;

        let release = match (commit.status, commit.bounds, preview) {
            (BoxDrawCommitStatusV1::Commit, Some(bounds), TextBoxCreatePreviewV1::Bounds(_)) => {
                TextBoxCreateReleaseV1::Commit {
                    page_id,
                    bounds: rect(bounds),
                }
            }
            _ => TextBoxCreateReleaseV1::NoChange,
        };
        if release == TextBoxCreateReleaseV1::NoChange {
            self.deactivate_to_select()?;
        }
        Ok(release)
    }

    pub fn cancel(&mut self) -> Result<(), String> {
        if self.gesture_token.is_some() {
            let transition =
                chaptera_canvas_creation_interaction::escape_canvas_tool_v1(&self.tool_state)
                    .map_err(|error| error.to_string())?;
            self.tool_state = transition.state;
        }
        self.gesture_token = None;
        self.page_id = None;
        self.draw = None;
        if self.active() {
            self.deactivate_to_select()?;
        }
        Ok(())
    }

    pub fn preview(&self) -> Result<TextBoxCreatePreviewV1, String> {
        let Some(draw) = self.draw.as_ref() else {
            return Ok(TextBoxCreatePreviewV1::None);
        };
        let preview = preview_box_draw_v1(draw).map_err(|error| error.to_string())?;
        Ok(match (preview.status, preview.bounds) {
            (BoxDrawPreviewStatusV1::Preview, Some(bounds)) => {
                TextBoxCreatePreviewV1::Bounds(rect(bounds))
            }
            _ => TextBoxCreatePreviewV1::None,
        })
    }

    pub fn commit_release(
        &mut self,
        editor: &mut EditorSession,
        release: TextBoxCreateReleaseV1,
    ) -> Result<Option<CreatedTextBoxV1>, String> {
        let TextBoxCreateReleaseV1::Commit { page_id, bounds } = release else {
            return Ok(None);
        };
        let node_id = NodeId::from_canonical(pub_model::new_editor_canonical_id());
        let story_id = StoryId::from_canonical(pub_model::new_editor_canonical_id());
        editor
            .create_text_box(
                node_id,
                story_id,
                page_id,
                bounds,
                chaptera_text_box_preset_v1(),
            )
            .map_err(|error| format!("CreateTextBox rejected: {} ({})", error, error.code()))?;
        self.deactivate_to_select()?;
        Ok(Some(CreatedTextBoxV1 {
            node_id,
            story_id,
            page_id,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page_id() -> PageId {
        serde_json::from_str(r#""11000000-0000-4000-8000-000000000001""#).unwrap()
    }

    fn point(x: i64, y: i64) -> pub_interaction::DocumentPoint {
        pub_interaction::DocumentPoint::new(LengthEmu::new(x), LengthEmu::new(y))
    }

    #[test]
    fn reverse_drag_normalizes_and_zero_size_is_no_change() {
        let mut session = TextBoxCreateSessionV1::default();
        session.activate().unwrap();
        session
            .pointer_down(page_id(), point(30, 40), "textbox-1".to_owned())
            .unwrap();
        assert_eq!(
            session.pointer_move(point(-10, -20)).unwrap(),
            TextBoxCreatePreviewV1::Bounds(RectEmu::new(
                LengthEmu::new(-10),
                LengthEmu::new(-20),
                LengthEmu::new(40),
                LengthEmu::new(60),
            ))
        );
        assert_eq!(
            session.pointer_up(point(-10, -20)).unwrap(),
            TextBoxCreateReleaseV1::Commit {
                page_id: page_id(),
                bounds: RectEmu::new(
                    LengthEmu::new(-10),
                    LengthEmu::new(-20),
                    LengthEmu::new(40),
                    LengthEmu::new(60),
                ),
            }
        );

        session.activate().unwrap();
        session
            .pointer_down(page_id(), point(0, 0), "textbox-2".to_owned())
            .unwrap();
        assert_eq!(
            session.pointer_up(point(0, 20)).unwrap(),
            TextBoxCreateReleaseV1::NoChange
        );
        assert!(!session.active(), "zero-size one-shot returns to Select");
    }

    #[test]
    fn cancel_clears_transient_draw_without_document_intent() {
        let mut session = TextBoxCreateSessionV1::default();
        session.activate().unwrap();
        session
            .pointer_down(page_id(), point(0, 0), "textbox-1".to_owned())
            .unwrap();
        session.pointer_move(point(100, 50)).unwrap();
        session.cancel().unwrap();
        assert_eq!(session.preview().unwrap(), TextBoxCreatePreviewV1::None);
        assert!(session.gesture_token.is_none());
        assert!(!session.active(), "cancelled one-shot returns to Select");
    }

    #[test]
    fn preset_is_explicit_and_uses_pinned_fallback_resource() {
        let preset = chaptera_text_box_preset_v1();
        assert_eq!(
            preset.resource_id,
            chaptera_desktop_fallback_font_resource::RESOURCE_ID
        );
        assert_eq!(
            preset.font_fingerprint_sha256,
            chaptera_desktop_fallback_font_resource::EXPECTED_SHA256
        );
        assert_eq!(preset.face_index, 0);
    }
}
