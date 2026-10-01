use std::collections::{HashMap, HashSet};

use chaptera_viewer_render_plan::{
    ExplicitRenderTextFontResourceV1, NodeRenderPlanV1, RenderTextFragmentV1,
    RenderTextLayoutDispositionV1, build_page_render_plan_with_text_layout_v1,
};
use pub_viewer::{ViewerGeometryDocument, ViewerPagePaintOrderV1};
use serde::Serialize;
use serde_json::Value;

pub const READER_SCENE_V1: &str = "chaptera.reader-scene.v1";

const MAX_INLINE_IMAGE_RESOURCE_BYTES: usize = 4 * 1024 * 1024;
const MAX_INLINE_IMAGE_TOTAL_BYTES: usize = 8 * 1024 * 1024;
const SHARED_FALLBACK_FONT_MIME: &str = "font/ttf";

#[derive(Debug, Serialize)]
pub struct ReaderSceneV1 {
    pub protocol_version: &'static str,
    pub document_id: String,
    pub source_hash: String,
    pub revision_id: String,
    pub scene_authority: &'static str,
    pub stacking_fidelity: &'static str,
    pub fidelity: ReaderFidelityV1,
    pub pages: Vec<ReaderPageV1>,
    pub nodes: Vec<ReaderNodeV1>,
    pub stories: Vec<ReaderStoryV1>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub resources: Vec<ReaderImageResourceV1>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub fonts: Vec<ReaderFontResourceV1>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<ReaderDiagnosticV1>,
}

#[derive(Debug, Serialize)]
pub struct ReaderFidelityV1 {
    pub state: &'static str,
    pub reasons: Vec<&'static str>,
}

#[derive(Debug, Serialize)]
pub struct ReaderPageV1 {
    pub page_id: String,
    pub order: u32,
    pub width_emu: i64,
    pub height_emu: i64,
}

#[derive(Debug, Serialize)]
pub struct ReaderNodeV1 {
    pub node_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin_node_id: Option<String>,
    pub page_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_node_id: Option<String>,
    pub kind: &'static str,
    pub bounds: ReaderRectV1,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_bounds: Option<ReaderRectV1>,
    pub transform: ReaderTransformV1,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub paint: Option<ReaderPaintV1>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_source_window: Option<ReaderImageSourceWindowV1>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub table: Option<ReaderTableV1>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_layout: Option<ReaderTextLayoutV1>,
}

#[derive(Debug, Serialize)]
pub struct ReaderRectV1 {
    pub x: i64,
    pub y: i64,
    pub width: i64,
    pub height: i64,
}

#[derive(Debug, Serialize)]
pub struct ReaderTransformV1 {
    pub a: String,
    pub b: String,
    pub c: String,
    pub d: String,
    pub tx: i64,
    pub ty: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReaderPaintV1 {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fill_rgb: Option<[u8; 3]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<ReaderLineV1>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReaderLineV1 {
    pub rgb: [u8; 3],
    pub width_emu: i64,
}

#[derive(Debug, Serialize)]
pub struct ReaderTableV1 {
    pub story_id: String,
    pub rows: u32,
    pub columns: u32,
    pub cells: Vec<ReaderTableCellV1>,
}

#[derive(Debug, Serialize)]
pub struct ReaderTableCellV1 {
    pub cell_id: String,
    pub row: u32,
    pub column: u32,
    #[serde(skip_serializing_if = "table_span_is_one")]
    pub row_span: u32,
    #[serde(skip_serializing_if = "table_span_is_one")]
    pub column_span: u32,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bounds: Option<ReaderRectV1>,
}

fn table_span_is_one(value: &u32) -> bool {
    *value == 1
}

#[derive(Debug, Serialize)]
pub struct ReaderTextLayoutV1 {
    pub disposition: &'static str,
    pub font_resource_id: String,
    pub font_fingerprint_sha256: String,
    pub font_size_emu: i64,
    pub line_height_emu: i64,
    pub lines: Vec<ReaderTextLineV1>,
}

#[derive(Debug, Serialize)]
pub struct ReaderTextLineV1 {
    pub line_index: u32,
    pub scalar_start: u32,
    pub scalar_end: u32,
    pub consumed_scalar_end: u32,
    pub text: String,
    pub measured_width_emu: i64,
    pub line_height_emu: i64,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub spans: Vec<ReaderTextSpanV1>,
}

#[derive(Debug, Serialize)]
pub struct ReaderTextSpanV1 {
    pub scalar_start: u32,
    pub scalar_end: u32,
    pub text: String,
    pub x_offset_emu: i64,
    pub measured_width_emu: i64,
    pub font_size_emu: i64,
}

#[derive(Debug, Serialize)]
pub struct ReaderStoryV1 {
    pub story_id: String,
    pub text: String,
    pub text_fidelity: &'static str,
}

#[derive(Debug, Serialize)]
pub struct ReaderImageSourceWindowV1 {
    pub left_q16: i64,
    pub top_q16: i64,
    pub right_q16: i64,
    pub bottom_q16: i64,
}

#[derive(Debug, Serialize)]
pub struct ReaderImageResourceV1 {
    pub resource_id: String,
    pub mime: String,
    pub availability: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inline_data_url: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ReaderFontResourceV1 {
    pub resource_id: &'static str,
    pub family_name: &'static str,
    pub mime: &'static str,
    pub expected_sha256: &'static str,
    pub availability: &'static str,
    pub inline_data_url: String,
}

#[derive(Debug, Serialize)]
pub struct ReaderDiagnosticV1 {
    pub code: String,
    pub severity: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin_id: Option<String>,
    pub message: String,
}

fn bind_visible_paint(
    paint_by_node: &mut HashMap<String, ReaderPaintV1>,
    visible_node_ids: &HashSet<String>,
    node_id: String,
    paint: ReaderPaintV1,
) -> Result<(), String> {
    if !visible_node_ids.contains(&node_id) {
        return Ok(());
    }
    if paint.fill_rgb.is_none() && paint.line.is_none() {
        return Ok(());
    }
    if paint_by_node.insert(node_id.clone(), paint).is_some() {
        return Err(format!("duplicate paint binding for node {node_id}"));
    }
    Ok(())
}

fn take_direct_render_text(
    render_text_by_node: &mut HashMap<String, String>,
    source_text_by_node: &HashMap<String, String>,
    node_id: &str,
) -> Option<String> {
    render_text_by_node
        .remove(node_id)
        .or_else(|| source_text_by_node.get(node_id).cloned())
}

fn reader_text_layout_from_render_text(
    text: &RenderTextFragmentV1,
) -> (Option<ReaderTextLayoutV1>, bool) {
    let Some(layout) = text.layout.as_ref() else {
        return (None, true);
    };
    let RenderTextLayoutDispositionV1::SharedResolved {
        font_resource_id,
        font_fingerprint_sha256,
        font_size_emu,
        line_height_emu,
    } = &layout.disposition
    else {
        return (None, true);
    };

    let partial = layout.lines.iter().any(|line| !line.spans.is_empty());
    let mapped = ReaderTextLayoutV1 {
        disposition: "shared_resolved",
        font_resource_id: font_resource_id.clone(),
        font_fingerprint_sha256: font_fingerprint_sha256.clone(),
        font_size_emu: *font_size_emu,
        line_height_emu: *line_height_emu,
        lines: layout
            .lines
            .iter()
            .map(|line| ReaderTextLineV1 {
                line_index: line.line_index,
                scalar_start: line.scalar_start,
                scalar_end: line.scalar_end,
                consumed_scalar_end: line.consumed_scalar_end,
                text: line.text.clone(),
                measured_width_emu: line.measured_width_emu,
                line_height_emu: line.line_height_emu,
                spans: line
                    .spans
                    .iter()
                    .map(|span| ReaderTextSpanV1 {
                        scalar_start: span.scalar_start,
                        scalar_end: span.scalar_end,
                        text: span.text.clone(),
                        x_offset_emu: span.x_offset_emu,
                        measured_width_emu: span.measured_width_emu,
                        font_size_emu: span.font_size_emu,
                    })
                    .collect(),
            })
            .collect(),
    };
    (Some(mapped), partial)
}

fn projected_node_kind(node: &NodeRenderPlanV1) -> Result<&'static str, String> {
    if node.table.is_some() {
        return Err("projected Scene tables are not admitted by the Cmo consumer".to_owned());
    }
    match (node.text.is_some(), node.image.is_some()) {
        (true, false) => Ok("text_frame"),
        (false, true) => Ok("picture_frame"),
        (false, false) => Ok("unknown"),
        (true, true) => Err(format!(
            "projected Scene instance for origin {:?} has conflicting text/image semantics",
            node.node_id
        )),
    }
}

fn insert_projected_nodes_after_targets(
    nodes: Vec<ReaderNodeV1>,
    mut projected_by_target: HashMap<String, Vec<ReaderNodeV1>>,
) -> Result<Vec<ReaderNodeV1>, String> {
    let projected_count = projected_by_target.values().map(Vec::len).sum::<usize>();
    let mut ordered = Vec::with_capacity(nodes.len() + projected_count);
    for node in nodes {
        let target_id = node.node_id.clone();
        ordered.push(node);
        if let Some(mut projected) = projected_by_target.remove(&target_id) {
            ordered.append(&mut projected);
        }
    }
    if !projected_by_target.is_empty() {
        let mut dangling = projected_by_target.into_keys().collect::<Vec<_>>();
        dangling.sort();
        return Err(format!(
            "projected Scene instances reference missing target frame(s): {}",
            dangling.join(",")
        ));
    }
    Ok(ordered)
}

pub fn from_viewer_geometry(
    document_id: String,
    source_hash: String,
    revision_id: String,
    geometry: &ViewerGeometryDocument,
    source_page_paint_orders: &[ViewerPagePaintOrderV1],
) -> Result<ReaderSceneV1, String> {
    let viewer_source_hash =
        serialized_string(&geometry.document.source.source_hash, "Viewer source hash")?;
    if viewer_source_hash != source_hash {
        return Err(format!(
            "Viewer source hash {viewer_source_hash} differs from durable source authority {source_hash}"
        ));
    }

    let mut pages = Vec::with_capacity(geometry.document.pages.len());
    let mut page_ids = HashSet::new();
    for page in &geometry.document.pages {
        if page.index == 0 {
            return Err("Viewer page index must be one-based".to_owned());
        }
        if page.width_emu <= 0 || page.height_emu <= 0 {
            return Err("Viewer page dimensions must be positive".to_owned());
        }
        let page_id = serialized_string(&page.id, "page id")?;
        if !page_ids.insert(page_id.clone()) {
            return Err(format!("duplicate Viewer page id {page_id}"));
        }
        pages.push(ReaderPageV1 {
            page_id,
            order: page.index - 1,
            width_emu: page.width_emu,
            height_emu: page.height_emu,
        });
    }
    pages.sort_by_key(|page| page.order);

    let mut parent_by_node = HashMap::new();
    let mut raw_nodes = Vec::with_capacity(geometry.scene.nodes.len());
    for node in &geometry.scene.nodes {
        let node_id = serialized_string(&node.origin, "node id")?;
        let parent_id = serialized_string(&node.parent_origin, "node parent id")?;
        if parent_by_node
            .insert(node_id.clone(), parent_id.clone())
            .is_some()
        {
            return Err(format!("duplicate Viewer node id {node_id}"));
        }
        let bounds = rect_from_serialized(&node.bounds)?;
        if bounds.width <= 0 || bounds.height <= 0 {
            return Err(format!("Viewer node {node_id} has non-positive bounds"));
        }
        raw_nodes.push((
            node_id,
            parent_id,
            bounds,
            transform_from_serialized(&node.transform)?,
        ));
    }

    let node_ids = parent_by_node.keys().cloned().collect::<HashSet<_>>();
    let mut page_cache = HashMap::new();
    for node_id in &node_ids {
        resolve_page(
            node_id,
            &parent_by_node,
            &page_ids,
            &mut page_cache,
            &mut HashSet::new(),
        )?;
    }

    let mut kind_by_node = node_ids
        .iter()
        .map(|node_id| (node_id.clone(), "unknown"))
        .collect::<HashMap<_, _>>();
    for frame in &geometry.story_frames {
        let node_id = serialized_string(&frame.frame_id, "story frame node id")?;
        if !node_ids.contains(&node_id) {
            return Err(format!("story frame references unknown node {node_id}"));
        }
        bind_kind(&mut kind_by_node, &node_id, "text_frame")?;
    }

    let mut resource_by_node = HashMap::new();
    let mut source_window_by_node = HashMap::new();
    let mut resources = Vec::with_capacity(geometry.images.len());
    let mut resource_ids = HashSet::new();
    let mut inline_image_budget = MAX_INLINE_IMAGE_TOTAL_BYTES;
    for image in &geometry.images {
        let resource_id = serialized_string(&image.resource_id, "image resource id")?;
        if !resource_ids.insert(resource_id.clone()) {
            return Err(format!("duplicate Viewer image resource {resource_id}"));
        }
        resources.push(reader_image_resource(
            resource_id.clone(),
            image.mime.clone(),
            &image.bytes,
            &mut inline_image_budget,
        ));

        for placement in &image.placements {
            let node_id = serialized_string(&placement.node_id, "image placement node id")?;
            if !node_ids.contains(&node_id) {
                return Err(format!("image placement references unknown node {node_id}"));
            }
            let Some(window) = placement.source_window.as_ref() else {
                continue;
            };
            let mapped = ReaderImageSourceWindowV1 {
                left_q16: window.left_q16,
                top_q16: window.top_q16,
                right_q16: window.right_q16,
                bottom_q16: window.bottom_q16,
            };
            if source_window_by_node
                .insert(node_id.clone(), mapped)
                .is_some()
            {
                return Err(format!("duplicate image placement for node {node_id}"));
            }
        }

        for node_id in &image.node_ids {
            let node_id = serialized_string(node_id, "image node id")?;
            if !node_ids.contains(&node_id) {
                return Err(format!("image resource references unknown node {node_id}"));
            }
            bind_kind(&mut kind_by_node, &node_id, "picture_frame")?;
            match resource_by_node.insert(node_id.clone(), resource_id.clone()) {
                Some(existing) if existing != resource_id => {
                    return Err(format!("node {node_id} has multiple image resources"));
                }
                _ => {}
            }
        }
    }

    let mut table_by_node = HashMap::new();
    for table in &geometry.tables {
        let node_id = serialized_string(&table.node_id, "table node id")?;
        if !node_ids.contains(&node_id) {
            return Err(format!("table references unknown node {node_id}"));
        }
        bind_kind(&mut kind_by_node, &node_id, "table")?;

        let mut cells = Vec::with_capacity(table.cells.len());
        for cell in &table.cells {
            let bounds = cell.bounds.as_ref().map(rect_from_serialized).transpose()?;
            if bounds
                .as_ref()
                .is_some_and(|bounds| bounds.width <= 0 || bounds.height <= 0)
            {
                return Err(format!(
                    "table {node_id} cell has non-positive resolved bounds"
                ));
            }
            cells.push(ReaderTableCellV1 {
                cell_id: serialized_string(&cell.id, "table cell id")?,
                row: cell.address.row,
                column: cell.address.column,
                row_span: cell.row_span,
                column_span: cell.column_span,
                text: cell.text.clone(),
                bounds,
            });
        }

        let mapped = ReaderTableV1 {
            story_id: serialized_string(&table.story_id, "table story id")?,
            rows: table.rows,
            columns: table.columns,
            cells,
        };
        if table_by_node.insert(node_id.clone(), mapped).is_some() {
            return Err(format!("duplicate table binding for node {node_id}"));
        }
    }

    let mut paint_by_node = HashMap::new();
    for paint in &geometry.paints {
        let node_id = serialized_string(&paint.node_id, "paint node id")?;
        let mapped = ReaderPaintV1 {
            fill_rgb: paint.solid_fill_rgb,
            line: paint.solid_line.as_ref().map(|line| ReaderLineV1 {
                rgb: line.rgb,
                width_emu: line.width_emu,
            }),
        };
        bind_visible_paint(&mut paint_by_node, &node_ids, node_id, mapped)?;
    }

    let mut fragments_by_node: HashMap<String, Vec<(u32, u32, String)>> = HashMap::new();
    for fragment in &geometry.text_fragments {
        let node_id = serialized_string(&fragment.frame_id, "text fragment frame id")?;
        if !node_ids.contains(&node_id) {
            return Err(format!("text fragment references unknown node {node_id}"));
        }
        fragments_by_node.entry(node_id).or_default().push((
            fragment.scalar_start,
            fragment.scalar_end,
            fragment.text.clone(),
        ));
    }
    let text_by_node = fragments_by_node
        .into_iter()
        .map(|(node_id, mut fragments)| {
            fragments.sort_by_key(|(start, end, _)| (*start, *end));
            let text = fragments.into_iter().map(|(_, _, text)| text).collect();
            (node_id, text)
        })
        .collect::<HashMap<_, String>>();

    chaptera_desktop_fallback_font_resource::validate()
        .map_err(|error| format!("shared fallback font validation failed: {error}"))?;
    let fallback_font = shared_text_font_resource();
    let mut render_text_by_node = HashMap::<String, String>::new();
    let mut text_layout_by_node = HashMap::new();
    let mut projected_nodes_by_target = HashMap::<String, Vec<ReaderNodeV1>>::new();
    let mut projected_instance_ids = HashSet::<String>::new();
    let mut projected_text_layout_count = 0_usize;
    let mut projected_kind_partial = false;
    let mut text_layout_partial = false;
    for page_index in 0..geometry.document.pages.len() {
        let plan = match build_page_render_plan_with_text_layout_v1(
            geometry,
            page_index,
            &fallback_font,
        ) {
            Ok(plan) => plan,
            Err(_) => {
                text_layout_partial = true;
                continue;
            }
        };
        let plan_page_id = serialized_string(&plan.page_id, "render-plan page id")?;

        for node in plan.nodes {
            let (mapped_layout, layout_partial) = match node.text.as_ref() {
                Some(text) => reader_text_layout_from_render_text(text),
                None => (None, false),
            };
            text_layout_partial |= layout_partial;

            if let Some(instance) = node.projected_scene_instance.as_ref() {
                if !projected_instance_ids.insert(instance.instance_id.clone()) {
                    return Err(format!(
                        "duplicate projected Scene instance {}",
                        instance.instance_id
                    ));
                }
                if node_ids.contains(&instance.instance_id) {
                    return Err(format!(
                        "projected Scene instance {} collides with authored node identity",
                        instance.instance_id
                    ));
                }
                if instance.target_page_id != plan_page_id
                    || !page_ids.contains(&instance.target_page_id)
                {
                    return Err(format!(
                        "projected Scene instance {} targets unexpected page {}",
                        instance.instance_id, instance.target_page_id
                    ));
                }

                let origin_node_id = serialized_string(&node.node_id, "projected origin node id")?;
                if origin_node_id != instance.origin_node_id {
                    return Err(format!(
                        "projected Scene instance {} origin {} differs from render-plan origin {}",
                        instance.instance_id, instance.origin_node_id, origin_node_id
                    ));
                }
                let mut projected_matches =
                    geometry.projected_instances.iter().filter(|projected| {
                        projected.scene_instance.instance_id == instance.instance_id
                    });
                let projected = projected_matches.next().ok_or_else(|| {
                    format!(
                        "render-plan projected instance {} has no Viewer projection record",
                        instance.instance_id
                    )
                })?;
                if projected_matches.next().is_some() {
                    return Err(format!(
                        "duplicate Viewer projected instance {}",
                        instance.instance_id
                    ));
                }
                if projected.scene_instance != *instance {
                    return Err(format!(
                        "render-plan projected instance {} differs from Viewer projection identity",
                        instance.instance_id
                    ));
                }
                let target_frame_node_id =
                    serialized_string(&projected.target_frame_node_id, "projected target frame")?;
                if !node_ids.contains(&target_frame_node_id) {
                    return Err(format!(
                        "projected Scene instance {} targets unknown Viewer frame {}",
                        instance.instance_id, target_frame_node_id
                    ));
                }

                let kind = projected_node_kind(&node)?;
                projected_kind_partial |= kind == "unknown";
                let bounds = rect_from_serialized(&node.bounds)?;
                let text_bounds = node
                    .text_bounds
                    .as_ref()
                    .map(rect_from_serialized)
                    .transpose()?;
                if text_bounds
                    .as_ref()
                    .is_some_and(|bounds| bounds.width <= 0 || bounds.height <= 0)
                {
                    return Err(format!(
                        "projected Scene instance {} has non-positive text bounds",
                        instance.instance_id
                    ));
                }
                if bounds.width <= 0 || bounds.height <= 0 {
                    return Err(format!(
                        "projected Scene instance {} has non-positive bounds",
                        instance.instance_id
                    ));
                }

                let (resource_id, image_source_window) = if let Some(image) = node.image.as_ref() {
                    let resource_id =
                        serialized_string(&image.resource_id, "projected image resource id")?;
                    if !resource_ids.contains(&resource_id) {
                        return Err(format!(
                            "projected Scene instance {} references unknown image resource {}",
                            instance.instance_id, resource_id
                        ));
                    }
                    let source_window =
                        image
                            .source_window
                            .as_ref()
                            .map(|window| ReaderImageSourceWindowV1 {
                                left_q16: window.left_q16,
                                top_q16: window.top_q16,
                                right_q16: window.right_q16,
                                bottom_q16: window.bottom_q16,
                            });
                    (Some(resource_id), source_window)
                } else {
                    (None, None)
                };

                let paint = if node.solid_fill_rgb.is_some() || node.solid_line.is_some() {
                    Some(ReaderPaintV1 {
                        fill_rgb: node.solid_fill_rgb,
                        line: node.solid_line.as_ref().map(|line| ReaderLineV1 {
                            rgb: line.rgb,
                            width_emu: line.width_emu,
                        }),
                    })
                } else {
                    None
                };
                if mapped_layout.is_some() {
                    projected_text_layout_count += 1;
                }
                projected_nodes_by_target
                    .entry(target_frame_node_id)
                    .or_default()
                    .push(ReaderNodeV1 {
                        node_id: instance.instance_id.clone(),
                        origin_node_id: Some(instance.origin_node_id.clone()),
                        page_id: instance.target_page_id.clone(),
                        parent_node_id: None,
                        kind,
                        bounds,
                        text_bounds,
                        transform: transform_from_serialized(&node.transform)?,
                        paint,
                        resource_id,
                        image_source_window,
                        table: None,
                        text: node.text.as_ref().map(|text| text.text.clone()),
                        text_layout: mapped_layout,
                    });
                continue;
            }

            let node_id = serialized_string(&node.node_id, "direct render-plan node id")?;
            if !node_ids.contains(&node_id) {
                return Err(format!(
                    "direct render-plan node references unknown Viewer node {node_id}"
                ));
            }
            if let Some(text) = node.text.as_ref()
                && render_text_by_node
                    .insert(node_id.clone(), text.text.clone())
                    .is_some()
            {
                return Err(format!(
                    "duplicate direct render-plan text binding for node {node_id}"
                ));
            }
            if let Some(mapped_layout) = mapped_layout
                && text_layout_by_node
                    .insert(node_id.clone(), mapped_layout)
                    .is_some()
            {
                return Err(format!("duplicate text layout binding for node {node_id}"));
            }
        }
    }
    if text_by_node.len() > text_layout_by_node.len() {
        text_layout_partial = true;
    }
    let text_layout_count = text_layout_by_node.len() + projected_text_layout_count;

    let mut nodes = Vec::with_capacity(raw_nodes.len());
    for (node_id, parent_id, bounds, transform) in raw_nodes {
        let page_id = page_cache
            .get(&node_id)
            .cloned()
            .ok_or_else(|| format!("node {node_id} has no resolved page"))?;
        let parent_node_id = node_ids.contains(&parent_id).then_some(parent_id);
        nodes.push(ReaderNodeV1 {
            origin_node_id: None,
            kind: kind_by_node
                .get(&node_id)
                .copied()
                .ok_or_else(|| format!("node kind missing for {node_id}"))?,
            paint: paint_by_node.remove(&node_id),
            resource_id: resource_by_node.remove(&node_id),
            image_source_window: source_window_by_node.remove(&node_id),
            table: table_by_node.remove(&node_id),
            text: take_direct_render_text(&mut render_text_by_node, &text_by_node, &node_id),
            text_layout: text_layout_by_node.remove(&node_id),
            node_id,
            page_id,
            parent_node_id,
            bounds,
            text_bounds: None,
            transform,
        });
    }

    let mut stacking_known =
        apply_source_page_paint_order(&mut nodes, &pages, source_page_paint_orders)?;
    if !projected_nodes_by_target.is_empty() {
        nodes = insert_projected_nodes_after_targets(nodes, projected_nodes_by_target)?;
        // Persisted source page-paint receipts do not claim a total order across
        // projected Cmo visuals. Preserve direct-node source order and the
        // render-plan target-frame anchor while keeping the overall claim partial.
        stacking_known = false;
    }

    let stories = geometry
        .document
        .stories
        .iter()
        .map(|story| {
            Ok(ReaderStoryV1 {
                story_id: serialized_string(&story.id, "story id")?,
                text: story.text.clone(),
                text_fidelity: "partial",
            })
        })
        .collect::<Result<Vec<_>, String>>()?;

    let fonts = if text_layout_count > 0 {
        vec![ReaderFontResourceV1 {
            resource_id: chaptera_desktop_fallback_font_resource::RESOURCE_ID,
            family_name: chaptera_desktop_fallback_font_resource::FAMILY_NAME,
            mime: SHARED_FALLBACK_FONT_MIME,
            expected_sha256: chaptera_desktop_fallback_font_resource::EXPECTED_SHA256,
            availability: "inline_data_url",
            inline_data_url: format!(
                "data:{SHARED_FALLBACK_FONT_MIME};base64,{}",
                base64_encode(chaptera_desktop_fallback_font_resource::bytes())
            ),
        }]
    } else {
        Vec::new()
    };

    let mut diagnostics = Vec::new();
    for diagnostic in &geometry.document.diagnostics {
        diagnostics.push(ReaderDiagnosticV1 {
            code: diagnostic.code.clone(),
            severity: serialized_severity(&diagnostic.severity)?,
            origin_id: None,
            message: diagnostic.message.clone(),
        });
    }
    for diagnostic in &geometry.scene.diagnostics {
        diagnostics.push(ReaderDiagnosticV1 {
            code: diagnostic.code.clone(),
            severity: "warning",
            origin_id: Some(serialized_string(&diagnostic.origin, "diagnostic origin")?),
            message: diagnostic.message.clone(),
        });
    }

    let mut reasons = Vec::new();
    if !nodes.is_empty() && !stacking_known {
        reasons.push("stacking_order_unavailable");
    }
    if kind_by_node.values().any(|kind| *kind == "unknown") || projected_kind_partial {
        reasons.push("node_kind_partial");
    }
    if resources
        .iter()
        .any(|resource| resource.inline_data_url.is_none())
    {
        reasons.push("image_resource_not_inline");
    }
    if text_layout_partial {
        reasons.push("text_layout_partial");
    }
    if diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity == "warning")
    {
        reasons.push("viewer_fidelity_warnings");
    }

    Ok(ReaderSceneV1 {
        protocol_version: READER_SCENE_V1,
        document_id,
        source_hash,
        revision_id,
        scene_authority: "server_viewer_projection",
        stacking_fidelity: if stacking_known {
            "source_back_to_front"
        } else {
            "unknown"
        },
        fidelity: ReaderFidelityV1 {
            state: if reasons.is_empty() {
                "supported"
            } else {
                "partial"
            },
            reasons,
        },
        pages,
        nodes,
        stories,
        resources,
        fonts,
        diagnostics,
    })
}

fn apply_source_page_paint_order(
    nodes: &mut [ReaderNodeV1],
    pages: &[ReaderPageV1],
    source_orders: &[ViewerPagePaintOrderV1],
) -> Result<bool, String> {
    if nodes.is_empty() {
        return Ok(false);
    }

    let page_order = pages
        .iter()
        .enumerate()
        .map(|(index, page)| (page.page_id.as_str(), index))
        .collect::<HashMap<_, _>>();
    let mut nodes_by_page = HashMap::<&str, HashSet<&str>>::new();
    for node in nodes.iter() {
        nodes_by_page
            .entry(node.page_id.as_str())
            .or_default()
            .insert(node.node_id.as_str());
    }

    let mut rank = HashMap::<String, (usize, usize)>::new();
    let mut seen_pages = HashSet::new();
    for order in source_orders {
        let page_id = serialized_string(&order.page_id, "source paint-order page id")?;
        let Some(page_rank) = page_order.get(page_id.as_str()).copied() else {
            continue;
        };
        if !seen_pages.insert(page_id.clone()) {
            return Ok(false);
        }

        let Some(expected_nodes) = nodes_by_page.get(page_id.as_str()) else {
            continue;
        };
        let mut actual_nodes = HashSet::new();
        for (stack_rank, node_id) in order.node_ids.iter().enumerate() {
            let node_id = serialized_string(node_id, "source paint-order node id")?;
            if !expected_nodes.contains(node_id.as_str())
                || !actual_nodes.insert(node_id.clone())
                || rank.insert(node_id, (page_rank, stack_rank)).is_some()
            {
                return Ok(false);
            }
        }
        if actual_nodes.len() != expected_nodes.len() {
            return Ok(false);
        }
    }

    if nodes_by_page
        .keys()
        .any(|page_id| !seen_pages.contains(*page_id))
        || rank.len() != nodes.len()
    {
        return Ok(false);
    }

    nodes.sort_by_key(|node| {
        rank.get(&node.node_id)
            .copied()
            .expect("complete source paint-order rank validated above")
    });
    Ok(true)
}

fn bind_kind(
    kind_by_node: &mut HashMap<String, &'static str>,
    node_id: &str,
    desired: &'static str,
) -> Result<(), String> {
    let kind = kind_by_node
        .get_mut(node_id)
        .ok_or_else(|| format!("semantic binding references unknown node {node_id}"))?;
    if *kind != "unknown" && *kind != desired {
        return Err(format!(
            "node {node_id} has conflicting semantic kinds {} and {desired}",
            *kind
        ));
    }
    *kind = desired;
    Ok(())
}

fn resolve_page(
    node_id: &str,
    parent_by_node: &HashMap<String, String>,
    page_ids: &HashSet<String>,
    page_cache: &mut HashMap<String, String>,
    visiting: &mut HashSet<String>,
) -> Result<String, String> {
    if let Some(page_id) = page_cache.get(node_id) {
        return Ok(page_id.clone());
    }
    if !visiting.insert(node_id.to_owned()) {
        return Err(format!("Viewer node parent cycle contains {node_id}"));
    }
    let parent = parent_by_node
        .get(node_id)
        .ok_or_else(|| format!("missing Viewer node {node_id}"))?;
    let page_id = if page_ids.contains(parent) {
        parent.clone()
    } else if parent_by_node.contains_key(parent) {
        resolve_page(parent, parent_by_node, page_ids, page_cache, visiting)?
    } else {
        return Err(format!(
            "Viewer node {node_id} parent {parent} resolves to neither page nor node"
        ));
    };
    visiting.remove(node_id);
    page_cache.insert(node_id.to_owned(), page_id.clone());
    Ok(page_id)
}

fn rect_from_serialized<T: Serialize>(value: &T) -> Result<ReaderRectV1, String> {
    let value = serde_json::to_value(value).map_err(|error| error.to_string())?;
    Ok(ReaderRectV1 {
        x: object_i64(&value, "x")?,
        y: object_i64(&value, "y")?,
        width: object_i64(&value, "width")?,
        height: object_i64(&value, "height")?,
    })
}

fn transform_from_serialized<T: Serialize>(value: &T) -> Result<ReaderTransformV1, String> {
    let value = serde_json::to_value(value).map_err(|error| error.to_string())?;
    Ok(ReaderTransformV1 {
        a: object_string(&value, "a")?,
        b: object_string(&value, "b")?,
        c: object_string(&value, "c")?,
        d: object_string(&value, "d")?,
        tx: object_i64(&value, "tx")?,
        ty: object_i64(&value, "ty")?,
    })
}

fn serialized_string<T: Serialize>(value: &T, name: &str) -> Result<String, String> {
    serde_json::to_value(value)
        .map_err(|error| error.to_string())?
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| format!("{name} must serialize as a string"))
}

fn serialized_severity<T: Serialize>(value: &T) -> Result<&'static str, String> {
    match serialized_string(value, "diagnostic severity")?.as_str() {
        "info" => Ok("info"),
        "fidelity_warning" => Ok("warning"),
        other => Err(format!("unsupported Viewer diagnostic severity {other}")),
    }
}

fn object_i64(value: &Value, key: &str) -> Result<i64, String> {
    value
        .get(key)
        .and_then(Value::as_i64)
        .ok_or_else(|| format!("{key} must be an integer"))
}

fn object_string(value: &Value, key: &str) -> Result<String, String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| format!("{key} must be a string"))
}

fn shared_text_font_resource() -> ExplicitRenderTextFontResourceV1<'static> {
    ExplicitRenderTextFontResourceV1 {
        resource_id: chaptera_desktop_fallback_font_resource::RESOURCE_ID,
        expected_sha256: chaptera_desktop_fallback_font_resource::EXPECTED_SHA256,
        face_index: 0,
        default_font_size_emu: chaptera_desktop_fallback_font_resource::FONT_SIZE_EMU,
        default_line_height_emu: chaptera_desktop_fallback_font_resource::LINE_HEIGHT_EMU,
        bytes: chaptera_desktop_fallback_font_resource::bytes(),
    }
}

fn reader_image_resource(
    resource_id: String,
    mime: String,
    bytes: &[u8],
    remaining_budget: &mut usize,
) -> ReaderImageResourceV1 {
    let inline_data_url = inline_image_data_url(&mime, bytes, remaining_budget);
    let availability = if inline_data_url.is_some() {
        "inline_data_url"
    } else {
        "descriptor_only"
    };
    ReaderImageResourceV1 {
        resource_id,
        mime,
        availability,
        inline_data_url,
    }
}

fn inline_image_data_url(mime: &str, bytes: &[u8], remaining_budget: &mut usize) -> Option<String> {
    if !matches!(mime, "image/png" | "image/jpeg" | "image/jpg" | "image/gif")
        || bytes.is_empty()
        || bytes.len() > MAX_INLINE_IMAGE_RESOURCE_BYTES
        || bytes.len() > *remaining_budget
    {
        return None;
    }
    *remaining_budget -= bytes.len();
    Some(format!("data:{mime};base64,{}", base64_encode(bytes)))
}

fn base64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut encoded = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0];
        let b1 = chunk.get(1).copied().unwrap_or(0);
        let b2 = chunk.get(2).copied().unwrap_or(0);

        encoded.push(char::from(TABLE[usize::from(b0 >> 2)]));
        encoded.push(char::from(
            TABLE[usize::from(((b0 & 0x03) << 4) | (b1 >> 4))],
        ));
        if chunk.len() > 1 {
            encoded.push(char::from(
                TABLE[usize::from(((b1 & 0x0f) << 2) | (b2 >> 6))],
            ));
        } else {
            encoded.push('=');
        }
        if chunk.len() > 2 {
            encoded.push(char::from(TABLE[usize::from(b2 & 0x3f)]));
        } else {
            encoded.push('=');
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use std::{
        collections::{BTreeMap, HashMap, HashSet},
        env, fs,
    };

    use pub_editor::Sha256Digest;
    use chaptera_viewer_render_plan::{
        RenderTextLayoutDispositionV1, build_page_render_plan_with_text_layout_v1,
    };
    use pub_reader::analyze_mature_0x2c_text_size_authority;
    use pub_viewer::{open_pub_bundle, viewer_geometry_environment_v0_1};
    use sha2::{Digest, Sha256};

    use super::{
        MAX_INLINE_IMAGE_RESOURCE_BYTES, MAX_INLINE_IMAGE_TOTAL_BYTES, ReaderNodeV1, ReaderPaintV1,
        ReaderRectV1, ReaderTransformV1, base64_encode, bind_visible_paint, from_viewer_geometry,
        inline_image_data_url, insert_projected_nodes_after_targets, reader_image_resource,
        shared_text_font_resource, take_direct_render_text,
    };

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum ProbeImageInlineAdmission {
        Inline,
        UnsupportedMime,
        EmptyPayload,
        PerResourceLimit,
        AggregateBudgetExhausted,
    }

    impl ProbeImageInlineAdmission {
        fn reason(self) -> Option<&'static str> {
            match self {
                Self::Inline => None,
                Self::UnsupportedMime => Some("unsupported_mime"),
                Self::EmptyPayload => Some("empty_payload"),
                Self::PerResourceLimit => Some("per_resource_limit"),
                Self::AggregateBudgetExhausted => Some("aggregate_budget_exhausted"),
            }
        }
    }

    fn classify_probe_image_inline_admission(
        mime: &str,
        byte_len: usize,
        remaining_budget: usize,
    ) -> ProbeImageInlineAdmission {
        if !matches!(mime, "image/png" | "image/jpeg" | "image/jpg" | "image/gif") {
            ProbeImageInlineAdmission::UnsupportedMime
        } else if byte_len == 0 {
            ProbeImageInlineAdmission::EmptyPayload
        } else if byte_len > MAX_INLINE_IMAGE_RESOURCE_BYTES {
            ProbeImageInlineAdmission::PerResourceLimit
        } else if byte_len > remaining_budget {
            ProbeImageInlineAdmission::AggregateBudgetExhausted
        } else {
            ProbeImageInlineAdmission::Inline
        }
    }

    #[test]
    #[ignore = "requires an explicitly pinned external PUB path"]
    fn real_reference_scene_projection_probe() {
        let path = env::var("CHAPTERA_READER_SCENE_PROBE_PUB")
            .expect("CHAPTERA_READER_SCENE_PROBE_PUB must name an exact pinned PUB");
        let expected_sha256 = env::var("CHAPTERA_READER_SCENE_PROBE_SHA256")
            .expect("CHAPTERA_READER_SCENE_PROBE_SHA256 must pin source identity");
        let bytes = fs::read(&path).expect("probe source must be readable");
        let actual_sha256 = format!("{:x}", Sha256::digest(&bytes));
        assert_eq!(
            actual_sha256, expected_sha256,
            "probe source identity drift"
        );
        let bundle = open_pub_bundle(&bytes, viewer_geometry_environment_v0_1())
            .expect("shared Viewer bundle must open the probe source");

        let mut source_explicit_line_any = 0_usize;
        let mut source_explicit_line_color = 0_usize;
        let mut source_explicit_line_width = 0_usize;
        let mut source_explicit_line_visible = 0_usize;
        let mut source_explicit_line_any_effective_none = 0_usize;
        let mut source_explicit_color_effective_missing = 0_usize;
        let mut source_explicit_width_effective_missing = 0_usize;
        let mut source_explicit_visible_effective_missing = 0_usize;
        let mut source_effective_line_any = 0_usize;
        let mut source_effective_line_complete_visible = 0_usize;
        let mut source_effective_line_complete_hidden = 0_usize;
        let mut source_effective_line_incomplete = 0_usize;
        let mut source_effective_line_presence = [0_usize; 8];
        for node in bundle.resolved_graph.nodes.values() {
            let explicit = &node.payload.explicit_paint.line;
            let explicit_any = explicit.color_rgb.is_some()
                || explicit.width_emu.is_some()
                || explicit.visible.is_some();
            source_explicit_line_any += usize::from(explicit_any);
            source_explicit_line_color += usize::from(explicit.color_rgb.is_some());
            source_explicit_line_width += usize::from(explicit.width_emu.is_some());
            source_explicit_line_visible += usize::from(explicit.visible.is_some());

            let Some(effective) = node.payload.effective_paint.as_ref() else {
                source_explicit_line_any_effective_none += usize::from(explicit_any);
                continue;
            };
            let line = &effective.line;
            source_explicit_color_effective_missing +=
                usize::from(explicit.color_rgb.is_some() && line.color_rgb.is_none());
            source_explicit_width_effective_missing +=
                usize::from(explicit.width_emu.is_some() && line.width_emu.is_none());
            source_explicit_visible_effective_missing +=
                usize::from(explicit.visible.is_some() && line.visible.is_none());

            let presence = (usize::from(line.color_rgb.is_some()) << 2)
                | (usize::from(line.width_emu.is_some()) << 1)
                | usize::from(line.visible.is_some());
            source_effective_line_presence[presence] += 1;
            let has_any = presence != 0;
            if !has_any {
                continue;
            }
            source_effective_line_any += 1;
            match (
                line.color_rgb.as_ref(),
                line.width_emu.as_ref(),
                line.visible.as_ref(),
            ) {
                (Some(_), Some(width), Some(visible)) if width.value > 0 && visible.value => {
                    source_effective_line_complete_visible += 1;
                }
                (Some(_), Some(width), Some(visible)) if width.value > 0 && !visible.value => {
                    source_effective_line_complete_hidden += 1;
                }
                _ => source_effective_line_incomplete += 1,
            }
        }

        let viewer_line_paints = bundle
            .geometry
            .paints
            .iter()
            .filter(|paint| paint.solid_line.is_some())
            .count();
        let viewer_line_only_paints = bundle
            .geometry
            .paints
            .iter()
            .filter(|paint| paint.solid_line.is_some() && paint.solid_fill_rgb.is_none())
            .count();
        let viewer_black_lines = bundle
            .geometry
            .paints
            .iter()
            .filter(|paint| {
                paint
                    .solid_line
                    .as_ref()
                    .is_some_and(|line| line.rgb == [0, 0, 0])
            })
            .count();

        match from_viewer_geometry(
            "probe:document".to_owned(),
            actual_sha256,
            "probe:source".to_owned(),
            &bundle.geometry,
            &bundle.source_page_paint_orders,
        ) {
            Ok(scene) => {
                let tables = scene
                    .nodes
                    .iter()
                    .filter_map(|node| node.table.as_ref())
                    .collect::<Vec<_>>();
                let table_cells = tables.iter().map(|table| table.cells.len()).sum::<usize>();
                let spanning_cells = tables
                    .iter()
                    .flat_map(|table| &table.cells)
                    .filter(|cell| cell.row_span > 1 || cell.column_span > 1)
                    .count();
                let bounded_table_cells = tables
                    .iter()
                    .flat_map(|table| &table.cells)
                    .filter(|cell| cell.bounds.is_some())
                    .count();
                let scene_line_nodes = scene
                    .nodes
                    .iter()
                    .filter(|node| {
                        node.paint
                            .as_ref()
                            .and_then(|paint| paint.line.as_ref())
                            .is_some()
                    })
                    .count();
                let scene_line_only_nodes = scene
                    .nodes
                    .iter()
                    .filter(|node| {
                        node.paint
                            .as_ref()
                            .is_some_and(|paint| paint.line.is_some() && paint.fill_rgb.is_none())
                    })
                    .count();
                let scene_black_lines = scene
                    .nodes
                    .iter()
                    .filter(|node| {
                        node.paint
                            .as_ref()
                            .and_then(|paint| paint.line.as_ref())
                            .is_some_and(|line| line.rgb == [0, 0, 0])
                    })
                    .count();
                let page_line_nodes = scene
                    .pages
                    .iter()
                    .map(|page| {
                        scene
                            .nodes
                            .iter()
                            .filter(|node| {
                                node.page_id == page.page_id
                                    && node
                                        .paint
                                        .as_ref()
                                        .and_then(|paint| paint.line.as_ref())
                                        .is_some()
                            })
                            .count()
                    })
                    .collect::<Vec<_>>();
                let projected_scene_nodes = scene
                    .nodes
                    .iter()
                    .filter(|node| node.origin_node_id.is_some())
                    .count();
                let projected_shared_layout_nodes = scene
                    .nodes
                    .iter()
                    .filter(|node| node.origin_node_id.is_some() && node.text_layout.is_some())
                    .count();
                let projected_shared_nonempty_lines = scene
                    .nodes
                    .iter()
                    .filter(|node| node.origin_node_id.is_some())
                    .filter_map(|node| node.text_layout.as_ref())
                    .flat_map(|layout| &layout.lines)
                    .filter(|line| !line.text.trim().is_empty())
                    .count();

                const GUEST_SCENE_BYTE_CAP: usize = 16 * 1024 * 1024;

                let mut descriptor_probe_budget = MAX_INLINE_IMAGE_TOTAL_BYTES;
                let mut inline_resource_count = 0_usize;
                let mut inline_raw_bytes = 0_usize;
                let mut descriptor_only_resource_count = 0_usize;
                let mut descriptor_only_mime_counts = BTreeMap::<String, usize>::new();
                let mut descriptor_only_reason_counts = BTreeMap::<&'static str, usize>::new();
                let mut descriptor_only_total_bytes = 0_usize;
                let mut budget_exhausted_images = Vec::new();
                for image in &bundle.geometry.images {
                    let admission = classify_probe_image_inline_admission(
                        &image.mime,
                        image.bytes.len(),
                        descriptor_probe_budget,
                    );
                    if admission == ProbeImageInlineAdmission::Inline {
                        descriptor_probe_budget -= image.bytes.len();
                        inline_resource_count += 1;
                        inline_raw_bytes += image.bytes.len();
                        continue;
                    }

                    descriptor_only_resource_count += 1;
                    *descriptor_only_mime_counts
                        .entry(image.mime.clone())
                        .or_default() += 1;
                    *descriptor_only_reason_counts
                        .entry(
                            admission
                                .reason()
                                .expect("non-inline probe admission must have a reason"),
                        )
                        .or_default() += 1;
                    descriptor_only_total_bytes += image.bytes.len();
                    if admission == ProbeImageInlineAdmission::AggregateBudgetExhausted {
                        budget_exhausted_images.push(image);
                    }
                }

                let descriptor_scene_resource_ids = scene
                    .resources
                    .iter()
                    .filter(|resource| resource.availability == "descriptor_only")
                    .map(|resource| resource.resource_id.as_str())
                    .collect::<HashSet<_>>();
                assert_eq!(
                    descriptor_only_resource_count,
                    descriptor_scene_resource_ids.len(),
                    "probe admission classifier drifted from Reader Scene resource availability"
                );
                let descriptor_only_visible_node_refs = scene
                    .nodes
                    .iter()
                    .filter(|node| {
                        node.resource_id.as_deref().is_some_and(|resource_id| {
                            descriptor_scene_resource_ids.contains(resource_id)
                        })
                    })
                    .count();

                let scene_json_bytes = serde_json::to_vec(&scene)
                    .expect("serialize exact Reader Scene for byte-envelope probe")
                    .len();
                let inline_data_url_bytes = scene
                    .resources
                    .iter()
                    .filter_map(|resource| resource.inline_data_url.as_ref())
                    .map(String::len)
                    .sum::<usize>();

                let mut hypothetical_scene_json_bytes = scene_json_bytes;
                for image in budget_exhausted_images {
                    let resource_id = super::serialized_string(
                        &image.resource_id,
                        "hypothetical image resource id",
                    )
                    .expect("exact probe image resource id");
                    let current = scene
                        .resources
                        .iter()
                        .find(|resource| resource.resource_id == resource_id)
                        .expect("budget-exhausted image must exist in Reader Scene");
                    assert_eq!(
                        current.availability, "descriptor_only",
                        "budget-exhausted image must currently be descriptor-only"
                    );

                    let mut unlimited_budget = usize::MAX;
                    let hypothetical = reader_image_resource(
                        resource_id,
                        image.mime.clone(),
                        &image.bytes,
                        &mut unlimited_budget,
                    );
                    assert_eq!(
                        hypothetical.availability, "inline_data_url",
                        "budget-exhausted supported image must inline when aggregate budget is removed"
                    );

                    let current_bytes = serde_json::to_vec(current)
                        .expect("serialize current descriptor resource")
                        .len();
                    let hypothetical_bytes = serde_json::to_vec(&hypothetical)
                        .expect("serialize hypothetical inline resource")
                        .len();
                    hypothetical_scene_json_bytes = hypothetical_scene_json_bytes
                        .checked_add(hypothetical_bytes)
                        .and_then(|bytes| bytes.checked_sub(current_bytes))
                        .expect("hypothetical Scene byte accounting overflow");
                }
                let current_scene_margin_bytes =
                    GUEST_SCENE_BYTE_CAP.saturating_sub(scene_json_bytes);
                let hypothetical_scene_margin_bytes =
                    GUEST_SCENE_BYTE_CAP.saturating_sub(hypothetical_scene_json_bytes);

                println!(
                    "CLOUD_READER_SCENE_PROJECTION_PROBE ok state={} stacking={} pages={} nodes={} projected_scene_nodes={} projected_shared_layout_nodes={} projected_shared_nonempty_lines={} tables={} table_cells={} spanning_cells={} bounded_table_cells={} inline_resource_count={} inline_raw_bytes={} inline_data_url_bytes={} descriptor_only_resource_count={} descriptor_only_mime_counts={:?} descriptor_only_reason_counts={:?} descriptor_only_total_bytes={} descriptor_only_visible_node_refs={} scene_json_bytes={} guest_scene_cap_bytes={} current_scene_margin_bytes={} hypothetical_budget_exhausted_inline_scene_json_bytes={} hypothetical_scene_margin_bytes={} source_explicit_line_any={} source_explicit_line_color={} source_explicit_line_width={} source_explicit_line_visible={} source_explicit_line_any_effective_none={} source_explicit_color_effective_missing={} source_explicit_width_effective_missing={} source_explicit_visible_effective_missing={} source_effective_line_presence={:?} source_effective_line_any={} source_effective_line_complete_visible={} source_effective_line_complete_hidden={} source_effective_line_incomplete={} viewer_line_paints={} viewer_line_only_paints={} viewer_black_lines={} scene_line_nodes={} scene_line_only_nodes={} scene_black_lines={} page_line_nodes={:?} reasons={:?}",
                    scene.fidelity.state,
                    scene.stacking_fidelity,
                    scene.pages.len(),
                    scene.nodes.len(),
                    projected_scene_nodes,
                    projected_shared_layout_nodes,
                    projected_shared_nonempty_lines,
                    tables.len(),
                    table_cells,
                    spanning_cells,
                    bounded_table_cells,
                    inline_resource_count,
                    inline_raw_bytes,
                    inline_data_url_bytes,
                    descriptor_only_resource_count,
                    descriptor_only_mime_counts,
                    descriptor_only_reason_counts,
                    descriptor_only_total_bytes,
                    descriptor_only_visible_node_refs,
                    scene_json_bytes,
                    GUEST_SCENE_BYTE_CAP,
                    current_scene_margin_bytes,
                    hypothetical_scene_json_bytes,
                    hypothetical_scene_margin_bytes,
                    source_explicit_line_any,
                    source_explicit_line_color,
                    source_explicit_line_width,
                    source_explicit_line_visible,
                    source_explicit_line_any_effective_none,
                    source_explicit_color_effective_missing,
                    source_explicit_width_effective_missing,
                    source_explicit_visible_effective_missing,
                    source_effective_line_presence,
                    source_effective_line_any,
                    source_effective_line_complete_visible,
                    source_effective_line_complete_hidden,
                    source_effective_line_incomplete,
                    viewer_line_paints,
                    viewer_line_only_paints,
                    viewer_black_lines,
                    scene_line_nodes,
                    scene_line_only_nodes,
                    scene_black_lines,
                    page_line_nodes,
                    scene.fidelity.reasons
                );
            }
            Err(error) => println!("CLOUD_READER_SCENE_PROJECTION_PROBE projection_error={error}"),
        }
    }

    #[test]
    #[ignore = "requires an explicitly pinned external PUB path"]
    fn real_reference_text_layout_fallback_census_probe() {
        let path = env::var("CHAPTERA_READER_SCENE_PROBE_PUB")
            .expect("CHAPTERA_READER_SCENE_PROBE_PUB must name an exact pinned PUB");
        let expected_sha256 = env::var("CHAPTERA_READER_SCENE_PROBE_SHA256")
            .expect("CHAPTERA_READER_SCENE_PROBE_SHA256 must pin source identity");
        let bytes = fs::read(&path).expect("probe source must be readable");
        let actual_sha256 = format!("{:x}", Sha256::digest(&bytes));
        assert_eq!(
            actual_sha256, expected_sha256,
            "probe source identity drift"
        );
        let source_hash: Sha256Digest = expected_sha256
            .parse()
            .expect("probe source SHA-256 must be canonical lowercase hex");

        let bundle = open_pub_bundle(&bytes, viewer_geometry_environment_v0_1())
            .expect("shared Viewer bundle must open the probe source");
        let font = shared_text_font_resource();

        let mut text_nodes = 0_usize;
        let mut shared_frames = 0_usize;
        let mut shared_lines = 0_usize;
        let mut shared_nonempty_lines = 0_usize;
        let mut layout_none = 0_usize;
        let mut backend_fallbacks = BTreeMap::<&'static str, usize>::new();
        let mut projected_text_nodes = 0_usize;
        let mut projected_typography_runs = 0_usize;
        let mut projected_complete_typography_nodes = 0_usize;
        let mut projected_single_family_nodes = 0_usize;
        let mut projected_source_family_fingerprints = BTreeMap::<String, usize>::new();
        let mut projected_blank_source_family_runs = 0_usize;
        let mut projected_source_sizes_emu = BTreeMap::<u32, usize>::new();
        let mut projected_backend_resources = BTreeMap::<String, usize>::new();
        let mut projected_layout_resources = BTreeMap::<String, usize>::new();
        let mut projected_layout_fingerprints = BTreeMap::<String, usize>::new();
        let mut projected_line_counts = BTreeMap::<usize, usize>::new();
        let mut projected_line_heights_emu = BTreeMap::<i64, usize>::new();
        let mut projected_text_bounds_nodes = 0_usize;
        let mut projected_uniform_insets_emu = BTreeMap::<i64, usize>::new();
        let mut projected_measured_width_total_emu = 0_i128;
        let mut projected_zero_typography_size_authority = Vec::new();

        for page_index in 0..bundle.geometry.document.pages.len() {
            let plan =
                build_page_render_plan_with_text_layout_v1(&bundle.geometry, page_index, &font)
                    .expect("exact reference render plan must build");

            for node in plan.nodes {
                let projected = node.projected_scene_instance.is_some();
                let Some(text) = node.text else {
                    continue;
                };
                text_nodes += 1;

                if projected {
                    projected_text_nodes += 1;
                    projected_typography_runs += text.typography.len();
                    if text.typography.is_empty()
                        && actual_sha256
                            == "bf9cda0f632b5820ab9dbdbe1b838b2a988b2f3fdd69253c22b4fc3aef9f11c3"
                    {
                        projected_zero_typography_size_authority.push(
                            analyze_mature_0x2c_text_size_authority(
                                std::io::Cursor::new(bytes.as_slice()),
                                source_hash,
                                text.story_id,
                                text.scalar_start,
                                text.scalar_end,
                            )
                            .expect("exact Carlton zero-typography size authority must diagnose"),
                        );
                    }
                    if let Some(text_bounds) = node.text_bounds {
                        projected_text_bounds_nodes += 1;
                        let left = text_bounds.x.get() - node.bounds.x.get();
                        let top = text_bounds.y.get() - node.bounds.y.get();
                        let right = node.bounds.x.get() + node.bounds.width.get()
                            - text_bounds.x.get()
                            - text_bounds.width.get();
                        let bottom = node.bounds.y.get() + node.bounds.height.get()
                            - text_bounds.y.get()
                            - text_bounds.height.get();
                        if left >= 0 && left == top && left == right && left == bottom {
                            *projected_uniform_insets_emu.entry(left).or_default() += 1;
                        }
                    }

                    let mut cursor = text.scalar_start;
                    let mut complete_coverage = !text.typography.is_empty();
                    let mut families = std::collections::BTreeSet::<String>::new();
                    for run in &text.typography {
                        let normalized_family = run.source_font_name.trim().to_lowercase();
                        complete_coverage &= run.scalar_start == cursor
                            && run.scalar_end > run.scalar_start
                            && run.scalar_end <= text.scalar_end
                            && !normalized_family.is_empty();
                        cursor = run.scalar_end;
                        if normalized_family.is_empty() {
                            projected_blank_source_family_runs += 1;
                        } else {
                            families.insert(normalized_family.clone());
                            let family_fingerprint =
                                format!("{:x}", Sha256::digest(normalized_family.as_bytes()));
                            *projected_source_family_fingerprints
                                .entry(family_fingerprint)
                                .or_default() += 1;
                        }
                        *projected_source_sizes_emu
                            .entry(run.text_size_emu)
                            .or_default() += 1;
                    }
                    complete_coverage &= cursor == text.scalar_end;
                    projected_complete_typography_nodes += usize::from(complete_coverage);
                    projected_single_family_nodes +=
                        usize::from(complete_coverage && families.len() == 1);

                    if let Some(resource_id) = text.backend_font_resource_id.as_ref() {
                        *projected_backend_resources
                            .entry(resource_id.clone())
                            .or_default() += 1;
                    }
                }

                let Some(layout) = text.layout else {
                    layout_none += 1;
                    continue;
                };
                match layout.disposition {
                    RenderTextLayoutDispositionV1::SharedResolved {
                        font_resource_id,
                        font_fingerprint_sha256,
                        ..
                    } => {
                        shared_frames += 1;
                        shared_lines += layout.lines.len();
                        shared_nonempty_lines += layout
                            .lines
                            .iter()
                            .filter(|line| !line.text.trim().is_empty())
                            .count();

                        if projected {
                            *projected_layout_resources
                                .entry(font_resource_id)
                                .or_default() += 1;
                            *projected_layout_fingerprints
                                .entry(font_fingerprint_sha256)
                                .or_default() += 1;
                            *projected_line_counts.entry(layout.lines.len()).or_default() += 1;
                            for line in &layout.lines {
                                *projected_line_heights_emu
                                    .entry(line.line_height_emu)
                                    .or_default() += 1;
                                projected_measured_width_total_emu +=
                                    i128::from(line.measured_width_emu);
                            }
                        }
                    }
                    RenderTextLayoutDispositionV1::BackendFallback { reason } => {
                        *backend_fallbacks.entry(reason.code()).or_default() += 1;
                    }
                }
            }
        }

        let backend_fallbacks_json =
            serde_json::to_string(&backend_fallbacks).expect("serialize fallback census");
        let projected_source_family_fingerprints_json =
            serde_json::to_string(&projected_source_family_fingerprints)
                .expect("serialize projected family fingerprints");
        let projected_source_sizes_json = serde_json::to_string(&projected_source_sizes_emu)
            .expect("serialize projected source sizes");
        let projected_backend_resources_json = serde_json::to_string(&projected_backend_resources)
            .expect("serialize projected backend resources");
        let projected_layout_resources_json = serde_json::to_string(&projected_layout_resources)
            .expect("serialize projected layout resources");
        let projected_layout_fingerprints_json =
            serde_json::to_string(&projected_layout_fingerprints)
                .expect("serialize projected layout fingerprints");
        let projected_line_counts_json =
            serde_json::to_string(&projected_line_counts).expect("serialize projected line counts");
        let projected_line_heights_json = serde_json::to_string(&projected_line_heights_emu)
            .expect("serialize projected line heights");
        let projected_uniform_insets_json = serde_json::to_string(&projected_uniform_insets_emu)
            .expect("serialize projected uniform text insets");
        let projected_zero_typography_size_authority_json =
            serde_json::to_string(&projected_zero_typography_size_authority)
                .expect("serialize projected zero-typography size authority");

        match actual_sha256.as_str() {
            "bf9cda0f632b5820ab9dbdbe1b838b2a988b2f3fdd69253c22b4fc3aef9f11c3" => {
                assert_eq!(
                    projected_text_nodes, 4,
                    "exact Carlton projected carrier count drift"
                );
                assert_eq!(
                    projected_text_bounds_nodes, 4,
                    "exact Carlton must retain source-backed text bounds on every projected carrier"
                );
                assert_eq!(
                    projected_uniform_insets_emu,
                    BTreeMap::from([(36_576_i64, 4_usize)]),
                    "exact Carlton projected carrier inset authority drift"
                );
                let projected_line_total = projected_line_counts
                    .iter()
                    .map(|(line_count, node_count)| line_count * node_count)
                    .sum::<usize>();
                assert_eq!(
                    projected_line_total, 31,
                    "exact Carlton projected source-backed line count drift"
                );
                assert_eq!(
                    projected_zero_typography_size_authority.len(),
                    1,
                    "exact Carlton must retain one projected zero-typography size discriminator"
                );
            }
            "077612c7a228bd20bded939afde129cbdedae9b01b4f138f4619e332e5d7bd2e" => {
                assert_eq!(
                    projected_text_nodes, 0,
                    "Virginia Devinettes must remain a zero-projected control"
                );
                assert_eq!(
                    projected_text_bounds_nodes, 0,
                    "Virginia Devinettes must not acquire projected carrier text bounds"
                );
                assert!(
                    projected_uniform_insets_emu.is_empty(),
                    "Virginia Devinettes must not acquire projected carrier inset authority"
                );
            }
            _ => {}
        }

        println!(
            "CLOUD_READER_TEXT_LAYOUT_FALLBACK_CENSUS source_sha256={} pages={} text_nodes={} shared_frames={} shared_lines={} shared_nonempty_lines={} layout_none={} backend_fallbacks={} projected_text_nodes={} projected_typography_runs={} projected_complete_typography_nodes={} projected_single_family_nodes={} projected_source_family_fingerprints={} projected_blank_source_family_runs={} projected_source_sizes_emu={} projected_backend_resources={} projected_layout_resources={} projected_layout_fingerprints={} projected_line_counts={} projected_line_heights_emu={} projected_text_bounds_nodes={} projected_uniform_insets_emu={} projected_measured_width_total_emu={} projected_zero_typography_size_authority={}",
            actual_sha256,
            bundle.geometry.document.pages.len(),
            text_nodes,
            shared_frames,
            shared_lines,
            shared_nonempty_lines,
            layout_none,
            backend_fallbacks_json,
            projected_text_nodes,
            projected_typography_runs,
            projected_complete_typography_nodes,
            projected_single_family_nodes,
            projected_source_family_fingerprints_json,
            projected_blank_source_family_runs,
            projected_source_sizes_json,
            projected_backend_resources_json,
            projected_layout_resources_json,
            projected_layout_fingerprints_json,
            projected_line_counts_json,
            projected_line_heights_json,
            projected_text_bounds_nodes,
            projected_uniform_insets_json,
            projected_measured_width_total_emu,
            projected_zero_typography_size_authority_json,
        );
    }

    fn test_reader_node(node_id: &str, page_id: &str) -> ReaderNodeV1 {
        ReaderNodeV1 {
            node_id: node_id.to_owned(),
            origin_node_id: None,
            page_id: page_id.to_owned(),
            parent_node_id: None,
            kind: "unknown",
            bounds: ReaderRectV1 {
                x: 0,
                y: 0,
                width: 1,
                height: 1,
            },
            text_bounds: None,
            transform: ReaderTransformV1 {
                a: "1".to_owned(),
                b: "0".to_owned(),
                c: "0".to_owned(),
                d: "1".to_owned(),
                tx: 0,
                ty: 0,
            },
            paint: None,
            resource_id: None,
            image_source_window: None,
            table: None,
            text: None,
            text_layout: None,
        }
    }

    #[test]
    fn direct_scene_text_prefers_render_plan_paint_text() {
        let raw = HashMap::from([("node".to_owned(), "raw target-frame tail".to_owned())]);
        let mut rendered = HashMap::from([("node".to_owned(), "\u{200b}\rX".to_owned())]);

        assert_eq!(
            take_direct_render_text(&mut rendered, &raw, "node").as_deref(),
            Some("\u{200b}\rX"),
            "Cloud Scene must consume render-plan marker suppression and paint cutoff"
        );
        assert!(rendered.is_empty());
        assert_eq!(
            take_direct_render_text(&mut rendered, &raw, "node").as_deref(),
            Some("raw target-frame tail"),
            "raw Viewer fragment is only a fallback when no render-plan text exists"
        );
    }

    #[test]
    fn projected_scene_nodes_insert_after_target_without_reordering_direct_nodes() {
        let direct = vec![
            test_reader_node("direct-a", "page"),
            test_reader_node("direct-b", "page"),
        ];
        let mut first = test_reader_node("projected-1", "page");
        first.origin_node_id = Some("carrier-1".to_owned());
        let mut second = test_reader_node("projected-2", "page");
        second.origin_node_id = Some("carrier-2".to_owned());

        let ordered = insert_projected_nodes_after_targets(
            direct,
            HashMap::from([("direct-a".to_owned(), vec![first, second])]),
        )
        .expect("known target frame must admit projected visual instances");

        assert_eq!(
            ordered
                .iter()
                .map(|node| node.node_id.as_str())
                .collect::<Vec<_>>(),
            vec!["direct-a", "projected-1", "projected-2", "direct-b"]
        );
        assert_eq!(ordered[1].origin_node_id.as_deref(), Some("carrier-1"));
        assert_eq!(ordered[2].origin_node_id.as_deref(), Some("carrier-2"));
    }

    #[test]
    fn projected_scene_nodes_fail_closed_on_missing_target() {
        let error = insert_projected_nodes_after_targets(
            vec![test_reader_node("direct-a", "page")],
            HashMap::from([(
                "missing-target".to_owned(),
                vec![test_reader_node("projected-1", "page")],
            )]),
        )
        .expect_err("dangling projected target must fail closed");
        assert!(error.contains("missing-target"));
    }

    #[test]
    fn non_visible_viewer_paint_is_ignored_but_visible_duplicates_fail_closed() {
        let visible_node_ids = HashSet::from(["visible".to_owned()]);
        let mut paint_by_node = HashMap::new();

        bind_visible_paint(
            &mut paint_by_node,
            &visible_node_ids,
            "hidden-carrier".to_owned(),
            ReaderPaintV1 {
                fill_rgb: Some([1, 2, 3]),
                line: None,
            },
        )
        .expect("hidden paint must not block visible Scene projection");
        assert!(paint_by_node.is_empty());

        bind_visible_paint(
            &mut paint_by_node,
            &visible_node_ids,
            "visible".to_owned(),
            ReaderPaintV1 {
                fill_rgb: Some([4, 5, 6]),
                line: None,
            },
        )
        .expect("visible paint should bind");
        assert_eq!(paint_by_node.len(), 1);

        let duplicate = bind_visible_paint(
            &mut paint_by_node,
            &visible_node_ids,
            "visible".to_owned(),
            ReaderPaintV1 {
                fill_rgb: Some([7, 8, 9]),
                line: None,
            },
        )
        .expect_err("visible duplicate paint must remain fail-closed");
        assert!(duplicate.contains("duplicate paint binding"));
    }

    #[test]
    fn base64_encoding_matches_rfc_4648_vectors() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
    }

    #[test]
    fn viewer_materialized_ole_preview_png_uses_generic_reader_image_resource_path() {
        let mut budget = MAX_INLINE_IMAGE_TOTAL_BYTES;
        let resource = reader_image_resource(
            "resource:legacy-ole-preview".to_owned(),
            "image/png".to_owned(),
            b"bounded-ole-preview-png",
            &mut budget,
        );

        assert_eq!(resource.resource_id, "resource:legacy-ole-preview");
        assert_eq!(resource.mime, "image/png");
        assert_eq!(resource.availability, "inline_data_url");
        assert_eq!(
            resource.inline_data_url.as_deref(),
            Some("data:image/png;base64,Ym91bmRlZC1vbGUtcHJldmlldy1wbmc=")
        );
        assert_eq!(
            budget,
            MAX_INLINE_IMAGE_TOTAL_BYTES - b"bounded-ole-preview-png".len()
        );
    }

    #[test]
    fn inline_image_resource_is_mime_allowlisted_and_budgeted() {
        let mut budget = MAX_INLINE_IMAGE_TOTAL_BYTES;
        let url = inline_image_data_url("image/png", b"png", &mut budget)
            .expect("bounded PNG should inline");
        assert_eq!(url, "data:image/png;base64,cG5n");
        assert_eq!(budget, MAX_INLINE_IMAGE_TOTAL_BYTES - 3);

        assert!(inline_image_data_url("image/svg+xml", b"<svg/>", &mut budget).is_none());

        let mut exhausted = 2;
        assert!(inline_image_data_url("image/png", b"png", &mut exhausted).is_none());
        assert_eq!(exhausted, 2);
    }

    #[test]
    fn descriptor_probe_classifier_matches_inline_admission_law() {
        assert_eq!(
            classify_probe_image_inline_admission("image/png", 3, MAX_INLINE_IMAGE_TOTAL_BYTES),
            ProbeImageInlineAdmission::Inline
        );
        assert_eq!(
            classify_probe_image_inline_admission("image/svg+xml", 3, MAX_INLINE_IMAGE_TOTAL_BYTES),
            ProbeImageInlineAdmission::UnsupportedMime
        );
        assert_eq!(
            classify_probe_image_inline_admission("image/png", 0, MAX_INLINE_IMAGE_TOTAL_BYTES),
            ProbeImageInlineAdmission::EmptyPayload
        );
        assert_eq!(
            classify_probe_image_inline_admission(
                "image/png",
                MAX_INLINE_IMAGE_RESOURCE_BYTES + 1,
                MAX_INLINE_IMAGE_TOTAL_BYTES
            ),
            ProbeImageInlineAdmission::PerResourceLimit
        );
        assert_eq!(
            classify_probe_image_inline_admission("image/png", 3, 2),
            ProbeImageInlineAdmission::AggregateBudgetExhausted
        );

        let mut product_budget = MAX_INLINE_IMAGE_TOTAL_BYTES;
        assert!(inline_image_data_url("image/png", b"png", &mut product_budget).is_some());
        assert_eq!(product_budget, MAX_INLINE_IMAGE_TOTAL_BYTES - b"png".len());

        let unchanged = product_budget;
        assert!(inline_image_data_url("image/svg+xml", b"svg", &mut product_budget).is_none());
        assert_eq!(product_budget, unchanged);

        let mut exhausted = 2;
        assert!(inline_image_data_url("image/png", b"png", &mut exhausted).is_none());
        assert_eq!(exhausted, 2);
    }
}
