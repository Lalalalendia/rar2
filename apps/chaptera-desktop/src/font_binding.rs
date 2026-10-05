//! Desktop fallback/source-font binding owned outside the monolithic shell.

use crate::{ViewerApp, ViewerGeometryDocument, fallback_font, source_font};
use chaptera_desktop_shaped_flow_runtime::current_story_boolean_typography_v1;
use chaptera_viewer_render_plan::{
    ExplicitRenderTextFontResourceV1, PageRenderPlanV1, RenderPlanErrorV1, RenderTextFragmentV1,
    RenderTypographyRunV1, build_page_render_plan_with_text_layout_and_typography_resolvers_v1,
    build_page_render_plan_with_text_layout_resolvers_v1,
    build_page_render_plan_with_text_layout_v1,
};
use eframe::egui;
use pub_editor::EditorSession;
use std::cell::RefCell;

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
    build_page_render_plan_with_text_layout_resolvers_v1(
        visual,
        page_index,
        &fallback,
        |fragment| source_fonts.resource_for_current_fragment(fragment),
        |_, run| source_fonts.resource_for_typography_run(run),
    )
}

fn current_fragment_typography_v1(
    editor: &EditorSession,
    fragment: &RenderTextFragmentV1,
) -> Result<Option<Vec<RenderTypographyRunV1>>, String> {
    if fragment.scalar_start >= fragment.scalar_end || fragment.typography.is_empty() {
        return Ok(None);
    }

    let current = current_story_boolean_typography_v1(editor, fragment.story_id)
        .map_err(|error| error.to_string())?;
    if current.is_empty() {
        return Ok(None);
    }

    let mut boundaries = vec![fragment.scalar_start, fragment.scalar_end];
    for run in &fragment.typography {
        if fragment.scalar_start < run.scalar_end && run.scalar_start < fragment.scalar_end {
            boundaries.push(fragment.scalar_start.max(run.scalar_start));
            boundaries.push(fragment.scalar_end.min(run.scalar_end));
        }
    }
    for run in &current {
        if fragment.scalar_start < run.scalar_end && run.scalar_start < fragment.scalar_end {
            boundaries.push(fragment.scalar_start.max(run.scalar_start));
            boundaries.push(fragment.scalar_end.min(run.scalar_end));
        }
    }
    boundaries.sort_unstable();
    boundaries.dedup();

    let mut resolved = Vec::with_capacity(boundaries.len().saturating_sub(1));
    for pair in boundaries.windows(2) {
        let start = pair[0];
        let end = pair[1];
        if start >= end {
            continue;
        }
        let source = fragment
            .typography
            .iter()
            .find(|run| run.scalar_start <= start && end <= run.scalar_end)
            .ok_or_else(|| {
                format!(
                    "source typography does not cover current interval {}..{} for {:?}",
                    start, end, fragment.story_id
                )
            })?;
        let mut run = source.clone();
        run.scalar_start = start;
        run.scalar_end = end;
        if let Some(current_run) = current
            .iter()
            .find(|run| run.scalar_start <= start && end <= run.scalar_end)
        {
            if current_run.bold.is_some() {
                run.bold = current_run.bold;
            }
            if current_run.italic.is_some() {
                run.italic = current_run.italic;
            }
        }
        resolved.push(run);
    }

    if resolved.first().is_none_or(|run| run.scalar_start != fragment.scalar_start)
        || resolved.last().is_none_or(|run| run.scalar_end != fragment.scalar_end)
    {
        return Err(format!(
            "current typography composition is incomplete for {:?} {}..{}",
            fragment.story_id, fragment.scalar_start, fragment.scalar_end
        ));
    }

    Ok(Some(resolved))
}

pub(super) fn build_desktop_page_render_plan_with_current_source_fonts(
    visual: &ViewerGeometryDocument,
    page_index: usize,
    source_fonts: &source_font::DesktopSourceFontRegistry,
    editor: &EditorSession,
) -> Result<PageRenderPlanV1, String> {
    let fallback = desktop_text_font_resource();
    let composition_error = RefCell::new(None);
    let plan = build_page_render_plan_with_text_layout_and_typography_resolvers_v1(
        visual,
        page_index,
        &fallback,
        |fragment| source_fonts.resource_for_current_fragment(fragment),
        |_, run| source_fonts.resource_for_typography_run(run),
        |fragment| match current_fragment_typography_v1(editor, fragment) {
            Ok(typography) => typography,
            Err(error) => {
                let mut slot = composition_error.borrow_mut();
                if slot.is_none() {
                    *slot = Some(error);
                }
                None
            }
        },
    )
    .map_err(|error| error.to_string())?;

    if let Some(error) = composition_error.into_inner() {
        return Err(error);
    }
    Ok(plan)
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
