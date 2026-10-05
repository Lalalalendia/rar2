use chaptera_caret_layout_feed::build_caret_map_from_shaped_flow_v1;
use chaptera_text_caret_map_adapter::ResolvedTextCaretMapV1;
use pub_editor::{
    EditOperation, EditorCurrentImageResourceV1, EditorSession, FormatPropertyV1, FormatValueV1,
};
use pub_layout::{
    BoundedLayoutEnvironment, BoundedShapedFlowRuntime, BoundedShapedFlowScene,
    BoundedShapingRuntime, font_fingerprint_sha256, project_bounded, resolve_bounded_shaped_flow,
};
use pub_model::{LengthEmu, NodeId, StoryId};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::BTreeSet, fmt};

pub const DESKTOP_SHAPED_FLOW_RUNTIME_V1: &str = "chaptera.desktop-shaped-flow-runtime.v1";
pub const CURRENT_FIXED_PDF_RESOURCE_INPUT_V1: &str =
    "chaptera.current-fixed-pdf-resource-input.v1";

#[derive(Debug, Clone, Copy)]
pub struct ExplicitDesktopFontResourceV1<'a> {
    pub resource_id: &'a str,
    pub expected_sha256: &'a str,
    pub face_index: u32,
    pub font_size_emu: LengthEmu,
    pub line_height_emu: LengthEmu,
    pub bytes: &'a [u8],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopStoryLayoutV1 {
    pub layout_revision_id: String,
    pub story_id: StoryId,
    pub story_scalar_len: u32,
    pub font_fingerprint_sha256: String,
    pub current_boolean_typography: Vec<DesktopCurrentBooleanTypographyRunV1>,
    pub shaped_flow: BoundedShapedFlowScene,
    pub caret_map: ResolvedTextCaretMapV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DesktopCurrentBooleanTypographyRunV1 {
    pub scalar_start: u32,
    pub scalar_end: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bold: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub italic: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CurrentFixedPdfStrokeV1 {
    pub rgb: [u8; 3],
    pub width_emu: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CurrentFixedPdfNodePaintV1 {
    pub node_id: NodeId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill_rgb: Option<[u8; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stroke: Option<CurrentFixedPdfStrokeV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CurrentFixedPdfFontV1 {
    pub fingerprint_sha256: String,
    pub face_index: u32,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CurrentFixedPdfResourceInputV1 {
    pub protocol_version: String,
    pub binding: Value,
    pub shaped_flow: BoundedShapedFlowScene,
    pub node_paints: Vec<CurrentFixedPdfNodePaintV1>,
    pub image_resources: Vec<EditorCurrentImageResourceV1>,
    pub font: CurrentFixedPdfFontV1,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopShapedFlowRuntimeError {
    pub code: &'static str,
    pub message: String,
}

impl DesktopShapedFlowRuntimeError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl fmt::Display for DesktopShapedFlowRuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for DesktopShapedFlowRuntimeError {}

pub fn validate_explicit_font_resource_v1(
    font: &ExplicitDesktopFontResourceV1<'_>,
) -> Result<String, DesktopShapedFlowRuntimeError> {
    if font.resource_id.is_empty() {
        return Err(DesktopShapedFlowRuntimeError::new(
            "font_resource_missing",
            "explicit Desktop font resource_id is required",
        ));
    }
    if font.bytes.is_empty() {
        return Err(DesktopShapedFlowRuntimeError::new(
            "font_resource_missing",
            "explicit Desktop font bytes are required",
        ));
    }
    if font.font_size_emu.get() <= 0 {
        return Err(DesktopShapedFlowRuntimeError::new(
            "invalid_font_size",
            "font_size_emu must be positive",
        ));
    }
    if font.line_height_emu.get() <= 0 {
        return Err(DesktopShapedFlowRuntimeError::new(
            "invalid_line_height",
            "line_height_emu must be positive",
        ));
    }

    let actual = font_fingerprint_sha256(font.bytes);
    if font.expected_sha256.is_empty() || actual != font.expected_sha256 {
        return Err(DesktopShapedFlowRuntimeError::new(
            "font_fingerprint_mismatch",
            format!(
                "explicit Desktop font fingerprint mismatch: expected={} actual={actual}",
                font.expected_sha256
            ),
        ));
    }
    Ok(actual)
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

pub fn build_current_story_layout_v1(
    editor: &EditorSession,
    story_id: StoryId,
    layout_revision_id: &str,
    font: &ExplicitDesktopFontResourceV1<'_>,
) -> Result<DesktopStoryLayoutV1, DesktopShapedFlowRuntimeError> {
    if layout_revision_id.is_empty() {
        return Err(DesktopShapedFlowRuntimeError::new(
            "invalid_layout_revision",
            "layout_revision_id is required",
        ));
    }

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

    let fingerprint = validate_explicit_font_resource_v1(font)?;
    let current_boolean_typography = current_story_boolean_typography_v1(editor, story_id)?;
    let authoring =
        pub_viewer::bounded_authoring_slice_from_resolved(editor.graph()).map_err(|error| {
            DesktopShapedFlowRuntimeError::new(
                "authoring_projection_failed",
                format!("resolved graph could not enter bounded layout projection: {error}"),
            )
        })?;
    let projection = project_bounded(authoring);

    let runtime = BoundedShapedFlowRuntime {
        shaping: BoundedShapingRuntime {
            layout: BoundedLayoutEnvironment {
                engine_revision: DESKTOP_SHAPED_FLOW_RUNTIME_V1.to_owned(),
                font_set_fingerprint: fingerprint.clone(),
                resource_fingerprint: font.resource_id.to_owned(),
            },
            face_index: font.face_index,
            font_size_emu: font.font_size_emu,
            font_bytes: font.bytes,
        },
        line_height: font.line_height_emu,
    };

    let shaped_flow = resolve_bounded_shaped_flow(&projection, &runtime).map_err(|error| {
        DesktopShapedFlowRuntimeError::new(
            "shaped_flow_failed",
            format!("authoritative bounded shaped flow failed: {error}"),
        )
    })?;

    let caret_map = build_caret_map_from_shaped_flow_v1(
        &shaped_flow,
        layout_revision_id,
        story_id,
        story_scalar_len,
    )
    .map_err(|error| {
        DesktopShapedFlowRuntimeError::new(
            "caret_feed_failed",
            format!("shaped-flow caret projection failed: {error}"),
        )
    })?;

    Ok(DesktopStoryLayoutV1 {
        layout_revision_id: layout_revision_id.to_owned(),
        story_id,
        story_scalar_len,
        font_fingerprint_sha256: fingerprint,
        current_boolean_typography,
        shaped_flow,
        caret_map,
    })
}

pub fn build_current_fixed_pdf_resource_input_v1(
    editor: &EditorSession,
    primary_story_id: StoryId,
    binding: Value,
    font: &ExplicitDesktopFontResourceV1<'_>,
) -> Result<CurrentFixedPdfResourceInputV1, DesktopShapedFlowRuntimeError> {
    let layout = build_current_story_layout_v1(
        editor,
        primary_story_id,
        "fixed-pdf:current-editor-state",
        font,
    )?;
    let image_resources = editor.current_image_resources_v1().map_err(|error| {
        DesktopShapedFlowRuntimeError::new(
            "current_image_resources_failed",
            format!("current Editor image resources could not be materialized: {error}"),
        )
    })?;

    Ok(CurrentFixedPdfResourceInputV1 {
        protocol_version: CURRENT_FIXED_PDF_RESOURCE_INPUT_V1.to_owned(),
        binding,
        shaped_flow: layout.shaped_flow,
        // Keep paint admission explicit. The Stage-0.2 real run will tell us
        // whether the exact Move/Resize targets require a paint resource seam.
        node_paints: Vec::new(),
        image_resources,
        font: CurrentFixedPdfFontV1 {
            fingerprint_sha256: layout.font_fingerprint_sha256,
            face_index: font.face_index,
            bytes: font.bytes.to_vec(),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pub_editor::{Sha256Digest, open_mature_0x2c_editor};
    use pub_model::EMU_PER_POINT;
    use sha2::{Digest, Sha256};
    use std::{env, fs};

    fn test_font() -> ExplicitDesktopFontResourceV1<'static> {
        let bytes = font_test_data::NOTOSERIF_AUTOHINT_SHAPING;
        let fingerprint = font_fingerprint_sha256(bytes);
        let leaked = Box::leak(fingerprint.into_boxed_str());
        ExplicitDesktopFontResourceV1 {
            resource_id: "dev-test:noto-serif-autohint-shaping",
            expected_sha256: leaked,
            face_index: 0,
            font_size_emu: LengthEmu::new(10 * EMU_PER_POINT),
            line_height_emu: LengthEmu::new(12 * EMU_PER_POINT),
            bytes,
        }
    }

    #[test]
    fn current_fixed_pdf_resource_input_is_typed_and_source_neutral() {
        let binding = serde_json::json!({
            "protocol_version": "chaptera.editor-fixed-pdf-binding.v1",
            "project_state_id": "sha256:test",
        });
        let encoded = serde_json::to_value(CurrentFixedPdfResourceInputV1 {
            protocol_version: CURRENT_FIXED_PDF_RESOURCE_INPUT_V1.to_owned(),
            binding: binding.clone(),
            shaped_flow: BoundedShapedFlowScene {
                environment: pub_layout::BoundedShapedFlowDescriptor {
                    shaping: pub_layout::BoundedShapingDescriptor {
                        layout: BoundedLayoutEnvironment {
                            engine_revision: "test".into(),
                            font_set_fingerprint: "font".into(),
                            resource_fingerprint: "resource".into(),
                        },
                        face_index: 0,
                        font_size_emu: LengthEmu::new(1),
                        shaper_revision: pub_layout::BOUNDED_SHAPER_REVISION.into(),
                    },
                    line_height: LengthEmu::new(2),
                },
                surfaces: Vec::new(),
                nodes: Vec::new(),
                lines: Vec::new(),
                origin_mapping: Vec::new(),
                line_origin_mapping: Vec::new(),
                diagnostics: Vec::new(),
            },
            node_paints: Vec::new(),
            image_resources: Vec::new(),
            font: CurrentFixedPdfFontV1 {
                fingerprint_sha256: "font".into(),
                face_index: 0,
                bytes: vec![1, 2, 3],
            },
        })
        .expect("serialize current fixed-PDF input");
        assert_eq!(
            encoded["protocol_version"],
            CURRENT_FIXED_PDF_RESOURCE_INPUT_V1
        );
        assert_eq!(encoded["binding"], binding);
        let text = serde_json::to_string(&encoded).expect("serialize source-neutral packet");
        assert!(!text.contains("source_refs"));
        assert!(!text.contains("Quill"));
        assert!(!text.contains("Escher"));
    }

    #[test]
    fn explicit_font_resource_rejects_fingerprint_mismatch() {
        let mut font = test_font();
        font.expected_sha256 = "00";
        let error = validate_explicit_font_resource_v1(&font).unwrap_err();
        assert_eq!(error.code, "font_fingerprint_mismatch");
    }

    #[test]
    fn real_51318_scoped_bold_reaches_current_boolean_typography_without_color_authority() {
        let Some(root) = env::var_os("CHAPTERA_TEXT_FORMAT_FIXTURES_DIR") else {
            eprintln!(
                "CHAPTERA_TEXT_FORMAT_FIXTURES_DIR not set; dedicated Desktop text-format gate owns this test"
            );
            return;
        };
        let path = std::path::PathBuf::from(root).join("51318.pub");
        let original = fs::read(&path).expect("read pinned 51318.pub");
        let digest = Sha256::digest(&original);
        let mut digest_bytes = [0_u8; 32];
        digest_bytes.copy_from_slice(&digest);
        let source_hash = Sha256Digest::from_bytes(digest_bytes);
        let mut editor =
            open_mature_0x2c_editor(&original, source_hash).expect("open pinned 51318.pub");

        let story_id = editor
            .graph()
            .stories
            .keys()
            .copied()
            .find(|story_id| {
                editor
                    .current_text_format_overlay_v1(*story_id)
                    .err()
                    .is_some_and(|error| {
                        error
                            .to_string()
                            .contains("bounded effective direct-RGB text color is unavailable")
                    })
                    && current_story_boolean_typography_v1(&editor, *story_id)
                        .ok()
                        .is_some_and(|runs| runs.iter().any(|run| run.bold.is_some()))
            })
            .expect("51318.pub must expose a color-blocked Story with bounded Bold authority");

        let before = current_story_boolean_typography_v1(&editor, story_id)
            .expect("source boolean typography");
        let first = before
            .iter()
            .find(|run| run.bold.is_some())
            .expect("known Bold interval")
            .clone();
        let desired = !first.bold.expect("known Bold value");
        let state_hash = editor
            .current_text_format_property_state_hash_v1(story_id, FormatPropertyV1::Bold)
            .expect("scoped Bold state hash");
        editor
            .set_text_format_property_scoped_v1(
                story_id,
                first.scalar_start,
                first.scalar_end,
                FormatPropertyV1::Bold,
                FormatValueV1::Bool(desired),
                &state_hash,
            )
            .expect("set scoped Bold");

        let after = current_story_boolean_typography_v1(&editor, story_id)
            .expect("edited boolean typography");
        let changed = after
            .iter()
            .find(|run| {
                run.scalar_start <= first.scalar_start && first.scalar_end <= run.scalar_end
            })
            .expect("edited interval remains covered");
        assert_eq!(changed.bold, Some(desired));
        assert_eq!(changed.italic, first.italic);
        assert!(
            editor
                .current_text_format_overlay_v1(story_id)
                .expect_err("unknown direct-RGB color must remain unresolved")
                .to_string()
                .contains("bounded effective direct-RGB text color is unavailable")
        );

        let layout =
            build_current_story_layout_v1(&editor, story_id, "layout:scoped-bold", &test_font())
                .expect("current shaped-flow layout consumes scoped boolean typography");
        assert_eq!(layout.current_boolean_typography, after);

        editor.undo().expect("undo scoped Bold");
        assert_eq!(
            current_story_boolean_typography_v1(&editor, story_id)
                .expect("boolean typography after Undo"),
            before
        );
        editor.redo().expect("redo scoped Bold");
        assert_eq!(
            current_story_boolean_typography_v1(&editor, story_id)
                .expect("boolean typography after Redo"),
            after
        );

        let project = editor.project();
        let serialized = serde_json::to_vec(&project).expect("serialize v0.16 project");
        let roundtrip: pub_editor::EditorProject =
            serde_json::from_slice(&serialized).expect("deserialize v0.16 project");
        let mut reopened =
            open_mature_0x2c_editor(&original, source_hash).expect("fresh reopen pinned source");
        reopened
            .apply_project(&roundtrip)
            .expect("replay scoped Bold on fresh source");
        assert_eq!(
            current_story_boolean_typography_v1(&reopened, story_id)
                .expect("replayed current boolean typography"),
            after
        );
        assert_eq!(reopened.source_hash(), source_hash);
        assert_eq!(
            fs::read(path).expect("re-read pinned 51318.pub"),
            original,
            "current typography projection must not mutate source PUB bytes"
        );
    }

    #[test]
    fn real_sample_newsletter_current_story_builds_deterministically_and_rebinds_after_edit() {
        let Some(path) = env::var_os("CHAPTERA_SAMPLE_NEWSLETTER") else {
            eprintln!(
                "CHAPTERA_SAMPLE_NEWSLETTER not set; dedicated real-fixture gate owns this test"
            );
            return;
        };

        let bytes = fs::read(path).expect("read pinned SampleNewsletter");
        let digest = Sha256::digest(&bytes);
        let mut digest_bytes = [0_u8; 32];
        digest_bytes.copy_from_slice(&digest);
        let source_hash = Sha256Digest::from_bytes(digest_bytes);
        let mut editor = open_mature_0x2c_editor(&bytes, source_hash)
            .expect("open real SampleNewsletter editor");
        let font = test_font();

        let story_ids = editor.graph().stories.keys().copied().collect::<Vec<_>>();
        let (story_id, before_layout) = story_ids
            .into_iter()
            .filter(|story_id| editor.can_replace_story_text(*story_id).is_ok())
            .find_map(|story_id| {
                build_current_story_layout_v1(&editor, story_id, "layout:before", &font)
                    .ok()
                    .filter(|layout| !layout.caret_map.lines.is_empty())
                    .map(|layout| (story_id, layout))
            })
            .expect(
                "real fixture should expose one editable Story with authoritative shaped lines",
            );

        let deterministic =
            build_current_story_layout_v1(&editor, story_id, "layout:before", &font)
                .expect("repeat current Story layout");
        assert_eq!(before_layout, deterministic);
        assert_eq!(before_layout.caret_map.layout_revision_id, "layout:before");
        assert!(!before_layout.caret_map.caret_stops.is_empty());

        editor
            .replace_story_range(story_id, 0, 0, "", "X")
            .expect("insert one scalar through existing EditorSession authority");
        let after_layout = build_current_story_layout_v1(&editor, story_id, "layout:after", &font)
            .expect("rebuild shaped flow after accepted Story edit");
        assert_eq!(
            after_layout.story_scalar_len,
            before_layout.story_scalar_len + 1
        );
        assert_eq!(after_layout.caret_map.layout_revision_id, "layout:after");
        assert_eq!(editor.source_hash(), source_hash);

        editor.undo().expect("undo real Story insertion");
        let restored = build_current_story_layout_v1(&editor, story_id, "layout:undo", &font)
            .expect("rebuild shaped flow after Undo");
        assert_eq!(restored.story_scalar_len, before_layout.story_scalar_len);
        assert_eq!(editor.source_hash(), source_hash);
    }
}
