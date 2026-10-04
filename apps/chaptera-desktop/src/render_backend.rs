//! Desktop egui execution backend for source-neutral Chaptera render plans.
//!
//! This module owns only how already-resolved document paint facts are executed
//! by egui. Product interaction state (selection, caret, drag/resize admission,
//! EditorSession state, commands and product chrome) stays in the shell.

use chaptera_viewer_render_plan::{
    NodeRenderPlanV1, RenderImageSourceWindowV1, RenderTextFragmentV1,
    RenderTextLayoutDispositionV1, uniform_text_color_rgb_v1,
};
use eframe::egui;

#[derive(Debug, Clone, PartialEq, Default, serde::Serialize)]
pub struct TextPaintMetrics {
    pub layout_section_count: usize,
    pub source_typography_sections: usize,
    pub fallback_sections: usize,
    pub executed_font_sizes_px: Vec<f32>,
    pub wrap_width_px: f32,
    pub galley_width_px: f32,
    pub galley_height_px: f32,
    pub clip_width_px: f32,
    pub clip_height_px: f32,
    pub overflow_delta_px: f32,
    pub line_count: usize,
    pub shared_resolved_layout: bool,
    pub shared_resolved_line_count: usize,
    pub backend_fallback_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct NodePaintOutcome {
    pub text_clipped: bool,
    pub text_metrics: Option<TextPaintMetrics>,
}

pub fn paint_page_surface(painter: &egui::Painter, page_rect: egui::Rect) {
    painter.rect_filled(page_rect, 0, egui::Color32::WHITE);
    painter.rect_stroke(
        page_rect,
        0,
        egui::Stroke::new(1.0_f32, egui::Color32::DARK_GRAY),
        egui::StrokeKind::Inside,
    );
}

pub fn physical_rect_to_egui(
    page_rect: egui::Rect,
    scene_scale: f32,
    x_emu: i64,
    y_emu: i64,
    width_emu: i64,
    height_emu: i64,
) -> Option<egui::Rect> {
    if !scene_scale.is_finite() || scene_scale <= 0.0 || width_emu <= 0 || height_emu <= 0 {
        return None;
    }

    let min = egui::pos2(
        page_rect.left() + x_emu as f32 * scene_scale,
        page_rect.top() + y_emu as f32 * scene_scale,
    );
    let size = egui::vec2(
        width_emu as f32 * scene_scale,
        height_emu as f32 * scene_scale,
    );
    Some(egui::Rect::from_min_size(min, size))
}

fn image_paint_geometry(
    node_rect: egui::Rect,
    source_window: Option<&RenderImageSourceWindowV1>,
) -> Option<(egui::Rect, egui::Rect)> {
    let destination = node_rect.shrink(1.0);
    if destination.width() <= 0.0 || destination.height() <= 0.0 {
        return None;
    }

    let Some(window) = source_window else {
        return Some((
            destination,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        ));
    };

    const Q16_ONE: f64 = 65_536.0;
    let left = window.left_q16 as f64 / Q16_ONE;
    let top = window.top_q16 as f64 / Q16_ONE;
    let right = window.right_q16 as f64 / Q16_ONE;
    let bottom = window.bottom_q16 as f64 / Q16_ONE;
    if !left.is_finite()
        || !top.is_finite()
        || !right.is_finite()
        || !bottom.is_finite()
        || right <= left
        || bottom <= top
    {
        return None;
    }

    let source_left = left.max(0.0);
    let source_top = top.max(0.0);
    let source_right = right.min(1.0);
    let source_bottom = bottom.min(1.0);
    if source_right <= source_left || source_bottom <= source_top {
        return None;
    }

    let window_width = right - left;
    let window_height = bottom - top;
    let dest_left = (source_left - left) / window_width;
    let dest_top = (source_top - top) / window_height;
    let dest_right = (source_right - left) / window_width;
    let dest_bottom = (source_bottom - top) / window_height;

    let image_rect = egui::Rect::from_min_max(
        egui::pos2(
            destination.left() + (dest_left as f32 * destination.width()),
            destination.top() + (dest_top as f32 * destination.height()),
        ),
        egui::pos2(
            destination.left() + (dest_right as f32 * destination.width()),
            destination.top() + (dest_bottom as f32 * destination.height()),
        ),
    );
    let uv_rect = egui::Rect::from_min_max(
        egui::pos2(source_left as f32, source_top as f32),
        egui::pos2(source_right as f32, source_bottom as f32),
    );
    Some((image_rect, uv_rect))
}

/// Paints document-owned layers that occur before shell/debug overlays.
///
/// The shell resolves temporary authoring preview state (for example a
/// replacement texture) before calling this function. No Editor state crosses
/// this boundary.
pub fn paint_document_node_base(
    painter: &egui::Painter,
    node: &NodeRenderPlanV1,
    node_rect: egui::Rect,
    texture: Option<egui::TextureId>,
) {
    if let Some(rgb) = node.solid_fill_rgb {
        painter.rect_filled(
            node_rect,
            0,
            egui::Color32::from_rgb(rgb[0], rgb[1], rgb[2]),
        );
    }

    if let Some(texture) = texture {
        let source_window = node
            .image
            .as_ref()
            .and_then(|image| image.source_window.as_ref());
        if let Some((image_rect, uv_rect)) = image_paint_geometry(node_rect, source_window) {
            painter.image(texture, image_rect, uv_rect, egui::Color32::WHITE);
        }
    }
}

fn text_clip_rect_for_node(
    node: &NodeRenderPlanV1,
    node_rect: egui::Rect,
    scene_scale: f32,
) -> egui::Rect {
    if let Some(bounds) = node.text_bounds {
        let relative_x = (bounds.x.get() - node.bounds.x.get()) as f32 * scene_scale;
        let relative_y = (bounds.y.get() - node.bounds.y.get()) as f32 * scene_scale;
        return egui::Rect::from_min_size(
            egui::pos2(node_rect.left() + relative_x, node_rect.top() + relative_y),
            egui::vec2(
                bounds.width.get() as f32 * scene_scale,
                bounds.height.get() as f32 * scene_scale,
            ),
        );
    }
    node_rect.shrink(2.0)
}

/// Paints document-owned layers that occur after shell/debug overlays.
///
/// Text overflow is returned as a fact. The product shell still owns the
/// user-facing red warning/affordance because it is preview UI rather than
/// document paint.
pub fn paint_document_node_foreground(
    painter: &egui::Painter,
    node: &NodeRenderPlanV1,
    node_rect: egui::Rect,
    scene_scale: f32,
) -> NodePaintOutcome {
    if node.decorative_border.is_none()
        && let Some(line) = node.solid_line.as_ref()
    {
        let line_width_px = line.width_emu as f32 * scene_scale;
        if line_width_px > 0.0_f32 {
            painter.rect_stroke(
                node_rect,
                0,
                egui::Stroke::new(
                    line_width_px,
                    egui::Color32::from_rgb(line.rgb[0], line.rgb[1], line.rgb[2]),
                ),
                egui::StrokeKind::Inside,
            );
        }
    }

    paint_bounded_table_text(painter, node, node_rect, scene_scale);

    let Some(fragment) = node
        .text
        .as_ref()
        .filter(|fragment| !fragment.text.is_empty())
    else {
        return NodePaintOutcome::default();
    };

    let text_clip_rect = text_clip_rect_for_node(node, node_rect, scene_scale);
    if !text_clip_rect.is_positive() {
        return NodePaintOutcome::default();
    }
    let text_painter = painter.with_clip_rect(text_clip_rect);

    if let Some(layout) = fragment.layout.as_ref()
        && let RenderTextLayoutDispositionV1::SharedResolved {
            font_resource_id,
            font_size_emu,
            line_height_emu,
            ..
        } = &layout.disposition
        && let Some(metrics) = paint_shared_resolved_text(
            &text_painter,
            fragment,
            layout.lines.as_slice(),
            SharedResolvedPaintParams {
                font_resource_id,
                font_size_emu: *font_size_emu,
                line_height_emu: *line_height_emu,
                scene_scale,
                clip_rect: text_clip_rect,
            },
        )
    {
        let text_clipped =
            preview_text_height_is_clipped(metrics.galley_height_px, text_clip_rect.height());
        return NodePaintOutcome {
            text_clipped,
            text_metrics: Some(metrics),
        };
    }

    let backend_fallback_reason = Some(match fragment.layout.as_ref() {
        Some(layout) => match &layout.disposition {
            RenderTextLayoutDispositionV1::BackendFallback { reason } => reason.code().to_owned(),
            RenderTextLayoutDispositionV1::SharedResolved { .. } => {
                "shared_layout_backend_execution_invalid".to_owned()
            }
        },
        None => "shared_layout_not_requested".to_owned(),
    });

    let text_color = uniform_text_color_rgb_v1(fragment)
        .map(|rgb| egui::Color32::from_rgb(rgb[0], rgb[1], rgb[2]))
        .unwrap_or(egui::Color32::BLACK);
    let (layout_job, usage) = layout_document_text(
        fragment,
        scene_scale,
        text_clip_rect.width().max(1.0_f32),
        fragment.backend_font_resource_id.as_deref(),
        text_color,
    );
    let executed_font_sizes_px = layout_job
        .sections
        .iter()
        .map(|section| section.format.font_id.size)
        .collect::<Vec<_>>();
    let layout_section_count = layout_job.sections.len();
    let galley = text_painter.layout_job(layout_job);
    let text_clipped = preview_text_height_is_clipped(galley.size().y, text_clip_rect.height());
    let text_metrics = TextPaintMetrics {
        layout_section_count,
        source_typography_sections: usage.source_typography_sections,
        fallback_sections: usage.fallback_sections,
        executed_font_sizes_px,
        wrap_width_px: text_clip_rect.width().max(1.0),
        galley_width_px: galley.size().x,
        galley_height_px: galley.size().y,
        clip_width_px: text_clip_rect.width(),
        clip_height_px: text_clip_rect.height(),
        overflow_delta_px: (galley.size().y - text_clip_rect.height()).max(0.0),
        line_count: galley.rows.len(),
        shared_resolved_layout: false,
        shared_resolved_line_count: 0,
        backend_fallback_reason,
    };
    text_painter.galley(text_clip_rect.min, galley, text_color);

    NodePaintOutcome {
        text_clipped,
        text_metrics: Some(text_metrics),
    }
}

fn paint_bounded_table_text(
    painter: &egui::Painter,
    node: &NodeRenderPlanV1,
    node_rect: egui::Rect,
    scene_scale: f32,
) {
    let Some(table) = node.table.as_ref() else {
        return;
    };
    if !scene_scale.is_finite() || scene_scale <= 0.0 {
        return;
    }

    let font_id = crate::fallback_font::font_id_for_scene_scale(scene_scale);
    for cell in &table.cells {
        let Some(bounds) = cell.bounds else {
            continue;
        };
        let relative_x = bounds.x.get() - node.bounds.x.get();
        let relative_y = bounds.y.get() - node.bounds.y.get();
        let cell_rect = egui::Rect::from_min_size(
            egui::pos2(
                node_rect.left() + relative_x as f32 * scene_scale,
                node_rect.top() + relative_y as f32 * scene_scale,
            ),
            egui::vec2(
                bounds.width.get() as f32 * scene_scale,
                bounds.height.get() as f32 * scene_scale,
            ),
        );
        if !cell_rect.is_positive() {
            continue;
        }

        if cell.fill_visible == Some(true)
            && let Some(rgb) = cell.fill_rgb
        {
            painter.rect_filled(
                cell_rect,
                0.0,
                egui::Color32::from_rgb(rgb[0], rgb[1], rgb[2]),
            );
        }

        let clip_rect = cell_rect.shrink(2.0);
        if !clip_rect.is_positive() || cell.text.is_empty() {
            continue;
        }

        let text = cell.text.replace('\r', "\n");
        let job = egui::text::LayoutJob::simple(
            text,
            font_id.clone(),
            egui::Color32::BLACK,
            clip_rect.width().max(1.0),
        );
        let cell_painter = painter.with_clip_rect(clip_rect);
        let galley = cell_painter.layout_job(job);
        cell_painter.galley(clip_rect.min, galley, egui::Color32::BLACK);
    }
}

fn shared_resolved_line_job(
    text: &str,
    font_id: egui::FontId,
    color: egui::Color32,
) -> egui::text::LayoutJob {
    egui::text::LayoutJob::simple(text.to_owned(), font_id, color, f32::INFINITY)
}

struct SharedResolvedPaintParams<'a> {
    font_resource_id: &'a str,
    font_size_emu: i64,
    line_height_emu: i64,
    scene_scale: f32,
    clip_rect: egui::Rect,
}

fn shared_resolved_block_height_px(
    font_size_px: f32,
    line_height_px: f32,
    line_count: usize,
) -> f32 {
    if line_count == 0 {
        return 0.0;
    }
    font_size_px + line_count.saturating_sub(1) as f32 * line_height_px
}

fn paint_shared_resolved_text(
    painter: &egui::Painter,
    fragment: &RenderTextFragmentV1,
    lines: &[chaptera_viewer_render_plan::RenderResolvedTextLineV1],
    params: SharedResolvedPaintParams<'_>,
) -> Option<TextPaintMetrics> {
    let SharedResolvedPaintParams {
        font_resource_id,
        font_size_emu,
        line_height_emu,
        scene_scale,
        clip_rect,
    } = params;
    if font_size_emu <= 0 || line_height_emu <= 0 || !scene_scale.is_finite() || scene_scale <= 0.0
    {
        return None;
    }

    let font_size_px = (font_size_emu as f32 * scene_scale).clamp(4.0, 512.0);
    let line_height_px = line_height_emu as f32 * scene_scale;
    if !font_size_px.is_finite() || !line_height_px.is_finite() || line_height_px <= 0.0 {
        return None;
    }

    if font_resource_id.is_empty() {
        return None;
    }
    let font_id = egui::FontId::new(
        font_size_px,
        egui::FontFamily::Name(font_resource_id.into()),
    );
    let mut max_width_px = 0.0_f32;
    let text_color = uniform_text_color_rgb_v1(fragment)
        .map(|rgb| egui::Color32::from_rgb(rgb[0], rgb[1], rgb[2]))
        .unwrap_or(egui::Color32::BLACK);

    for (expected_index, line) in lines.iter().enumerate() {
        if usize::try_from(line.line_index).ok() != Some(expected_index)
            || line.line_height_emu != line_height_emu
        {
            return None;
        }

        let job = shared_resolved_line_job(&line.text, font_id.clone(), text_color);
        let galley = painter.layout_job(job);
        max_width_px = max_width_px.max(galley.size().x);
        let y = clip_rect.top() + line.line_index as f32 * line_height_px;
        let x = clip_rect.left() + line.x_offset_emu as f32 * scene_scale;
        painter.galley(egui::pos2(x, y), galley, text_color);
    }

    let resolved_height_px =
        shared_resolved_block_height_px(font_size_px, line_height_px, lines.len());
    let source_typography_sections = fragment.typography.len();
    let fallback_sections = usize::from(fragment.typography.is_empty());

    Some(TextPaintMetrics {
        layout_section_count: lines.len(),
        source_typography_sections,
        fallback_sections,
        executed_font_sizes_px: if lines.is_empty() {
            Vec::new()
        } else {
            vec![font_size_px]
        },
        // Shared lines are already broken upstream; zero deliberately records
        // that the backend did not choose a wrapping width for this path.
        wrap_width_px: 0.0,
        galley_width_px: max_width_px,
        galley_height_px: resolved_height_px,
        clip_width_px: clip_rect.width(),
        clip_height_px: clip_rect.height(),
        overflow_delta_px: (resolved_height_px - clip_rect.height()).max(0.0),
        line_count: lines.len(),
        shared_resolved_layout: true,
        shared_resolved_line_count: lines.len(),
        backend_fallback_reason: None,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct TextLayoutUsage {
    source_typography_sections: usize,
    fallback_sections: usize,
}

fn layout_document_text(
    fragment: &RenderTextFragmentV1,
    scene_scale: f32,
    wrap_width_px: f32,
    backend_font_resource_id: Option<&str>,
    text_color: egui::Color32,
) -> (egui::text::LayoutJob, TextLayoutUsage) {
    let fallback = backend_font_resource_id
        .filter(|resource_id| !resource_id.is_empty())
        .map(|resource_id| {
            egui::FontId::new(
                crate::fallback_font::screen_font_size(scene_scale),
                egui::FontFamily::Name(resource_id.into()),
            )
        })
        .unwrap_or_else(|| crate::fallback_font::font_id_for_scene_scale(scene_scale));
    let fallback_job = || {
        (
            egui::text::LayoutJob::simple(
                fragment.text.clone(),
                fallback.clone(),
                text_color,
                wrap_width_px.max(1.0),
            ),
            TextLayoutUsage {
                source_typography_sections: 0,
                fallback_sections: usize::from(!fragment.text.is_empty()),
            },
        )
    };

    let Some(fragment_scalar_len) = fragment.scalar_end.checked_sub(fragment.scalar_start) else {
        return fallback_job();
    };
    if usize::try_from(fragment_scalar_len).ok() != Some(fragment.text.chars().count()) {
        return fallback_job();
    }

    let mut source_sections = fragment
        .typography
        .iter()
        .filter_map(|run| {
            let start = run.scalar_start.max(fragment.scalar_start);
            let end = run.scalar_end.min(fragment.scalar_end);
            if start >= end {
                return None;
            }
            let size = source_text_size_px(run.text_size_emu, scene_scale)?;
            Some((
                start - fragment.scalar_start,
                end - fragment.scalar_start,
                size,
            ))
        })
        .collect::<Vec<_>>();

    if source_sections.is_empty() {
        return fallback_job();
    }
    source_sections.sort_by_key(|(start, end, size)| (*start, *end, size.to_bits()));
    if source_sections.windows(2).any(|pair| pair[1].0 < pair[0].1) {
        return fallback_job();
    }

    let source_section_count = source_sections.len();
    let mut job = egui::text::LayoutJob::default();
    job.wrap.max_width = wrap_width_px.max(1.0);
    let mut cursor = 0_u32;

    for (start, end, size) in source_sections {
        if cursor < start {
            let Some(text) = scalar_slice(&fragment.text, cursor, start) else {
                return fallback_job();
            };
            append_text_section(&mut job, text, fallback.clone(), text_color);
        }
        let Some(text) = scalar_slice(&fragment.text, start, end) else {
            return fallback_job();
        };
        let family = backend_font_resource_id
            .filter(|resource_id| !resource_id.is_empty())
            .map(|resource_id| egui::FontFamily::Name(resource_id.into()))
            .unwrap_or_else(crate::fallback_font::family);
        append_text_section(&mut job, text, egui::FontId::new(size, family), text_color);
        cursor = end;
    }

    if cursor < fragment_scalar_len {
        let Some(text) = scalar_slice(&fragment.text, cursor, fragment_scalar_len) else {
            return fallback_job();
        };
        append_text_section(&mut job, text, fallback.clone(), text_color);
    }

    if job.text != fragment.text {
        return fallback_job();
    }
    let source_typography_sections = source_section_count;
    let fallback_sections = job
        .sections
        .len()
        .saturating_sub(source_typography_sections);
    (
        job,
        TextLayoutUsage {
            source_typography_sections,
            fallback_sections,
        },
    )
}

fn append_text_section(
    job: &mut egui::text::LayoutJob,
    text: &str,
    font_id: egui::FontId,
    color: egui::Color32,
) {
    job.append(text, 0.0, egui::TextFormat::simple(font_id, color));
}

fn source_text_size_px(text_size_emu: u32, scene_scale: f32) -> Option<f32> {
    const MIN_PX: f32 = 4.0;
    const MAX_PX: f32 = 512.0;
    if text_size_emu == 0 || !scene_scale.is_finite() || scene_scale <= 0.0 {
        return None;
    }
    let px = text_size_emu as f32 * scene_scale;
    px.is_finite().then(|| px.clamp(MIN_PX, MAX_PX))
}

fn scalar_slice(text: &str, start: u32, end: u32) -> Option<&str> {
    if start > end {
        return None;
    }
    let start = scalar_boundary_to_byte(text, start)?;
    let end = scalar_boundary_to_byte(text, end)?;
    text.get(start..end)
}

fn scalar_boundary_to_byte(text: &str, target: u32) -> Option<usize> {
    if target == 0 {
        return Some(0);
    }
    let mut scalar = 0_u32;
    for (byte_offset, _) in text.char_indices() {
        if scalar == target {
            return Some(byte_offset);
        }
        scalar = scalar.checked_add(1)?;
    }
    (scalar == target).then_some(text.len())
}

fn preview_text_height_is_clipped(galley_height: f32, clip_height: f32) -> bool {
    const EPSILON_PX: f32 = 0.5;
    galley_height > clip_height + EPSILON_PX
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn physical_rect_conversion_rejects_invalid_paint_geometry() {
        let page = egui::Rect::from_min_size(egui::pos2(100.0, 50.0), egui::vec2(400.0, 300.0));

        assert!(physical_rect_to_egui(page, 0.0, 1, 1, 10, 10).is_none());
        assert!(physical_rect_to_egui(page, 1.0, 1, 1, 0, 10).is_none());
        assert!(physical_rect_to_egui(page, 1.0, 1, 1, 10, -1).is_none());
    }

    #[test]
    fn physical_rect_conversion_uses_page_origin_and_scene_scale() {
        let page = egui::Rect::from_min_size(egui::pos2(100.0, 50.0), egui::vec2(400.0, 300.0));
        let rect = physical_rect_to_egui(page, 0.5, 20, 30, 100, 80).expect("valid physical rect");

        assert_eq!(rect.min, egui::pos2(110.0, 65.0));
        assert_eq!(rect.size(), egui::vec2(50.0, 40.0));
    }

    fn assert_close(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() < 0.001,
            "expected {expected}, got {actual}"
        );
    }

    #[test]
    fn image_paint_geometry_keeps_full_texture_when_no_source_window_exists() {
        let node = egui::Rect::from_min_max(egui::pos2(10.0, 20.0), egui::pos2(210.0, 120.0));
        let (destination, uv) = image_paint_geometry(node, None).expect("full image");
        assert_eq!(destination, node.shrink(1.0));
        assert_eq!(
            uv,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0))
        );
    }

    #[test]
    fn image_paint_geometry_executes_positive_crop_as_uv_viewport() {
        let node = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(202.0, 102.0));
        let window = RenderImageSourceWindowV1 {
            left_q16: 0,
            top_q16: 16_384,
            right_q16: 65_536,
            bottom_q16: 49_152,
        };
        let (destination, uv) = image_paint_geometry(node, Some(&window)).expect("positive crop");
        assert_eq!(destination, node.shrink(1.0));
        assert_close(uv.left(), 0.0);
        assert_close(uv.top(), 0.25);
        assert_close(uv.right(), 1.0);
        assert_close(uv.bottom(), 0.75);
    }

    #[test]
    fn image_paint_geometry_executes_negative_fit_crop_as_letterboxed_content() {
        let node = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(202.0, 102.0));
        let window = RenderImageSourceWindowV1 {
            left_q16: -65_536,
            top_q16: 0,
            right_q16: 131_072,
            bottom_q16: 65_536,
        };
        let (destination, uv) = image_paint_geometry(node, Some(&window)).expect("negative crop");
        let frame = node.shrink(1.0);
        assert_close(destination.left(), frame.left() + frame.width() / 3.0);
        assert_close(destination.right(), frame.right() - frame.width() / 3.0);
        assert_close(destination.top(), frame.top());
        assert_close(destination.bottom(), frame.bottom());
        assert_eq!(
            uv,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0))
        );
    }

    #[test]
    fn preview_text_clipping_uses_visible_galley_height_only() {
        assert!(!preview_text_height_is_clipped(100.0, 100.0));
        assert!(!preview_text_height_is_clipped(100.4, 100.0));
        assert!(preview_text_height_is_clipped(100.6, 100.0));
    }

    #[test]
    fn shared_resolved_block_height_separates_first_line_from_baseline_advance() {
        assert_eq!(shared_resolved_block_height_px(14.0, 17.5, 0), 0.0);
        assert_eq!(shared_resolved_block_height_px(14.0, 17.5, 1), 14.0);
        assert_eq!(shared_resolved_block_height_px(14.0, 17.5, 2), 31.5);
    }

    #[test]
    fn shared_resolved_line_job_disables_backend_wrapping() {
        let job = shared_resolved_line_job(
            "one already resolved line",
            egui::FontId::new(12.0, crate::fallback_font::family()),
            egui::Color32::BLACK,
        );
        assert!(job.wrap.max_width.is_infinite());
        assert_eq!(job.text, "one already resolved line");
    }

    #[test]
    fn typography_sections_change_size_but_keep_pinned_fallback_family() {
        let fragment: RenderTextFragmentV1 = serde_json::from_value(serde_json::json!({
            "story_id": "00000000-0000-0000-0000-000000000001",
            "scalar_start": 0,
            "scalar_end": 6,
            "text": "ABCDEF",
            "line_count": 1,
            "typography": [
                {
                    "scalar_start": 0,
                    "scalar_end": 2,
                    "source_font_name": "Rockwell Condensed",
                    "text_size_emu": 304800,
                    "font_inherited": false,
                    "size_inherited": false
                },
                {
                    "scalar_start": 4,
                    "scalar_end": 6,
                    "source_font_name": "Arial",
                    "text_size_emu": 177800,
                    "font_inherited": true,
                    "size_inherited": true
                }
            ]
        }))
        .expect("render text fragment");

        let scene_scale = 1.0 / 12_700.0;
        let (job, usage) =
            layout_document_text(&fragment, scene_scale, 400.0, None, egui::Color32::BLACK);
        assert_eq!(job.text, "ABCDEF");
        assert_eq!(job.sections.len(), 3);
        let sizes = job
            .sections
            .iter()
            .map(|section| section.format.font_id.size)
            .collect::<Vec<_>>();
        assert_eq!(
            sizes,
            vec![
                24.0,
                crate::fallback_font::screen_font_size(scene_scale),
                14.0
            ]
        );
        assert_eq!(usage.source_typography_sections, 2);
        assert_eq!(usage.fallback_sections, 1);
        assert!(
            job.sections
                .iter()
                .all(|section| section.format.font_id.family == crate::fallback_font::family())
        );
    }

    #[test]
    fn backend_fallback_uses_explicit_source_font_resource_when_available() {
        let fragment: RenderTextFragmentV1 = serde_json::from_value(serde_json::json!({
            "story_id": "00000000-0000-0000-0000-000000000001",
            "scalar_start": 0,
            "scalar_end": 4,
            "text": "ABCD",
            "line_count": 1,
            "typography": [
                {"scalar_start":0,"scalar_end":4,"source_font_name":"Arial","text_size_emu":152400,"font_inherited":false,"size_inherited":false}
            ],
            "backend_font_resource_id": "chaptera.desktop.environment-font.test.face0"
        }))
        .expect("render text fragment");

        let (job, usage) = layout_document_text(
            &fragment,
            1.0 / 12_700.0,
            400.0,
            fragment.backend_font_resource_id.as_deref(),
            egui::Color32::BLACK,
        );
        assert_eq!(usage.source_typography_sections, 1);
        assert_eq!(usage.fallback_sections, 0);
        assert_eq!(job.sections.len(), 1);
        assert_eq!(
            job.sections[0].format.font_id.family,
            egui::FontFamily::Name("chaptera.desktop.environment-font.test.face0".into())
        );
    }

    #[test]
    fn overlapping_typography_fails_closed_to_one_fallback_section() {
        let fragment: RenderTextFragmentV1 = serde_json::from_value(serde_json::json!({
            "story_id": "00000000-0000-0000-0000-000000000001",
            "scalar_start": 0,
            "scalar_end": 4,
            "text": "ABCD",
            "line_count": 1,
            "typography": [
                {"scalar_start": 0, "scalar_end": 3, "source_font_name": "A", "text_size_emu": 152400, "font_inherited": false, "size_inherited": false},
                {"scalar_start": 2, "scalar_end": 4, "source_font_name": "B", "text_size_emu": 304800, "font_inherited": false, "size_inherited": false}
            ]
        }))
        .expect("render text fragment");
        let (job, usage) =
            layout_document_text(&fragment, 1.0 / 12_700.0, 400.0, None, egui::Color32::BLACK);
        assert_eq!(job.sections.len(), 1);
        assert_eq!(usage.source_typography_sections, 0);
        assert_eq!(usage.fallback_sections, 1);
        assert_eq!(
            job.sections[0].format.font_id,
            crate::fallback_font::font_id_for_scene_scale(1.0 / 12_700.0)
        );
    }
}
