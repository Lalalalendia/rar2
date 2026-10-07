use super::DesktopShapedFlowRuntimeError;
use pub_editor::{EditOperation, EditorSession, FormatPropertyV1, FormatValueV1};
use pub_model::StoryId;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DesktopCurrentBooleanTypographyRunV1 {
    pub scalar_start: u32,
    pub scalar_end: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bold: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub italic: Option<bool>,
}

fn has_scoped_property_history_v1(
    editor: &EditorSession,
    story_id: StoryId,
    property: FormatPropertyV1,
) -> bool {
    editor.operations().iter().any(|operation| {
        matches!(
            operation,
            EditOperation::SetTextFormatPropertyScopedV1 {
                story_id: operation_story_id,
                property: operation_property,
                ..
            } | EditOperation::ClearTextFormatPropertyOverrideScopedV1 {
                story_id: operation_story_id,
                property: operation_property,
                ..
            } if *operation_story_id == story_id && *operation_property == property
        )
    })
}

fn current_boolean_property_segments_v1(
    editor: &EditorSession,
    story_id: StoryId,
    story_scalar_len: u32,
    property: FormatPropertyV1,
) -> Result<Option<Vec<pub_editor::EffectivePropertySegmentV1>>, DesktopShapedFlowRuntimeError> {
    match editor.current_text_format_property_segments_v1(story_id, property, 0, story_scalar_len) {
        Ok(segments) => Ok(Some(segments)),
        Err(_error) if !has_scoped_property_history_v1(editor, story_id, property) => {
            // This projection is additive to the established shaped-flow path. A source Story
            // may not yet expose bounded authority for a particular boolean property, and plain
            // text edits can invalidate source-relative ranges. In either case, absence remains
            // explicit None rather than inventing false or blocking unrelated layout.
            Ok(None)
        }
        Err(error) => Err(DesktopShapedFlowRuntimeError::new(
            "current_boolean_typography_unavailable",
            format!(
                "scoped {property:?} history exists but current property state is unavailable: {error}"
            ),
        )),
    }
}

fn boolean_value_covering_v1(
    segments: Option<&[pub_editor::EffectivePropertySegmentV1]>,
    start: u32,
    end: u32,
) -> Result<Option<bool>, DesktopShapedFlowRuntimeError> {
    let Some(segments) = segments else {
        return Ok(None);
    };
    let segment = segments
        .iter()
        .find(|segment| segment.start_scalar <= start && end <= segment.end_scalar)
        .ok_or_else(|| {
            DesktopShapedFlowRuntimeError::new(
                "current_boolean_typography_invalid",
                "effective property segments do not cover one current typography interval",
            )
        })?;
    match &segment.value {
        FormatValueV1::Bool(value) => Ok(Some(*value)),
        _ => Err(DesktopShapedFlowRuntimeError::new(
            "current_boolean_typography_invalid",
            "Bold/Italic effective property value is not boolean",
        )),
    }
}

pub fn current_story_boolean_typography_v1(
    editor: &EditorSession,
    story_id: StoryId,
) -> Result<Vec<DesktopCurrentBooleanTypographyRunV1>, DesktopShapedFlowRuntimeError> {
    let story = editor.graph().stories.get(&story_id).ok_or_else(|| {
        DesktopShapedFlowRuntimeError::new(
            "story_missing",
            "requested Story is absent from current EditorSession graph",
        )
    })?;
    let story_scalar_len = u32::try_from(story.text.chars().count()).map_err(|_| {
        DesktopShapedFlowRuntimeError::new(
            "story_extent_overflow",
            "current Story scalar length exceeds the V1 u32 domain",
        )
    })?;
    if story_scalar_len == 0 {
        return Ok(Vec::new());
    }

    let bold = current_boolean_property_segments_v1(
        editor,
        story_id,
        story_scalar_len,
        FormatPropertyV1::Bold,
    )?;
    let italic = current_boolean_property_segments_v1(
        editor,
        story_id,
        story_scalar_len,
        FormatPropertyV1::Italic,
    )?;
    if bold.is_none() && italic.is_none() {
        return Ok(Vec::new());
    }

    let mut boundaries = BTreeSet::from([0, story_scalar_len]);
    for segments in [bold.as_deref(), italic.as_deref()].into_iter().flatten() {
        for segment in segments {
            boundaries.insert(segment.start_scalar);
            boundaries.insert(segment.end_scalar);
        }
    }

    let points = boundaries.into_iter().collect::<Vec<_>>();
    let mut out = Vec::with_capacity(points.len().saturating_sub(1));
    for pair in points.windows(2) {
        let scalar_start = pair[0];
        let scalar_end = pair[1];
        if scalar_start >= scalar_end {
            continue;
        }
        out.push(DesktopCurrentBooleanTypographyRunV1 {
            scalar_start,
            scalar_end,
            bold: boolean_value_covering_v1(bold.as_deref(), scalar_start, scalar_end)?,
            italic: boolean_value_covering_v1(italic.as_deref(), scalar_start, scalar_end)?,
        });
    }
    Ok(out)
}

