use std::collections::{HashMap, HashSet};

use chaptera_viewer_render_plan::{
    ExplicitRenderTextFontResourceV1, RenderTextLayoutDispositionV1,
    build_page_render_plan_with_text_layout_v1,
};
use pub_viewer::ViewerGeometryDocument;
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

pub fn from_viewer_geometry(
    document_id: String,
    source_hash: String,
    revision_id: String,
    geometry: &ViewerGeometryDocument,
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
        let inline_data_url =
            inline_image_data_url(&image.mime, &image.bytes, &mut inline_image_budget);
        let availability = if inline_data_url.is_some() {
            "inline_data_url"
        } else {
            "descriptor_only"
        };
        resources.push(ReaderImageResourceV1 {
            resource_id: resource_id.clone(),
            mime: image.mime.clone(),
            availability,
            inline_data_url,
        });

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
        if !node_ids.contains(&node_id) {
            return Err(format!("paint references unknown node {node_id}"));
        }
        let mapped = ReaderPaintV1 {
            fill_rgb: paint.solid_fill_rgb,
            line: paint.solid_line.as_ref().map(|line| ReaderLineV1 {
                rgb: line.rgb,
                width_emu: line.width_emu,
            }),
        };
        if mapped.fill_rgb.is_none() && mapped.line.is_none() {
            continue;
        }
        if paint_by_node.insert(node_id.clone(), mapped).is_some() {
            return Err(format!("duplicate paint binding for node {node_id}"));
        }
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
    if !nodes.is_empty() {
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
        stacking_fidelity: "unknown",
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
    use std::{env, fs};

    use pub_viewer::{open_pub_geometry, viewer_geometry_environment_v0_1};
    use sha2::{Digest, Sha256};

    use super::{
        MAX_INLINE_IMAGE_TOTAL_BYTES, base64_encode, from_viewer_geometry, inline_image_data_url,
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

        let geometry = open_pub_geometry(&bytes, viewer_geometry_environment_v0_1())
            .expect("shared Viewer geometry must open the probe source");
        match from_viewer_geometry(
            "probe:document".to_owned(),
            actual_sha256,
            "probe:source".to_owned(),
            &geometry,
        ) {
            Ok(scene) => println!(
                "CLOUD_READER_SCENE_PROJECTION_PROBE ok state={} pages={} nodes={} reasons={:?}",
                scene.fidelity.state,
                scene.pages.len(),
                scene.nodes.len(),
                scene.fidelity.reasons
            ),
            Err(error) => println!("CLOUD_READER_SCENE_PROJECTION_PROBE projection_error={error}"),
        }
    }

    #[test]
    fn base64_encoding_matches_rfc_4648_vectors() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
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
