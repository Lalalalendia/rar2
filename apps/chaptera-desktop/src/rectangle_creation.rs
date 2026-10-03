use chaptera_canvas_creation_interaction::{
    BoxDrawCommitStatusV1, BoxDrawPreviewStatusV1, BoxDrawTransactionV1, CanvasToolStateV1,
    PointEmuV1, RectEmuV1, activate_canvas_tool_v1, commit_box_draw_v1,
    default_canvas_tool_state_v1, end_pointer_gesture_v1, preview_box_draw_v1,
    rectangle_create_tool_v1, select_tool_v1, start_box_draw_v1, start_pointer_gesture_v1,
    update_box_draw_v1, update_pointer_gesture_v1,
};
use pub_editor::{
    AuthoredShapePaintV1, AuthoredSolidFillV1, AuthoredSolidStrokeV1, EditorSession, LengthEmu,
    NodeId, PageId, RectEmu, Srgb8V1,
};

#[derive(Debug, Clone)]
pub struct RectangleCreateSessionV1 {
    pub tool_state: CanvasToolStateV1,
    pub gesture_token: Option<String>,
    pub page_id: Option<PageId>,
    pub draw: Option<BoxDrawTransactionV1>,
}

impl Default for RectangleCreateSessionV1 {
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
pub enum RectangleCreatePreviewV1 {
    None,
    Bounds(RectEmu),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RectangleCreateReleaseV1 {
    NoChange,
    Commit { page_id: PageId, bounds: RectEmu },
}

pub fn chaptera_rectangle_paint_v1() -> AuthoredShapePaintV1 {
    AuthoredShapePaintV1 {
        fill: AuthoredSolidFillV1 {
            visible: true,
            color: Srgb8V1 {
                r: 255,
                g: 255,
                b: 255,
            },
        },
        stroke: AuthoredSolidStrokeV1 {
            visible: true,
            color: Srgb8V1 { r: 0, g: 0, b: 0 },
            width_emu: 12_700,
        },
        provenance: pub_editor::AuthoredEntityProvenanceV1::AuthorCreated,
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

impl RectangleCreateSessionV1 {
    pub fn active(&self) -> bool {
        self.tool_state.active_tool == rectangle_create_tool_v1()
    }

    pub fn activate(&mut self) -> Result<(), String> {
        let transition = activate_canvas_tool_v1(&self.tool_state, rectangle_create_tool_v1())
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
            return Err("Rectangle tool is not active.".to_owned());
        }
        if self.draw.is_some() || self.gesture_token.is_some() {
            return Err("Rectangle gesture is already active.".to_owned());
        }
        let transition = start_pointer_gesture_v1(
            &self.tool_state,
            rectangle_create_tool_v1(),
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
    ) -> Result<RectangleCreatePreviewV1, String> {
        let token = self
            .gesture_token
            .as_deref()
            .ok_or_else(|| "Rectangle gesture is not active.".to_owned())?;
        let draw = self
            .draw
            .as_ref()
            .ok_or_else(|| "Rectangle draw transaction is missing.".to_owned())?;

        let transition =
            update_pointer_gesture_v1(&self.tool_state, &rectangle_create_tool_v1(), token)
                .map_err(|error| error.to_string())?;
        let draw =
            update_box_draw_v1(draw, point(point_document)?).map_err(|error| error.to_string())?;
        let preview = preview_box_draw_v1(&draw).map_err(|error| error.to_string())?;

        self.tool_state = transition.state;
        self.draw = Some(draw);

        Ok(match (preview.status, preview.bounds) {
            (BoxDrawPreviewStatusV1::Preview, Some(bounds)) => {
                RectangleCreatePreviewV1::Bounds(rect(bounds))
            }
            _ => RectangleCreatePreviewV1::None,
        })
    }

    pub fn pointer_up(
        &mut self,
        point_document: pub_interaction::DocumentPoint,
    ) -> Result<RectangleCreateReleaseV1, String> {
        let preview = self.pointer_move(point_document)?;
        let token = self
            .gesture_token
            .clone()
            .ok_or_else(|| "Rectangle gesture token is missing.".to_owned())?;
        let page_id = self
            .page_id
            .ok_or_else(|| "Rectangle target page is missing.".to_owned())?;
        let draw = self
            .draw
            .as_ref()
            .ok_or_else(|| "Rectangle draw transaction is missing.".to_owned())?;
        let commit = commit_box_draw_v1(draw).map_err(|error| error.to_string())?;
        let ended = end_pointer_gesture_v1(&self.tool_state, &rectangle_create_tool_v1(), &token)
            .map_err(|error| error.to_string())?;

        self.tool_state = ended.state;
        self.gesture_token = None;
        self.page_id = None;
        self.draw = None;

        Ok(match (commit.status, commit.bounds, preview) {
            (BoxDrawCommitStatusV1::Commit, Some(bounds), RectangleCreatePreviewV1::Bounds(_)) => {
                RectangleCreateReleaseV1::Commit {
                    page_id,
                    bounds: rect(bounds),
                }
            }
            _ => RectangleCreateReleaseV1::NoChange,
        })
    }

    pub fn cancel(&mut self) -> Result<(), String> {
        if self.gesture_token.is_none() {
            self.draw = None;
            self.page_id = None;
            return Ok(());
        }
        let transition =
            chaptera_canvas_creation_interaction::escape_canvas_tool_v1(&self.tool_state)
                .map_err(|error| error.to_string())?;
        self.tool_state = transition.state;
        self.gesture_token = None;
        self.page_id = None;
        self.draw = None;
        Ok(())
    }

    pub fn preview(&self) -> Result<RectangleCreatePreviewV1, String> {
        let Some(draw) = self.draw.as_ref() else {
            return Ok(RectangleCreatePreviewV1::None);
        };
        let preview = preview_box_draw_v1(draw).map_err(|error| error.to_string())?;
        Ok(match (preview.status, preview.bounds) {
            (BoxDrawPreviewStatusV1::Preview, Some(bounds)) => {
                RectangleCreatePreviewV1::Bounds(rect(bounds))
            }
            _ => RectangleCreatePreviewV1::None,
        })
    }

    pub fn commit_release(
        &mut self,
        editor: &mut EditorSession,
        release: RectangleCreateReleaseV1,
    ) -> Result<Option<NodeId>, String> {
        let RectangleCreateReleaseV1::Commit { page_id, bounds } = release else {
            return Ok(None);
        };
        let node_id = NodeId::from_canonical(pub_model::new_editor_canonical_id());
        editor
            .create_shape(node_id, page_id, bounds, chaptera_rectangle_paint_v1())
            .map_err(|error| format!("CreateShape rejected: {} ({})", error, error.code()))?;
        self.deactivate_to_select()?;
        Ok(Some(node_id))
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
        let mut session = RectangleCreateSessionV1::default();
        session.activate().unwrap();
        session
            .pointer_down(page_id(), point(30, 40), "rectangle-1".to_owned())
            .unwrap();
        assert_eq!(
            session.pointer_move(point(-10, -20)).unwrap(),
            RectangleCreatePreviewV1::Bounds(RectEmu::new(
                LengthEmu::new(-10),
                LengthEmu::new(-20),
                LengthEmu::new(40),
                LengthEmu::new(60),
            ))
        );
        assert_eq!(
            session.pointer_up(point(-10, -20)).unwrap(),
            RectangleCreateReleaseV1::Commit {
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
            .pointer_down(page_id(), point(0, 0), "rectangle-2".to_owned())
            .unwrap();
        assert_eq!(
            session.pointer_up(point(0, 20)).unwrap(),
            RectangleCreateReleaseV1::NoChange
        );
    }

    #[test]
    fn cancel_clears_transient_draw_without_document_intent() {
        let mut session = RectangleCreateSessionV1::default();
        session.activate().unwrap();
        session
            .pointer_down(page_id(), point(0, 0), "rectangle-1".to_owned())
            .unwrap();
        session.pointer_move(point(100, 50)).unwrap();
        session.cancel().unwrap();
        assert_eq!(session.preview().unwrap(), RectangleCreatePreviewV1::None);
        assert!(session.gesture_token.is_none());
    }

    #[test]
    fn default_paint_is_explicit_and_deterministic() {
        let paint = chaptera_rectangle_paint_v1();
        assert!(paint.fill.visible);
        assert_eq!(
            paint.fill.color,
            Srgb8V1 {
                r: 255,
                g: 255,
                b: 255
            }
        );
        assert!(paint.stroke.visible);
        assert_eq!(paint.stroke.color, Srgb8V1 { r: 0, g: 0, b: 0 });
        assert_eq!(paint.stroke.width_emu, 12_700);
    }
}
