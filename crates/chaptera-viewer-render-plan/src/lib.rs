//! Source-neutral render-plan boundary for Chaptera document surfaces.
//!
//! The plan answers only what the current Viewer document intends to paint.
//! It deliberately contains no egui types, EditorSession state, parser-private
//! carrier names, source offsets, or mutable authoring commands.

#[cfg(feature = "projected-scene-instances")]
use chaptera_scene_instance::{SceneInstanceV1, SceneProjectionKindV1};
use pub_layout::{
    BoundedBreakKind, BoundedLayoutEnvironment, BoundedLayoutProjection, BoundedShapedFlowRuntime,
    BoundedShapedGlyph, BoundedShapingDescriptor, BoundedShapingRuntime, ProjectedNodeGeometry,
    ProjectedPage, ProjectedStory, ProjectedStoryFrame, break_policy_for_shaped_text,
    compatible_natural_line_height_emu_v1, font_fingerprint_sha256, resolve_bounded_shaped_flow,
    shape_bounded_ltr_segment,
};
use pub_line_placement::{
    LayoutPlacementContextV1, ParagraphAlignmentV1, ParagraphLinePlacementInputV1,
    ResolvedLineInputV1, resolve_paragraph_line_placement_v1,
};
#[cfg(feature = "projected-scene-instances")]
use pub_model::CanonicalId;
use pub_model::{
    Affine2D, LengthEmu, NodeId, PageId, RectEmu, ResourceId, Size2D, StoryId, TableCellId,
};
#[cfg(feature = "projected-scene-instances")]
use pub_viewer::ViewerProjectedSceneInstanceV1;
use pub_viewer::{
    ViewerDecorativeBorderSlotV1, ViewerGeometryDocument, ViewerParagraphAlignment,
    ViewerParagraphLineSpacing, ViewerScriptFontEntryDisposition, ViewerStoryFrame,
    ViewerTextVerticalAlignment,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fmt;

pub const PAGE_RENDER_PLAN_SCHEMA_V1: &str = "chaptera.page-render-plan.v1";
pub const SHARED_TEXT_LAYOUT_REVISION_V1: &str = "chaptera.viewer.shared-text-layout.v1";

fn is_zero_i64(value: &i64) -> bool {
    *value == 0
}

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_bounds: Option<RectEmu>,
    pub transform: Affine2D,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub solid_fill_rgb: Option<[u8; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub solid_line: Option<RenderSolidLineV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decorative_border: Option<RenderDecorativeBorderV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<RenderImageRefV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<RenderTextFragmentV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub table: Option<RenderTableV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthoredPageRenderNodeV1 {
    pub node_id: NodeId,
    pub bounds: RectEmu,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub solid_fill_rgb: Option<[u8; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub solid_line: Option<RenderSolidLineV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthoredPageRenderLaneV1 {
    pub page_id: PageId,
    /// Exact authored back/bottom -> front/top order.
    pub nodes: Vec<AuthoredPageRenderNodeV1>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill_rgb: Option<[u8; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill_visible: Option<bool>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RenderDecorativeBorderSlotV1 {
    TopLeft,
    Top,
    TopRight,
    Right,
    BottomRight,
    Bottom,
    BottomLeft,
    Left,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenderDecorativeBorderSlotRefV1 {
    pub slot: RenderDecorativeBorderSlotV1,
    pub resource_id: ResourceId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenderDecorativeBorderV1 {
    pub name: String,
    pub corner_extent_emu: u32,
    pub horizontal_extent_emu: u32,
    pub vertical_extent_emu: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stretch_pictures: Option<bool>,
    pub slots: Vec<RenderDecorativeBorderSlotRefV1>,
}

fn render_decorative_border_slot_v1(
    slot: ViewerDecorativeBorderSlotV1,
) -> RenderDecorativeBorderSlotV1 {
    match slot {
        ViewerDecorativeBorderSlotV1::TopLeft => RenderDecorativeBorderSlotV1::TopLeft,
        ViewerDecorativeBorderSlotV1::Top => RenderDecorativeBorderSlotV1::Top,
        ViewerDecorativeBorderSlotV1::TopRight => RenderDecorativeBorderSlotV1::TopRight,
        ViewerDecorativeBorderSlotV1::Right => RenderDecorativeBorderSlotV1::Right,
        ViewerDecorativeBorderSlotV1::BottomRight => RenderDecorativeBorderSlotV1::BottomRight,
        ViewerDecorativeBorderSlotV1::Bottom => RenderDecorativeBorderSlotV1::Bottom,
        ViewerDecorativeBorderSlotV1::BottomLeft => RenderDecorativeBorderSlotV1::BottomLeft,
        ViewerDecorativeBorderSlotV1::Left => RenderDecorativeBorderSlotV1::Left,
    }
}

fn render_decorative_border_v1(
    visual: &ViewerGeometryDocument,
    node_id: NodeId,
) -> Option<RenderDecorativeBorderV1> {
    let border = visual
        .decorative_borders
        .iter()
        .find(|border| border.node_id == node_id)?;
    (border.slots.len() == 8).then(|| RenderDecorativeBorderV1 {
        name: border.name.clone(),
        corner_extent_emu: border.corner_extent_emu,
        horizontal_extent_emu: border.horizontal_extent_emu,
        vertical_extent_emu: border.vertical_extent_emu,
        stretch_pictures: border.stretch_pictures,
        slots: border
            .slots
            .iter()
            .map(|slot| RenderDecorativeBorderSlotRefV1 {
                slot: render_decorative_border_slot_v1(slot.slot),
                resource_id: slot.resource_id,
            })
            .collect(),
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenderDecorativeBorderPlacementV1 {
    pub slot: RenderDecorativeBorderSlotV1,
    pub resource_id: ResourceId,
    pub bounds: RectEmu,
}

fn decorative_border_slot_rank_v1(slot: RenderDecorativeBorderSlotV1) -> u8 {
    match slot {
        RenderDecorativeBorderSlotV1::TopLeft => 0,
        RenderDecorativeBorderSlotV1::Top => 1,
        RenderDecorativeBorderSlotV1::TopRight => 2,
        RenderDecorativeBorderSlotV1::Right => 3,
        RenderDecorativeBorderSlotV1::BottomRight => 4,
        RenderDecorativeBorderSlotV1::Bottom => 5,
        RenderDecorativeBorderSlotV1::BottomLeft => 6,
        RenderDecorativeBorderSlotV1::Left => 7,
    }
}

fn decorative_border_resource_v1(
    border: &RenderDecorativeBorderV1,
    slot: RenderDecorativeBorderSlotV1,
) -> Option<ResourceId> {
    let mut matches = border.slots.iter().filter(|entry| entry.slot == slot);
    let resource_id = matches.next()?.resource_id;
    matches.next().is_none().then_some(resource_id)
}

fn rect_emu_v1(x: i64, y: i64, width: i64, height: i64) -> Option<RectEmu> {
    (width > 0 && height > 0).then(|| {
        RectEmu::new(
            LengthEmu::new(x),
            LengthEmu::new(y),
            LengthEmu::new(width),
            LengthEmu::new(height),
        )
    })
}

fn rounded_positive_tile_count_v1(interior: i64, border_width: i64) -> Option<i64> {
    if interior <= 0 || border_width <= 0 {
        return None;
    }
    Some(((interior + border_width / 2) / border_width).max(1))
}

/// Resolves only the source-neutral destination rectangles for one decorative border.
///
/// This deliberately consumes the already-admitted border width only as placement
/// geometry. It does not turn BorderArt into an ordinary stroke and does not infer
/// any authoring semantics. stretch_pictures must come from the bounded Reader
/// marker projection; callers must fail closed when that state is unknown.
pub fn layout_decorative_border_v1(
    border: &RenderDecorativeBorderV1,
    bounds: RectEmu,
    border_width_emu: i64,
    stretch_pictures: bool,
) -> Option<Vec<RenderDecorativeBorderPlacementV1>> {
    let width = bounds.width.get();
    let height = bounds.height.get();
    let x = bounds.x.get();
    let y = bounds.y.get();
    let b = border_width_emu;

    if b <= 0 || width < 2 * b || height < 2 * b {
        return None;
    }

    let ordered_slots = [
        RenderDecorativeBorderSlotV1::TopLeft,
        RenderDecorativeBorderSlotV1::Top,
        RenderDecorativeBorderSlotV1::TopRight,
        RenderDecorativeBorderSlotV1::Right,
        RenderDecorativeBorderSlotV1::BottomRight,
        RenderDecorativeBorderSlotV1::Bottom,
        RenderDecorativeBorderSlotV1::BottomLeft,
        RenderDecorativeBorderSlotV1::Left,
    ];
    let resources = ordered_slots
        .iter()
        .copied()
        .map(|slot| decorative_border_resource_v1(border, slot).map(|resource| (slot, resource)))
        .collect::<Option<Vec<_>>>()?;
    if border.slots.len() != ordered_slots.len() {
        return None;
    }
    let resource = |slot| {
        resources
            .iter()
            .find(|(candidate, _)| *candidate == slot)
            .map(|(_, resource_id)| *resource_id)
    };

    let mut placements = Vec::new();
    let mut push = |slot, px, py, pw, ph| -> Option<()> {
        placements.push(RenderDecorativeBorderPlacementV1 {
            slot,
            resource_id: resource(slot)?,
            bounds: rect_emu_v1(px, py, pw, ph)?,
        });
        Some(())
    };

    push(RenderDecorativeBorderSlotV1::TopLeft, x, y, b, b)?;
    push(
        RenderDecorativeBorderSlotV1::TopRight,
        x + width - b,
        y,
        b,
        b,
    )?;
    push(
        RenderDecorativeBorderSlotV1::BottomRight,
        x + width - b,
        y + height - b,
        b,
        b,
    )?;
    push(
        RenderDecorativeBorderSlotV1::BottomLeft,
        x,
        y + height - b,
        b,
        b,
    )?;

    let horizontal_interior = width - 2 * b;
    let vertical_interior = height - 2 * b;

    if stretch_pictures {
        if horizontal_interior > 0 {
            let count = rounded_positive_tile_count_v1(horizontal_interior, b)?;
            for index in 0..count {
                let start = b + index * horizontal_interior / count;
                let end = b + (index + 1) * horizontal_interior / count;
                let tile_width = end - start;
                push(
                    RenderDecorativeBorderSlotV1::Top,
                    x + start,
                    y,
                    tile_width,
                    b,
                )?;
                push(
                    RenderDecorativeBorderSlotV1::Bottom,
                    x + width - end,
                    y + height - b,
                    tile_width,
                    b,
                )?;
            }
        }
        if vertical_interior > 0 {
            let count = rounded_positive_tile_count_v1(vertical_interior, b)?;
            for index in 0..count {
                let start = b + index * vertical_interior / count;
                let end = b + (index + 1) * vertical_interior / count;
                let tile_height = end - start;
                push(
                    RenderDecorativeBorderSlotV1::Right,
                    x + width - b,
                    y + start,
                    b,
                    tile_height,
                )?;
                push(
                    RenderDecorativeBorderSlotV1::Left,
                    x,
                    y + height - end,
                    b,
                    tile_height,
                )?;
            }
        }
    } else {
        let horizontal_count = width / b;
        let vertical_count = height / b;
        if horizontal_count < 2 || vertical_count < 2 {
            return None;
        }

        if horizontal_count > 2 {
            let residual = width - horizontal_count * b;
            let gaps = horizontal_count - 1;
            for index in 1..horizontal_count - 1 {
                let offset = index * b + (index * residual + gaps / 2) / gaps;
                push(RenderDecorativeBorderSlotV1::Top, x + offset, y, b, b)?;
                push(
                    RenderDecorativeBorderSlotV1::Bottom,
                    x + width - b - offset,
                    y + height - b,
                    b,
                    b,
                )?;
            }
        }

        if vertical_count > 2 {
            let residual = height - vertical_count * b;
            let gaps = vertical_count - 1;
            for index in 1..vertical_count - 1 {
                let offset = index * b + (index * residual + gaps / 2) / gaps;
                push(
                    RenderDecorativeBorderSlotV1::Right,
                    x + width - b,
                    y + offset,
                    b,
                    b,
                )?;
                push(
                    RenderDecorativeBorderSlotV1::Left,
                    x,
                    y + height - b - offset,
                    b,
                    b,
                )?;
            }
        }
    }

    placements.sort_by_key(|placement| decorative_border_slot_rank_v1(placement.slot));
    Some(placements)
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paragraph_alignments: Vec<RenderParagraphAlignmentRunV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backend_font_resource_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layout: Option<RenderTextLayoutV1>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RenderParagraphAlignmentV1 {
    Center,
    Right,
    InterWord,
    Distribute,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenderParagraphAlignmentRunV1 {
    pub scalar_start: u32,
    pub scalar_end: u32,
    pub alignment: RenderParagraphAlignmentV1,
    pub source_value: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenderTypographyRunV1 {
    pub scalar_start: u32,
    pub scalar_end: u32,
    pub source_font_name: String,
    pub text_size_emu: u32,
    pub font_inherited: bool,
    pub size_inherited: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color_rgb: Option<[u8; 3]>,
    #[serde(default)]
    pub color_inherited: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bold: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub italic: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ScalarSourceFontFamilyAuthorityV1 {
    Authoritative(String),
    Absent,
    Invalid,
}

pub fn uniform_text_color_rgb_v1(fragment: &RenderTextFragmentV1) -> Option<[u8; 3]> {
    if fragment.scalar_start >= fragment.scalar_end || fragment.typography.is_empty() {
        return None;
    }

    let mut cursor = fragment.scalar_start;
    let mut resolved = None;
    for run in &fragment.typography {
        if run.scalar_start != cursor
            || run.scalar_end <= run.scalar_start
            || run.scalar_end > fragment.scalar_end
        {
            return None;
        }
        let color = run.color_rgb?;
        match resolved {
            None => resolved = Some(color),
            Some(existing) if existing == color => {}
            Some(_) => return None,
        }
        cursor = run.scalar_end;
    }

    (cursor == fragment.scalar_end)
        .then_some(resolved)
        .flatten()
}

fn normalize_source_font_family_v1(name: &str) -> String {
    name.trim().to_lowercase()
}

fn scalar_source_font_family_authority_v1(
    fragment: &RenderTextFragmentV1,
) -> ScalarSourceFontFamilyAuthorityV1 {
    if fragment.typography.is_empty() {
        return ScalarSourceFontFamilyAuthorityV1::Absent;
    }

    let mut cursor = fragment.scalar_start;
    let mut family: Option<(String, String)> = None;
    let mut blank_family = false;

    for run in &fragment.typography {
        if run.scalar_start != cursor
            || run.scalar_end <= run.scalar_start
            || run.scalar_end > fragment.scalar_end
        {
            return ScalarSourceFontFamilyAuthorityV1::Invalid;
        }

        let display = run.source_font_name.trim();
        if display.is_empty() {
            blank_family = true;
        } else {
            if blank_family {
                return ScalarSourceFontFamilyAuthorityV1::Invalid;
            }
            let normalized = normalize_source_font_family_v1(display);
            match family.as_ref() {
                None => family = Some((display.to_owned(), normalized)),
                Some((_, existing)) if *existing == normalized => {}
                Some(_) => return ScalarSourceFontFamilyAuthorityV1::Invalid,
            }
        }
        cursor = run.scalar_end;
    }

    if cursor != fragment.scalar_end {
        return ScalarSourceFontFamilyAuthorityV1::Invalid;
    }

    match (family, blank_family) {
        (Some((display, _)), false) => ScalarSourceFontFamilyAuthorityV1::Authoritative(display),
        (None, true) => ScalarSourceFontFamilyAuthorityV1::Absent,
        _ => ScalarSourceFontFamilyAuthorityV1::Invalid,
    }
}

fn one_resolved_script_font_entry_v1(
    entries: &[pub_viewer::ViewerScriptFontEntry],
    script_slot: u16,
) -> Option<&pub_viewer::ViewerScriptFontEntry> {
    let mut matches = entries
        .iter()
        .filter(|entry| entry.script_slot == script_slot);
    let entry = matches.next()?;
    if matches.next().is_some()
        || entry.disposition != ViewerScriptFontEntryDisposition::Resolved
        || entry
            .source_font_name
            .as_deref()
            .is_none_or(|name| name.trim().is_empty())
    {
        return None;
    }
    Some(entry)
}

fn ascii_latin_script_font_family_v1(
    visual: &ViewerGeometryDocument,
    fragment: &RenderTextFragmentV1,
) -> Option<String> {
    if fragment.text.is_empty() || !fragment.text.is_ascii() {
        return None;
    }

    let story = visual
        .document
        .stories
        .iter()
        .find(|story| story.id == fragment.story_id)?;

    let mut maps = visual
        .script_font_maps
        .iter()
        .filter(|map| map.story_id == fragment.story_id)
        .filter(|map| map.applies_to_story_text(&story.text))
        .filter(|map| {
            map.scalar_end > fragment.scalar_start && map.scalar_start < fragment.scalar_end
        })
        .collect::<Vec<_>>();
    maps.sort_by_key(|map| (map.scalar_start, map.scalar_end));

    let mut cursor = fragment.scalar_start;
    let mut selected: Option<(u32, String, String)> = None;

    for map in maps {
        let start = map.scalar_start.max(fragment.scalar_start);
        let end = map.scalar_end.min(fragment.scalar_end);
        if start != cursor || end <= start {
            return None;
        }

        let default = one_resolved_script_font_entry_v1(&map.entries, 0)?;
        let ascii_latin = one_resolved_script_font_entry_v1(&map.entries, 1)?;
        let latin = one_resolved_script_font_entry_v1(&map.entries, 2)?;

        let entries = [default, ascii_latin, latin];
        let first_name = entries[0].source_font_name.as_deref()?.trim();
        let first = (
            entries[0].source_font_index,
            first_name.to_owned(),
            normalize_source_font_family_v1(first_name),
        );
        if entries.iter().skip(1).any(|entry| {
            let Some(name) = entry.source_font_name.as_deref() else {
                return true;
            };
            entry.source_font_index != first.0 || normalize_source_font_family_v1(name) != first.2
        }) {
            return None;
        }

        match selected.as_ref() {
            None => selected = Some(first),
            Some((ordinal, _, normalized)) if *ordinal == first.0 && *normalized == first.2 => {}
            Some(_) => return None,
        }
        cursor = end;
    }

    if cursor != fragment.scalar_end {
        return None;
    }

    selected.map(|(_, display, _)| display)
}

/// Returns a complete scalar-typography family only when the fragment has
/// contiguous, unambiguous non-blank family authority.
pub fn complete_scalar_source_font_family_v1(fragment: &RenderTextFragmentV1) -> Option<String> {
    match scalar_source_font_family_authority_v1(fragment) {
        ScalarSourceFontFamilyAuthorityV1::Authoritative(family) => Some(family),
        ScalarSourceFontFamilyAuthorityV1::Absent | ScalarSourceFontFamilyAuthorityV1::Invalid => {
            None
        }
    }
}

/// Returns bounded source family authority for a render fragment without
/// inventing font size, style, substitution, or general script fallback.
///
/// Complete scalar typography wins. ScriptFonts is admitted only for an
/// ASCII-only fragment whose Default/AsciiLatin/Latin slots agree exactly
/// across complete contiguous source ranges.
pub fn effective_source_font_family_v1(
    visual: &ViewerGeometryDocument,
    fragment: &RenderTextFragmentV1,
) -> Option<String> {
    match scalar_source_font_family_authority_v1(fragment) {
        ScalarSourceFontFamilyAuthorityV1::Authoritative(family) => Some(family),
        ScalarSourceFontFamilyAuthorityV1::Absent => {
            ascii_latin_script_font_family_v1(visual, fragment)
        }
        ScalarSourceFontFamilyAuthorityV1::Invalid => None,
    }
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
    #[serde(default, skip_serializing_if = "is_zero_i64")]
    pub vertical_offset_emu: i64,
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
pub struct RenderResolvedShapingV1 {
    pub environment: BoundedShapingDescriptor,
    pub units_per_em: u32,
    pub glyphs: Vec<BoundedShapedGlyph>,
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
    #[serde(default, skip_serializing_if = "is_zero_i64")]
    pub x_offset_emu: i64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub spans: Vec<RenderResolvedTextSpanV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shaping: Option<RenderResolvedShapingV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenderResolvedTextSpanV1 {
    pub scalar_start: u32,
    pub scalar_end: u32,
    pub text: String,
    pub x_offset_emu: i64,
    pub measured_width_emu: i64,
    pub font_size_emu: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_resource_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_fingerprint_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shaping: Option<RenderResolvedShapingV1>,
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
    AuthoredLanePageMismatch {
        plan_page_id: PageId,
        lane_page_id: PageId,
    },
    AuthoredLaneDuplicateNode {
        node_id: NodeId,
    },
    AuthoredLaneBaseCollision {
        node_id: NodeId,
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
            Self::AuthoredLanePageMismatch {
                plan_page_id,
                lane_page_id,
            } => write!(
                formatter,
                "authored render lane page {lane_page_id:?} does not match render-plan page {plan_page_id:?}"
            ),
            Self::AuthoredLaneDuplicateNode { node_id } => {
                write!(
                    formatter,
                    "authored render lane contains duplicate node {node_id:?}"
                )
            }
            Self::AuthoredLaneBaseCollision { node_id } => write!(
                formatter,
                "authored render lane node {node_id:?} collides with the existing base render lane"
            ),
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

fn render_paragraph_alignment_runs_v1(
    visual: &ViewerGeometryDocument,
    story_id: StoryId,
    story_text: &str,
    scalar_start: u32,
    scalar_end: u32,
) -> Vec<RenderParagraphAlignmentRunV1> {
    visual
        .paragraph_alignments
        .iter()
        .filter(|run| run.story_id == story_id)
        .filter(|run| run.applies_to_story_text(story_text))
        .filter_map(|run| {
            let start = run.scalar_start.max(scalar_start);
            let end = run.scalar_end.min(scalar_end);
            (start < end).then_some(RenderParagraphAlignmentRunV1 {
                scalar_start: start,
                scalar_end: end,
                alignment: match run.alignment {
                    ViewerParagraphAlignment::Center => RenderParagraphAlignmentV1::Center,
                    ViewerParagraphAlignment::Right => RenderParagraphAlignmentV1::Right,
                    ViewerParagraphAlignment::InterWord => RenderParagraphAlignmentV1::InterWord,
                    ViewerParagraphAlignment::Distribute => RenderParagraphAlignmentV1::Distribute,
                },
                source_value: run.source_value,
            })
        })
        .collect()
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

    let (scalar_start, scalar_end, text, line_count) =
        if instance.scene_instance.projection_kind == SceneProjectionKindV1::InheritedMaster {
            let origin_node_id =
                parse_node_id(&instance.scene_instance.origin_node_id, "origin_node_id")?;
            let Some(fragment) = visual.text_fragments.iter().find(|fragment| {
                fragment.frame_id == origin_node_id && fragment.story_id == story_id
            }) else {
                return Ok(None);
            };
            (
                fragment.scalar_start,
                fragment.scalar_end,
                fragment.text.clone(),
                fragment.line_count,
            )
        } else {
            let scalar_end = u32::try_from(story.text.chars().count()).unwrap_or(u32::MAX);
            (0, scalar_end, story.text.clone(), 0)
        };

    let typography = visual
        .typography_runs
        .iter()
        .filter(|run| run.story_id == story_id)
        .filter(|run| run.applies_to_story_text(&story.text))
        .filter_map(|run| {
            let run_start = run.scalar_start.max(scalar_start);
            let run_end = run.scalar_end.min(scalar_end);
            (run_start < run_end).then(|| RenderTypographyRunV1 {
                scalar_start: run_start,
                scalar_end: run_end,
                source_font_name: run.source_font_name.clone(),
                text_size_emu: run.text_size_emu,
                font_inherited: run.font_inherited,
                size_inherited: run.size_inherited,
                color_rgb: run.color_rgb,
                color_inherited: run.color_inherited,
                bold: run.bold.map(|value| value.effective_value),
                italic: run.italic.map(|value| value.effective_value),
            })
        })
        .collect();

    Ok(Some(RenderTextFragmentV1 {
        story_id,
        scalar_start,
        scalar_end,
        text,
        line_count,
        typography,
        paragraph_alignments: render_paragraph_alignment_runs_v1(
            visual,
            story_id,
            &story.text,
            scalar_start,
            scalar_end,
        ),
        backend_font_resource_id: None,
        layout: None,
    }))
}

fn unique_story_frame_for_node(
    visual: &ViewerGeometryDocument,
    node_id: NodeId,
) -> Option<&ViewerStoryFrame> {
    let mut matches = visual
        .story_frames
        .iter()
        .filter(|frame| frame.frame_id == node_id);
    let first = matches.next()?;
    matches.next().is_none().then_some(first)
}

fn resolved_vertical_offset_emu_v1(
    alignment: Option<ViewerTextVerticalAlignment>,
    content_height_emu: i64,
    laid_out_height_emu: i64,
) -> i64 {
    if content_height_emu <= 0
        || laid_out_height_emu < 0
        || laid_out_height_emu > content_height_emu
    {
        return 0;
    }
    let remaining = content_height_emu - laid_out_height_emu;
    match alignment {
        Some(ViewerTextVerticalAlignment::Center) => remaining / 2,
        Some(ViewerTextVerticalAlignment::Bottom) => remaining,
        Some(ViewerTextVerticalAlignment::Top) | None => 0,
    }
}

fn uniform_laid_out_height_emu_v1(
    first_line_extent_emu: i64,
    baseline_advance_emu: i64,
    line_count: usize,
) -> Option<i64> {
    if line_count == 0 {
        return Some(0);
    }
    if first_line_extent_emu <= 0 || baseline_advance_emu <= 0 {
        return None;
    }
    let subsequent_count = i64::try_from(line_count.checked_sub(1)?).ok()?;
    first_line_extent_emu.checked_add(subsequent_count.checked_mul(baseline_advance_emu)?)
}

/// Append the authoritative Chaptera-authored lane after the existing base/imported
/// lane without changing either lane's internal order.
///
/// The caller owns provenance admission and converts its authored runtime state
/// into this source-neutral render shape. This function owns only the effective
/// paint-order composition law shared by every render-plan consumer.
pub fn apply_authored_page_render_lane_v1(
    plan: &mut PageRenderPlanV1,
    lane: &AuthoredPageRenderLaneV1,
) -> Result<(), RenderPlanErrorV1> {
    if plan.page_id != lane.page_id {
        return Err(RenderPlanErrorV1::AuthoredLanePageMismatch {
            plan_page_id: plan.page_id,
            lane_page_id: lane.page_id,
        });
    }

    let base_ids = plan
        .nodes
        .iter()
        .map(|node| node.node_id)
        .collect::<BTreeSet<_>>();
    let mut seen = BTreeSet::new();
    for authored in &lane.nodes {
        if !seen.insert(authored.node_id) {
            return Err(RenderPlanErrorV1::AuthoredLaneDuplicateNode {
                node_id: authored.node_id,
            });
        }
        if base_ids.contains(&authored.node_id) {
            return Err(RenderPlanErrorV1::AuthoredLaneBaseCollision {
                node_id: authored.node_id,
            });
        }
    }

    plan.nodes
        .extend(lane.nodes.iter().map(|authored| NodeRenderPlanV1 {
            node_id: authored.node_id,
            #[cfg(feature = "projected-scene-instances")]
            projected_scene_instance: None,
            bounds: authored.bounds,
            text_bounds: None,
            transform: Affine2D::identity(),
            solid_fill_rgb: authored.solid_fill_rgb,
            solid_line: authored.solid_line.clone(),
            decorative_border: None,
            image: None,
            text: None,
            table: None,
        }));
    Ok(())
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
                                color_rgb: run.color_rgb,
                                color_inherited: run.color_inherited,
                                bold: run.bold.map(|value| value.effective_value),
                                italic: run.italic.map(|value| value.effective_value),
                            })
                        })
                        .collect(),
                    paragraph_alignments: visual
                        .document
                        .stories
                        .iter()
                        .find(|story| story.id == fragment.story_id)
                        .map(|story| {
                            render_paragraph_alignment_runs_v1(
                                visual,
                                fragment.story_id,
                                &story.text,
                                fragment.scalar_start,
                                fragment.scalar_end,
                            )
                        })
                        .unwrap_or_default(),
                    backend_font_resource_id: None,
                    layout: None,
                });
            #[cfg(feature = "projected-scene-instances")]
            {
                let projected_for_frame = visual.projected_instances.iter().filter(|projected| {
                    projected.scene_instance.target_page_id == page.id.as_canonical().to_string()
                        && projected.target_frame_node_id == Some(node.origin)
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
                            fill_rgb: cell.fill_rgb,
                            fill_visible: cell.fill_visible,
                        })
                        .collect(),
                });
            let text_bounds = unique_story_frame_for_node(visual, node.origin)
                .and_then(|frame| frame.text_content_bounds);
            NodeRenderPlanV1 {
                node_id: node.origin,
                #[cfg(feature = "projected-scene-instances")]
                projected_scene_instance: None,
                bounds: node.bounds,
                text_bounds,
                transform: node.transform.clone(),
                solid_fill_rgb: paint.and_then(|paint| paint.solid_fill_rgb),
                solid_line: paint
                    .and_then(|paint| paint.solid_line.as_ref())
                    .map(|line| RenderSolidLineV1 {
                        rgb: line.rgb,
                        width_emu: line.width_emu,
                    }),
                decorative_border: render_decorative_border_v1(visual, node.origin),
                image,
                text,
                table,
            }
        })
        .collect::<Vec<_>>();

    #[cfg(feature = "projected-scene-instances")]
    {
        let mut inherited_master_nodes = Vec::new();

        for projected in visual.projected_instances.iter().filter(|projected| {
            projected.scene_instance.target_page_id == page.id.as_canonical().to_string()
        }) {
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
            let table = visual
                .tables
                .iter()
                .find(|table| table.node_id == origin_node_id)
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
                            fill_rgb: cell.fill_rgb,
                            fill_visible: cell.fill_visible,
                        })
                        .collect(),
                });
            let node = NodeRenderPlanV1 {
                node_id: origin_node_id,
                projected_scene_instance: Some(projected.scene_instance.clone()),
                bounds: projected.bounds,
                text_bounds: projected.text_content_bounds,
                transform: projected.transform.clone(),
                solid_fill_rgb: paint.and_then(|paint| paint.solid_fill_rgb),
                solid_line: paint
                    .and_then(|paint| paint.solid_line.as_ref())
                    .map(|line| RenderSolidLineV1 {
                        rgb: line.rgb,
                        width_emu: line.width_emu,
                    }),
                decorative_border: render_decorative_border_v1(visual, origin_node_id),
                image,
                text: projected_text(visual, projected)?,
                table,
            };

            match projected.scene_instance.projection_kind {
                SceneProjectionKindV1::InheritedMaster => {
                    if projected.target_frame_node_id.is_some() {
                        return Err(RenderPlanErrorV1::ProjectedKindUnsupported {
                            instance_id: projected.scene_instance.instance_id.clone(),
                        });
                    }
                    inherited_master_nodes.push(node);
                }
                SceneProjectionKindV1::CmoStorySlot => {
                    let Some(target_frame_node_id) = projected.target_frame_node_id else {
                        return Err(RenderPlanErrorV1::ProjectedKindUnsupported {
                            instance_id: projected.scene_instance.instance_id.clone(),
                        });
                    };
                    let insert_at = nodes
                        .iter()
                        .position(|candidate| {
                            candidate.projected_scene_instance.is_none()
                                && candidate.node_id == target_frame_node_id
                        })
                        .map(|index| index + 1)
                        .unwrap_or(nodes.len());
                    nodes.insert(insert_at, node);
                }
                SceneProjectionKindV1::DirectPageLocal => {
                    return Err(RenderPlanErrorV1::ProjectedKindUnsupported {
                        instance_id: projected.scene_instance.instance_id.clone(),
                    });
                }
            }
        }

        // Native Publisher acceptance proves the bounded ordinary single-master
        // stacking law: inherited master paints below page-local content. The
        // Viewer producer already preserves source order within the master lane.
        inherited_master_nodes.extend(nodes);
        nodes = inherited_master_nodes;
    }

    Ok(PageRenderPlanV1 {
        schema_version: PAGE_RENDER_PLAN_SCHEMA_V1.to_owned(),
        page_id: page.id,
        page_size: surface.size,
        nodes,
    })
}

#[derive(Clone)]
struct RenderTextLayoutTargetV1 {
    page_id: PageId,
    page_size: Size2D,
    node_id: NodeId,
    projected_target_frame_node_id: Option<NodeId>,
    vertical_alignment: Option<ViewerTextVerticalAlignment>,
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
    resolve_font: F,
) -> Result<PageRenderPlanV1, RenderPlanErrorV1>
where
    F: FnMut(&RenderTextFragmentV1) -> Option<ExplicitRenderTextFontResourceV1<'a>>,
{
    build_page_render_plan_with_text_layout_resolvers_v1(
        visual,
        page_index,
        fallback_font,
        resolve_font,
        |_, _| None,
    )
}

pub fn build_page_render_plan_with_text_layout_resolvers_v1<'a, F, G>(
    visual: &ViewerGeometryDocument,
    page_index: usize,
    fallback_font: &ExplicitRenderTextFontResourceV1<'a>,
    mut resolve_font: F,
    mut resolve_span_font: G,
) -> Result<PageRenderPlanV1, RenderPlanErrorV1>
where
    F: FnMut(&RenderTextFragmentV1) -> Option<ExplicitRenderTextFontResourceV1<'a>>,
    G: FnMut(
        &RenderTextFragmentV1,
        &RenderTypographyRunV1,
    ) -> Option<ExplicitRenderTextFontResourceV1<'a>>,
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
                        .and_then(|projected| projected.target_frame_node_id)
                })
            }
            #[cfg(not(feature = "projected-scene-instances"))]
            {
                None
            }
        };
        let vertical_alignment = if projected_target_frame_node_id.is_none() {
            unique_story_frame_for_node(visual, node.node_id)
                .and_then(|frame| frame.vertical_alignment)
        } else {
            None
        };
        let target = RenderTextLayoutTargetV1 {
            page_id,
            page_size,
            node_id: node.node_id,
            projected_target_frame_node_id,
            vertical_alignment,
            bounds: node.text_bounds.unwrap_or(node.bounds),
            transform: node.transform.clone(),
        };
        let resolved_font = resolve_font(fragment);
        if resolved_font.is_none()
            && projected_target_frame_node_id.is_none()
            && let Some(layout) = resolve_mixed_family_text_layout_v1(
                visual,
                target.clone(),
                fragment,
                &mut resolve_span_font,
            )
        {
            fragment.backend_font_resource_id = None;
            fragment.layout = Some(layout);
            continue;
        }
        let font_is_source_resolved = resolved_font.is_some();
        fragment.backend_font_resource_id = resolved_font
            .as_ref()
            .map(|font| font.resource_id.to_owned());
        let font = resolved_font.as_ref().unwrap_or(fallback_font);
        fragment.layout = Some(resolve_text_layout_v1(
            visual,
            target,
            fragment,
            font,
            font_is_source_resolved,
        ));
    }

    Ok(plan)
}

fn resolved_line_x_offset_emu_v1(
    fragment: &RenderTextFragmentV1,
    node_id: NodeId,
    bounds: &RectEmu,
    line_index: u32,
    scalar_range: std::ops::Range<u32>,
    measured_width_emu: i64,
    layout_environment_fingerprint: &str,
) -> i64 {
    let scalar_start = scalar_range.start;
    let scalar_end = scalar_range.end;
    let mut matching = fragment.paragraph_alignments.iter().filter(|run| {
        run.scalar_start <= scalar_start
            && run.scalar_end >= scalar_end
            && scalar_start < scalar_end
    });
    let Some(run) = matching.next() else {
        return 0;
    };
    if matching.next().is_some() {
        return 0;
    }
    let alignment = match run.alignment {
        RenderParagraphAlignmentV1::Center => ParagraphAlignmentV1::Center,
        RenderParagraphAlignmentV1::Right => ParagraphAlignmentV1::Right,
        RenderParagraphAlignmentV1::InterWord | RenderParagraphAlignmentV1::Distribute => {
            return 0;
        }
    };
    let input = ParagraphLinePlacementInputV1 {
        context: LayoutPlacementContextV1 {
            authoring_revision: SHARED_TEXT_LAYOUT_REVISION_V1.to_owned(),
            layout_environment_fingerprint: layout_environment_fingerprint.to_owned(),
        },
        alignment,
        story_overset: false,
        lines: vec![ResolvedLineInputV1 {
            line_index: 0,
            story_id: story_id_string(fragment.story_id),
            frame_node_id: node_id_string(node_id),
            frame_line_index: line_index,
            scalar_start,
            scalar_end,
            content_leading_x_emu: 0,
            content_width_emu: bounds.width.get(),
            measured_width_emu,
        }],
    };
    resolve_paragraph_line_placement_v1(&input)
        .ok()
        .and_then(|scene| scene.lines.into_iter().next())
        .map(|line| line.line_origin_x_emu)
        .unwrap_or(0)
}

fn story_id_string(id: StoryId) -> String {
    format!("{:?}", id)
}

fn node_id_string(id: NodeId) -> String {
    format!("{:?}", id)
}

fn fallback_layout(reason: RenderTextLayoutFallbackReasonV1) -> RenderTextLayoutV1 {
    RenderTextLayoutV1 {
        disposition: RenderTextLayoutDispositionV1::BackendFallback { reason },
        vertical_offset_emu: 0,
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

const PUBLISHER_SINGLE_POINT_EQUIVALENT_EMU_V1: u32 = 12 * 12_700;
const PUBLISHER_ONE_POINT_FIVE_POINT_EQUIVALENT_EMU_V1: u32 = 18 * 12_700;

fn source_paragraph_line_spacing_v1(
    visual: &ViewerGeometryDocument,
    fragment: &RenderTextFragmentV1,
) -> Option<ViewerParagraphLineSpacing> {
    let story = visual
        .document
        .stories
        .iter()
        .find(|story| story.id == fragment.story_id)?;

    let mut intersecting = visual
        .paragraph_line_spacings
        .iter()
        .filter(|run| run.story_id == fragment.story_id)
        .filter(|run| run.applies_to_story_text(&story.text))
        .filter(|run| {
            run.scalar_end > fragment.scalar_start && run.scalar_start < fragment.scalar_end
        });

    let run = intersecting.next()?;
    if intersecting.next().is_some()
        || run.scalar_start > fragment.scalar_start
        || run.scalar_end < fragment.scalar_end
    {
        return None;
    }
    Some(run.line_spacing)
}

fn scale_proportional_line_height_emu_v1(
    natural_line_height_emu: i64,
    point_equivalent_emu: u32,
) -> Option<i64> {
    if natural_line_height_emu <= 0
        || !matches!(
            point_equivalent_emu,
            PUBLISHER_SINGLE_POINT_EQUIVALENT_EMU_V1
                | PUBLISHER_ONE_POINT_FIVE_POINT_EQUIVALENT_EMU_V1
        )
    {
        return None;
    }

    let numerator = i128::from(natural_line_height_emu) * i128::from(point_equivalent_emu);
    let denominator = i128::from(PUBLISHER_SINGLE_POINT_EQUIVALENT_EMU_V1);
    let rounded = (numerator + denominator / 2) / denominator;
    let line_height_emu = i64::try_from(rounded).ok()?;
    (line_height_emu > 0).then_some(line_height_emu)
}

fn resolved_uniform_line_height_emu_v1(
    visual: &ViewerGeometryDocument,
    fragment: &RenderTextFragmentV1,
    font_size_emu: i64,
    font: &ExplicitRenderTextFontResourceV1<'_>,
    font_is_source_resolved: bool,
) -> Option<i64> {
    if let Some(line_spacing) = source_paragraph_line_spacing_v1(visual, fragment) {
        match line_spacing {
            ViewerParagraphLineSpacing::Absolute { spacing_emu } if spacing_emu > 0 => {
                return Some(i64::from(spacing_emu));
            }
            ViewerParagraphLineSpacing::Proportional {
                point_equivalent_emu,
            } if font_is_source_resolved => {
                if let Some(natural_line_height_emu) = compatible_natural_line_height_emu_v1(
                    font.bytes,
                    font.face_index,
                    LengthEmu::new(font_size_emu),
                )
                .map(LengthEmu::get)
                    && let Some(line_height_emu) = scale_proportional_line_height_emu_v1(
                        natural_line_height_emu,
                        point_equivalent_emu,
                    )
                {
                    return Some(line_height_emu);
                }
            }
            ViewerParagraphLineSpacing::Absolute { .. }
            | ViewerParagraphLineSpacing::Proportional { .. } => {}
        }
    }

    scaled_line_height_emu(
        font_size_emu,
        font.default_font_size_emu,
        font.default_line_height_emu,
    )
}

fn resolve_text_layout_v1(
    visual: &ViewerGeometryDocument,
    target: RenderTextLayoutTargetV1,
    fragment: &RenderTextFragmentV1,
    font: &ExplicitRenderTextFontResourceV1<'_>,
    font_is_source_resolved: bool,
) -> RenderTextLayoutV1 {
    let RenderTextLayoutTargetV1 {
        page_id,
        page_size,
        node_id,
        projected_target_frame_node_id,
        vertical_alignment,
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
            return resolve_mixed_size_text_layout_v1(
                fragment,
                font,
                node_id,
                &bounds,
                &fingerprint,
                vertical_alignment,
            );
        }
        Err(reason) => return fallback_layout(reason),
    };
    let Some(line_height_emu) = resolved_uniform_line_height_emu_v1(
        visual,
        fragment,
        font_size_emu,
        font,
        font_is_source_resolved,
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
    let shaping_environment = scene.environment.shaping.clone();
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

    let lines: Vec<RenderResolvedTextLineV1> = source_lines
        .into_iter()
        .map(|line| RenderResolvedTextLineV1 {
            line_index: line.frame_line_index,
            scalar_start: line.scalar_start,
            scalar_end: line.scalar_end,
            consumed_scalar_end: line.consumed_scalar_end,
            text: line.text,
            measured_width_emu: line.measured_width.get(),
            line_height_emu,
            x_offset_emu: resolved_line_x_offset_emu_v1(
                fragment,
                node_id,
                &bounds,
                line.frame_line_index,
                line.scalar_start..line.scalar_end,
                line.measured_width.get(),
                &fingerprint,
            ),
            spans: Vec::new(),
            shaping: Some(RenderResolvedShapingV1 {
                environment: shaping_environment.clone(),
                units_per_em: line.units_per_em,
                glyphs: line.glyphs,
            }),
        })
        .collect();

    let first_line_extent_emu = compatible_natural_line_height_emu_v1(
        font.bytes,
        font.face_index,
        LengthEmu::new(font_size_emu),
    )
    .map(LengthEmu::get)
    .map(|extent| extent.min(line_height_emu))
    .unwrap_or(line_height_emu);
    let laid_out_height_emu =
        uniform_laid_out_height_emu_v1(first_line_extent_emu, line_height_emu, lines.len())
            .unwrap_or(0);
    RenderTextLayoutV1 {
        disposition: RenderTextLayoutDispositionV1::SharedResolved {
            font_resource_id: font.resource_id.to_owned(),
            font_fingerprint_sha256: fingerprint,
            font_size_emu,
            line_height_emu,
        },
        vertical_offset_emu: resolved_vertical_offset_emu_v1(
            vertical_alignment,
            bounds.height.get(),
            laid_out_height_emu,
        ),
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
struct PreparedTypographyRunV1 {
    run: AdmittedTypographyRunV1,
    advance_prefix_emu: Vec<i64>,
    shaping: RenderResolvedShapingV1,
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

fn prepare_typography_run_v1(
    run: AdmittedTypographyRunV1,
    shaped: &pub_layout::BoundedShapedText,
) -> Result<PreparedTypographyRunV1, RenderTextLayoutFallbackReasonV1> {
    let glyphs = &shaped.glyphs;
    let scalar_len = usize::try_from(
        run.scalar_end
            .checked_sub(run.scalar_start)
            .ok_or(RenderTextLayoutFallbackReasonV1::SharedLayoutFailed)?,
    )
    .map_err(|_| RenderTextLayoutFallbackReasonV1::SharedLayoutFailed)?;
    let mut advances = vec![0_i64; scalar_len.saturating_add(1)];
    for glyph in glyphs {
        if glyph.cluster < run.scalar_start || glyph.cluster >= run.scalar_end {
            return Err(RenderTextLayoutFallbackReasonV1::SharedLayoutFailed);
        }
        let local = usize::try_from(glyph.cluster - run.scalar_start)
            .map_err(|_| RenderTextLayoutFallbackReasonV1::SharedLayoutFailed)?;
        let slot = local
            .checked_add(1)
            .ok_or(RenderTextLayoutFallbackReasonV1::SharedLayoutFailed)?;
        advances[slot] = advances[slot]
            .checked_add(glyph.x_advance.get())
            .ok_or(RenderTextLayoutFallbackReasonV1::SharedLayoutFailed)?;
    }
    for index in 1..advances.len() {
        advances[index] = advances[index - 1]
            .checked_add(advances[index])
            .ok_or(RenderTextLayoutFallbackReasonV1::SharedLayoutFailed)?;
    }
    Ok(PreparedTypographyRunV1 {
        run,
        advance_prefix_emu: advances,
        shaping: RenderResolvedShapingV1 {
            environment: shaped.environment.clone(),
            units_per_em: shaped.units_per_em,
            glyphs: shaped.glyphs.clone(),
        },
    })
}

fn prepared_run_width_v1(
    prepared: &PreparedTypographyRunV1,
    scalar_start: u32,
    scalar_end: u32,
) -> Result<i64, RenderTextLayoutFallbackReasonV1> {
    if scalar_start < prepared.run.scalar_start
        || scalar_end > prepared.run.scalar_end
        || scalar_start > scalar_end
    {
        return Err(RenderTextLayoutFallbackReasonV1::SharedLayoutFailed);
    }
    let start = usize::try_from(scalar_start - prepared.run.scalar_start)
        .map_err(|_| RenderTextLayoutFallbackReasonV1::SharedLayoutFailed)?;
    let end = usize::try_from(scalar_end - prepared.run.scalar_start)
        .map_err(|_| RenderTextLayoutFallbackReasonV1::SharedLayoutFailed)?;
    prepared.advance_prefix_emu[end]
        .checked_sub(prepared.advance_prefix_emu[start])
        .ok_or(RenderTextLayoutFallbackReasonV1::SharedLayoutFailed)
}

fn reuse_mixed_line_candidate_v1(
    scalars: &[char],
    cursor: u32,
    consumed_scalar_end: u32,
    prepared_runs: &[PreparedTypographyRunV1],
    font: &ExplicitRenderTextFontResourceV1<'_>,
) -> Result<MixedLineCandidateV1, RenderTextLayoutFallbackReasonV1> {
    let scalar_end = consumed_scalar_end;
    let text = scalar_text_range_v1(scalars, cursor, scalar_end)
        .ok_or(RenderTextLayoutFallbackReasonV1::SharedLayoutFailed)?;
    let mut spans = Vec::new();
    let mut measured_width_emu = 0_i64;
    let mut line_height_emu = font.default_line_height_emu;

    for prepared in prepared_runs {
        let run = prepared.run;
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
        let span_width_emu = prepared_run_width_v1(prepared, span_start, span_end)?;
        let span_glyphs = prepared
            .shaping
            .glyphs
            .iter()
            .filter(|glyph| glyph.cluster >= span_start && glyph.cluster < span_end)
            .cloned()
            .collect::<Vec<_>>();
        spans.push(RenderResolvedTextSpanV1 {
            scalar_start: span_start,
            scalar_end: span_end,
            text: span_text,
            x_offset_emu: measured_width_emu,
            measured_width_emu: span_width_emu,
            font_size_emu: run.font_size_emu,
            font_resource_id: None,
            font_fingerprint_sha256: None,
            shaping: Some(RenderResolvedShapingV1 {
                environment: prepared.shaping.environment.clone(),
                units_per_em: prepared.shaping.units_per_em,
                glyphs: span_glyphs,
            }),
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
            font_resource_id: None,
            font_fingerprint_sha256: None,
            shaping: Some(RenderResolvedShapingV1 {
                environment: shaped.environment,
                units_per_em: shaped.units_per_em,
                glyphs: shaped.glyphs,
            }),
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
    node_id: NodeId,
    bounds: &RectEmu,
    fingerprint: &str,
    vertical_alignment: Option<ViewerTextVerticalAlignment>,
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
    let mut prepared_runs = Vec::with_capacity(runs.len());
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
        let prepared = match prepare_typography_run_v1(*run, &shaped) {
            Ok(prepared) => prepared,
            Err(reason) => return fallback_layout(reason),
        };
        prepared_runs.push(prepared);
        policy_glyphs.extend(shaped.glyphs);
    }
    let policy = match break_policy_for_shaped_text(&fragment.text, &policy_glyphs) {
        Ok(policy) => policy,
        Err(_) => return fallback_layout(RenderTextLayoutFallbackReasonV1::SharedLayoutFailed),
    };

    let mut cursor = fragment.scalar_start;
    let mut cursor_safe_without_reshaping = true;
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
            let evaluated = if cursor_safe_without_reshaping
                && candidate.safe_without_reshaping
                && candidate.kind == BoundedBreakKind::Allowed
            {
                match reuse_mixed_line_candidate_v1(
                    &scalars,
                    cursor,
                    candidate.scalar_boundary,
                    &prepared_runs,
                    font,
                ) {
                    Ok(evaluated) => evaluated,
                    Err(reason) => return fallback_layout(reason),
                }
            } else {
                match shape_mixed_line_candidate_v1(
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
                }
            };
            let fits_width = evaluated.measured_width_emu <= bounds.width.get();
            let fits_height = used_height_emu
                .checked_add(evaluated.line_height_emu)
                .is_some_and(|height| height <= bounds.height.get());
            if fits_width && fits_height {
                chosen = Some((evaluated, candidate.safe_without_reshaping));
            }
            if candidate.kind == BoundedBreakKind::Mandatory {
                break;
            }
        }

        let Some((chosen, chosen_boundary_safe_without_reshaping)) = chosen else {
            break;
        };
        used_height_emu = match used_height_emu.checked_add(chosen.line_height_emu) {
            Some(value) => value,
            None => return fallback_layout(RenderTextLayoutFallbackReasonV1::SharedLayoutFailed),
        };
        let x_offset_emu = resolved_line_x_offset_emu_v1(
            fragment,
            node_id,
            bounds,
            line_index,
            cursor..chosen.scalar_end,
            chosen.measured_width_emu,
            fingerprint,
        );
        lines.push(RenderResolvedTextLineV1 {
            line_index,
            scalar_start: cursor,
            scalar_end: chosen.scalar_end,
            consumed_scalar_end: chosen.consumed_scalar_end,
            text: chosen.text,
            measured_width_emu: chosen.measured_width_emu,
            line_height_emu: chosen.line_height_emu,
            x_offset_emu,
            spans: chosen.spans,
            shaping: None,
        });
        cursor = chosen.consumed_scalar_end;
        cursor_safe_without_reshaping = chosen_boundary_safe_without_reshaping;
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
        vertical_offset_emu: resolved_vertical_offset_emu_v1(
            vertical_alignment,
            bounds.height.get(),
            used_height_emu,
        ),
        lines,
    }
}

#[derive(Debug, Clone)]
struct ResolvedFamilyTypographyRunV1<'a> {
    scalar_start: u32,
    scalar_end: u32,
    font_size_emu: i64,
    font: ExplicitRenderTextFontResourceV1<'a>,
    font_fingerprint_sha256: String,
}

fn admitted_mixed_family_typography_runs_v1<'a, G>(
    fragment: &RenderTextFragmentV1,
    resolve_span_font: &mut G,
) -> Option<Vec<ResolvedFamilyTypographyRunV1<'a>>>
where
    G: FnMut(
        &RenderTextFragmentV1,
        &RenderTypographyRunV1,
    ) -> Option<ExplicitRenderTextFontResourceV1<'a>>,
{
    if fragment.text.is_empty() || !fragment.text.is_ascii() || fragment.typography.len() < 2 {
        return None;
    }

    let mut cursor = fragment.scalar_start;
    let mut admitted = Vec::with_capacity(fragment.typography.len());

    for run in &fragment.typography {
        if run.scalar_start != cursor
            || run.scalar_end <= run.scalar_start
            || run.scalar_end > fragment.scalar_end
            || run.text_size_emu == 0
        {
            return None;
        }
        let display_family = run.source_font_name.trim();
        if display_family.is_empty() {
            return None;
        }
        let font = resolve_span_font(fragment, run)?;
        if font.resource_id.is_empty()
            || font.bytes.is_empty()
            || font.default_font_size_emu <= 0
            || font.default_line_height_emu <= 0
        {
            return None;
        }
        let fingerprint = font_fingerprint_sha256(font.bytes);
        if font.expected_sha256.is_empty() || fingerprint != font.expected_sha256 {
            return None;
        }

        admitted.push(ResolvedFamilyTypographyRunV1 {
            scalar_start: run.scalar_start,
            scalar_end: run.scalar_end,
            font_size_emu: i64::from(run.text_size_emu),
            font,
            font_fingerprint_sha256: fingerprint,
        });
        cursor = run.scalar_end;
    }

    if cursor != fragment.scalar_end {
        return None;
    }
    Some(admitted)
}

fn mixed_family_layout_fingerprint_v1(runs: &[ResolvedFamilyTypographyRunV1<'_>]) -> String {
    let mut fingerprint = String::from("chaptera.mixed-family-layout.v1");
    for run in runs {
        fingerprint.push('|');
        fingerprint.push_str(run.font.resource_id);
        fingerprint.push(':');
        fingerprint.push_str(&run.font_fingerprint_sha256);
    }
    fingerprint
}

fn mixed_family_line_base_height_v1(
    cursor: u32,
    runs: &[ResolvedFamilyTypographyRunV1<'_>],
) -> Result<i64, RenderTextLayoutFallbackReasonV1> {
    runs.iter()
        .find(|run| run.scalar_start <= cursor && cursor < run.scalar_end)
        .map(|run| run.font.default_line_height_emu)
        .ok_or(RenderTextLayoutFallbackReasonV1::SharedLayoutFailed)
}

fn shape_mixed_family_line_candidate_v1(
    scalars: &[char],
    cursor: u32,
    consumed_scalar_end: u32,
    kind: BoundedBreakKind,
    runs: &[ResolvedFamilyTypographyRunV1<'_>],
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
    let mut line_height_emu = mixed_family_line_base_height_v1(cursor, runs)?;

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
            run.font.default_font_size_emu,
            run.font.default_line_height_emu,
        ) else {
            return Err(RenderTextLayoutFallbackReasonV1::FontResourceInvalid);
        };
        let runtime = BoundedShapingRuntime {
            layout: BoundedLayoutEnvironment {
                engine_revision: SHARED_TEXT_LAYOUT_REVISION_V1.to_owned(),
                font_set_fingerprint: run.font_fingerprint_sha256.clone(),
                resource_fingerprint: run.font.resource_id.to_owned(),
            },
            face_index: run.font.face_index,
            font_size_emu: LengthEmu::new(run.font_size_emu),
            font_bytes: run.font.bytes,
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
            font_resource_id: Some(run.font.resource_id.to_owned()),
            font_fingerprint_sha256: Some(run.font_fingerprint_sha256.clone()),
            shaping: Some(RenderResolvedShapingV1 {
                environment: shaped.environment,
                units_per_em: shaped.units_per_em,
                glyphs: shaped.glyphs,
            }),
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

fn resolve_mixed_family_text_layout_v1<'a, G>(
    visual: &ViewerGeometryDocument,
    target: RenderTextLayoutTargetV1,
    fragment: &RenderTextFragmentV1,
    resolve_span_font: &mut G,
) -> Option<RenderTextLayoutV1>
where
    G: FnMut(
        &RenderTextFragmentV1,
        &RenderTypographyRunV1,
    ) -> Option<ExplicitRenderTextFontResourceV1<'a>>,
{
    if target.projected_target_frame_node_id.is_some()
        || target.bounds.width.get() <= 0
        || target.bounds.height.get() <= 0
    {
        return None;
    }

    let story = visual
        .document
        .stories
        .iter()
        .find(|story| story.id == fragment.story_id)?;
    let story_scalar_len = u32::try_from(story.text.chars().count()).ok()?;
    if fragment.scalar_start != 0
        || fragment.scalar_end != story_scalar_len
        || fragment.text != story.text
    {
        return None;
    }
    admitted_layout_frame_ordinal(
        visual,
        fragment.story_id,
        target.node_id,
        target.projected_target_frame_node_id,
    )
    .ok()?;

    let runs = admitted_mixed_family_typography_runs_v1(fragment, resolve_span_font)?;
    let scalars: Vec<char> = fragment.text.chars().collect();

    let mut policy_glyphs = Vec::new();
    for run in &runs {
        let run_text = scalar_text_range_v1(&scalars, run.scalar_start, run.scalar_end)?;
        let runtime = BoundedShapingRuntime {
            layout: BoundedLayoutEnvironment {
                engine_revision: SHARED_TEXT_LAYOUT_REVISION_V1.to_owned(),
                font_set_fingerprint: run.font_fingerprint_sha256.clone(),
                resource_fingerprint: run.font.resource_id.to_owned(),
            },
            face_index: run.font.face_index,
            font_size_emu: LengthEmu::new(run.font_size_emu),
            font_bytes: run.font.bytes,
        };
        let shaped = shape_bounded_ltr_segment(&run_text, run.scalar_start, &runtime).ok()?;
        policy_glyphs.extend(shaped.glyphs);
    }
    let policy = break_policy_for_shaped_text(&fragment.text, &policy_glyphs).ok()?;
    let layout_fingerprint = mixed_family_layout_fingerprint_v1(&runs);

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
            let evaluated = shape_mixed_family_line_candidate_v1(
                &scalars,
                cursor,
                candidate.scalar_boundary,
                candidate.kind,
                &runs,
            )
            .ok()?;
            let fits_width = evaluated.measured_width_emu <= target.bounds.width.get();
            let fits_height = used_height_emu
                .checked_add(evaluated.line_height_emu)
                .is_some_and(|height| height <= target.bounds.height.get());
            if fits_width && fits_height {
                chosen = Some(evaluated);
            }
            if candidate.kind == BoundedBreakKind::Mandatory {
                break;
            }
        }

        let chosen = chosen?;
        used_height_emu = used_height_emu.checked_add(chosen.line_height_emu)?;
        let x_offset_emu = resolved_line_x_offset_emu_v1(
            fragment,
            target.node_id,
            &target.bounds,
            line_index,
            cursor..chosen.scalar_end,
            chosen.measured_width_emu,
            &layout_fingerprint,
        );
        lines.push(RenderResolvedTextLineV1 {
            line_index,
            scalar_start: cursor,
            scalar_end: chosen.scalar_end,
            consumed_scalar_end: chosen.consumed_scalar_end,
            text: chosen.text,
            measured_width_emu: chosen.measured_width_emu,
            line_height_emu: chosen.line_height_emu,
            x_offset_emu,
            spans: chosen.spans,
            shaping: None,
        });
        cursor = chosen.consumed_scalar_end;
        line_index = line_index.checked_add(1)?;
    }

    if cursor != fragment.scalar_end {
        return None;
    }

    let first = runs.first()?;
    let max_font_size_emu = runs
        .iter()
        .map(|run| run.font_size_emu)
        .max()
        .unwrap_or(first.font_size_emu);
    let max_line_height_emu = lines
        .iter()
        .map(|line| line.line_height_emu)
        .max()
        .unwrap_or(first.font.default_line_height_emu);

    Some(RenderTextLayoutV1 {
        disposition: RenderTextLayoutDispositionV1::SharedResolved {
            font_resource_id: first.font.resource_id.to_owned(),
            font_fingerprint_sha256: first.font_fingerprint_sha256.clone(),
            font_size_emu: max_font_size_emu,
            line_height_emu: max_line_height_emu,
        },
        vertical_offset_emu: resolved_vertical_offset_emu_v1(
            target.vertical_alignment,
            target.bounds.height.get(),
            used_height_emu,
        ),
        lines,
    })
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
        ViewerNodePaint, ViewerPage, ViewerParagraphLineSpacingRun, ViewerScriptFontEntry,
        ViewerScriptFontMap, ViewerSolidLine, ViewerSource, ViewerTable, ViewerTableCell,
        ViewerTextFragment, ViewerTypographyRun, viewer_story_text_sha256,
    };

    fn canonical(byte: u8) -> CanonicalId {
        CanonicalId::from_bytes([byte; 16])
    }

    fn decorative_border_fixture() -> RenderDecorativeBorderV1 {
        let slots = [
            RenderDecorativeBorderSlotV1::TopLeft,
            RenderDecorativeBorderSlotV1::Top,
            RenderDecorativeBorderSlotV1::TopRight,
            RenderDecorativeBorderSlotV1::Right,
            RenderDecorativeBorderSlotV1::BottomRight,
            RenderDecorativeBorderSlotV1::Bottom,
            RenderDecorativeBorderSlotV1::BottomLeft,
            RenderDecorativeBorderSlotV1::Left,
        ]
        .into_iter()
        .enumerate()
        .map(|(index, slot)| RenderDecorativeBorderSlotRefV1 {
            slot,
            resource_id: ResourceId::from_canonical(canonical(
                u8::try_from(index + 20).expect("fixture id"),
            )),
        })
        .collect();

        RenderDecorativeBorderV1 {
            name: "fixture-border".to_owned(),
            corner_extent_emu: 20,
            horizontal_extent_emu: 20,
            vertical_extent_emu: 20,
            stretch_pictures: Some(true),
            slots,
        }
    }

    #[test]
    fn decorative_border_stretch_partitions_edge_interiors_exactly() {
        let border = decorative_border_fixture();
        let bounds = RectEmu::new(
            LengthEmu::new(10),
            LengthEmu::new(20),
            LengthEmu::new(100),
            LengthEmu::new(80),
        );
        let placements =
            layout_decorative_border_v1(&border, bounds, 20, true).expect("stretch layout");

        let slots = placements.iter().map(|row| row.slot).collect::<Vec<_>>();
        assert_eq!(slots[0], RenderDecorativeBorderSlotV1::TopLeft);
        assert_eq!(slots[1], RenderDecorativeBorderSlotV1::Top);
        assert_eq!(slots[4], RenderDecorativeBorderSlotV1::TopRight);
        assert_eq!(slots[5], RenderDecorativeBorderSlotV1::Right);
        assert_eq!(slots[7], RenderDecorativeBorderSlotV1::BottomRight);
        assert_eq!(slots[8], RenderDecorativeBorderSlotV1::Bottom);
        assert_eq!(slots[11], RenderDecorativeBorderSlotV1::BottomLeft);
        assert_eq!(slots[12], RenderDecorativeBorderSlotV1::Left);

        let top = placements
            .iter()
            .filter(|row| row.slot == RenderDecorativeBorderSlotV1::Top)
            .collect::<Vec<_>>();
        assert_eq!(top.len(), 3);
        assert_eq!(top.first().expect("first top").bounds.x.get(), 30);
        let top_end = top.last().expect("last top").bounds.x.get()
            + top.last().expect("last top").bounds.width.get();
        assert_eq!(top_end, 90);

        let right = placements
            .iter()
            .filter(|row| row.slot == RenderDecorativeBorderSlotV1::Right)
            .collect::<Vec<_>>();
        assert_eq!(right.len(), 2);
        assert_eq!(right.first().expect("first right").bounds.y.get(), 40);
        let right_end = right.last().expect("last right").bounds.y.get()
            + right.last().expect("last right").bounds.height.get();
        assert_eq!(right_end, 80);
    }

    #[test]
    fn decorative_border_repeat_distributes_residual_space_deterministically() {
        let border = decorative_border_fixture();
        let bounds = RectEmu::new(
            LengthEmu::new(0),
            LengthEmu::new(0),
            LengthEmu::new(105),
            LengthEmu::new(85),
        );
        let placements =
            layout_decorative_border_v1(&border, bounds, 20, false).expect("repeat layout");

        let top = placements
            .iter()
            .filter(|row| row.slot == RenderDecorativeBorderSlotV1::Top)
            .collect::<Vec<_>>();
        assert_eq!(top.len(), 3);
        assert_eq!(
            top.iter().map(|row| row.bounds.x.get()).collect::<Vec<_>>(),
            vec![21, 43, 64]
        );
        assert!(
            top.iter()
                .all(|row| row.bounds.width.get() == 20 && row.bounds.height.get() == 20)
        );

        let left = placements
            .iter()
            .filter(|row| row.slot == RenderDecorativeBorderSlotV1::Left)
            .collect::<Vec<_>>();
        assert_eq!(left.len(), 2);
        assert!(
            left[0].bounds.y.get() > left[1].bounds.y.get(),
            "left edge must preserve bottom-to-top traversal"
        );
    }

    #[test]
    fn decorative_border_layout_fails_closed_for_degenerate_or_ambiguous_input() {
        let border = decorative_border_fixture();
        let small = RectEmu::new(
            LengthEmu::new(0),
            LengthEmu::new(0),
            LengthEmu::new(30),
            LengthEmu::new(40),
        );
        assert!(layout_decorative_border_v1(&border, small, 20, true).is_none());
        assert!(layout_decorative_border_v1(&border, small, 0, true).is_none());

        let mut ambiguous = border.clone();
        ambiguous.slots.push(ambiguous.slots[0].clone());
        let regular = RectEmu::new(
            LengthEmu::new(0),
            LengthEmu::new(0),
            LengthEmu::new(100),
            LengthEmu::new(100),
        );
        assert!(layout_decorative_border_v1(&ambiguous, regular, 20, true).is_none());
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
                preset_shape: None,
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
                color_rgb: None,
                color_inherited: false,
                source_story_text_sha256: viewer_story_text_sha256("hello"),
            }],
            paragraph_alignments: Vec::new(),
            paragraph_line_spacings: Vec::new(),
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
                    content_rotation_degrees: None,
                    recolor: None,
                }],
                bytes: vec![0x89, b'P', b'N', b'G'],
            }],
            decorative_borders: Vec::new(),
            decorative_border_resources: Vec::new(),
        }
    }

    fn render_fragment(
        story_id: StoryId,
        text: &str,
        typography: Vec<RenderTypographyRunV1>,
    ) -> RenderTextFragmentV1 {
        RenderTextFragmentV1 {
            story_id,
            scalar_start: 0,
            scalar_end: u32::try_from(text.chars().count()).expect("test scalar length"),
            text: text.to_owned(),
            line_count: 1,
            typography,
            paragraph_alignments: Vec::new(),
            backend_font_resource_id: None,
            layout: None,
        }
    }

    fn script_entry(
        slot: u16,
        ordinal: u32,
        family: Option<&str>,
        disposition: ViewerScriptFontEntryDisposition,
    ) -> ViewerScriptFontEntry {
        ViewerScriptFontEntry {
            script_slot: slot,
            source_font_index: ordinal,
            source_font_name: family.map(str::to_owned),
            disposition,
        }
    }

    fn latin_map(
        story_id: StoryId,
        text: &str,
        start: u32,
        end: u32,
        family: &str,
        ordinal: u32,
    ) -> ViewerScriptFontMap {
        ViewerScriptFontMap {
            story_id,
            scalar_start: start,
            scalar_end: end,
            entries: vec![
                script_entry(
                    0,
                    ordinal,
                    Some(family),
                    ViewerScriptFontEntryDisposition::Resolved,
                ),
                script_entry(
                    1,
                    ordinal,
                    Some(family),
                    ViewerScriptFontEntryDisposition::Resolved,
                ),
                script_entry(
                    2,
                    ordinal,
                    Some(family),
                    ViewerScriptFontEntryDisposition::Resolved,
                ),
            ],
            source_story_text_sha256: viewer_story_text_sha256(text),
        }
    }

    #[test]
    fn shaping_safe_mixed_candidate_reuse_matches_independent_reshape() {
        let text = "Hello wide world";
        let scalars = text.chars().collect::<Vec<_>>();
        let font_bytes = font_test_data::NOTOSERIF_AUTOHINT_SHAPING;
        let fingerprint = pub_layout::font_fingerprint_sha256(font_bytes);
        let font = ExplicitRenderTextFontResourceV1 {
            resource_id: "test:noto-serif",
            expected_sha256: &fingerprint,
            face_index: 0,
            default_font_size_emu: 12 * 12_700,
            default_line_height_emu: 14 * 12_700,
            bytes: font_bytes,
        };
        let runs = vec![
            AdmittedTypographyRunV1 {
                scalar_start: 0,
                scalar_end: 6,
                font_size_emu: 12 * 12_700,
            },
            AdmittedTypographyRunV1 {
                scalar_start: 6,
                scalar_end: 16,
                font_size_emu: 18 * 12_700,
            },
        ];

        let mut prepared = Vec::new();
        let mut policy_glyphs = Vec::new();
        for run in &runs {
            let run_text =
                scalar_text_range_v1(&scalars, run.scalar_start, run.scalar_end).unwrap();
            let runtime = BoundedShapingRuntime {
                layout: BoundedLayoutEnvironment {
                    engine_revision: SHARED_TEXT_LAYOUT_REVISION_V1.to_owned(),
                    font_set_fingerprint: fingerprint.clone(),
                    resource_fingerprint: font.resource_id.to_owned(),
                },
                face_index: font.face_index,
                font_size_emu: LengthEmu::new(run.font_size_emu),
                font_bytes,
            };
            let shaped = shape_bounded_ltr_segment(&run_text, run.scalar_start, &runtime).unwrap();
            prepared.push(prepare_typography_run_v1(*run, &shaped).unwrap());
            policy_glyphs.extend(shaped.glyphs);
        }

        let policy = break_policy_for_shaped_text(text, &policy_glyphs).unwrap();
        for (cursor, boundary) in [(0, 11), (6, 11)] {
            let candidate = policy
                .candidates
                .iter()
                .find(|candidate| candidate.scalar_boundary == boundary)
                .expect("space boundary");
            assert!(candidate.safe_without_reshaping);
            assert!(!candidate.requires_reshaping);
            assert_eq!(candidate.kind, BoundedBreakKind::Allowed);

            let reshaped = shape_mixed_line_candidate_v1(
                &scalars,
                cursor,
                boundary,
                candidate.kind,
                &runs,
                &font,
                &fingerprint,
            )
            .unwrap();
            let reused =
                reuse_mixed_line_candidate_v1(&scalars, cursor, boundary, &prepared, &font)
                    .unwrap();

            assert_eq!(reused.scalar_end, reshaped.scalar_end);
            assert_eq!(reused.consumed_scalar_end, reshaped.consumed_scalar_end);
            assert_eq!(reused.text, reshaped.text);
            assert_eq!(reused.measured_width_emu, reshaped.measured_width_emu);
            assert_eq!(reused.line_height_emu, reshaped.line_height_emu);
            assert_eq!(reused.spans, reshaped.spans);
            assert!(reused.spans.iter().all(|span| {
                span.shaping
                    .as_ref()
                    .is_some_and(|shaping| shaping.units_per_em > 0 && !shaping.glyphs.is_empty())
            }));
        }
    }

    #[test]
    fn uniform_layout_height_uses_first_line_extent_then_baseline_advance() {
        assert_eq!(uniform_laid_out_height_emu_v1(198_000, 222_250, 0), Some(0));
        assert_eq!(
            uniform_laid_out_height_emu_v1(198_000, 222_250, 1),
            Some(198_000)
        );
        assert_eq!(
            uniform_laid_out_height_emu_v1(198_000, 222_250, 2),
            Some(420_250)
        );
        assert_eq!(
            uniform_laid_out_height_emu_v1(198_000, 222_250, 3),
            Some(642_500)
        );
    }

    #[test]
    fn absolute_paragraph_line_spacing_overrides_only_one_complete_fresh_run() {
        let mut visual = fixture();
        let story_id = visual.document.stories[0].id;
        let story_text = visual.document.stories[0].text.clone();
        let fragment = render_fragment(story_id, &story_text, Vec::new());
        let fallback = 15 * 12_700;
        let absolute = 18 * 12_700;
        let font = ExplicitRenderTextFontResourceV1 {
            resource_id: "test:fallback",
            expected_sha256: "",
            face_index: 0,
            default_font_size_emu: 12 * 12_700,
            default_line_height_emu: fallback,
            bytes: &[],
        };

        assert_eq!(
            resolved_uniform_line_height_emu_v1(&visual, &fragment, 12 * 12_700, &font, false),
            Some(fallback)
        );

        visual.paragraph_line_spacings = vec![ViewerParagraphLineSpacingRun {
            story_id,
            scalar_start: fragment.scalar_start,
            scalar_end: fragment.scalar_end,
            line_spacing: ViewerParagraphLineSpacing::Absolute {
                spacing_emu: u32::try_from(absolute).expect("absolute spacing"),
            },
            source_value: Some(1_828_801),
            source_story_text_sha256: viewer_story_text_sha256(&story_text),
        }];
        assert_eq!(
            resolved_uniform_line_height_emu_v1(&visual, &fragment, 12 * 12_700, &font, false),
            Some(absolute)
        );

        visual.paragraph_line_spacings[0].line_spacing = ViewerParagraphLineSpacing::Proportional {
            point_equivalent_emu: 18 * 12_700,
        };
        assert_eq!(
            resolved_uniform_line_height_emu_v1(&visual, &fragment, 12 * 12_700, &font, false),
            Some(fallback)
        );

        visual.paragraph_line_spacings[0].line_spacing = ViewerParagraphLineSpacing::Absolute {
            spacing_emu: u32::try_from(absolute).expect("absolute spacing"),
        };
        visual.paragraph_line_spacings[0].scalar_end = fragment.scalar_end - 1;
        assert_eq!(
            resolved_uniform_line_height_emu_v1(&visual, &fragment, 12 * 12_700, &font, false),
            Some(fallback)
        );

        visual.paragraph_line_spacings[0].scalar_end = fragment.scalar_end;
        visual
            .paragraph_line_spacings
            .push(visual.paragraph_line_spacings[0].clone());
        assert_eq!(
            resolved_uniform_line_height_emu_v1(&visual, &fragment, 12 * 12_700, &font, false),
            Some(fallback)
        );

        visual.paragraph_line_spacings.truncate(1);
        visual.paragraph_line_spacings[0].source_story_text_sha256 =
            viewer_story_text_sha256("stale");
        assert_eq!(
            resolved_uniform_line_height_emu_v1(&visual, &fragment, 12 * 12_700, &font, false),
            Some(fallback)
        );
    }

    #[test]
    fn proportional_line_height_scales_only_proven_single_and_one_point_five_modes() {
        let natural = 198_636;
        assert_eq!(
            scale_proportional_line_height_emu_v1(
                natural,
                PUBLISHER_SINGLE_POINT_EQUIVALENT_EMU_V1,
            ),
            Some(natural)
        );
        assert_eq!(
            scale_proportional_line_height_emu_v1(
                natural,
                PUBLISHER_ONE_POINT_FIVE_POINT_EQUIVALENT_EMU_V1,
            ),
            Some(297_954)
        );
        assert_eq!(
            scale_proportional_line_height_emu_v1(natural, 24 * 12_700),
            None
        );
    }

    #[test]
    fn paragraph_alignment_line_offset_requires_one_complete_executable_range() {
        let story_id = StoryId::from_canonical(canonical(3));
        let node_id = NodeId::from_canonical(canonical(2));
        let bounds = RectEmu::new(
            LengthEmu::new(0),
            LengthEmu::new(0),
            LengthEmu::new(120),
            LengthEmu::new(200),
        );
        let mut fragment = render_fragment(story_id, "hello", Vec::new());

        // Missing authority keeps the existing leading origin.
        assert_eq!(
            resolved_line_x_offset_emu_v1(&fragment, node_id, &bounds, 0, 0..5, 100, "layout:test"),
            0
        );

        fragment.paragraph_alignments = vec![RenderParagraphAlignmentRunV1 {
            scalar_start: 0,
            scalar_end: 5,
            alignment: RenderParagraphAlignmentV1::Center,
            source_value: 1,
        }];
        assert_eq!(
            resolved_line_x_offset_emu_v1(&fragment, node_id, &bounds, 0, 0..5, 100, "layout:test"),
            10
        );

        // Two complete authorities are ambiguous and therefore fail closed.
        fragment
            .paragraph_alignments
            .push(RenderParagraphAlignmentRunV1 {
                scalar_start: 0,
                scalar_end: 5,
                alignment: RenderParagraphAlignmentV1::Right,
                source_value: 2,
            });
        assert_eq!(
            resolved_line_x_offset_emu_v1(&fragment, node_id, &bounds, 0, 0..5, 100, "layout:test"),
            0
        );

        // A line that crosses paragraph-range boundaries has no complete range.
        fragment.paragraph_alignments = vec![
            RenderParagraphAlignmentRunV1 {
                scalar_start: 0,
                scalar_end: 2,
                alignment: RenderParagraphAlignmentV1::Right,
                source_value: 2,
            },
            RenderParagraphAlignmentRunV1 {
                scalar_start: 2,
                scalar_end: 5,
                alignment: RenderParagraphAlignmentV1::Right,
                source_value: 2,
            },
        ];
        assert_eq!(
            resolved_line_x_offset_emu_v1(&fragment, node_id, &bounds, 0, 1..4, 60, "layout:test"),
            0
        );

        // Preserved but non-executable Publisher justification values stay leading.
        for alignment in [
            RenderParagraphAlignmentV1::InterWord,
            RenderParagraphAlignmentV1::Distribute,
        ] {
            fragment.paragraph_alignments = vec![RenderParagraphAlignmentRunV1 {
                scalar_start: 0,
                scalar_end: 5,
                alignment,
                source_value: 3,
            }];
            assert_eq!(
                resolved_line_x_offset_emu_v1(
                    &fragment,
                    node_id,
                    &bounds,
                    0,
                    0..5,
                    100,
                    "layout:test",
                ),
                0
            );
        }
    }

    #[test]
    fn uniform_text_color_requires_complete_uniform_coverage() {
        let story_id = fixture().document.stories[0].id;
        let uniform = render_fragment(
            story_id,
            "hello",
            vec![
                RenderTypographyRunV1 {
                    scalar_start: 0,
                    scalar_end: 2,
                    source_font_name: "Arial".to_owned(),
                    text_size_emu: 152_400,
                    font_inherited: false,
                    size_inherited: false,
                    color_rgb: Some([255, 204, 0]),
                    color_inherited: false,
                    bold: None,
                    italic: None,
                },
                RenderTypographyRunV1 {
                    scalar_start: 2,
                    scalar_end: 5,
                    source_font_name: "Arial".to_owned(),
                    text_size_emu: 152_400,
                    font_inherited: false,
                    size_inherited: false,
                    color_rgb: Some([255, 204, 0]),
                    color_inherited: true,
                    bold: None,
                    italic: None,
                },
            ],
        );
        assert_eq!(uniform_text_color_rgb_v1(&uniform), Some([255, 204, 0]));

        let mut mixed = uniform.clone();
        mixed.typography[1].color_rgb = Some([0, 0, 0]);
        assert_eq!(uniform_text_color_rgb_v1(&mixed), None);

        let mut missing = uniform.clone();
        missing.typography[1].color_rgb = None;
        assert_eq!(uniform_text_color_rgb_v1(&missing), None);

        let mut gap = uniform;
        gap.typography[1].scalar_start = 3;
        assert_eq!(uniform_text_color_rgb_v1(&gap), None);
    }

    #[test]
    fn effective_family_prefers_complete_scalar_typography() {
        let mut visual = fixture();
        let story_id = visual.document.stories[0].id;
        let text = "hello";
        visual.script_font_maps = vec![latin_map(story_id, text, 0, 5, "Caladea", 32)];
        let fragment = render_fragment(
            story_id,
            text,
            vec![RenderTypographyRunV1 {
                scalar_start: 0,
                scalar_end: 5,
                source_font_name: "Source Font".to_owned(),
                text_size_emu: 152_400,
                font_inherited: false,
                size_inherited: false,
                color_rgb: None,
                color_inherited: false,
                bold: None,
                italic: None,
            }],
        );

        assert_eq!(
            effective_source_font_family_v1(&visual, &fragment).as_deref(),
            Some("Source Font")
        );
    }

    #[test]
    fn effective_family_admits_strict_ascii_latin_scriptfonts() {
        let mut visual = fixture();
        let story_id = visual.document.stories[0].id;
        let text = "hello";
        visual.script_font_maps = vec![latin_map(story_id, text, 0, 5, "Caladea", 32)];
        let fragment = render_fragment(story_id, text, Vec::new());

        assert_eq!(
            effective_source_font_family_v1(&visual, &fragment).as_deref(),
            Some("Caladea")
        );
    }

    #[test]
    fn effective_family_scriptfonts_fail_closed_on_slot_range_and_script_ambiguity() {
        let story_id = fixture().document.stories[0].id;
        let text = "hello";
        let fragment = render_fragment(story_id, text, Vec::new());

        let mut missing_slot = fixture();
        let mut map = latin_map(story_id, text, 0, 5, "Caladea", 32);
        map.entries.retain(|entry| entry.script_slot != 2);
        missing_slot.script_font_maps = vec![map];
        assert_eq!(
            effective_source_font_family_v1(&missing_slot, &fragment),
            None
        );

        let mut unresolved = fixture();
        let mut map = latin_map(story_id, text, 0, 5, "Caladea", 32);
        map.entries
            .iter_mut()
            .find(|entry| entry.script_slot == 1)
            .expect("ascii slot")
            .disposition = ViewerScriptFontEntryDisposition::UnresolvedSentinel;
        unresolved.script_font_maps = vec![map];
        assert_eq!(
            effective_source_font_family_v1(&unresolved, &fragment),
            None
        );

        let mut mixed = fixture();
        let mut map = latin_map(story_id, text, 0, 5, "Caladea", 32);
        let latin = map
            .entries
            .iter_mut()
            .find(|entry| entry.script_slot == 2)
            .expect("latin slot");
        latin.source_font_index = 7;
        latin.source_font_name = Some("Arial".to_owned());
        mixed.script_font_maps = vec![map];
        assert_eq!(effective_source_font_family_v1(&mixed, &fragment), None);

        let mut gap = fixture();
        gap.script_font_maps = vec![
            latin_map(story_id, text, 0, 2, "Caladea", 32),
            latin_map(story_id, text, 3, 5, "Caladea", 32),
        ];
        assert_eq!(effective_source_font_family_v1(&gap, &fragment), None);

        let mut non_ascii = fixture();
        non_ascii.document.stories[0].text = "héllo".to_owned();
        non_ascii.script_font_maps = vec![latin_map(story_id, "héllo", 0, 5, "Caladea", 32)];
        let non_ascii_fragment = render_fragment(story_id, "héllo", Vec::new());
        assert_eq!(
            effective_source_font_family_v1(&non_ascii, &non_ascii_fragment),
            None
        );
    }

    #[test]
    fn effective_family_does_not_override_invalid_scalar_family_evidence() {
        let mut visual = fixture();
        let story_id = visual.document.stories[0].id;
        let text = "hello";
        visual.script_font_maps = vec![latin_map(story_id, text, 0, 5, "Caladea", 32)];
        let fragment = render_fragment(
            story_id,
            text,
            vec![
                RenderTypographyRunV1 {
                    scalar_start: 0,
                    scalar_end: 2,
                    source_font_name: "Arial".to_owned(),
                    text_size_emu: 152_400,
                    font_inherited: false,
                    size_inherited: false,
                    color_rgb: None,
                    color_inherited: false,
                    bold: None,
                    italic: None,
                },
                RenderTypographyRunV1 {
                    scalar_start: 2,
                    scalar_end: 5,
                    source_font_name: "Caladea".to_owned(),
                    text_size_emu: 152_400,
                    font_inherited: false,
                    size_inherited: false,
                    color_rgb: None,
                    color_inherited: false,
                    bold: None,
                    italic: None,
                },
            ],
        );

        assert_eq!(effective_source_font_family_v1(&visual, &fragment), None);
    }

    #[test]
    fn mixed_family_line_height_uses_family_at_line_cursor() {
        let first_bytes: &[u8] = b"source-free-first-font";
        let second_bytes: &[u8] = b"source-free-second-font";
        let first_sha = font_fingerprint_sha256(first_bytes);
        let second_sha = font_fingerprint_sha256(second_bytes);
        let runs = vec![
            ResolvedFamilyTypographyRunV1 {
                scalar_start: 0,
                scalar_end: 2,
                font_size_emu: 152_400,
                font: ExplicitRenderTextFontResourceV1 {
                    resource_id: "font-first",
                    expected_sha256: &first_sha,
                    face_index: 0,
                    default_font_size_emu: 152_400,
                    default_line_height_emu: 300_000,
                    bytes: first_bytes,
                },
                font_fingerprint_sha256: first_sha.clone(),
            },
            ResolvedFamilyTypographyRunV1 {
                scalar_start: 2,
                scalar_end: 4,
                font_size_emu: 152_400,
                font: ExplicitRenderTextFontResourceV1 {
                    resource_id: "font-second",
                    expected_sha256: &second_sha,
                    face_index: 0,
                    default_font_size_emu: 152_400,
                    default_line_height_emu: 100_000,
                    bytes: second_bytes,
                },
                font_fingerprint_sha256: second_sha.clone(),
            },
        ];

        assert_eq!(mixed_family_line_base_height_v1(0, &runs), Ok(300_000));
        assert_eq!(mixed_family_line_base_height_v1(2, &runs), Ok(100_000));
    }

    #[test]
    fn mixed_family_layout_executes_real_shaping_with_per_span_resources() {
        let mut visual = fixture();
        let story_id = visual.document.stories[0].id;
        let node_id = visual.scene.nodes[0].origin;
        let page_id = visual.document.pages[0].id;
        visual.document.stories[0].text = "ABCD".to_owned();
        visual.story_frames.push(pub_viewer::ViewerStoryFrame {
            story_id,
            frame_id: node_id,
            ordinal: 0,
            text_content_bounds: None,
            vertical_alignment: None,
        });

        let fragment = render_fragment(
            story_id,
            "ABCD",
            vec![
                RenderTypographyRunV1 {
                    scalar_start: 0,
                    scalar_end: 2,
                    source_font_name: "Family A".to_owned(),
                    text_size_emu: 152_400,
                    font_inherited: false,
                    size_inherited: false,
                    color_rgb: None,
                    color_inherited: false,
                    bold: None,
                    italic: None,
                },
                RenderTypographyRunV1 {
                    scalar_start: 2,
                    scalar_end: 4,
                    source_font_name: "Family B".to_owned(),
                    text_size_emu: 152_400,
                    font_inherited: false,
                    size_inherited: false,
                    color_rgb: None,
                    color_inherited: false,
                    bold: None,
                    italic: None,
                },
            ],
        );

        let first_bytes: &[u8] = font_test_data::AHEM;
        let second_bytes: &[u8] = font_test_data::TINOS_SUBSET;
        let first_sha = font_fingerprint_sha256(first_bytes);
        let second_sha = font_fingerprint_sha256(second_bytes);
        let mut resolver = |_: &RenderTextFragmentV1, run: &RenderTypographyRunV1| match run
            .source_font_name
            .as_str()
        {
            "Family A" => Some(ExplicitRenderTextFontResourceV1 {
                resource_id: "font-family-a",
                expected_sha256: &first_sha,
                face_index: 0,
                default_font_size_emu: 152_400,
                default_line_height_emu: 190_500,
                bytes: first_bytes,
            }),
            "Family B" => Some(ExplicitRenderTextFontResourceV1 {
                resource_id: "font-family-b",
                expected_sha256: &second_sha,
                face_index: 0,
                default_font_size_emu: 152_400,
                default_line_height_emu: 190_500,
                bytes: second_bytes,
            }),
            _ => None,
        };

        let layout = resolve_mixed_family_text_layout_v1(
            &visual,
            RenderTextLayoutTargetV1 {
                page_id,
                page_size: Size2D::new(LengthEmu::new(10_000_000), LengthEmu::new(10_000_000)),
                node_id,
                projected_target_frame_node_id: None,
                vertical_alignment: None,
                bounds: RectEmu::new(
                    LengthEmu::new(0),
                    LengthEmu::new(0),
                    LengthEmu::new(5_000_000),
                    LengthEmu::new(5_000_000),
                ),
                transform: Affine2D::identity(),
            },
            &fragment,
            &mut resolver,
        )
        .expect("real mixed-family shaping must produce one shared layout");

        let RenderTextLayoutDispositionV1::SharedResolved {
            font_resource_id, ..
        } = &layout.disposition
        else {
            panic!("mixed-family execution must remain shared-resolved");
        };
        assert_eq!(font_resource_id, "font-family-a");
        assert!(!layout.lines.is_empty());

        let spans = layout
            .lines
            .iter()
            .flat_map(|line| line.spans.iter())
            .collect::<Vec<_>>();
        assert!(spans.iter().any(|span| {
            span.font_resource_id.as_deref() == Some("font-family-a")
                && span.scalar_start == 0
                && span.scalar_end == 2
        }));
        assert!(spans.iter().any(|span| {
            span.font_resource_id.as_deref() == Some("font-family-b")
                && span.scalar_start == 2
                && span.scalar_end == 4
        }));
        assert_eq!(
            layout
                .lines
                .iter()
                .map(|line| line.text.as_str())
                .collect::<String>(),
            "ABCD"
        );
        assert!(spans.iter().all(|span| {
            span.shaping
                .as_ref()
                .is_some_and(|shaping| shaping.units_per_em > 0 && !shaping.glyphs.is_empty())
        }));
    }

    #[test]
    fn mixed_family_admission_binds_exact_resource_per_typography_run() {
        let story_id = fixture().document.stories[0].id;
        let fragment = render_fragment(
            story_id,
            "ABCD",
            vec![
                RenderTypographyRunV1 {
                    scalar_start: 0,
                    scalar_end: 2,
                    source_font_name: "Elephant".to_owned(),
                    text_size_emu: 304_800,
                    font_inherited: false,
                    size_inherited: false,
                    color_rgb: None,
                    color_inherited: false,
                    bold: None,
                    italic: None,
                },
                RenderTypographyRunV1 {
                    scalar_start: 2,
                    scalar_end: 4,
                    source_font_name: "Times New Roman".to_owned(),
                    text_size_emu: 228_600,
                    font_inherited: false,
                    size_inherited: false,
                    color_rgb: None,
                    color_inherited: false,
                    bold: None,
                    italic: None,
                },
            ],
        );
        let elephant_bytes: &[u8] = b"source-free-elephant-test-font";
        let times_bytes: &[u8] = b"source-free-times-test-font";
        let elephant_sha = font_fingerprint_sha256(elephant_bytes);
        let times_sha = font_fingerprint_sha256(times_bytes);

        let mut resolver = |_: &RenderTextFragmentV1, run: &RenderTypographyRunV1| match run
            .source_font_name
            .as_str()
        {
            "Elephant" => Some(ExplicitRenderTextFontResourceV1 {
                resource_id: "font-elephant",
                expected_sha256: &elephant_sha,
                face_index: 0,
                default_font_size_emu: 152_400,
                default_line_height_emu: 190_500,
                bytes: elephant_bytes,
            }),
            "Times New Roman" => Some(ExplicitRenderTextFontResourceV1 {
                resource_id: "font-times",
                expected_sha256: &times_sha,
                face_index: 0,
                default_font_size_emu: 152_400,
                default_line_height_emu: 190_500,
                bytes: times_bytes,
            }),
            _ => None,
        };

        let runs = admitted_mixed_family_typography_runs_v1(&fragment, &mut resolver)
            .expect("complete mixed-family runs must be admitted");
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].scalar_start..runs[0].scalar_end, 0..2);
        assert_eq!(runs[0].font.resource_id, "font-elephant");
        assert_eq!(runs[0].font_fingerprint_sha256, elephant_sha);
        assert_eq!(runs[1].scalar_start..runs[1].scalar_end, 2..4);
        assert_eq!(runs[1].font.resource_id, "font-times");
        assert_eq!(runs[1].font_fingerprint_sha256, times_sha);
    }

    #[test]
    fn same_family_style_runs_admit_distinct_exact_resources() {
        let story_id = fixture().document.stories[0].id;
        let fragment = render_fragment(
            story_id,
            "ABCD",
            vec![
                RenderTypographyRunV1 {
                    scalar_start: 0,
                    scalar_end: 2,
                    source_font_name: "Arial".to_owned(),
                    text_size_emu: 152_400,
                    font_inherited: false,
                    size_inherited: false,
                    color_rgb: None,
                    color_inherited: false,
                    bold: Some(false),
                    italic: Some(false),
                },
                RenderTypographyRunV1 {
                    scalar_start: 2,
                    scalar_end: 4,
                    source_font_name: "Arial".to_owned(),
                    text_size_emu: 152_400,
                    font_inherited: false,
                    size_inherited: false,
                    color_rgb: None,
                    color_inherited: false,
                    bold: Some(true),
                    italic: Some(false),
                },
            ],
        );
        let regular_bytes: &[u8] = b"source-free-arial-regular-test-font";
        let bold_bytes: &[u8] = b"source-free-arial-bold-test-font";
        let regular_sha = font_fingerprint_sha256(regular_bytes);
        let bold_sha = font_fingerprint_sha256(bold_bytes);

        let mut resolver = |_: &RenderTextFragmentV1, run: &RenderTypographyRunV1| match run.bold {
            Some(false) => Some(ExplicitRenderTextFontResourceV1 {
                resource_id: "font-arial-regular",
                expected_sha256: &regular_sha,
                face_index: 0,
                default_font_size_emu: 152_400,
                default_line_height_emu: 190_500,
                bytes: regular_bytes,
            }),
            Some(true) => Some(ExplicitRenderTextFontResourceV1 {
                resource_id: "font-arial-bold",
                expected_sha256: &bold_sha,
                face_index: 0,
                default_font_size_emu: 152_400,
                default_line_height_emu: 190_500,
                bytes: bold_bytes,
            }),
            None => None,
        };

        let runs = admitted_mixed_family_typography_runs_v1(&fragment, &mut resolver)
            .expect("same-family distinct physical resources must be admitted");
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].font.resource_id, "font-arial-regular");
        assert_eq!(runs[1].font.resource_id, "font-arial-bold");
        assert_ne!(
            runs[0].font_fingerprint_sha256,
            runs[1].font_fingerprint_sha256
        );
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
                fill_rgb: Some([10, 20, 30]),
                fill_visible: Some(true),
            }],
            borders: Vec::new(),
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
        assert_eq!(table.cells[0].fill_rgb, Some([10, 20, 30]));
        assert_eq!(table.cells[0].fill_visible, Some(true));
    }

    #[cfg(feature = "projected-scene-instances")]
    #[test]
    fn text_vertical_alignment_offsets_are_bounded() {
        assert_eq!(resolved_vertical_offset_emu_v1(None, 1_000, 400), 0);
        assert_eq!(
            resolved_vertical_offset_emu_v1(Some(ViewerTextVerticalAlignment::Top), 1_000, 400),
            0
        );
        assert_eq!(
            resolved_vertical_offset_emu_v1(Some(ViewerTextVerticalAlignment::Center), 1_000, 400),
            300
        );
        assert_eq!(
            resolved_vertical_offset_emu_v1(Some(ViewerTextVerticalAlignment::Bottom), 1_000, 400),
            600
        );
        assert_eq!(
            resolved_vertical_offset_emu_v1(Some(ViewerTextVerticalAlignment::Bottom), 300, 400),
            0
        );
    }

    #[cfg(feature = "projected-scene-instances")]
    #[test]
    fn inherited_master_lane_paints_before_direct_page_local_lane() {
        let mut visual = fixture();
        let page_id = visual.document.pages[0].id;
        let direct_node_id = visual.scene.nodes[0].origin;
        let master_node_id = NodeId::from_canonical(canonical(8));
        let master_page_id = PageId::from_canonical(canonical(9));
        let instance = SceneInstanceV1 {
            schema_version: SCENE_INSTANCE_SCHEMA_V1.to_owned(),
            instance_id: "sha256:inherited-master-lane-fixture".to_owned(),
            projection_kind: SceneProjectionKindV1::InheritedMaster,
            origin_node_id: master_node_id.as_canonical().to_string(),
            target_page_id: page_id.as_canonical().to_string(),
            source_parent_origin: Some(master_page_id.as_canonical().to_string()),
            story_authority_id: None,
            cmo_slot_index: None,
            cmo_scalar_index: None,
        };
        visual
            .projected_instances
            .push(pub_viewer::ViewerProjectedSceneInstanceV1 {
                scene_instance: instance,
                target_frame_node_id: None,
                target_frame_paint_scalar_end: None,
                text_content_bounds: None,
                bounds: RectEmu::new(
                    LengthEmu::new(5),
                    LengthEmu::new(6),
                    LengthEmu::new(70),
                    LengthEmu::new(80),
                ),
                transform: Affine2D::identity(),
            });

        let plan = build_page_render_plan_v1(&visual, 0).expect("render plan");
        assert_eq!(plan.nodes.len(), 2);
        assert_eq!(plan.nodes[0].node_id, master_node_id);
        assert_eq!(
            plan.nodes[0]
                .projected_scene_instance
                .as_ref()
                .map(|instance| instance.projection_kind),
            Some(SceneProjectionKindV1::InheritedMaster)
        );
        assert_eq!(plan.nodes[1].node_id, direct_node_id);
        assert!(plan.nodes[1].projected_scene_instance.is_none());
    }

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
            text_content_bounds: None,
            vertical_alignment: None,
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
                text_content_bounds: None,
                vertical_alignment: None,
            },
            pub_viewer::ViewerStoryFrame {
                story_id: target_story_id,
                frame_id: NodeId::from_canonical(canonical(10)),
                ordinal: 1,
                text_content_bounds: None,
                vertical_alignment: None,
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
                target_frame_node_id: Some(origin_node_id),
                target_frame_paint_scalar_end: None,
                text_content_bounds: None,
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
                target_frame_node_id: Some(frame_id),
                target_frame_paint_scalar_end: None,
                text_content_bounds: None,
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
                target_frame_node_id: Some(frame_id),
                target_frame_paint_scalar_end: Some(3),
                text_content_bounds: None,
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
    fn authored_lane_appends_after_base_in_exact_back_to_front_order() {
        let mut plan = build_page_render_plan_v1(&fixture(), 0).expect("base render plan");
        let base_len = plan.nodes.len();
        let page_id = plan.page_id;
        let back: NodeId = serde_json::from_str("\"01890f47-0d10-7abc-8def-0123456789ab\"")
            .expect("authored NodeId");
        let front: NodeId = serde_json::from_str("\"01890f47-0d11-7abc-8def-0123456789ab\"")
            .expect("authored NodeId");
        let lane = AuthoredPageRenderLaneV1 {
            page_id,
            nodes: vec![
                AuthoredPageRenderNodeV1 {
                    node_id: back,
                    bounds: RectEmu::new(
                        LengthEmu::new(10),
                        LengthEmu::new(20),
                        LengthEmu::new(300),
                        LengthEmu::new(400),
                    ),
                    solid_fill_rgb: Some([1, 2, 3]),
                    solid_line: None,
                },
                AuthoredPageRenderNodeV1 {
                    node_id: front,
                    bounds: RectEmu::new(
                        LengthEmu::new(30),
                        LengthEmu::new(40),
                        LengthEmu::new(300),
                        LengthEmu::new(400),
                    ),
                    solid_fill_rgb: Some([4, 5, 6]),
                    solid_line: Some(RenderSolidLineV1 {
                        rgb: [7, 8, 9],
                        width_emu: 12_700,
                    }),
                },
            ],
        };

        apply_authored_page_render_lane_v1(&mut plan, &lane).expect("authored lane projection");
        assert_eq!(plan.nodes.len(), base_len + 2);
        assert_eq!(plan.nodes[base_len].node_id, back);
        assert_eq!(plan.nodes[base_len + 1].node_id, front);
        assert_eq!(plan.nodes[base_len].solid_fill_rgb, Some([1, 2, 3]));
        assert_eq!(
            plan.nodes[base_len + 1]
                .solid_line
                .as_ref()
                .map(|line| line.rgb),
            Some([7, 8, 9])
        );
    }

    #[test]
    fn authored_lane_reorder_changes_only_effective_paint_order() {
        let base = build_page_render_plan_v1(&fixture(), 0).expect("base render plan");
        let base_ids = base
            .nodes
            .iter()
            .map(|node| node.node_id)
            .collect::<Vec<_>>();
        let page_id = base.page_id;
        let a: NodeId = serde_json::from_str("\"01890f47-0d20-7abc-8def-0123456789ab\"")
            .expect("authored NodeId");
        let b: NodeId = serde_json::from_str("\"01890f47-0d21-7abc-8def-0123456789ab\"")
            .expect("authored NodeId");
        let bounds_a = RectEmu::new(
            LengthEmu::new(100),
            LengthEmu::new(200),
            LengthEmu::new(300),
            LengthEmu::new(400),
        );
        let bounds_b = RectEmu::new(
            LengthEmu::new(110),
            LengthEmu::new(210),
            LengthEmu::new(300),
            LengthEmu::new(400),
        );
        let node = |node_id, bounds, color| AuthoredPageRenderNodeV1 {
            node_id,
            bounds,
            solid_fill_rgb: Some(color),
            solid_line: None,
        };

        let mut first = base.clone();
        apply_authored_page_render_lane_v1(
            &mut first,
            &AuthoredPageRenderLaneV1 {
                page_id,
                nodes: vec![node(a, bounds_a, [1, 2, 3]), node(b, bounds_b, [4, 5, 6])],
            },
        )
        .expect("A then B");

        let mut reordered = base;
        apply_authored_page_render_lane_v1(
            &mut reordered,
            &AuthoredPageRenderLaneV1 {
                page_id,
                nodes: vec![node(b, bounds_b, [4, 5, 6]), node(a, bounds_a, [1, 2, 3])],
            },
        )
        .expect("B then A");

        assert_eq!(
            first.nodes[..base_ids.len()]
                .iter()
                .map(|node| node.node_id)
                .collect::<Vec<_>>(),
            base_ids,
            "base/imported lane order must remain untouched"
        );
        assert_eq!(
            reordered.nodes[..base_ids.len()]
                .iter()
                .map(|node| node.node_id)
                .collect::<Vec<_>>(),
            base_ids,
            "reorder must not perturb the base/imported lane"
        );
        assert_eq!(
            first.nodes[base_ids.len()..]
                .iter()
                .map(|node| node.node_id)
                .collect::<Vec<_>>(),
            vec![a, b]
        );
        assert_eq!(
            reordered.nodes[base_ids.len()..]
                .iter()
                .map(|node| node.node_id)
                .collect::<Vec<_>>(),
            vec![b, a]
        );

        let first_a = first
            .nodes
            .iter()
            .find(|node| node.node_id == a)
            .expect("A");
        let reordered_a = reordered
            .nodes
            .iter()
            .find(|node| node.node_id == a)
            .expect("A reordered");
        let first_b = first
            .nodes
            .iter()
            .find(|node| node.node_id == b)
            .expect("B");
        let reordered_b = reordered
            .nodes
            .iter()
            .find(|node| node.node_id == b)
            .expect("B reordered");
        assert_eq!(first_a.bounds, reordered_a.bounds);
        assert_eq!(first_b.bounds, reordered_b.bounds);
        assert_eq!(first_a.solid_fill_rgb, reordered_a.solid_fill_rgb);
        assert_eq!(first_b.solid_fill_rgb, reordered_b.solid_fill_rgb);
    }

    #[test]
    fn authored_lane_rejects_duplicate_and_base_collision_without_partial_append() {
        let mut plan = build_page_render_plan_v1(&fixture(), 0).expect("base render plan");
        let original = plan.clone();
        let base_node = plan.nodes[0].node_id;
        let duplicate: NodeId = serde_json::from_str("\"01890f47-0d12-7abc-8def-0123456789ab\"")
            .expect("authored NodeId");

        let duplicate_lane = AuthoredPageRenderLaneV1 {
            page_id: plan.page_id,
            nodes: vec![
                AuthoredPageRenderNodeV1 {
                    node_id: duplicate,
                    bounds: RectEmu::new(
                        LengthEmu::new(0),
                        LengthEmu::new(0),
                        LengthEmu::new(10),
                        LengthEmu::new(10),
                    ),
                    solid_fill_rgb: None,
                    solid_line: None,
                },
                AuthoredPageRenderNodeV1 {
                    node_id: duplicate,
                    bounds: RectEmu::new(
                        LengthEmu::new(0),
                        LengthEmu::new(0),
                        LengthEmu::new(10),
                        LengthEmu::new(10),
                    ),
                    solid_fill_rgb: None,
                    solid_line: None,
                },
            ],
        };
        assert!(matches!(
            apply_authored_page_render_lane_v1(&mut plan, &duplicate_lane),
            Err(RenderPlanErrorV1::AuthoredLaneDuplicateNode { .. })
        ));
        assert_eq!(plan, original);

        let collision_lane = AuthoredPageRenderLaneV1 {
            page_id: plan.page_id,
            nodes: vec![AuthoredPageRenderNodeV1 {
                node_id: base_node,
                bounds: RectEmu::new(
                    LengthEmu::new(0),
                    LengthEmu::new(0),
                    LengthEmu::new(10),
                    LengthEmu::new(10),
                ),
                solid_fill_rgb: None,
                solid_line: None,
            }],
        };
        assert!(matches!(
            apply_authored_page_render_lane_v1(&mut plan, &collision_lane),
            Err(RenderPlanErrorV1::AuthoredLaneBaseCollision { .. })
        ));
        assert_eq!(plan, original);
    }

    #[test]
    fn missing_page_is_typed_failure() {
        assert!(matches!(
            build_page_render_plan_v1(&fixture(), 1),
            Err(RenderPlanErrorV1::PageIndexOutOfBounds { page_index: 1 })
        ));
    }
}
