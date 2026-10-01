//! Source-neutral render-plan boundary for Chaptera document surfaces.
//!
//! The plan answers only what the current Viewer document intends to paint.
//! It deliberately contains no egui types, EditorSession state, parser-private
//! carrier names, source offsets, or mutable authoring commands.

#[cfg(feature = "projected-scene-instances")]
use chaptera_scene_instance::{SceneInstanceV1, SceneProjectionKindV1};
use pub_layout::{
    BoundedBreakKind, BoundedLayoutEnvironment, BoundedLayoutProjection, BoundedShapedFlowRuntime,
    BoundedShapingRuntime, ProjectedNodeGeometry, ProjectedPage, ProjectedStory,
    ProjectedStoryFrame, break_policy_for_shaped_text, font_fingerprint_sha256,
    resolve_bounded_shaped_flow, shape_bounded_ltr_segment,
};
#[cfg(feature = "projected-scene-instances")]
use pub_model::CanonicalId;
use pub_model::{
    Affine2D, LengthEmu, NodeId, PageId, RectEmu, ResourceId, Size2D, StoryId, TableCellId,
};
use pub_viewer::ViewerGeometryDocument;
#[cfg(feature = "projected-scene-instances")]
use pub_viewer::ViewerProjectedSceneInstanceV1;
use serde::{Deserialize, Serialize};
use std::fmt;

pub const PAGE_RENDER_PLAN_SCHEMA_V1: &str = "chaptera.page-render-plan.v1";
pub const SHARED_TEXT_LAYOUT_REVISION_V1: &str = "chaptera.viewer.shared-text-layout.v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageRenderPlanV1 {
    pub schema_version: String,
    pub page_id: PageId,
    pub page_size: Size2D,
    pub nodes: Vec<NodeRenderPlanV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeRenderPlanV1 {
    pub node_id: NodeId,
    #[cfg(feature = "projected-scene-instances")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub projected_scene_instance: Option<SceneInstanceV1>,
    pub bounds: RectEmu,
    pub transform: Affine2D,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub solid_fill_rgb: Option<[u8; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub solid_line: Option<RenderSolidLineV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<RenderImageRefV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<RenderTextFragmentV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub table: Option<RenderTableV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenderTableV1 {
    pub story_id: StoryId,
    pub rows: u32,
    pub columns: u32,
    pub cells: Vec<RenderTableCellV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenderTableCellV1 {
    pub id: TableCellId,
    pub row: u32,
    pub column: u32,
    #[serde(
        default = "default_render_table_span",
        skip_serializing_if = "render_table_span_is_one"
    )]
    pub row_span: u32,
    #[serde(
        default = "default_render_table_span",
        skip_serializing_if = "render_table_span_is_one"
    )]
    pub column_span: u32,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bounds: Option<RectEmu>,
}

fn default_render_table_span() -> u32 {
    1
}

fn render_table_span_is_one(value: &u32) -> bool {
    *value == 1
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenderSolidLineV1 {
    pub rgb: [u8; 3],
    pub width_emu: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenderImageSourceWindowV1 {
    pub left_q16: i64,
    pub top_q16: i64,
    pub right_q16: i64,
    pub bottom_q16: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenderImageRefV1 {
    pub resource_id: ResourceId,
    pub mime: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_window: Option<RenderImageSourceWindowV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenderTextFragmentV1 {
    pub story_id: StoryId,
    pub scalar_start: u32,
    pub scalar_end: u32,
    pub text: String,
    pub line_count: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub typography: Vec<RenderTypographyRunV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backend_font_resource_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layout: Option<RenderTextLayoutV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenderTypographyRunV1 {
    pub scalar_start: u32,
    pub scalar_end: u32,
    pub source_font_name: String,
    pub text_size_emu: u32,
    pub font_inherited: bool,
    pub size_inherited: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct ExplicitRenderTextFontResourceV1<'a> {
    pub resource_id: &'a str,
    pub expected_sha256: &'a str,
    pub face_index: u32,
    pub default_font_size_emu: i64,
    pub default_line_height_emu: i64,
    pub bytes: &'a [u8],
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenderTextLayoutV1 {
    pub disposition: RenderTextLayoutDispositionV1,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lines: Vec<RenderResolvedTextLineV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RenderTextLayoutDispositionV1 {
    SharedResolved {
        font_resource_id: String,
        font_fingerprint_sha256: String,
        font_size_emu: i64,
        line_height_emu: i64,
    },
    BackendFallback {
        reason: RenderTextLayoutFallbackReasonV1,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RenderTextLayoutFallbackReasonV1 {
    StoryMissing,
    StoryExtentMismatch,
    SingleFrameRequired,
    FrameGeometryInvalid,
    TypographyCoverageGap,
    MixedTypographySize,
    FontResourceInvalid,
    FontFingerprintMismatch,
    SharedLayoutFailed,
    SharedLayoutIncomplete,
}

impl RenderTextLayoutFallbackReasonV1 {
    pub const fn code(self) -> &'static str {
        match self {
            Self::StoryMissing => "story_missing",
            Self::StoryExtentMismatch => "story_extent_mismatch",
            Self::SingleFrameRequired => "single_frame_required",
            Self::FrameGeometryInvalid => "frame_geometry_invalid",
            Self::TypographyCoverageGap => "typography_coverage_gap",
            Self::MixedTypographySize => "mixed_typography_size",
            Self::FontResourceInvalid => "font_resource_invalid",
            Self::FontFingerprintMismatch => "font_fingerprint_mismatch",
            Self::SharedLayoutFailed => "shared_layout_failed",
            Self::SharedLayoutIncomplete => "shared_layout_incomplete",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenderResolvedTextLineV1 {
    pub line_index: u32,
    pub scalar_start: u32,
    pub scalar_end: u32,
    pub consumed_scalar_end: u32,
    pub text: String,
    pub measured_width_emu: i64,
    pub line_height_emu: i64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub spans: Vec<RenderResolvedTextSpanV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenderResolvedTextSpanV1 {
    pub scalar_start: u32,
    pub scalar_end: u32,
    pub text: String,
    pub x_offset_emu: i64,
    pub measured_width_emu: i64,
    pub font_size_emu: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenderPlanErrorV1 {
    PageIndexOutOfBounds {
        page_index: usize,
    },
    PageSurfaceMissing {
        page_id: PageId,
    },
    #[cfg(feature = "projected-scene-instances")]
    ProjectedIdentityInvalid {
        field: &'static str,
        value: String,
    },
    #[cfg(feature = "projected-scene-instances")]
    ProjectedKindUnsupported {
        instance_id: String,
    },
}

impl fmt::Display for RenderPlanErrorV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PageIndexOutOfBounds { page_index } => {
                write!(formatter, "viewer page index {page_index} is unavailable")
            }
            Self::PageSurfaceMissing { page_id } => {
                write!(formatter, "viewer page {page_id:?} has no resolved surface")
            }
            #[cfg(feature = "projected-scene-instances")]
            Self::ProjectedIdentityInvalid { field, value } => {
                write!(
                    formatter,
                    "projected {field} is not canonical identity: {value}"
                )
            }
            #[cfg(feature = "projected-scene-instances")]
            Self::ProjectedKindUnsupported { instance_id } => {
                write!(
                    formatter,
                    "projected instance {instance_id} has unsupported projection kind"
                )
            }
        }
    }
}

impl std::error::Error for RenderPlanErrorV1 {}

#[cfg(feature = "projected-scene-instances")]
fn suppress_projected_object_marker_glyphs(text: &str) -> String {
    text.chars()
        .map(|ch| if ch == '\u{FFFC}' { '\u{200B}' } else { ch })
        .collect()
}

#[cfg(feature = "projected-scene-instances")]
fn clip_render_text_at_story_scalar_end(fragment: &mut RenderTextFragmentV1, scalar_end: u32) {
    let clipped_end = fragment.scalar_end.min(scalar_end);
    if clipped_end <= fragment.scalar_start {
        fragment.scalar_end = fragment.scalar_start;
        fragment.text.clear();
        fragment.typography.clear();
        fragment.layout = None;
        fragment.line_count = 0;
        return;
    }
    if clipped_end == fragment.scalar_end {
        return;
    }

    let keep = usize::try_from(clipped_end - fragment.scalar_start)
        .expect("u32 scalar span fits usize on supported targets");
    fragment.text = fragment.text.chars().take(keep).collect();
    fragment.scalar_end = clipped_end;
    fragment.layout = None;
    fragment.line_count = 0;
    for run in &mut fragment.typography {
        run.scalar_end = run.scalar_end.min(clipped_end);
    }
    fragment
        .typography
        .retain(|run| run.scalar_start < run.scalar_end);
}

#[cfg(feature = "projected-scene-instances")]
fn parse_node_id(value: &str, field: &'static str) -> Result<NodeId, RenderPlanErrorV1> {
    value
        .parse::<CanonicalId>()
        .map(NodeId::from_canonical)
        .map_err(|_| RenderPlanErrorV1::ProjectedIdentityInvalid {
            field,
            value: value.to_owned(),
        })
}

#[cfg(feature = "projected-scene-instances")]
fn parse_story_id(value: &str, field: &'static str) -> Result<StoryId, RenderPlanErrorV1> {
    value
        .parse::<CanonicalId>()
        .map(StoryId::from_canonical)
        .map_err(|_| RenderPlanErrorV1::ProjectedIdentityInvalid {
            field,
            value: value.to_owned(),
        })
}

#[cfg(feature = "projected-scene-instances")]
fn projected_text(
    visual: &ViewerGeometryDocument,
    instance: &ViewerProjectedSceneInstanceV1,
) -> Result<Option<RenderTextFragmentV1>, RenderPlanErrorV1> {
    let Some(story_text) = instance.scene_instance.story_authority_id.as_deref() else {
        return Ok(None);
    };
    let story_id = parse_story_id(story_text, "story_authority_id")?;
    let Some(story) = visual
        .document
        .stories
        .iter()
        .find(|story| story.id == story_id)
    else {
        return Ok(None);
    };
    let scalar_end = u32::try_from(story.text.chars().count()).unwrap_or(u32::MAX);
    let typography = visual
        .typography_runs
        .iter()
        .filter(|run| run.story_id == story_id)
        .filter(|run| run.applies_to_story_text(&story.text))
        .filter_map(|run| {
            let scalar_start = run.scalar_start.min(scalar_end);
            let scalar_end = run.scalar_end.min(scalar_end);
            (scalar_start < scalar_end).then(|| RenderTypographyRunV1 {
                scalar_start,
                scalar_end,
                source_font_name: run.source_font_name.clone(),
                text_size_emu: run.text_size_emu,
                font_inherited: run.font_inherited,
                size_inherited: run.size_inherited,
            })
        })
        .collect();
    Ok(Some(RenderTextFragmentV1 {
        story_id,
        scalar_start: 0,
        scalar_end,
        text: story.text.clone(),
        line_count: 0,
        typography,
        backend_font_resource_id: None,
        layout: None,
    }))
}

pub fn build_page_render_plan_v1(
    visual: &ViewerGeometryDocument,
    page_index: usize,
) -> Result<PageRenderPlanV1, RenderPlanErrorV1> {
    let page = visual
        .document
        .pages
        .get(page_index)
        .ok_or(RenderPlanErrorV1::PageIndexOutOfBounds { page_index })?;
    let surface = visual
        .scene
        .surfaces
        .iter()
        .find(|surface| surface.origin == page.id)
        .ok_or(RenderPlanErrorV1::PageSurfaceMissing { page_id: page.id })?;

    let parent_origin = page.id.into_canonical();
    let mut nodes = visual
        .scene
        .nodes
        .iter()
        .filter(|node| node.parent_origin == parent_origin)
        .map(|node| {
            let mut text = visual
                .text_fragments
                .iter()
                .find(|fragment| fragment.frame_id == node.origin)
                .map(|fragment| RenderTextFragmentV1 {
                    story_id: fragment.story_id,
                    scalar_start: fragment.scalar_start,
                    scalar_end: fragment.scalar_end,
                    text: fragment.text.clone(),
                    line_count: fragment.line_count,
                    typography: visual
                        .typography_runs
                        .iter()
                        .filter(|run| run.story_id == fragment.story_id)
                        .filter(|run| {
                            run.applies_to_story_text(
                                visual
                                    .document
                                    .stories
                                    .iter()
                                    .find(|story| story.id == fragment.story_id)
                                    .map(|story| story.text.as_str())
                                    .unwrap_or_default(),
                            )
                        })
                        .filter_map(|run| {
                            let scalar_start = run.scalar_start.max(fragment.scalar_start);
                            let scalar_end = run.scalar_end.min(fragment.scalar_end);
                            (scalar_start < scalar_end).then(|| RenderTypographyRunV1 {
                                scalar_start,
                                scalar_end,
                                source_font_name: run.source_font_name.clone(),
                                text_size_emu: run.text_size_emu,
                                font_inherited: run.font_inherited,
                                size_inherited: run.size_inherited,
                            })
                        })
                        .collect(),
                    backend_font_resource_id: None,
                    layout: None,
                });
            #[cfg(feature = "projected-scene-instances")]
            {
                let projected_for_frame = visual.projected_instances.iter().filter(|projected| {
                    projected.scene_instance.target_page_id == page.id.as_canonical().to_string()
                        && projected.target_frame_node_id == node.origin
                });
                let mut has_projection = false;
                let mut paint_scalar_end = None::<u32>;
                for projected in projected_for_frame {
                    has_projection = true;
                    if let Some(candidate) = projected.target_frame_paint_scalar_end {
                        paint_scalar_end = Some(
                            paint_scalar_end.map_or(candidate, |current| current.min(candidate)),
                        );
                    }
                }
                if let Some(rendered) = text.as_mut() {
                    if let Some(scalar_end) = paint_scalar_end {
                        clip_render_text_at_story_scalar_end(rendered, scalar_end);
                    }
                    if has_projection {
                        rendered.text = suppress_projected_object_marker_glyphs(&rendered.text);
                    }
                }
            }
            let paint = visual
                .paints
                .iter()
                .find(|paint| paint.node_id == node.origin);
            let image = visual
                .images
                .iter()
                .find(|image| image.node_ids.contains(&node.origin))
                .map(|image| {
                    let source_window = image
                        .placements
                        .iter()
                        .find(|placement| placement.node_id == node.origin)
                        .and_then(|placement| placement.source_window.as_ref())
                        .map(|window| RenderImageSourceWindowV1 {
                            left_q16: window.left_q16,
                            top_q16: window.top_q16,
                            right_q16: window.right_q16,
                            bottom_q16: window.bottom_q16,
                        });
                    RenderImageRefV1 {
                        resource_id: image.resource_id,
                        mime: image.mime.clone(),
                        source_window,
                    }
                });
            let table = visual
                .tables
                .iter()
                .find(|table| table.node_id == node.origin)
                .map(|table| RenderTableV1 {
                    story_id: table.story_id,
                    rows: table.rows,
                    columns: table.columns,
                    cells: table
                        .cells
                        .iter()
                        .map(|cell| RenderTableCellV1 {
                            id: cell.id,
                            row: cell.address.row,
                            column: cell.address.column,
                            row_span: cell.row_span,
                            column_span: cell.column_span,
                            text: cell.text.clone(),
                            bounds: cell.bounds,
                        })
                        .collect(),
                });
            NodeRenderPlanV1 {
                node_id: node.origin,
                #[cfg(feature = "projected-scene-instances")]
                projected_scene_instance: None,
                bounds: node.bounds,
                transform: node.transform.clone(),
                solid_fill_rgb: paint.and_then(|paint| paint.solid_fill_rgb),
                solid_line: paint
                    .and_then(|paint| paint.solid_line.as_ref())
                    .map(|line| RenderSolidLineV1 {
                        rgb: line.rgb,
                        width_emu: line.width_emu,
                    }),
                image,
                text,
                table,
            }
        })
        .collect::<Vec<_>>();

    #[cfg(feature = "projected-scene-instances")]
    for projected in visual.projected_instances.iter().filter(|projected| {
        projected.scene_instance.target_page_id == page.id.as_canonical().to_string()
    }) {
        if projected.scene_instance.projection_kind != SceneProjectionKindV1::CmoStorySlot {
            return Err(RenderPlanErrorV1::ProjectedKindUnsupported {
                instance_id: projected.scene_instance.instance_id.clone(),
            });
        }
        let origin_node_id =
            parse_node_id(&projected.scene_instance.origin_node_id, "origin_node_id")?;
        let paint = visual
            .paints
            .iter()
            .find(|paint| paint.node_id == origin_node_id);
        let image = visual
            .images
            .iter()
            .find(|image| image.node_ids.contains(&origin_node_id))
            .map(|image| {
                let source_window = image
                    .placements
                    .iter()
                    .find(|placement| placement.node_id == origin_node_id)
                    .and_then(|placement| placement.source_window.as_ref())
                    .map(|window| RenderImageSourceWindowV1 {
                        left_q16: window.left_q16,
                        top_q16: window.top_q16,
                        right_q16: window.right_q16,
                        bottom_q16: window.bottom_q16,
                    });
                RenderImageRefV1 {
                    resource_id: image.resource_id,
                    mime: image.mime.clone(),
                    source_window,
                }
            });
        let text = projected_text(visual, projected)?;
        let has_shape_paint = paint.is_some_and(|paint| {
            paint.solid_fill_rgb.is_some() || paint.solid_line.is_some()
        });
        // A source-backed text content box may replace render-plan bounds only
        // for text-only projected carriers. Viewer keeps canonical carrier
        // geometry separately; projected carriers with paint/images stay on
        // shape bounds rather than conflating inner margins with object bounds.
        let render_bounds = if text.is_some() && image.is_none() && !has_shape_paint {
            projected.text_content_bounds.unwrap_or(projected.bounds)
        } else {
            projected.bounds
        };
        let node = NodeRenderPlanV1 {
            node_id: origin_node_id,
            projected_scene_instance: Some(projected.scene_instance.clone()),
            bounds: render_bounds,
            transform: projected.transform.clone(),
            solid_fill_rgb: paint.and_then(|paint| paint.solid_fill_rgb),
            solid_line: paint
                .and_then(|paint| paint.solid_line.as_ref())
                .map(|line| RenderSolidLineV1 {
                    rgb: line.rgb,
                    width_emu: line.width_emu,
                }),
            image,
            text,
            table: None,
        };

        let insert_at = nodes
            .iter()
            .position(|candidate| {
                candidate.projected_scene_instance.is_none()
                    && candidate.node_id == projected.target_frame_node_id
            })
            .map(|index| index + 1)
            .unwrap_or(nodes.len());
        nodes.insert(insert_at, node);
    }

    Ok(PageRenderPlanV1 {
        schema_version: PAGE_RENDER_PLAN_SCHEMA_V1.to_owned(),
        page_id: page.id,
        page_size: surface.size,
        nodes,
    })
}

struct RenderTextLayoutTargetV1 {
    page_id: PageId,
    page_size: Size2D,
    node_id: NodeId,
    projected_target_frame_node_id: Option<NodeId>,
    bounds: RectEmu,
    transform: Affine2D,
}

pub fn build_page_render_plan_with_text_layout_v1(
    visual: &ViewerGeometryDocument,
    page_index: usize,
    font: &ExplicitRenderTextFontResourceV1<'_>,
) -> Result<PageRenderPlanV1, RenderPlanErrorV1> {
    build_page_render_plan_with_text_layout_resolver_v1(visual, page_index, font, |_| None)
}

pub fn build_page_render_plan_with_text_layout_resolver_v1<'a, F>(
    visual: &ViewerGeometryDocument,
    page_index: usize,
    fallback_font: &ExplicitRenderTextFontResourceV1<'a>,
    mut resolve_font: F,
) -> Result<PageRenderPlanV1, RenderPlanErrorV1>
where
    F: FnMut(&RenderTextFragmentV1) -> Option<ExplicitRenderTextFontResourceV1<'a>>,
{
    let mut plan = build_page_render_plan_v1(visual, page_index)?;
    let page_id = plan.page_id;
    let page_size = plan.page_size;

    for node in &mut plan.nodes {
        let Some(fragment) = node.text.as_mut() else {
            continue;
        };
        let projected_target_frame_node_id = {
            #[cfg(feature = "projected-scene-instances")]
            {
                node.projected_scene_instance.as_ref().and_then(|instance| {
                    visual
                        .projected_instances
                        .iter()
                        .find(|projected| {
                            projected.scene_instance.instance_id == instance.instance_id
                        })
                        .map(|projected| projected.target_frame_node_id)
                })
            }
            #[cfg(not(feature = "projected-scene-instances"))]
            {
                None
            }
        };
        let target = RenderTextLayoutTargetV1 {
            page_id,
            page_size,
            node_id: node.node_id,
            projected_target_frame_node_id,
            bounds: node.bounds,
            transform: node.transform.clone(),
        };
        let resolved_font = resolve_font(fragment);
        fragment.backend_font_resource_id = resolved_font
            .as_ref()
            .map(|font| font.resource_id.to_owned());
        let font = resolved_font.as_ref().unwrap_or(fallback_font);
        fragment.layout = Some(resolve_text_layout_v1(visual, target, fragment, font));
    }

    Ok(plan)
}

fn fallback_layout(reason: RenderTextLayoutFallbackReasonV1) -> RenderTextLayoutV1 {
    RenderTextLayoutV1 {
        disposition: RenderTextLayoutDispositionV1::BackendFallback { reason },
        lines: Vec::new(),
    }
}

fn admitted_layout_frame_ordinal(
    visual: &ViewerGeometryDocument,
    story_id: StoryId,
    node_id: NodeId,
    projected_target_frame_node_id: Option<NodeId>,
) -> Result<u32, RenderTextLayoutFallbackReasonV1> {
    let Some(target_frame_node_id) = projected_target_frame_node_id else {
        let mut frames = visual
            .story_frames
            .iter()
            .filter(|frame| frame.story_id == story_id);
        let Some(frame) = frames.next() else {
            return Err(RenderTextLayoutFallbackReasonV1::SingleFrameRequired);
        };
        if frames.next().is_some() || frame.frame_id != node_id {
            return Err(RenderTextLayoutFallbackReasonV1::SingleFrameRequired);
        }
        return Ok(frame.ordinal);
    };

    // Projected Cmo carrier Stories can originate on pages excluded by the
    // customer-page presentation profile, so their source StoryFrame is not
    // present in Viewer story_frames. Canonical carrier identity instead comes
    // from SceneInstanceV1 + story_authority_id. Admission here validates only
    // the separate host target-frame topology; the layout frame itself keeps
    // the carrier node/story identity and uses projected slot bounds.
    let mut target_frame_matches = visual
        .story_frames
        .iter()
        .filter(|candidate| candidate.frame_id == target_frame_node_id);
    let Some(target_frame) = target_frame_matches.next() else {
        return Err(RenderTextLayoutFallbackReasonV1::SingleFrameRequired);
    };
    if target_frame_matches.next().is_some() {
        return Err(RenderTextLayoutFallbackReasonV1::SingleFrameRequired);
    }

    let mut target_story_frames = visual
        .story_frames
        .iter()
        .filter(|candidate| candidate.story_id == target_frame.story_id);
    let Some(single_target_frame) = target_story_frames.next() else {
        return Err(RenderTextLayoutFallbackReasonV1::SingleFrameRequired);
    };
    if target_story_frames.next().is_some() || single_target_frame.frame_id != target_frame_node_id
    {
        return Err(RenderTextLayoutFallbackReasonV1::SingleFrameRequired);
    }

    Ok(0)
}

fn projected_incomplete_layout_is_explicit_overset(
    projected_target_frame_node_id: Option<NodeId>,
    diagnostics: &[pub_layout::ResolveDiagnostic],
    story_id: StoryId,
) -> bool {
    projected_target_frame_node_id.is_some()
        && diagnostics.len() == 1
        && diagnostics[0].code == "story_overset"
        && diagnostics[0].origin == story_id.into_canonical()
}

fn resolve_text_layout_v1(
    visual: &ViewerGeometryDocument,
    target: RenderTextLayoutTargetV1,
    fragment: &RenderTextFragmentV1,
    font: &ExplicitRenderTextFontResourceV1<'_>,
) -> RenderTextLayoutV1 {
    let RenderTextLayoutTargetV1 {
        page_id,
        page_size,
        node_id,
        projected_target_frame_node_id,
        bounds,
        transform,
    } = target;
    let Some(story) = visual
        .document
        .stories
        .iter()
        .find(|story| story.id == fragment.story_id)
    else {
        return fallback_layout(RenderTextLayoutFallbackReasonV1::StoryMissing);
    };

    let Ok(story_scalar_len) = u32::try_from(story.text.chars().count()) else {
        return fallback_layout(RenderTextLayoutFallbackReasonV1::StoryExtentMismatch);
    };
    if fragment.scalar_start != 0
        || fragment.scalar_end != story_scalar_len
        || fragment.text != story.text
    {
        return fallback_layout(RenderTextLayoutFallbackReasonV1::StoryExtentMismatch);
    }

    let frame_ordinal = match admitted_layout_frame_ordinal(
        visual,
        fragment.story_id,
        node_id,
        projected_target_frame_node_id,
    ) {
        Ok(ordinal) => ordinal,
        Err(reason) => return fallback_layout(reason),
    };

    if bounds.width.get() <= 0 || bounds.height.get() <= 0 {
        return fallback_layout(RenderTextLayoutFallbackReasonV1::FrameGeometryInvalid);
    }
    if font.resource_id.is_empty()
        || font.bytes.is_empty()
        || font.default_font_size_emu <= 0
        || font.default_line_height_emu <= 0
    {
        return fallback_layout(RenderTextLayoutFallbackReasonV1::FontResourceInvalid);
    }

    let fingerprint = font_fingerprint_sha256(font.bytes);
    if font.expected_sha256.is_empty() || fingerprint != font.expected_sha256 {
        return fallback_layout(RenderTextLayoutFallbackReasonV1::FontFingerprintMismatch);
    }

    let font_size_emu = match admitted_font_size_emu(fragment, font.default_font_size_emu) {
        Ok(size) => size,
        Err(RenderTextLayoutFallbackReasonV1::MixedTypographySize)
            if projected_target_frame_node_id.is_none() =>
        {
            return resolve_mixed_size_text_layout_v1(fragment, font, &bounds, &fingerprint);
        }
        Err(reason) => return fallback_layout(reason),
    };
    let Some(line_height_emu) = scaled_line_height_emu(
        font_size_emu,
        font.default_font_size_emu,
        font.default_line_height_emu,
    ) else {
        return fallback_layout(RenderTextLayoutFallbackReasonV1::FontResourceInvalid);
    };

    let projection = BoundedLayoutProjection {
        pages: vec![ProjectedPage {
            origin: page_id,
            size: page_size,
            bleed: None,
            margins: None,
        }],
        node_geometry: vec![ProjectedNodeGeometry {
            origin: node_id,
            parent_origin: page_id.into_canonical(),
            bounds,
            transform,
        }],
        stories: vec![ProjectedStory {
            origin: story.id,
            text: story.text.clone(),
            paragraph_origins: Vec::new(),
            run_origins: Vec::new(),
        }],
        story_frames: vec![ProjectedStoryFrame {
            story_origin: story.id,
            frame_origin: node_id,
            ordinal: frame_ordinal,
            previous_frame_origin: None,
            next_frame_origin: None,
        }],
        tables: Vec::new(),
        guides: Vec::new(),
        diagnostics: Vec::new(),
    };

    let runtime = BoundedShapedFlowRuntime {
        shaping: BoundedShapingRuntime {
            layout: BoundedLayoutEnvironment {
                engine_revision: SHARED_TEXT_LAYOUT_REVISION_V1.to_owned(),
                font_set_fingerprint: fingerprint.clone(),
                resource_fingerprint: font.resource_id.to_owned(),
            },
            face_index: font.face_index,
            font_size_emu: LengthEmu::new(font_size_emu),
            font_bytes: font.bytes,
        },
        line_height: LengthEmu::new(line_height_emu),
    };

    let Ok(scene) = resolve_bounded_shaped_flow(&projection, &runtime) else {
        return fallback_layout(RenderTextLayoutFallbackReasonV1::SharedLayoutFailed);
    };
    let projected_explicit_overset = projected_incomplete_layout_is_explicit_overset(
        projected_target_frame_node_id,
        &scene.diagnostics,
        story.id,
    );
    let mut source_lines = scene
        .lines
        .into_iter()
        .filter(|line| line.story_origin == story.id && line.frame_origin == node_id)
        .collect::<Vec<_>>();
    source_lines.sort_by_key(|line| line.frame_line_index);

    if story_scalar_len > 0
        && source_lines.last().map(|line| line.consumed_scalar_end) != Some(story_scalar_len)
        && !projected_explicit_overset
    {
        return fallback_layout(RenderTextLayoutFallbackReasonV1::SharedLayoutIncomplete);
    }

    let lines = source_lines
        .into_iter()
        .map(|line| RenderResolvedTextLineV1 {
            line_index: line.frame_line_index,
            scalar_start: line.scalar_start,
            scalar_end: line.scalar_end,
            consumed_scalar_end: line.consumed_scalar_end,
            text: line.text,
            measured_width_emu: line.measured_width.get(),
            line_height_emu,
            spans: Vec::new(),
        })
        .collect();

    RenderTextLayoutV1 {
        disposition: RenderTextLayoutDispositionV1::SharedResolved {
            font_resource_id: font.resource_id.to_owned(),
            font_fingerprint_sha256: fingerprint,
            font_size_emu,
            line_height_emu,
        },
        lines,
    }
}

#[derive(Debug, Clone, Copy)]
struct AdmittedTypographyRunV1 {
    scalar_start: u32,
    scalar_end: u32,
    font_size_emu: i64,
}

#[derive(Debug)]
struct MixedLineCandidateV1 {
    scalar_end: u32,
    consumed_scalar_end: u32,
    text: String,
    measured_width_emu: i64,
    line_height_emu: i64,
    spans: Vec<RenderResolvedTextSpanV1>,
}

fn admitted_typography_runs_v1(
    fragment: &RenderTextFragmentV1,
    default_font_size_emu: i64,
) -> Result<Vec<AdmittedTypographyRunV1>, RenderTextLayoutFallbackReasonV1> {
    if fragment.typography.is_empty() {
        if default_font_size_emu <= 0 {
            return Err(RenderTextLayoutFallbackReasonV1::FontResourceInvalid);
        }
        return Ok(vec![AdmittedTypographyRunV1 {
            scalar_start: fragment.scalar_start,
            scalar_end: fragment.scalar_end,
            font_size_emu: default_font_size_emu,
        }]);
    }

    let mut cursor = fragment.scalar_start;
    let mut admitted = Vec::with_capacity(fragment.typography.len());
    for run in &fragment.typography {
        if run.scalar_start != cursor
            || run.scalar_end <= run.scalar_start
            || run.scalar_end > fragment.scalar_end
            || run.text_size_emu == 0
        {
            return Err(RenderTextLayoutFallbackReasonV1::TypographyCoverageGap);
        }
        admitted.push(AdmittedTypographyRunV1 {
            scalar_start: run.scalar_start,
            scalar_end: run.scalar_end,
            font_size_emu: i64::from(run.text_size_emu),
        });
        cursor = run.scalar_end;
    }
    if cursor != fragment.scalar_end {
        return Err(RenderTextLayoutFallbackReasonV1::TypographyCoverageGap);
    }
    Ok(admitted)
}

fn admitted_font_size_emu(
    fragment: &RenderTextFragmentV1,
    default_font_size_emu: i64,
) -> Result<i64, RenderTextLayoutFallbackReasonV1> {
    let admitted = admitted_typography_runs_v1(fragment, default_font_size_emu)?;
    let Some(first) = admitted.first().map(|run| run.font_size_emu) else {
        return Err(RenderTextLayoutFallbackReasonV1::TypographyCoverageGap);
    };
    if admitted.iter().all(|run| run.font_size_emu == first) {
        Ok(first)
    } else {
        Err(RenderTextLayoutFallbackReasonV1::MixedTypographySize)
    }
}

fn scalar_text_range_v1(scalars: &[char], start: u32, end: u32) -> Option<String> {
    let start = usize::try_from(start).ok()?;
    let end = usize::try_from(end).ok()?;
    (start <= end && end <= scalars.len()).then(|| scalars[start..end].iter().collect())
}

fn shape_mixed_line_candidate_v1(
    scalars: &[char],
    cursor: u32,
    consumed_scalar_end: u32,
    kind: BoundedBreakKind,
    runs: &[AdmittedTypographyRunV1],
    font: &ExplicitRenderTextFontResourceV1<'_>,
    fingerprint: &str,
) -> Result<MixedLineCandidateV1, RenderTextLayoutFallbackReasonV1> {
    let mut scalar_end = consumed_scalar_end;
    if kind == BoundedBreakKind::Mandatory {
        while scalar_end > cursor {
            let index = usize::try_from(scalar_end - 1)
                .map_err(|_| RenderTextLayoutFallbackReasonV1::SharedLayoutFailed)?;
            if !matches!(scalars.get(index).copied(), Some('\r' | '\n')) {
                break;
            }
            scalar_end -= 1;
        }
    }

    let text = scalar_text_range_v1(scalars, cursor, scalar_end)
        .ok_or(RenderTextLayoutFallbackReasonV1::SharedLayoutFailed)?;
    let mut spans = Vec::new();
    let mut measured_width_emu = 0_i64;
    let mut line_height_emu = font.default_line_height_emu;

    for run in runs {
        let span_start = run.scalar_start.max(cursor);
        let span_end = run.scalar_end.min(scalar_end);
        if span_start >= span_end {
            continue;
        }
        let span_text = scalar_text_range_v1(scalars, span_start, span_end)
            .ok_or(RenderTextLayoutFallbackReasonV1::SharedLayoutFailed)?;
        let Some(span_line_height_emu) = scaled_line_height_emu(
            run.font_size_emu,
            font.default_font_size_emu,
            font.default_line_height_emu,
        ) else {
            return Err(RenderTextLayoutFallbackReasonV1::FontResourceInvalid);
        };
        let runtime = BoundedShapingRuntime {
            layout: BoundedLayoutEnvironment {
                engine_revision: SHARED_TEXT_LAYOUT_REVISION_V1.to_owned(),
                font_set_fingerprint: fingerprint.to_owned(),
                resource_fingerprint: font.resource_id.to_owned(),
            },
            face_index: font.face_index,
            font_size_emu: LengthEmu::new(run.font_size_emu),
            font_bytes: font.bytes,
        };
        let shaped = shape_bounded_ltr_segment(&span_text, span_start, &runtime)
            .map_err(|_| RenderTextLayoutFallbackReasonV1::SharedLayoutFailed)?;
        let span_width_emu = shaped.total_x_advance.get();
        spans.push(RenderResolvedTextSpanV1 {
            scalar_start: span_start,
            scalar_end: span_end,
            text: span_text,
            x_offset_emu: measured_width_emu,
            measured_width_emu: span_width_emu,
            font_size_emu: run.font_size_emu,
        });
        measured_width_emu = measured_width_emu
            .checked_add(span_width_emu)
            .ok_or(RenderTextLayoutFallbackReasonV1::SharedLayoutFailed)?;
        line_height_emu = line_height_emu.max(span_line_height_emu);
    }

    Ok(MixedLineCandidateV1 {
        scalar_end,
        consumed_scalar_end,
        text,
        measured_width_emu,
        line_height_emu,
        spans,
    })
}

fn resolve_mixed_size_text_layout_v1(
    fragment: &RenderTextFragmentV1,
    font: &ExplicitRenderTextFontResourceV1<'_>,
    bounds: &RectEmu,
    fingerprint: &str,
) -> RenderTextLayoutV1 {
    let runs = match admitted_typography_runs_v1(fragment, font.default_font_size_emu) {
        Ok(runs) => runs,
        Err(reason) => return fallback_layout(reason),
    };
    if runs.len() < 2
        || runs
            .iter()
            .all(|run| run.font_size_emu == runs[0].font_size_emu)
    {
        return fallback_layout(RenderTextLayoutFallbackReasonV1::MixedTypographySize);
    }

    let scalars: Vec<char> = fragment.text.chars().collect();
    let scalar_count = match u32::try_from(scalars.len()) {
        Ok(value) => value,
        Err(_) => return fallback_layout(RenderTextLayoutFallbackReasonV1::SharedLayoutFailed),
    };
    if fragment.scalar_start != 0 || fragment.scalar_end != scalar_count {
        return fallback_layout(RenderTextLayoutFallbackReasonV1::StoryExtentMismatch);
    }

    let mut policy_glyphs = Vec::new();
    for run in &runs {
        let Some(run_text) = scalar_text_range_v1(&scalars, run.scalar_start, run.scalar_end)
        else {
            return fallback_layout(RenderTextLayoutFallbackReasonV1::SharedLayoutFailed);
        };
        let runtime = BoundedShapingRuntime {
            layout: BoundedLayoutEnvironment {
                engine_revision: SHARED_TEXT_LAYOUT_REVISION_V1.to_owned(),
                font_set_fingerprint: fingerprint.to_owned(),
                resource_fingerprint: font.resource_id.to_owned(),
            },
            face_index: font.face_index,
            font_size_emu: LengthEmu::new(run.font_size_emu),
            font_bytes: font.bytes,
        };
        let shaped = match shape_bounded_ltr_segment(&run_text, run.scalar_start, &runtime) {
            Ok(shaped) => shaped,
            Err(_) => return fallback_layout(RenderTextLayoutFallbackReasonV1::SharedLayoutFailed),
        };
        policy_glyphs.extend(shaped.glyphs);
    }
    let policy = match break_policy_for_shaped_text(&fragment.text, &policy_glyphs) {
        Ok(policy) => policy,
        Err(_) => return fallback_layout(RenderTextLayoutFallbackReasonV1::SharedLayoutFailed),
    };

    let mut cursor = fragment.scalar_start;
    let mut used_height_emu = 0_i64;
    let mut line_index = 0_u32;
    let mut lines = Vec::new();

    while cursor < fragment.scalar_end {
        let mut chosen = None;
        for candidate in policy
            .candidates
            .iter()
            .filter(|candidate| candidate.scalar_boundary > cursor)
        {
            let evaluated = match shape_mixed_line_candidate_v1(
                &scalars,
                cursor,
                candidate.scalar_boundary,
                candidate.kind,
                &runs,
                font,
                fingerprint,
            ) {
                Ok(evaluated) => evaluated,
                Err(reason) => return fallback_layout(reason),
            };
            let fits_width = evaluated.measured_width_emu <= bounds.width.get();
            let fits_height = used_height_emu
                .checked_add(evaluated.line_height_emu)
                .is_some_and(|height| height <= bounds.height.get());
            if fits_width && fits_height {
                chosen = Some(evaluated);
            }
            if candidate.kind == BoundedBreakKind::Mandatory {
                break;
            }
        }

        let Some(chosen) = chosen else {
            break;
        };
        used_height_emu = match used_height_emu.checked_add(chosen.line_height_emu) {
            Some(value) => value,
            None => return fallback_layout(RenderTextLayoutFallbackReasonV1::SharedLayoutFailed),
        };
        lines.push(RenderResolvedTextLineV1 {
            line_index,
            scalar_start: cursor,
            scalar_end: chosen.scalar_end,
            consumed_scalar_end: chosen.consumed_scalar_end,
            text: chosen.text,
            measured_width_emu: chosen.measured_width_emu,
            line_height_emu: chosen.line_height_emu,
            spans: chosen.spans,
        });
        cursor = chosen.consumed_scalar_end;
        line_index = match line_index.checked_add(1) {
            Some(value) => value,
            None => return fallback_layout(RenderTextLayoutFallbackReasonV1::SharedLayoutFailed),
        };
    }

    if cursor != fragment.scalar_end {
        return fallback_layout(RenderTextLayoutFallbackReasonV1::SharedLayoutIncomplete);
    }

    let max_font_size_emu = runs
        .iter()
        .map(|run| run.font_size_emu)
        .max()
        .unwrap_or(font.default_font_size_emu);
    let max_line_height_emu = lines
        .iter()
        .map(|line| line.line_height_emu)
        .max()
        .unwrap_or(font.default_line_height_emu);

    RenderTextLayoutV1 {
        disposition: RenderTextLayoutDispositionV1::SharedResolved {
            font_resource_id: font.resource_id.to_owned(),
            font_fingerprint_sha256: fingerprint.to_owned(),
            font_size_emu: max_font_size_emu,
            line_height_emu: max_line_height_emu,
        },
        lines,
    }
}

fn scaled_line_height_emu(
    font_size_emu: i64,
    default_font_size_emu: i64,
    default_line_height_emu: i64,
) -> Option<i64> {
    if font_size_emu <= 0 || default_font_size_emu <= 0 || default_line_height_emu <= 0 {
        return None;
    }
    let numerator = i128::from(font_size_emu).checked_mul(i128::from(default_line_height_emu))?;
    let denominator = i128::from(default_font_size_emu);
    let rounded = numerator
        .checked_add(denominator / 2)?
        .checked_div(denominator)?;
    let value = i64::try_from(rounded).ok()?;
    (value > 0).then_some(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature = "projected-scene-instances")]
    use chaptera_scene_instance::{
        SCENE_INSTANCE_SCHEMA_V1, SceneInstanceV1, SceneProjectionKindV1,
    };
    use pub_layout::{
        BoundedLayoutEnvironment, BoundedResolvedScene, ResolvedPhysicalNode, ResolvedSurface,
    };
    use pub_model::{
        Affine2D, CanonicalId, LengthEmu, RectEmu, Sha256Digest, Size2D, TableCellAddress,
        TableCellId,
    };
    use pub_viewer::{
        ViewerDocument, ViewerEmbeddedImage, ViewerImagePlacementV1, ViewerImageSourceWindowV1,
        ViewerNodePaint, ViewerPage, ViewerSolidLine, ViewerSource, ViewerTable, ViewerTableCell,
        ViewerTextFragment, ViewerTypographyRun, viewer_story_text_sha256,
    };

    fn canonical(byte: u8) -> CanonicalId {
        CanonicalId::from_bytes([byte; 16])
    }

    fn fixture() -> ViewerGeometryDocument {
        let page_id = PageId::from_canonical(canonical(1));
        let node_id = NodeId::from_canonical(canonical(2));
        let story_id = StoryId::from_canonical(canonical(3));
        let resource_id = ResourceId::from_canonical(canonical(4));
        let page_size = Size2D::new(LengthEmu::new(1000), LengthEmu::new(2000));

        ViewerGeometryDocument {
            schema_version: "viewer.v1".into(),
            document: ViewerDocument {
                schema_version: "viewer.document.v1".into(),
                source: ViewerSource {
                    format: "pub".into(),
                    format_version: Some("0x2c".into()),
                    source_hash: Sha256Digest::from_bytes([0x11; 32]),
                    byte_len: 12,
                },
                pages: vec![ViewerPage {
                    index: 1,
                    id: page_id,
                    width_emu: 1000,
                    height_emu: 2000,
                }],
                stories: vec![pub_viewer::ViewerStory {
                    id: story_id,
                    text: "hello".into(),
                }],
                diagnostics: Vec::new(),
            },
            scene: BoundedResolvedScene {
                environment: BoundedLayoutEnvironment {
                    engine_revision: "test".into(),
                    font_set_fingerprint: "fonts:test".into(),
                    resource_fingerprint: "resources:test".into(),
                },
                surfaces: vec![ResolvedSurface {
                    origin: page_id,
                    size: page_size,
                    bleed: None,
                    margins: None,
                }],
                nodes: vec![ResolvedPhysicalNode {
                    origin: node_id,
                    parent_origin: page_id.into_canonical(),
                    bounds: RectEmu::new(
                        LengthEmu::new(10),
                        LengthEmu::new(20),
                        LengthEmu::new(300),
                        LengthEmu::new(400),
                    ),
                    transform: Affine2D::identity(),
                }],
                origin_mapping: Vec::new(),
                diagnostics: Vec::new(),
            },
            paints: vec![ViewerNodePaint {
                node_id,
                solid_fill_rgb: Some([1, 2, 3]),
                solid_line: Some(ViewerSolidLine {
                    rgb: [4, 5, 6],
                    width_emu: 12700,
                }),
            }],
            story_frames: Vec::new(),
            text_fragments: vec![ViewerTextFragment {
                story_id,
                frame_id: node_id,
                scalar_start: 0,
                scalar_end: 5,
                text: "hello".into(),
                line_count: 1,
            }],
            #[cfg(feature = "projected-scene-instances")]
            projected_instances: Vec::new(),
            typography_runs: vec![ViewerTypographyRun {
                story_id,
                scalar_start: 0,
                scalar_end: 2,
                source_font_name: "Source Font".into(),
                text_size_emu: 24 * 12_700,
                font_inherited: false,
                size_inherited: true,
                source_story_text_sha256: viewer_story_text_sha256("hello"),
            }],
            script_font_maps: Vec::new(),
            tables: Vec::new(),
            images: vec![ViewerEmbeddedImage {
                resource_id,
                mime: "image/png".into(),
                node_ids: vec![node_id],
                placements: vec![ViewerImagePlacementV1 {
                    node_id,
                    source_window: Some(ViewerImageSourceWindowV1 {
                        left_q16: 8_192,
                        top_q16: 16_384,
                        right_q16: 57_344,
                        bottom_q16: 49_152,
                    }),
                }],
                bytes: vec![0x89, b'P', b'N', b'G'],
            }],
        }
    }

    #[test]
    fn plan_collects_document_paint_facts_without_backend_state() {
        let visual = fixture();
        let plan = build_page_render_plan_v1(&visual, 0).expect("render plan");

        assert_eq!(plan.schema_version, PAGE_RENDER_PLAN_SCHEMA_V1);
        assert_eq!(plan.nodes.len(), 1);
        let node = &plan.nodes[0];
        assert_eq!(node.solid_fill_rgb, Some([1, 2, 3]));
        assert_eq!(
            node.solid_line.as_ref().map(|line| line.rgb),
            Some([4, 5, 6])
        );
        assert_eq!(
            node.image.as_ref().map(|image| image.mime.as_str()),
            Some("image/png")
        );
        assert_eq!(
            node.image
                .as_ref()
                .and_then(|image| image.source_window.as_ref())
                .map(|window| (
                    window.left_q16,
                    window.top_q16,
                    window.right_q16,
                    window.bottom_q16,
                )),
            Some((8_192, 16_384, 57_344, 49_152))
        );
        assert_eq!(
            node.text.as_ref().map(|text| text.text.as_str()),
            Some("hello")
        );
        assert!(node.table.is_none());
        let typography = &node.text.as_ref().expect("text").typography;
        assert_eq!(typography.len(), 1);
        assert_eq!(typography[0].scalar_start, 0);
        assert_eq!(typography[0].scalar_end, 2);
        assert_eq!(typography[0].text_size_emu, 24 * 12_700);
        assert!(typography[0].size_inherited);
    }

    #[test]
    fn table_payload_reaches_render_plan_with_cell_text_and_bounds() {
        let mut visual = fixture();
        let node_id = visual.scene.nodes[0].origin;
        let story_id = visual.document.stories[0].id;
        let cell_id = TableCellId::from_canonical(canonical(5));
        let cell_bounds = RectEmu::new(
            LengthEmu::new(10),
            LengthEmu::new(20),
            LengthEmu::new(150),
            LengthEmu::new(200),
        );

        // Mature TABLE owns its text through the table payload, not a normal
        // StoryFrame. Keep the fixture aligned with that product boundary.
        visual.text_fragments.clear();
        visual.typography_runs.clear();
        visual.tables.push(ViewerTable {
            node_id,
            story_id,
            rows: 1,
            columns: 1,
            cells: vec![ViewerTableCell {
                id: cell_id,
                address: TableCellAddress { row: 0, column: 0 },
                row_span: 1,
                column_span: 1,
                text: "cell".into(),
                bounds: Some(cell_bounds),
            }],
        });

        let plan = build_page_render_plan_v1(&visual, 0).expect("render plan");
        let node = &plan.nodes[0];
        assert!(node.text.is_none());

        let table = node.table.as_ref().expect("table payload");
        assert_eq!(table.story_id, story_id);
        assert_eq!((table.rows, table.columns), (1, 1));
        assert_eq!(table.cells.len(), 1);
        assert_eq!(table.cells[0].id, cell_id);
        assert_eq!(table.cells[0].row, 0);
        assert_eq!(table.cells[0].column, 0);
        assert_eq!(table.cells[0].row_span, 1);
        assert_eq!(table.cells[0].column_span, 1);
        assert_eq!(table.cells[0].text, "cell");
        assert_eq!(table.cells[0].bounds, Some(cell_bounds));
    }

    #[cfg(feature = "projected-scene-instances")]
    #[test]
    fn projected_cmo_layout_admits_hidden_carrier_with_single_target_frame() {
        let mut visual = fixture();
        let carrier_node_id = visual.scene.nodes[0].origin;
        let carrier_story_id = visual.document.stories[0].id;
        let target_story_id = StoryId::from_canonical(canonical(8));
        let target_frame_node_id = NodeId::from_canonical(canonical(9));
        visual.story_frames.push(pub_viewer::ViewerStoryFrame {
            story_id: target_story_id,
            frame_id: target_frame_node_id,
            ordinal: 0,
        });

        assert_eq!(
            admitted_layout_frame_ordinal(
                &visual,
                carrier_story_id,
                carrier_node_id,
                Some(target_frame_node_id),
            ),
            Ok(0),
            "projected Cmo must not require a hidden carrier-page StoryFrame"
        );

        assert_eq!(
            admitted_layout_frame_ordinal(&visual, carrier_story_id, carrier_node_id, None),
            Err(RenderTextLayoutFallbackReasonV1::SingleFrameRequired),
            "ordinary nodes still require their own canonical StoryFrame"
        );
    }

    #[cfg(feature = "projected-scene-instances")]
    #[test]
    fn projected_cmo_layout_keeps_multi_frame_target_topology_fail_closed() {
        let mut visual = fixture();
        let carrier_node_id = visual.scene.nodes[0].origin;
        let carrier_story_id = visual.document.stories[0].id;
        let target_story_id = StoryId::from_canonical(canonical(8));
        let target_frame_node_id = NodeId::from_canonical(canonical(9));
        visual.story_frames.extend([
            pub_viewer::ViewerStoryFrame {
                story_id: target_story_id,
                frame_id: target_frame_node_id,
                ordinal: 0,
            },
            pub_viewer::ViewerStoryFrame {
                story_id: target_story_id,
                frame_id: NodeId::from_canonical(canonical(10)),
                ordinal: 1,
            },
        ]);

        assert_eq!(
            admitted_layout_frame_ordinal(
                &visual,
                carrier_story_id,
                carrier_node_id,
                Some(target_frame_node_id),
            ),
            Err(RenderTextLayoutFallbackReasonV1::SingleFrameRequired)
        );
    }

    #[cfg(feature = "projected-scene-instances")]
    #[test]
    fn projected_cmo_admits_only_explicit_story_overset_as_partial_shared_layout() {
        let story_id = StoryId::from_canonical(canonical(3));
        let frame_id = NodeId::from_canonical(canonical(9));
        let overset = pub_layout::ResolveDiagnostic {
            code: "story_overset".into(),
            severity: pub_layout::ResolveSeverity::FidelityWarning,
            origin: story_id.into_canonical(),
            message: "bounded fixture".into(),
        };
        assert!(projected_incomplete_layout_is_explicit_overset(
            Some(frame_id),
            std::slice::from_ref(&overset),
            story_id,
        ));
        assert!(!projected_incomplete_layout_is_explicit_overset(
            None,
            std::slice::from_ref(&overset),
            story_id,
        ));

        let unbreakable = pub_layout::ResolveDiagnostic {
            code: "unbreakable_shaped_line".into(),
            severity: pub_layout::ResolveSeverity::FidelityWarning,
            origin: frame_id.into_canonical(),
            message: "bounded fixture".into(),
        };
        assert!(!projected_incomplete_layout_is_explicit_overset(
            Some(frame_id),
            &[overset, unbreakable],
            story_id,
        ));
    }

    #[cfg(feature = "projected-scene-instances")]
    #[test]
    fn canonical_cmo_scene_instance_reaches_render_plan_without_new_identity() {
        let mut visual = fixture();
        let page_id = visual.document.pages[0].id;
        let origin_node_id = visual.scene.nodes[0].origin;
        let story_id = visual.document.stories[0].id;
        // Canonical instance derivation is owned and tested by
        // chaptera-scene-instance and by the exact Cmo slot-flow consumer.
        // This seam test proves Viewer/render-plan preserves that already-owned
        // typed authority verbatim instead of deriving a second identity.
        let instance = SceneInstanceV1 {
            schema_version: SCENE_INSTANCE_SCHEMA_V1.to_owned(),
            instance_id: "sha256:scene-instance-authority-fixture".to_owned(),
            projection_kind: SceneProjectionKindV1::CmoStorySlot,
            origin_node_id: origin_node_id.as_canonical().to_string(),
            target_page_id: page_id.as_canonical().to_string(),
            source_parent_origin: None,
            story_authority_id: Some(story_id.as_canonical().to_string()),
            cmo_slot_index: Some(0),
            cmo_scalar_index: Some(0),
        };
        visual
            .projected_instances
            .push(pub_viewer::ViewerProjectedSceneInstanceV1 {
                scene_instance: instance.clone(),
                target_frame_node_id: origin_node_id,
                target_frame_paint_scalar_end: None,
                bounds: RectEmu::new(
                    LengthEmu::new(50),
                    LengthEmu::new(60),
                    LengthEmu::new(70),
                    LengthEmu::new(80),
                ),
                transform: Affine2D::identity(),
            });

        let plan = build_page_render_plan_v1(&visual, 0).expect("render plan");
        let projected = plan
            .nodes
            .iter()
            .find(|node| node.projected_scene_instance.is_some())
            .expect("projected node");
        assert_eq!(
            projected
                .projected_scene_instance
                .as_ref()
                .map(|value| value.instance_id.as_str()),
            Some(instance.instance_id.as_str())
        );
        assert_eq!(projected.node_id, origin_node_id);
    }

    #[cfg(feature = "projected-scene-instances")]
    #[test]
    fn projected_slot_suppresses_only_marker_glyphs_and_keeps_scalar_count() {
        let mut visual = fixture();
        let page_id = visual.document.pages[0].id;
        let frame_id = visual.scene.nodes[0].origin;
        let source = "\u{FFFC}\r\u{FFFC}\r\u{FFFC}am.";
        visual.document.stories[0].text = source.to_owned();
        visual.text_fragments[0].text = source.to_owned();
        visual.text_fragments[0].scalar_end =
            u32::try_from(source.chars().count()).expect("bounded fixture");
        let instance = SceneInstanceV1 {
            schema_version: SCENE_INSTANCE_SCHEMA_V1.to_owned(),
            instance_id: "sha256:marker-suppression-instance-fixture".to_owned(),
            projection_kind: SceneProjectionKindV1::CmoStorySlot,
            origin_node_id: frame_id.as_canonical().to_string(),
            target_page_id: page_id.as_canonical().to_string(),
            source_parent_origin: None,
            story_authority_id: None,
            cmo_slot_index: Some(0),
            cmo_scalar_index: Some(0),
        };
        visual
            .projected_instances
            .push(pub_viewer::ViewerProjectedSceneInstanceV1 {
                scene_instance: instance,
                target_frame_node_id: frame_id,
                target_frame_paint_scalar_end: None,
                bounds: visual.scene.nodes[0].bounds,
                transform: Affine2D::identity(),
            });

        let plan = build_page_render_plan_v1(&visual, 0).expect("render plan");
        let direct = plan
            .nodes
            .iter()
            .find(|node| node.projected_scene_instance.is_none() && node.node_id == frame_id)
            .expect("direct frame");
        let rendered = &direct.text.as_ref().expect("text").text;
        assert!(!rendered.contains('\u{FFFC}'));
        assert_eq!(rendered.chars().count(), source.chars().count());
        assert!(rendered.ends_with("am."));
    }

    #[cfg(feature = "projected-scene-instances")]
    #[test]
    fn proven_first_nonfit_clips_direct_target_paint_without_mutating_story() {
        let mut visual = fixture();
        let page_id = visual.document.pages[0].id;
        let frame_id = visual.scene.nodes[0].origin;
        let source = "\u{FFFC}\rX\u{FFFC}tail";
        visual.document.stories[0].text = source.to_owned();
        visual.text_fragments[0].text = source.to_owned();
        visual.text_fragments[0].scalar_end =
            u32::try_from(source.chars().count()).expect("bounded fixture");

        let instance = SceneInstanceV1 {
            schema_version: SCENE_INSTANCE_SCHEMA_V1.to_owned(),
            instance_id: "sha256:first-nonfit-paint-fixture".to_owned(),
            projection_kind: SceneProjectionKindV1::CmoStorySlot,
            origin_node_id: frame_id.as_canonical().to_string(),
            target_page_id: page_id.as_canonical().to_string(),
            source_parent_origin: None,
            story_authority_id: None,
            cmo_slot_index: Some(0),
            cmo_scalar_index: Some(0),
        };
        visual
            .projected_instances
            .push(pub_viewer::ViewerProjectedSceneInstanceV1 {
                scene_instance: instance,
                target_frame_node_id: frame_id,
                target_frame_paint_scalar_end: Some(3),
                bounds: visual.scene.nodes[0].bounds,
                transform: Affine2D::identity(),
            });

        let plan = build_page_render_plan_v1(&visual, 0).expect("render plan");
        let direct = plan
            .nodes
            .iter()
            .find(|node| node.projected_scene_instance.is_none() && node.node_id == frame_id)
            .expect("direct frame");
        let rendered = direct
            .text
            .as_ref()
            .expect("pre-boundary direct text remains");
        assert_eq!(rendered.scalar_start, 0);
        assert_eq!(rendered.scalar_end, 3);
        assert_eq!(rendered.text, "\u{200B}\rX");
        assert!(
            rendered
                .typography
                .iter()
                .all(|run| run.scalar_end <= rendered.scalar_end)
        );
        assert_eq!(
            visual.document.stories[0].text, source,
            "paint clipping must not mutate canonical Viewer Story text"
        );
    }

    #[test]
    fn serialized_plan_is_source_neutral_and_does_not_retain_image_bytes() {
        let plan = build_page_render_plan_v1(&fixture(), 0).expect("render plan");
        let json = serde_json::to_string(&plan).expect("serialize plan");

        for forbidden in [
            "Escher",
            "Quill",
            "Contents",
            "byte_range",
            "offset",
            "89504e47",
        ] {
            assert!(!json.contains(forbidden), "render plan leaked {forbidden}");
        }
        assert!(json.contains("image/png"));
        assert!(json.contains("hello"));
    }

    #[test]
    fn missing_page_is_typed_failure() {
        assert!(matches!(
            build_page_render_plan_v1(&fixture(), 1),
            Err(RenderPlanErrorV1::PageIndexOutOfBounds { page_index: 1 })
        ));
    }
}
