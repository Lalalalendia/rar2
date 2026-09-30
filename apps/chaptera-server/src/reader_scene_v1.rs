use std::collections::{HashMap, HashSet};

use chaptera_viewer_render_plan::{
    ExplicitRenderTextFontResourceV1, RenderTextLayoutDispositionV1,
    build_page_render_plan_with_text_layout_v1,
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
    pub page_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_node_id: Option<String>,
    pub kind: &'static str,
    pub bounds: ReaderRectV1,
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
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bounds: Option<ReaderRectV1>,
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
    let mut text_layout_by_node = HashMap::new();
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
        for node in plan.nodes {
            let Some(text) = node.text else {
                continue;
            };
            let Some(layout) = text.layout else {
                text_layout_partial = true;
                continue;
            };
            let RenderTextLayoutDispositionV1::SharedResolved {
                font_resource_id,
                font_fingerprint_sha256,
                font_size_emu,
                line_height_emu,
            } = layout.disposition
            else {
                text_layout_partial = true;
                continue;
            };
            if layout.lines.iter().any(|line| !line.spans.is_empty()) {
                // Mixed-size layout is deterministic and shared, but paragraph-level
                // vertical metrics are not yet Publisher-exact. Keep the document Partial.
                text_layout_partial = true;
            }
            let node_id = serialized_string(&node.node_id, "text layout node id")?;
            let mapped = ReaderTextLayoutV1 {
                disposition: "shared_resolved",
                font_resource_id,
                font_fingerprint_sha256,
                font_size_emu,
                line_height_emu,
                lines: layout
                    .lines
                    .into_iter()
                    .map(|line| ReaderTextLineV1 {
                        line_index: line.line_index,
                        scalar_start: line.scalar_start,
                        scalar_end: line.scalar_end,
                        consumed_scalar_end: line.consumed_scalar_end,
                        text: line.text,
                        measured_width_emu: line.measured_width_emu,
                        line_height_emu: line.line_height_emu,
                        spans: line
                            .spans
                            .into_iter()
                            .map(|span| ReaderTextSpanV1 {
                                scalar_start: span.scalar_start,
                                scalar_end: span.scalar_end,
                                text: span.text,
                                x_offset_emu: span.x_offset_emu,
                                measured_width_emu: span.measured_width_emu,
                                font_size_emu: span.font_size_emu,
                            })
                            .collect(),
                    })
                    .collect(),
            };
            if text_layout_by_node
                .insert(node_id.clone(), mapped)
                .is_some()
            {
                return Err(format!("duplicate text layout binding for node {node_id}"));
            }
        }
    }
    let text_layout_count = text_layout_by_node.len();
    if text_by_node.len() > text_layout_count {
        text_layout_partial = true;
    }

    let mut nodes = Vec::with_capacity(raw_nodes.len());
    for (node_id, parent_id, bounds, transform) in raw_nodes {
        let page_id = page_cache
            .get(&node_id)
            .cloned()
            .ok_or_else(|| format!("node {node_id} has no resolved page"))?;
        let parent_node_id = node_ids.contains(&parent_id).then_some(parent_id);
        nodes.push(ReaderNodeV1 {
            kind: kind_by_node
                .get(&node_id)
                .copied()
                .ok_or_else(|| format!("node kind missing for {node_id}"))?,
            paint: paint_by_node.remove(&node_id),
            resource_id: resource_by_node.remove(&node_id),
            image_source_window: source_window_by_node.remove(&node_id),
            table: table_by_node.remove(&node_id),
            text: text_by_node.get(&node_id).cloned(),
            text_layout: text_layout_by_node.remove(&node_id),
            node_id,
            page_id,
            parent_node_id,
            bounds,
            transform,
        });
    }

    let stacking_known =
        apply_source_page_paint_order(&mut nodes, &pages, source_page_paint_orders)?;

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
    if kind_by_node.values().any(|kind| *kind == "unknown") {
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

    use chaptera_viewer_render_plan::{
        RenderTextLayoutDispositionV1, build_page_render_plan_with_text_layout_v1,
    };
    use pub_viewer::{open_pub_bundle, viewer_geometry_environment_v0_1};
    use sha2::{Digest, Sha256};

    use super::{
        MAX_INLINE_IMAGE_TOTAL_BYTES, ReaderPaintV1, base64_encode, bind_visible_paint,
        from_viewer_geometry, inline_image_data_url, reader_image_resource,
        shared_text_font_resource,
    };

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
        match from_viewer_geometry(
            "probe:document".to_owned(),
            actual_sha256,
            "probe:source".to_owned(),
            &bundle.geometry,
            &bundle.source_page_paint_orders,
        ) {
            Ok(scene) => println!(
                "CLOUD_READER_SCENE_PROJECTION_PROBE ok state={} stacking={} pages={} nodes={} reasons={:?}",
                scene.fidelity.state,
                scene.stacking_fidelity,
                scene.pages.len(),
                scene.nodes.len(),
                scene.fidelity.reasons
            ),
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

        let bundle = open_pub_bundle(&bytes, viewer_geometry_environment_v0_1())
            .expect("shared Viewer bundle must open the probe source");
        let font = shared_text_font_resource();

        let mut text_nodes = 0_usize;
        let mut shared_frames = 0_usize;
        let mut shared_lines = 0_usize;
        let mut shared_nonempty_lines = 0_usize;
        let mut layout_none = 0_usize;
        let mut backend_fallbacks = BTreeMap::<&'static str, usize>::new();
        let mut story_extent_profiles = BTreeMap::<String, usize>::new();
        let mut typography_gap_profiles = BTreeMap::<String, usize>::new();

        for page_index in 0..bundle.geometry.document.pages.len() {
            let plan =
                build_page_render_plan_with_text_layout_v1(&bundle.geometry, page_index, &font)
                    .expect("exact reference render plan must build");

            for node in plan.nodes {
                let Some(text) = node.text else {
                    continue;
                };
                text_nodes += 1;
                let Some(layout) = text.layout else {
                    layout_none += 1;
                    continue;
                };
                match layout.disposition {
                    RenderTextLayoutDispositionV1::SharedResolved { .. } => {
                        shared_frames += 1;
                        shared_lines += layout.lines.len();
                        shared_nonempty_lines += layout
                            .lines
                            .iter()
                            .filter(|line| !line.text.trim().is_empty())
                            .count();
                    }
                    RenderTextLayoutDispositionV1::BackendFallback { reason } => {
                        *backend_fallbacks.entry(reason.code()).or_default() += 1;
                        if reason.code() == "story_extent_mismatch" {
                            let story = bundle
                                .geometry
                                .document
                                .stories
                                .iter()
                                .find(|story| story.id == text.story_id);
                            let profile = if let Some(story) = story {
                                let story_len = i64::try_from(story.text.chars().count())
                                    .expect("story scalar count fits i64");
                                let fragment_text_len = i64::try_from(text.text.chars().count())
                                    .expect("fragment scalar count fits i64");
                                format!(
                                    "page{}:start={}:end_delta={}:fragment_len_delta={}:text_equal={}",
                                    page_index + 1,
                                    text.scalar_start,
                                    i64::from(text.scalar_end) - story_len,
                                    fragment_text_len - story_len,
                                    text.text == story.text,
                                )
                            } else {
                                format!("page{}:story_missing", page_index + 1)
                            };
                            *story_extent_profiles.entry(profile).or_default() += 1;
                        } else if reason.code() == "typography_coverage_gap" {
                            let mut cursor = text.scalar_start;
                            let mut discontinuities = 0_usize;
                            let mut zero_sizes = 0_usize;
                            for run in &text.typography {
                                if run.scalar_start != cursor || run.scalar_end <= run.scalar_start {
                                    discontinuities += 1;
                                }
                                if run.text_size_emu == 0 {
                                    zero_sizes += 1;
                                }
                                cursor = run.scalar_end;
                            }
                            let profile = format!(
                                "page{}:fragment={}..{}:runs={}:last_end_delta={}:discontinuities={}:zero_sizes={}",
                                page_index + 1,
                                text.scalar_start,
                                text.scalar_end,
                                text.typography.len(),
                                i64::from(cursor) - i64::from(text.scalar_end),
                                discontinuities,
                                zero_sizes,
                            );
                            *typography_gap_profiles.entry(profile).or_default() += 1;
                        }
                    }
                }
            }
        }

        let backend_fallbacks_json =
            serde_json::to_string(&backend_fallbacks).expect("serialize fallback census");
        let story_extent_profiles_json =
            serde_json::to_string(&story_extent_profiles).expect("serialize story extent profiles");
        let typography_gap_profiles_json = serde_json::to_string(&typography_gap_profiles)
            .expect("serialize typography gap profiles");
        println!(
            "CLOUD_READER_TEXT_LAYOUT_FALLBACK_CENSUS source_sha256={} pages={} text_nodes={} shared_frames={} shared_lines={} shared_nonempty_lines={} layout_none={} backend_fallbacks={} story_extent_profiles={} typography_gap_profiles={}",
            actual_sha256,
            bundle.geometry.document.pages.len(),
            text_nodes,
            shared_frames,
            shared_lines,
            shared_nonempty_lines,
            layout_none,
            backend_fallbacks_json,
            story_extent_profiles_json,
            typography_gap_profiles_json,
        );
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
}
