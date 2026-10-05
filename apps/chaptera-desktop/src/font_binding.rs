//! Desktop fallback/source-font binding owned outside the monolithic shell.

use crate::{ViewerApp, ViewerGeometryDocument, fallback_font, source_font};
use chaptera_viewer_render_plan::{
    ExplicitRenderTextFontResourceV1, PageRenderPlanV1, RenderPlanErrorV1, RenderTypographyRunV1,
    build_page_render_plan_with_text_layout_resolver_v1,
    build_page_render_plan_with_text_layout_v1,
};
use pub_editor::{EditOperation, EditorSession, FormatPropertyV1, FormatValueV1, StoryId};
use eframe::egui;

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

fn story_has_scoped_boolean_history(editor: &EditorSession, story_id: StoryId) -> bool {
    editor.operations().iter().any(|operation| {
        matches!(
            operation,
            EditOperation::SetTextFormatPropertyScopedV1 {
                story_id: operation_story_id,
                property: FormatPropertyV1::Bold | FormatPropertyV1::Italic,
                ..
            } | EditOperation::ClearTextFormatPropertyOverrideScopedV1 {
                story_id: operation_story_id,
                property: FormatPropertyV1::Bold | FormatPropertyV1::Italic,
                ..
            } if *operation_story_id == story_id
        )
    })
}

fn effective_boolean_at(
    segments: &[pub_editor::EffectivePropertySegmentV1],
    start: u32,
    end: u32,
    label: &str,
) -> Result<bool, String> {
    let mut matching = segments
        .iter()
        .filter(|segment| segment.start_scalar <= start && end <= segment.end_scalar);
    let segment = matching
        .next()
        .ok_or_else(|| format!("{label} scoped typography has a coverage gap at {start}..{end}"))?;
    if matching.next().is_some() {
        return Err(format!(
            "{label} scoped typography is ambiguous at {start}..{end}"
        ));
    }
    match segment.value {
        FormatValueV1::Bool(value) => Ok(value),
        _ => Err(format!(
            "{label} scoped typography is not boolean at {start}..{end}"
        )),
    }
}

fn current_boolean_typography_runs(
    editor: &EditorSession,
    story_id: StoryId,
    fragment_start: u32,
    fragment_end: u32,
    source: &[RenderTypographyRunV1],
) -> Result<Vec<RenderTypographyRunV1>, String> {
    if fragment_start >= fragment_end {
        return Ok(Vec::new());
    }
    if source.is_empty() {
        return Err("current scoped Bold/Italic has no source typography carrier".to_owned());
    }

    let bold = editor
        .current_text_format_property_segments_v1(
            story_id,
            FormatPropertyV1::Bold,
            fragment_start,
            fragment_end,
        )
        .map_err(|error| format!("current scoped Bold unavailable: {error}"))?;
    let italic = editor
        .current_text_format_property_segments_v1(
            story_id,
            FormatPropertyV1::Italic,
            fragment_start,
            fragment_end,
        )
        .map_err(|error| format!("current scoped Italic unavailable: {error}"))?;

    let mut boundaries = vec![fragment_start, fragment_end];
    for run in source {
        let start = run.scalar_start.max(fragment_start);
        let end = run.scalar_end.min(fragment_end);
        if start < end {
            boundaries.push(start);
            boundaries.push(end);
        }
    }
    for segment in bold.iter().chain(italic.iter()) {
        let start = segment.start_scalar.max(fragment_start);
        let end = segment.end_scalar.min(fragment_end);
        if start < end {
            boundaries.push(start);
            boundaries.push(end);
        }
    }
    boundaries.sort_unstable();
    boundaries.dedup();

    let mut out = Vec::new();
    for pair in boundaries.windows(2) {
        let start = pair[0];
        let end = pair[1];
        if start >= end {
            continue;
        }
        let mut matching = source
            .iter()
            .filter(|run| run.scalar_start <= start && end <= run.scalar_end);
        let source_run = matching.next().ok_or_else(|| {
            format!("source typography has a coverage gap at {start}..{end}")
        })?;
        if matching.next().is_some() {
            return Err(format!(
                "source typography is ambiguous at {start}..{end}"
            ));
        }

        let mut run = source_run.clone();
        run.scalar_start = start;
        run.scalar_end = end;
        run.bold = Some(effective_boolean_at(&bold, start, end, "Bold")?);
        run.italic = Some(effective_boolean_at(&italic, start, end, "Italic")?);
        out.push(run);
    }

    if out.first().map(|run| run.scalar_start) != Some(fragment_start)
        || out.last().map(|run| run.scalar_end) != Some(fragment_end)
        || out.windows(2).any(|pair| pair[0].scalar_end != pair[1].scalar_start)
    {
        return Err("current scoped typography does not cover the full render fragment".to_owned());
    }

    Ok(out)
}

pub(super) fn apply_current_scoped_boolean_typography(
    plan: &mut PageRenderPlanV1,
    editor: &EditorSession,
) -> Result<usize, String> {
    let mut changed = 0_usize;
    for fragment in plan.nodes.iter_mut().filter_map(|node| node.text.as_mut()) {
        if !story_has_scoped_boolean_history(editor, fragment.story_id) {
            continue;
        }
        fragment.typography = current_boolean_typography_runs(
            editor,
            fragment.story_id,
            fragment.scalar_start,
            fragment.scalar_end,
            &fragment.typography,
        )?;
        // The existing shared layout was resolved before current Chaptera
        // Bold/Italic entered the render input. Do not claim it is current.
        // Styled-face execution will re-resolve this path in the next bounded seam.
        fragment.layout = None;
        changed += 1;
    }
    Ok(changed)
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


#[cfg(test)]
mod tests {
    use super::*;
    use pub_editor::{Sha256Digest, open_mature_0x2c_editor};
    use sha2::{Digest, Sha256};
    use std::{env, fs};

    fn bool_value(value: &FormatValueV1) -> Option<bool> {
        match value {
            FormatValueV1::Bool(value) => Some(*value),
            _ => None,
        }
    }

    #[test]
    fn real_51318_scoped_bold_reaches_current_render_typography_input() {
        let Some(path) = env::var_os("CHAPTERA_TEXT_FORMAT_51318") else {
            eprintln!(
                "CHAPTERA_TEXT_FORMAT_51318 not set; dedicated render-consume gate owns real evidence"
            );
            return;
        };

        let original = fs::read(&path).expect("read pinned 51318.pub");
        let digest = Sha256::digest(&original);
        let mut digest_bytes = [0_u8; 32];
        digest_bytes.copy_from_slice(&digest);
        let source_hash = Sha256Digest::from_bytes(digest_bytes);
        let mut editor =
            open_mature_0x2c_editor(&original, source_hash).expect("open pinned 51318.pub");
        let visual = pub_viewer::open_mature_0x2c_geometry(
            &original,
            pub_viewer::viewer_geometry_environment_v0_1(),
        )
        .expect("open pinned 51318 Viewer geometry");

        let mut witness = None;
        'pages: for page_index in 0..visual.document.pages.len() {
            let plan = chaptera_viewer_render_plan::build_page_render_plan_v1(&visual, page_index)
                .expect("source render plan");
            for fragment in plan.nodes.iter().filter_map(|node| node.text.as_ref()) {
                if fragment.scalar_start >= fragment.scalar_end || fragment.typography.is_empty() {
                    continue;
                }
                let full_error = match editor.current_text_format_overlay_v1(fragment.story_id) {
                    Ok(_) => continue,
                    Err(error) => error.to_string(),
                };
                if !full_error.contains("bounded effective direct-RGB text color is unavailable") {
                    continue;
                }
                let bold = match editor.current_text_format_property_segments_v1(
                    fragment.story_id,
                    FormatPropertyV1::Bold,
                    fragment.scalar_start,
                    fragment.scalar_end,
                ) {
                    Ok(segments) if !segments.is_empty() => segments,
                    _ => continue,
                };
                let italic = match editor.current_text_format_property_segments_v1(
                    fragment.story_id,
                    FormatPropertyV1::Italic,
                    fragment.scalar_start,
                    fragment.scalar_end,
                ) {
                    Ok(segments) if !segments.is_empty() => segments,
                    _ => continue,
                };
                if bold.iter().any(|segment| bool_value(&segment.value).is_none())
                    || italic
                        .iter()
                        .any(|segment| bool_value(&segment.value).is_none())
                {
                    continue;
                }
                let all_bold = bold
                    .iter()
                    .all(|segment| bool_value(&segment.value) == Some(true));
                witness = Some((
                    page_index,
                    fragment.story_id,
                    fragment.scalar_start,
                    fragment.scalar_end,
                    fragment.typography.clone(),
                    !all_bold,
                    full_error,
                ));
                break 'pages;
            }
        }

        let (
            page_index,
            story_id,
            scalar_start,
            scalar_end,
            source_typography,
            desired_bold,
            full_error,
        ) = witness.expect(
            "51318.pub must expose one rendered color-blocked range with scoped Bold/Italic authority",
        );

        let before_hash = editor
            .current_text_format_property_state_hash_v1(story_id, FormatPropertyV1::Bold)
            .expect("current scoped Bold hash");
        editor
            .set_text_format_property_scoped_v1(
                story_id,
                scalar_start,
                scalar_end,
                FormatPropertyV1::Bold,
                FormatValueV1::Bool(desired_bold),
                &before_hash,
            )
            .expect("Set scoped Bold for current render input");

        let edited_project = editor.project();
        let render_current = |session: &EditorSession| {
            let mut plan =
                chaptera_viewer_render_plan::build_page_render_plan_v1(&visual, page_index)
                    .expect("render plan");
            let changed = apply_current_scoped_boolean_typography(&mut plan, session)
                .expect("apply current scoped typography");
            let typography = plan
                .nodes
                .iter()
                .filter_map(|node| node.text.as_ref())
                .find(|fragment| {
                    fragment.story_id == story_id
                        && fragment.scalar_start == scalar_start
                        && fragment.scalar_end == scalar_end
                })
                .expect("same render fragment")
                .typography
                .clone();
            (changed, typography)
        };

        let (changed, edited_typography) = render_current(&editor);
        assert!(changed > 0);
        assert!(edited_typography.iter().all(|run| run.bold == Some(desired_bold)));
        assert!(
            editor
                .current_text_format_overlay_v1(story_id)
                .expect_err("scoped Bold must not invent unresolved color")
                .to_string()
                .contains("bounded effective direct-RGB text color is unavailable")
        );

        editor.undo().expect("Undo scoped Bold");
        let (_, undone_typography) = render_current(&editor);
        assert_eq!(undone_typography, source_typography);

        editor.redo().expect("Redo scoped Bold");
        let (_, redone_typography) = render_current(&editor);
        assert_eq!(redone_typography, edited_typography);

        let clear_hash = editor
            .current_text_format_property_state_hash_v1(story_id, FormatPropertyV1::Bold)
            .expect("edited scoped Bold hash");
        editor
            .clear_text_format_property_override_scoped_v1(
                story_id,
                scalar_start,
                scalar_end,
                FormatPropertyV1::Bold,
                &clear_hash,
            )
            .expect("Clear scoped Bold override");
        let (_, cleared_typography) = render_current(&editor);
        assert_eq!(cleared_typography, source_typography);

        let serialized =
            serde_json::to_vec(&edited_project).expect("serialize edited v0.16 project");
        let project_roundtrip: pub_editor::EditorProject =
            serde_json::from_slice(&serialized).expect("deserialize edited v0.16 project");
        let mut reopened =
            open_mature_0x2c_editor(&original, source_hash).expect("fresh reopen 51318.pub");
        reopened
            .apply_project(&project_roundtrip)
            .expect("fresh replay scoped Bold project");
        let (_, replayed_typography) = render_current(&reopened);
        assert_eq!(replayed_typography, edited_typography);

        assert_eq!(reopened.source_hash(), source_hash);
        assert_eq!(
            fs::read(&path).expect("re-read pinned 51318.pub"),
            original,
            "render consumption must not mutate source PUB bytes"
        );
        eprintln!(
            "render-consume witness story={} range={}..{} legacy_full_overlay_error={}",
            story_id.as_canonical(),
            scalar_start,
            scalar_end,
            full_error
        );
    }
}
