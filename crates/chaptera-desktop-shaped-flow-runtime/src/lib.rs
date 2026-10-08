mod current_typography;
mod fixed_pdf_pages;
mod fixed_pdf_resources;
mod font_resource;

use chaptera_caret_layout_feed::build_caret_map_from_shaped_flow_v1;
use chaptera_text_caret_map_adapter::ResolvedTextCaretMapV1;
use fixed_pdf_pages::bounded_authoring_slice_for_pages_v1;
#[cfg(test)]
use fixed_pdf_pages::qualified_page_set_error_v1;
use pub_editor::{
    EditorSession, EffectiveParagraphAlignmentValueV1, ImportedParagraphFlowConstraintV1,
};
use pub_layout::{
    BoundedLayoutEnvironment, BoundedLayoutProjection, BoundedParagraphFlowConstraint,
    BoundedParagraphFlowRun, BoundedShapedFlowRuntime, BoundedShapedFlowScene,
    BoundedShapingRuntime, project_bounded, resolve_bounded_shaped_flow_with_paragraph_flow,
};
use pub_line_placement::{
    LayoutPlacementContextV1, ParagraphAlignmentV1, ParagraphLinePlacementInputV1,
    ResolvedLineInputV1, resolve_paragraph_line_placement_v1,
};
use pub_model::{PageId, StoryId};
use std::{collections::BTreeMap, fmt};

pub use current_typography::{
    DesktopCurrentBooleanTypographyRunV1, current_story_boolean_typography_v1,
};
pub use fixed_pdf_resources::{
    CurrentFixedPdfFontV1, CurrentFixedPdfNodePaintV1, CurrentFixedPdfResourceInputV1,
    CurrentFixedPdfStrokeV1, build_current_fixed_pdf_resource_input_for_pages_v1,
    build_current_fixed_pdf_resource_input_v1,
};
pub use font_resource::{ExplicitDesktopFontResourceV1, validate_explicit_font_resource_v1};

pub const DESKTOP_SHAPED_FLOW_RUNTIME_V1: &str = "chaptera.desktop-shaped-flow-runtime.v1";
pub const CURRENT_FIXED_PDF_RESOURCE_INPUT_V1: &str =
    "chaptera.current-fixed-pdf-resource-input.v1";

fn retain_story_shaping_scope_v1(
    projection: &mut BoundedLayoutProjection,
    story_id: StoryId,
) -> Result<(), DesktopShapedFlowRuntimeError> {
    projection.stories.retain(|story| story.origin == story_id);
    if projection.stories.len() != 1 {
        return Err(DesktopShapedFlowRuntimeError::new(
            "story_missing",
            "requested Story is absent from bounded layout projection",
        ));
    }

    // Scope only the expensive shaped-text loop. Keep StoryFrames and every
    // geometry/projection diagnostic input unchanged for this slice.
    Ok(())
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

fn effective_line_alignment_v1(
    editor: &EditorSession,
    story_id: StoryId,
    scalar_start: u32,
    consumed_scalar_end: u32,
) -> Result<Option<ParagraphAlignmentV1>, DesktopShapedFlowRuntimeError> {
    let paragraphs = match editor.imported_paragraphs_v1() {
        Ok(paragraphs) => paragraphs,
        Err(_) => return Ok(None),
    };
    let start = u64::from(scalar_start);
    let end = u64::from(consumed_scalar_end);
    let Some(paragraph) = paragraphs.iter().find(|paragraph| {
        paragraph.story_id == story_id
            && paragraph.range.start <= start
            && start < paragraph.range.end
            && end <= paragraph.range.end
    }) else {
        return Ok(None);
    };

    let effective = editor
        .effective_paragraph_alignment_v1(paragraph.paragraph_id)
        .map_err(|error| {
            DesktopShapedFlowRuntimeError::new(
                "paragraph_alignment_projection_failed",
                format!("current ParagraphId alignment could not be resolved: {error}"),
            )
        })?;
    Ok(rigid_line_alignment_v1(effective.effective))
}

fn rigid_line_alignment_v1(
    effective: Option<EffectiveParagraphAlignmentValueV1>,
) -> Option<ParagraphAlignmentV1> {
    match effective {
        Some(EffectiveParagraphAlignmentValueV1::Left) => Some(ParagraphAlignmentV1::Left),
        Some(EffectiveParagraphAlignmentValueV1::Center) => Some(ParagraphAlignmentV1::Center),
        Some(EffectiveParagraphAlignmentValueV1::Right) => Some(ParagraphAlignmentV1::Right),
        Some(EffectiveParagraphAlignmentValueV1::Justify)
        | Some(EffectiveParagraphAlignmentValueV1::InterWord)
        | Some(EffectiveParagraphAlignmentValueV1::Distribute)
        | None => None,
    }
}

fn current_story_line_offsets_v1(
    editor: &EditorSession,
    story_id: StoryId,
    layout_revision_id: &str,
    shaped_flow: &BoundedShapedFlowScene,
) -> Result<BTreeMap<u32, i64>, DesktopShapedFlowRuntimeError> {
    let mut source_lines = shaped_flow
        .lines
        .iter()
        .filter(|line| line.story_origin == story_id)
        .collect::<Vec<_>>();
    source_lines.sort_by_key(|line| (line.scalar_start, line.frame_origin, line.frame_line_index));

    let mut offsets = BTreeMap::new();
    for (ordinal, line) in source_lines.into_iter().enumerate() {
        let Some(alignment) = effective_line_alignment_v1(
            editor,
            story_id,
            line.scalar_start,
            line.consumed_scalar_end,
        )?
        else {
            continue;
        };
        let frame = shaped_flow
            .nodes
            .iter()
            .find(|node| node.origin == line.frame_origin)
            .ok_or_else(|| {
                DesktopShapedFlowRuntimeError::new(
                    "paragraph_alignment_frame_missing",
                    "paragraph-aligned shaped line has no current frame geometry",
                )
            })?;
        let placement = resolve_paragraph_line_placement_v1(&ParagraphLinePlacementInputV1 {
            context: LayoutPlacementContextV1 {
                authoring_revision: layout_revision_id.to_owned(),
                layout_environment_fingerprint: DESKTOP_SHAPED_FLOW_RUNTIME_V1.to_owned(),
            },
            alignment,
            story_overset: false,
            lines: vec![ResolvedLineInputV1 {
                line_index: 0,
                story_id: story_id.as_canonical().to_string(),
                frame_node_id: line.frame_origin.as_canonical().to_string(),
                frame_line_index: line.frame_line_index,
                scalar_start: line.scalar_start,
                scalar_end: line.scalar_end,
                content_leading_x_emu: 0,
                content_width_emu: frame.bounds.width.get(),
                measured_width_emu: line.measured_width.get(),
            }],
        })
        .map_err(|error| {
            DesktopShapedFlowRuntimeError::new(
                "paragraph_alignment_placement_failed",
                format!("current paragraph line placement failed: {error}"),
            )
        })?;
        let offset = placement
            .lines
            .first()
            .expect("one-line placement returns one line")
            .line_origin_x_emu;
        offsets.insert(
            u32::try_from(ordinal).map_err(|_| {
                DesktopShapedFlowRuntimeError::new(
                    "line_count_overflow",
                    "paragraph-aligned shaped line ordinal exceeds u32",
                )
            })?,
            offset,
        );
    }
    Ok(offsets)
}

fn apply_line_offsets_to_caret_map_v1(
    caret_map: &mut ResolvedTextCaretMapV1,
    offsets: &BTreeMap<u32, i64>,
) -> Result<(), DesktopShapedFlowRuntimeError> {
    let shifted = |value: i64, offset: i64| {
        value.checked_add(offset).ok_or_else(|| {
            DesktopShapedFlowRuntimeError::new(
                "metric_overflow",
                "paragraph alignment offset overflowed caret geometry",
            )
        })
    };

    for line in &mut caret_map.lines {
        let Some(offset) = offsets.get(&line.flow_ordinal).copied() else {
            continue;
        };
        for cluster in &mut line.clusters {
            cluster.page_x_start_emu = shifted(cluster.page_x_start_emu, offset)?;
            cluster.page_x_end_emu = shifted(cluster.page_x_end_emu, offset)?;
            cluster.frame_x_start_emu = shifted(cluster.frame_x_start_emu, offset)?;
            cluster.frame_x_end_emu = shifted(cluster.frame_x_end_emu, offset)?;
            for stop in &mut cluster.internal_caret_stops {
                stop.page_x_emu = shifted(stop.page_x_emu, offset)?;
                stop.frame_x_emu = shifted(stop.frame_x_emu, offset)?;
            }
        }
    }
    for stop in &mut caret_map.caret_stops {
        let Some(offset) = offsets.get(&stop.flow_ordinal).copied() else {
            continue;
        };
        stop.page_x_emu = shifted(stop.page_x_emu, offset)?;
        stop.frame_x_emu = shifted(stop.frame_x_emu, offset)?;
    }
    Ok(())
}

fn current_story_paragraph_flow_v1(
    editor: &EditorSession,
    story_id: StoryId,
) -> Result<Vec<BoundedParagraphFlowRun>, DesktopShapedFlowRuntimeError> {
    let bindings = editor
        .imported_paragraph_flow_constraints_v1()
        .map_err(|error| {
            DesktopShapedFlowRuntimeError::new(
                "paragraph_flow_unavailable",
                format!("imported paragraph-flow authority is unavailable: {error}"),
            )
        })?;

    let mut runs = Vec::new();
    for binding in bindings
        .into_iter()
        .filter(|binding| binding.story_id == story_id)
    {
        let scalar_start = u32::try_from(binding.range.start).map_err(|_| {
            DesktopShapedFlowRuntimeError::new(
                "paragraph_flow_range_overflow",
                "paragraph-flow scalar start exceeds the V1 u32 domain",
            )
        })?;
        let scalar_end = u32::try_from(binding.range.end).map_err(|_| {
            DesktopShapedFlowRuntimeError::new(
                "paragraph_flow_range_overflow",
                "paragraph-flow scalar end exceeds the V1 u32 domain",
            )
        })?;
        let constraint = match binding.constraint {
            ImportedParagraphFlowConstraintV1::StartInNextTextBox => {
                BoundedParagraphFlowConstraint::StartInNextTextBox
            }
            ImportedParagraphFlowConstraintV1::KeepLinesTogether => {
                BoundedParagraphFlowConstraint::KeepLinesTogether
            }
            ImportedParagraphFlowConstraintV1::KeepWithNext => {
                BoundedParagraphFlowConstraint::KeepWithNext
            }
            ImportedParagraphFlowConstraintV1::WidowControl => {
                BoundedParagraphFlowConstraint::WidowControl
            }
        };
        runs.push(BoundedParagraphFlowRun {
            story_origin: binding.story_id,
            scalar_start,
            scalar_end,
            constraint,
        });
    }

    runs.sort_by_key(|run| {
        (
            run.story_origin,
            run.scalar_start,
            run.scalar_end,
            run.constraint,
        )
    });
    Ok(runs)
}

pub fn build_current_story_layout_v1(
    editor: &EditorSession,
    story_id: StoryId,
    layout_revision_id: &str,
    font: &ExplicitDesktopFontResourceV1<'_>,
) -> Result<DesktopStoryLayoutV1, DesktopShapedFlowRuntimeError> {
    build_current_story_layout_core_v1(editor, story_id, layout_revision_id, font, None, true)
}

/// Fixed-output callers intentionally retain full projected Story shaping.
/// Their resource packet may need text for moved/resized nodes outside the
/// primary edited Story.
fn build_current_story_layout_with_pages_v1(
    editor: &EditorSession,
    story_id: StoryId,
    layout_revision_id: &str,
    font: &ExplicitDesktopFontResourceV1<'_>,
    page_ids: Option<&[PageId]>,
) -> Result<DesktopStoryLayoutV1, DesktopShapedFlowRuntimeError> {
    build_current_story_layout_core_v1(editor, story_id, layout_revision_id, font, page_ids, false)
}

fn build_current_story_layout_core_v1(
    editor: &EditorSession,
    story_id: StoryId,
    layout_revision_id: &str,
    font: &ExplicitDesktopFontResourceV1<'_>,
    page_ids: Option<&[PageId]>,
    scope_shaping_to_current_story: bool,
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
    let current_paragraph_flow = current_story_paragraph_flow_v1(editor, story_id)?;
    let authoring = match page_ids {
        Some(page_ids) => bounded_authoring_slice_for_pages_v1(editor, page_ids)?,
        None => {
            pub_viewer::bounded_authoring_slice_from_resolved(editor.graph()).map_err(|error| {
                DesktopShapedFlowRuntimeError::new(
                    "authoring_projection_failed",
                    format!("resolved graph could not enter bounded layout projection: {error}"),
                )
            })?
        }
    };
    let mut projection = project_bounded(authoring);
    if scope_shaping_to_current_story {
        retain_story_shaping_scope_v1(&mut projection, story_id)?;
    }

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

    let shaped_flow = resolve_bounded_shaped_flow_with_paragraph_flow(
        &projection,
        &runtime,
        &current_paragraph_flow,
    )
    .map_err(|error| {
        DesktopShapedFlowRuntimeError::new(
            "shaped_flow_failed",
            format!("authoritative bounded shaped flow failed: {error}"),
        )
    })?;

    let mut caret_map = build_caret_map_from_shaped_flow_v1(
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
    let line_offsets =
        current_story_line_offsets_v1(editor, story_id, layout_revision_id, &shaped_flow)?;
    apply_line_offsets_to_caret_map_v1(&mut caret_map, &line_offsets)?;

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

#[cfg(test)]
mod tests {
    use super::*;
    use pub_editor::{FormatPropertyV1, FormatValueV1, Sha256Digest, open_mature_0x2c_editor};
    use pub_layout::font_fingerprint_sha256;
    use pub_model::{
        Affine2D, CanonicalId, EMU_PER_POINT, LengthEmu, NodeId, Page, RectEmu, Size2D, Story,
        StoryFrame,
    };
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

    fn test_canonical_id(byte: u8) -> CanonicalId {
        CanonicalId::from_bytes([byte; 16])
    }

    #[test]
    fn story_shaping_scope_removes_unrelated_text_but_preserves_geometry_projection() {
        let target_story = StoryId::from_canonical(test_canonical_id(1));
        let other_story = StoryId::from_canonical(test_canonical_id(2));
        let target_frame = NodeId::from_canonical(test_canonical_id(3));
        let other_frame = NodeId::from_canonical(test_canonical_id(4));
        let page_id = PageId::from_canonical(test_canonical_id(5));
        let page_origin = page_id.into_canonical();

        let authoring = pub_layout::BoundedAuthoringSlice {
            pages: vec![Page {
                id: page_id,
                size: Size2D::new(LengthEmu::new(1000), LengthEmu::new(1000)),
                bleed: None,
                margins: None,
                children: vec![target_frame, other_frame],
                extensions: Vec::new(),
            }],
            node_geometry: vec![
                pub_layout::BoundedNodeGeometryInput {
                    node_id: target_frame,
                    parent_origin: page_origin,
                    bounds: RectEmu::new(
                        LengthEmu::ZERO,
                        LengthEmu::ZERO,
                        LengthEmu::new(400),
                        LengthEmu::new(400),
                    ),
                    transform: Affine2D::identity(),
                },
                pub_layout::BoundedNodeGeometryInput {
                    node_id: other_frame,
                    parent_origin: page_origin,
                    bounds: RectEmu::new(
                        LengthEmu::new(500),
                        LengthEmu::ZERO,
                        LengthEmu::new(400),
                        LengthEmu::new(400),
                    ),
                    transform: Affine2D::identity(),
                },
            ],
            stories: vec![
                Story {
                    id: target_story,
                    text: "target".to_owned(),
                    paragraphs: Vec::new(),
                    runs: Vec::new(),
                    fields: Vec::new(),
                    hyperlinks: Vec::new(),
                    source_refs: Vec::new(),
                },
                Story {
                    id: other_story,
                    text: "unrelated".to_owned(),
                    paragraphs: Vec::new(),
                    runs: Vec::new(),
                    fields: Vec::new(),
                    hyperlinks: Vec::new(),
                    source_refs: Vec::new(),
                },
            ],
            story_frames: vec![
                StoryFrame {
                    story_id: target_story,
                    frame_id: target_frame,
                    ordinal: 0,
                    previous: None,
                    next: None,
                },
                StoryFrame {
                    story_id: other_story,
                    frame_id: other_frame,
                    ordinal: 0,
                    previous: None,
                    next: None,
                },
            ],
            tables: Vec::new(),
            guides: Vec::new(),
            unknown_layout_state: Vec::new(),
        };

        let mut projection = project_bounded(authoring);
        let pages_before = projection.pages.clone();
        let geometry_before = projection.node_geometry.clone();
        let frames_before = projection.story_frames.clone();
        let tables_before = projection.tables.clone();
        let guides_before = projection.guides.clone();
        let diagnostics_before = projection.diagnostics.clone();

        retain_story_shaping_scope_v1(&mut projection, target_story)
            .expect("retain target Story shaping scope");

        assert_eq!(projection.stories.len(), 1);
        assert_eq!(projection.stories[0].origin, target_story);
        assert_eq!(projection.story_frames, frames_before);
        assert_eq!(projection.pages, pages_before);
        assert_eq!(projection.node_geometry, geometry_before);
        assert_eq!(projection.tables, tables_before);
        assert_eq!(projection.guides, guides_before);
        assert_eq!(projection.diagnostics, diagnostics_before);
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
    fn qualified_fixed_pdf_page_set_rejects_empty_projection() {
        let bytes = b"not-a-real-pub";
        let digest = Sha256::digest(bytes);
        let mut digest_bytes = [0_u8; 32];
        digest_bytes.copy_from_slice(&digest);
        let source_hash = Sha256Digest::from_bytes(digest_bytes);
        let editor = open_mature_0x2c_editor(bytes, source_hash);
        assert!(
            editor.is_err(),
            "test precondition: invalid source stays invalid"
        );

        // The page-set guard is intentionally before layout projection; keep a
        // direct source-free assertion on the stable error contract by using a
        // tiny helper rather than requiring a network/native fixture here.
        assert_eq!(
            qualified_page_set_error_v1(&[]).unwrap_err().code,
            "qualified_pages_missing"
        );
    }

    #[test]
    fn explicit_font_resource_rejects_fingerprint_mismatch() {
        let mut font = test_font();
        font.expected_sha256 = "00";
        let error = validate_explicit_font_resource_v1(&font).unwrap_err();
        assert_eq!(error.code, "font_fingerprint_mismatch");
    }

    #[test]
    fn real_51318_scoped_bold_reaches_current_boolean_typography() {
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
                current_story_boolean_typography_v1(&editor, *story_id)
                    .ok()
                    .is_some_and(|runs| runs.iter().any(|run| run.bold.is_some()))
            })
            .expect("51318.pub must expose a Story with bounded Bold authority");

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
            editor.graph().stories.len() > 1,
            "51318 must remain a multi-Story witness for Story-local shaping"
        );
        let shaping_authoring = pub_viewer::bounded_authoring_slice_from_resolved(editor.graph())
            .expect("project multi-Story fixture before shaping scope");
        let mut shaping_scope = project_bounded(shaping_authoring);
        let pages_before = shaping_scope.pages.clone();
        let geometry_before = shaping_scope.node_geometry.clone();
        let frames_before = shaping_scope.story_frames.clone();
        let tables_before = shaping_scope.tables.clone();
        let diagnostics_before = shaping_scope.diagnostics.clone();
        retain_story_shaping_scope_v1(&mut shaping_scope, story_id)
            .expect("retain current Story shaping scope");
        assert_eq!(shaping_scope.stories.len(), 1);
        assert_eq!(shaping_scope.stories[0].origin, story_id);
        assert_eq!(
            shaping_scope.story_frames, frames_before,
            "Story-local shaping slice must not change frame projection yet"
        );
        assert_eq!(
            shaping_scope.pages, pages_before,
            "Story-local shaping slice must not change page geometry scope yet"
        );
        assert_eq!(
            shaping_scope.node_geometry, geometry_before,
            "Story-local shaping slice must not change node geometry scope yet"
        );
        assert_eq!(
            shaping_scope.tables, tables_before,
            "Story-local shaping slice must not change table projection scope yet"
        );
        assert_eq!(
            shaping_scope.diagnostics, diagnostics_before,
            "Story-local shaping slice must not change projection diagnostics"
        );

        let layout =
            build_current_story_layout_v1(&editor, story_id, "layout:scoped-bold", &test_font())
                .expect("current shaped-flow layout consumes scoped boolean typography");
        assert_eq!(layout.current_boolean_typography, after);
        assert!(
            layout
                .shaped_flow
                .lines
                .iter()
                .all(|line| line.story_origin == story_id),
            "current Story layout must not shape unrelated Stories"
        );

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
    fn ordinary_justify_never_falls_back_to_rigid_lcr_line_offset() {
        assert_eq!(
            rigid_line_alignment_v1(Some(EffectiveParagraphAlignmentValueV1::Justify)),
            None
        );
        assert_eq!(
            rigid_line_alignment_v1(Some(EffectiveParagraphAlignmentValueV1::Left)),
            Some(ParagraphAlignmentV1::Left)
        );
    }

    fn expected_paragraph_offset_v1(
        alignment: ParagraphAlignmentV1,
        content_width_emu: i64,
        measured_width_emu: i64,
    ) -> i64 {
        let remaining = content_width_emu - measured_width_emu;
        match alignment {
            ParagraphAlignmentV1::Left => 0,
            ParagraphAlignmentV1::Center => remaining / 2,
            ParagraphAlignmentV1::Right => remaining,
        }
    }

    fn assert_caret_flow_shift_v1(
        raw: &ResolvedTextCaretMapV1,
        shifted: &ResolvedTextCaretMapV1,
        flow_ordinal: u32,
        expected_offset: i64,
    ) {
        let raw_line = raw
            .lines
            .iter()
            .find(|line| line.flow_ordinal == flow_ordinal)
            .expect("raw caret line");
        let shifted_line = shifted
            .lines
            .iter()
            .find(|line| line.flow_ordinal == flow_ordinal)
            .expect("shifted caret line");
        assert_eq!(raw_line.clusters.len(), shifted_line.clusters.len());
        for (raw_cluster, shifted_cluster) in raw_line.clusters.iter().zip(&shifted_line.clusters) {
            assert_eq!(
                shifted_cluster.page_x_start_emu,
                raw_cluster.page_x_start_emu + expected_offset
            );
            assert_eq!(
                shifted_cluster.page_x_end_emu,
                raw_cluster.page_x_end_emu + expected_offset
            );
            assert_eq!(
                shifted_cluster.frame_x_start_emu,
                raw_cluster.frame_x_start_emu + expected_offset
            );
            assert_eq!(
                shifted_cluster.frame_x_end_emu,
                raw_cluster.frame_x_end_emu + expected_offset
            );
            assert_eq!(
                raw_cluster.internal_caret_stops.len(),
                shifted_cluster.internal_caret_stops.len()
            );
            for (raw_stop, shifted_stop) in raw_cluster
                .internal_caret_stops
                .iter()
                .zip(&shifted_cluster.internal_caret_stops)
            {
                assert_eq!(
                    shifted_stop.page_x_emu,
                    raw_stop.page_x_emu + expected_offset
                );
                assert_eq!(
                    shifted_stop.frame_x_emu,
                    raw_stop.frame_x_emu + expected_offset
                );
            }
        }

        let raw_stops = raw
            .caret_stops
            .iter()
            .filter(|stop| stop.flow_ordinal == flow_ordinal)
            .collect::<Vec<_>>();
        let shifted_stops = shifted
            .caret_stops
            .iter()
            .filter(|stop| stop.flow_ordinal == flow_ordinal)
            .collect::<Vec<_>>();
        assert_eq!(raw_stops.len(), shifted_stops.len());
        assert!(!raw_stops.is_empty());
        for (raw_stop, shifted_stop) in raw_stops.into_iter().zip(shifted_stops) {
            assert_eq!(
                shifted_stop.page_x_emu,
                raw_stop.page_x_emu + expected_offset
            );
            assert_eq!(
                shifted_stop.frame_x_emu,
                raw_stop.frame_x_emu + expected_offset
            );
        }
    }

    #[test]
    fn real_carlton_paragraph_alignment_moves_caret_and_clear_restores_source_geometry() {
        let Some(path) = env::var_os("CHAPTERA_CARLTON_PUB") else {
            eprintln!("CHAPTERA_CARLTON_PUB not set; dedicated shaped-flow gate owns this test");
            return;
        };

        let bytes = fs::read(path).expect("read pinned Carlton March PUB");
        let digest = Sha256::digest(&bytes);
        let mut digest_bytes = [0_u8; 32];
        digest_bytes.copy_from_slice(&digest);
        let source_hash = Sha256Digest::from_bytes(digest_bytes);
        let mut editor =
            open_mature_0x2c_editor(&bytes, source_hash).expect("open Carlton EditorSession");
        let font = test_font();
        let imported = editor
            .imported_paragraphs_v1()
            .expect("project canonical Carlton ParagraphIds");

        let mut candidate = None;
        for story_id in editor.graph().stories.keys().copied().collect::<Vec<_>>() {
            if editor.can_enter_story_text_session(story_id).is_err() {
                continue;
            }
            let Ok(baseline) =
                build_current_story_layout_v1(&editor, story_id, "paragraph:source", &font)
            else {
                continue;
            };
            if baseline.caret_map.lines.is_empty() {
                continue;
            }
            let raw = build_caret_map_from_shaped_flow_v1(
                &baseline.shaped_flow,
                "paragraph:raw",
                story_id,
                baseline.story_scalar_len,
            )
            .expect("raw caret map from the same shaped flow");

            let mut lines = baseline
                .shaped_flow
                .lines
                .iter()
                .filter(|line| line.story_origin == story_id)
                .collect::<Vec<_>>();
            lines.sort_by_key(|line| (line.scalar_start, line.frame_origin, line.frame_line_index));

            for (ordinal, line) in lines.into_iter().enumerate() {
                let start = u64::from(line.scalar_start);
                let end = u64::from(line.consumed_scalar_end);
                let Some(paragraph) = imported.iter().find(|paragraph| {
                    paragraph.story_id == story_id
                        && paragraph.range.start <= start
                        && start < paragraph.range.end
                        && end <= paragraph.range.end
                }) else {
                    continue;
                };
                let Some(frame) = baseline
                    .shaped_flow
                    .nodes
                    .iter()
                    .find(|node| node.origin == line.frame_origin)
                else {
                    continue;
                };
                let remaining = frame.bounds.width.get() - line.measured_width.get();
                if remaining <= 2 {
                    continue;
                }
                let Ok(flow_ordinal) = u32::try_from(ordinal) else {
                    continue;
                };
                if !raw.lines.iter().any(|caret_line| {
                    caret_line.flow_ordinal == flow_ordinal && !caret_line.clusters.is_empty()
                }) {
                    continue;
                }
                let Ok(effective) = editor.effective_paragraph_alignment_v1(paragraph.paragraph_id)
                else {
                    continue;
                };
                let base_alignment = match effective.effective {
                    Some(EffectiveParagraphAlignmentValueV1::Left) => ParagraphAlignmentV1::Left,
                    Some(EffectiveParagraphAlignmentValueV1::Center) => {
                        ParagraphAlignmentV1::Center
                    }
                    Some(EffectiveParagraphAlignmentValueV1::Right) => ParagraphAlignmentV1::Right,
                    _ => continue,
                };

                let content_width_emu = frame.bounds.width.get();
                let measured_width_emu = line.measured_width.get();
                candidate = Some((
                    story_id,
                    paragraph.paragraph_id,
                    baseline,
                    raw,
                    flow_ordinal,
                    content_width_emu,
                    measured_width_emu,
                    base_alignment,
                ));
                break;
            }
            if candidate.is_some() {
                break;
            }
        }

        let (
            story_id,
            paragraph_id,
            baseline,
            raw,
            flow_ordinal,
            content_width_emu,
            measured_width_emu,
            base_alignment,
        ) = candidate
            .expect("Carlton must expose one editable paragraph line with horizontal slack");

        assert_caret_flow_shift_v1(
            &raw,
            &baseline.caret_map,
            flow_ordinal,
            expected_paragraph_offset_v1(base_alignment, content_width_emu, measured_width_emu),
        );

        let mut paragraph_history_active = false;
        for (alignment, authored) in [
            (
                ParagraphAlignmentV1::Left,
                pub_editor::AuthoredParagraphAlignmentValueV1::Left,
            ),
            (
                ParagraphAlignmentV1::Center,
                pub_editor::AuthoredParagraphAlignmentValueV1::Center,
            ),
            (
                ParagraphAlignmentV1::Right,
                pub_editor::AuthoredParagraphAlignmentValueV1::Right,
            ),
        ] {
            let current = editor
                .effective_paragraph_alignment_v1(paragraph_id)
                .expect("current paragraph alignment");
            let already = matches!(
                (current.effective, alignment),
                (
                    Some(EffectiveParagraphAlignmentValueV1::Left),
                    ParagraphAlignmentV1::Left
                ) | (
                    Some(EffectiveParagraphAlignmentValueV1::Center),
                    ParagraphAlignmentV1::Center
                ) | (
                    Some(EffectiveParagraphAlignmentValueV1::Right),
                    ParagraphAlignmentV1::Right
                )
            );
            if !already {
                editor
                    .set_paragraph_alignment_override_v1(vec![paragraph_id], authored)
                    .expect("set current paragraph alignment");
                paragraph_history_active = true;
            }

            assert!(editor.can_enter_story_text_session(story_id).is_ok());
            if paragraph_history_active {
                assert_eq!(
                    editor
                        .can_replace_story_text(story_id)
                        .expect_err("ParagraphId history must fence Story text mutation")
                        .code(),
                    "paragraph_alignment_lifecycle_unsupported"
                );
            }

            let layout = build_current_story_layout_v1(
                &editor,
                story_id,
                &format!("paragraph:{alignment:?}"),
                &font,
            )
            .expect("rebuild current paragraph layout");
            assert_caret_flow_shift_v1(
                &raw,
                &layout.caret_map,
                flow_ordinal,
                expected_paragraph_offset_v1(alignment, content_width_emu, measured_width_emu),
            );
        }

        let restore_from = if base_alignment != ParagraphAlignmentV1::Center {
            pub_editor::AuthoredParagraphAlignmentValueV1::Center
        } else {
            pub_editor::AuthoredParagraphAlignmentValueV1::Left
        };
        editor
            .set_paragraph_alignment_override_v1(vec![paragraph_id], restore_from)
            .expect("establish explicit override before Clear");
        editor
            .clear_paragraph_alignment_override_v1(vec![paragraph_id])
            .expect("Clear paragraph alignment override");
        assert!(editor.can_enter_story_text_session(story_id).is_ok());
        assert_eq!(
            editor
                .can_replace_story_text(story_id)
                .expect_err("Clear must retain the ParagraphId lifecycle fence")
                .code(),
            "paragraph_alignment_lifecycle_unsupported"
        );

        let cleared = build_current_story_layout_v1(&editor, story_id, "paragraph:clear", &font)
            .expect("rebuild paragraph layout after Clear");
        assert_caret_flow_shift_v1(
            &raw,
            &cleared.caret_map,
            flow_ordinal,
            expected_paragraph_offset_v1(base_alignment, content_width_emu, measured_width_emu),
        );
        assert_eq!(editor.source_hash(), source_hash);
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
