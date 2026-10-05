//! Desktop fallback/source-font binding owned outside the monolithic shell.

use crate::{ViewerApp, ViewerGeometryDocument, fallback_font, source_font};
use chaptera_desktop_shaped_flow_runtime::{
    DesktopCurrentBooleanTypographyRunV1, current_story_boolean_typography_v1,
};
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

fn compose_current_fragment_typography_v1(
    fragment: &RenderTextFragmentV1,
    current: &[DesktopCurrentBooleanTypographyRunV1],
) -> Result<Option<Vec<RenderTypographyRunV1>>, String> {
    if fragment.scalar_start >= fragment.scalar_end
        || fragment.typography.is_empty()
        || current.is_empty()
    {
        return Ok(None);
    }

    let mut boundaries = vec![fragment.scalar_start, fragment.scalar_end];
    for run in &fragment.typography {
        if fragment.scalar_start < run.scalar_end && run.scalar_start < fragment.scalar_end {
            boundaries.push(fragment.scalar_start.max(run.scalar_start));
            boundaries.push(fragment.scalar_end.min(run.scalar_end));
        }
    }
    for run in current {
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

fn current_fragment_typography_v1(
    editor: &EditorSession,
    fragment: &RenderTextFragmentV1,
) -> Result<Option<Vec<RenderTypographyRunV1>>, String> {
    if !editor.graph().stories.contains_key(&fragment.story_id) {
        return Ok(None);
    }
    let current = current_story_boolean_typography_v1(editor, fragment.story_id)
        .map_err(|error| error.to_string())?;
    compose_current_fragment_typography_v1(fragment, &current)
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

#[cfg(test)]
mod tests {
    use super::*;
    use pub_model::{CanonicalId, StoryId};

    fn story_id() -> StoryId {
        StoryId::from_canonical(CanonicalId::from_bytes([0x42; 16]))
    }

    fn fragment_with_style(bold: Option<bool>, italic: Option<bool>) -> RenderTextFragmentV1 {
        RenderTextFragmentV1 {
            story_id: story_id(),
            scalar_start: 0,
            scalar_end: 4,
            text: "ABCD".to_owned(),
            line_count: 1,
            typography: vec![RenderTypographyRunV1 {
                scalar_start: 0,
                scalar_end: 4,
                source_font_name: "Example Family".to_owned(),
                text_size_emu: 152_400,
                font_inherited: false,
                size_inherited: false,
                color_rgb: None,
                color_inherited: false,
                bold,
                italic,
            }],
            paragraph_alignments: Vec::new(),
            backend_font_resource_id: None,
            layout: None,
        }
    }

    #[test]
    fn current_boolean_overlay_splits_source_runs_without_inventing_unset_properties() {
        let fragment = fragment_with_style(Some(false), Some(false));
        let current = vec![
            DesktopCurrentBooleanTypographyRunV1 {
                scalar_start: 0,
                scalar_end: 2,
                bold: None,
                italic: None,
            },
            DesktopCurrentBooleanTypographyRunV1 {
                scalar_start: 2,
                scalar_end: 4,
                bold: Some(true),
                italic: None,
            },
        ];

        let runs = compose_current_fragment_typography_v1(&fragment, &current)
            .expect("composition")
            .expect("current typography");
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].scalar_start..runs[0].scalar_end, 0..2);
        assert_eq!((runs[0].bold, runs[0].italic), (Some(false), Some(false)));
        assert_eq!(runs[1].scalar_start..runs[1].scalar_end, 2..4);
        assert_eq!((runs[1].bold, runs[1].italic), (Some(true), Some(false)));
    }

    #[test]
    fn current_boolean_overlay_preserves_unknown_instead_of_normalizing_false() {
        let fragment = fragment_with_style(None, None);
        let current = vec![DesktopCurrentBooleanTypographyRunV1 {
            scalar_start: 0,
            scalar_end: 4,
            bold: None,
            italic: Some(true),
        }];

        let runs = compose_current_fragment_typography_v1(&fragment, &current)
            .expect("composition")
            .expect("current typography");
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].bold, None);
        assert_eq!(runs[0].italic, Some(true));
    }
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
