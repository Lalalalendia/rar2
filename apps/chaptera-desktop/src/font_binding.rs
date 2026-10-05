//! Desktop fallback/source-font binding owned outside the monolithic shell.

use crate::{ViewerApp, ViewerGeometryDocument, fallback_font, source_font};
use chaptera_viewer_render_plan::{
    ExplicitRenderTextFontResourceV1, PageRenderPlanV1, RenderPlanErrorV1,
    RenderTextFragmentV1, RenderTypographyRunV1,
    build_page_render_plan_with_text_layout_and_typography_resolvers_v1,
    build_page_render_plan_with_text_layout_resolver_v1,
    build_page_render_plan_with_text_layout_v1,
};
use eframe::egui;
use pub_editor::{EditorSession, FormatPropertyV1, FormatValueV1};

fn desktop_text_font_resource() -> ExplicitRenderTextFontResourceV1<'static> {
    ExplicitRenderTextFontResourceV1 {
        resource_id: chaptera_desktop_fallback_font_resource::RESOURCE_ID,
        expected_sha256: chaptera_desktop_fallback_font_resource::EXPECTED_SHA256,
        face_index: 0,
        default_font_size_emu: chaptera_desktop_fallback_font_resource::FONT_SIZE_EMU,
        default_line_height_emu: chaptera_desktop_fallback_font_resource::LINE_HEIGHT_EMU,
        bytes: chaptera_desktop_fallback_font_resource::bytes(),
    }
}

pub(super) fn build_desktop_page_render_plan(
    visual: &ViewerGeometryDocument,
    page_index: usize,
) -> Result<PageRenderPlanV1, RenderPlanErrorV1> {
    build_page_render_plan_with_text_layout_v1(visual, page_index, &desktop_text_font_resource())
}

pub(super) fn build_desktop_page_render_plan_with_source_fonts(
    visual: &ViewerGeometryDocument,
    page_index: usize,
    source_fonts: &source_font::DesktopSourceFontRegistry,
) -> Result<PageRenderPlanV1, RenderPlanErrorV1> {
    let fallback = desktop_text_font_resource();
    build_page_render_plan_with_text_layout_resolver_v1(visual, page_index, &fallback, |fragment| {
        source_fonts.resource_for_fragment(fragment)
    })
}

fn bool_value_at(
    segments: &[pub_editor::EffectivePropertySegmentV1],
    start: u32,
    end: u32,
) -> Option<bool> {
    let segment = segments
        .iter()
        .find(|segment| segment.start_scalar <= start && end <= segment.end_scalar)?;
    match &segment.value {
        FormatValueV1::Bool(value) => Some(*value),
        _ => None,
    }
}

fn current_fragment_typography_v1(
    editor: &EditorSession,
    fragment: &RenderTextFragmentV1,
) -> Option<Vec<RenderTypographyRunV1>> {
    if fragment.scalar_start >= fragment.scalar_end || fragment.typography.is_empty() {
        return None;
    }

    let bold = editor
        .current_text_format_property_segments_v1(
            fragment.story_id,
            FormatPropertyV1::Bold,
            fragment.scalar_start,
            fragment.scalar_end,
        )
        .ok()?;
    let italic = editor
        .current_text_format_property_segments_v1(
            fragment.story_id,
            FormatPropertyV1::Italic,
            fragment.scalar_start,
            fragment.scalar_end,
        )
        .ok()?;

    let mut boundaries = vec![fragment.scalar_start, fragment.scalar_end];
    for run in &fragment.typography {
        if fragment.scalar_start < run.scalar_end && run.scalar_start < fragment.scalar_end {
            boundaries.push(fragment.scalar_start.max(run.scalar_start));
            boundaries.push(fragment.scalar_end.min(run.scalar_end));
        }
    }
    for segment in bold.iter().chain(&italic) {
        boundaries.push(segment.start_scalar);
        boundaries.push(segment.end_scalar);
    }
    boundaries.sort_unstable();
    boundaries.dedup();

    let mut current = Vec::with_capacity(boundaries.len().saturating_sub(1));
    for pair in boundaries.windows(2) {
        let start = pair[0];
        let end = pair[1];
        if start >= end {
            continue;
        }
        let source = fragment
            .typography
            .iter()
            .find(|run| run.scalar_start <= start && end <= run.scalar_end)?;
        let mut run = source.clone();
        run.scalar_start = start;
        run.scalar_end = end;
        run.bold = Some(bool_value_at(&bold, start, end)?);
        run.italic = Some(bool_value_at(&italic, start, end)?);
        current.push(run);
    }

    let first = current.first()?;
    let last = current.last()?;
    (first.scalar_start == fragment.scalar_start && last.scalar_end == fragment.scalar_end)
        .then_some(current)
}

pub(super) fn build_desktop_page_render_plan_with_current_source_fonts(
    visual: &ViewerGeometryDocument,
    page_index: usize,
    source_fonts: &source_font::DesktopSourceFontRegistry,
    editor: &EditorSession,
) -> Result<PageRenderPlanV1, RenderPlanErrorV1> {
    let fallback = desktop_text_font_resource();
    build_page_render_plan_with_text_layout_and_typography_resolvers_v1(
        visual,
        page_index,
        &fallback,
        |fragment| source_fonts.resource_for_current_fragment(fragment),
        |_, run| source_fonts.resource_for_typography_run(run),
        |fragment| current_fragment_typography_v1(editor, fragment),
    )
}

pub(super) fn install_startup_font(ctx: &egui::Context) {
    fallback_font::install(ctx).expect("pinned Chaptera fallback font resource must validate");
}

impl ViewerApp {
    pub(super) fn prepare_canvas_fonts(&mut self, ui: &egui::Ui) -> bool {
        if self.source_fonts_install_attempted {
            return false;
        }

        self.source_fonts_install_attempted = true;
        let additional = self.source_fonts.egui_fonts();
        match fallback_font::install_with_additional(ui.ctx(), &additional) {
            Ok(()) => {
                self.source_fonts_active = true;
            }
            Err(_) => {
                self.source_fonts_active = false;
                let _ = fallback_font::install(ui.ctx());
            }
        }
        self.page_frame_cache.clear();

        // egui applies FontDefinitions at the next pass boundary. Do not
        // build or paint a render plan that names a newly registered
        // source-font family in the same pass that calls set_fonts.
        ui.ctx().request_repaint();
        true
    }
}
