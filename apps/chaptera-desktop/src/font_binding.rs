//! Desktop fallback/source-font binding owned outside the monolithic shell.

use crate::{ViewerApp, ViewerGeometryDocument, fallback_font, source_font};
use chaptera_viewer_render_plan::{
    ExplicitRenderTextFontResourceV1, PageRenderPlanV1, RenderPlanErrorV1, RenderTypographyRunV1,
    build_page_render_plan_with_text_layout_resolver_v1,
    build_page_render_plan_with_text_layout_v1,
};
use eframe::egui;
use pub_editor::{
    EditOperation, EditorSession, EffectivePropertySegmentV1, FormatPropertyV1, FormatValueV1,
};

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

fn operation_matches_boolean_property_v1(
    operation: &EditOperation,
    story_id: pub_editor::StoryId,
    property: FormatPropertyV1,
) -> bool {
    match operation {
        EditOperation::SetTextFormatProperty {
            story_id: operation_story_id,
            property: operation_property,
            ..
        }
        | EditOperation::ClearTextFormatPropertyOverride {
            story_id: operation_story_id,
            property: operation_property,
            ..
        }
        | EditOperation::SetTextFormatPropertyScopedV1 {
            story_id: operation_story_id,
            property: operation_property,
            ..
        }
        | EditOperation::ClearTextFormatPropertyOverrideScopedV1 {
            story_id: operation_story_id,
            property: operation_property,
            ..
        } => *operation_story_id == story_id && *operation_property == property,
        _ => false,
    }
}

fn current_edited_boolean_segments_v1(
    editor: &EditorSession,
    story_id: pub_editor::StoryId,
    property: FormatPropertyV1,
    scalar_start: u32,
    scalar_end: u32,
) -> Result<Option<Vec<EffectivePropertySegmentV1>>, String> {
    if !editor
        .operations()
        .iter()
        .any(|operation| operation_matches_boolean_property_v1(operation, story_id, property))
    {
        return Ok(None);
    }

    let segments = editor
        .current_text_format_property_segments_v1(story_id, property, scalar_start, scalar_end)
        .map_err(|error| {
            format!(
                "current {property:?} property state cannot enter render input for Story {story_id:?}: {error}"
            )
        })?;
    if segments.is_empty() {
        return Err(format!(
            "current {property:?} property state returned no render segments for Story {story_id:?}"
        ));
    }

    let mut cursor = scalar_start;
    for segment in &segments {
        if segment.start_scalar != cursor
            || segment.end_scalar <= segment.start_scalar
            || segment.end_scalar > scalar_end
        {
            return Err(format!(
                "current {property:?} property state is not contiguous over render range {scalar_start}..{scalar_end}"
            ));
        }
        if !matches!(&segment.value, FormatValueV1::Bool(_)) {
            return Err(format!(
                "current {property:?} property state is not boolean over render range {scalar_start}..{scalar_end}"
            ));
        }
        cursor = segment.end_scalar;
    }
    if cursor != scalar_end {
        return Err(format!(
            "current {property:?} property state does not cover render range {scalar_start}..{scalar_end}"
        ));
    }
    Ok(Some(segments))
}

fn boolean_value_for_subrange_v1(
    segments: Option<&[EffectivePropertySegmentV1]>,
    scalar_start: u32,
    scalar_end: u32,
) -> Result<Option<bool>, String> {
    let Some(segments) = segments else {
        return Ok(None);
    };
    let segment = segments
        .iter()
        .find(|segment| {
            segment.start_scalar <= scalar_start && scalar_end <= segment.end_scalar
        })
        .ok_or_else(|| {
            format!(
                "current boolean text-format state has no segment for render subrange {scalar_start}..{scalar_end}"
            )
        })?;
    match &segment.value {
        FormatValueV1::Bool(value) => Ok(Some(*value)),
        _ => Err("current boolean text-format segment carries a non-boolean value".to_owned()),
    }
}

fn split_render_typography_for_current_booleans_v1(
    source: &[RenderTypographyRunV1],
    bold: Option<&[EffectivePropertySegmentV1]>,
    italic: Option<&[EffectivePropertySegmentV1]>,
) -> Result<Vec<RenderTypographyRunV1>, String> {
    let mut out = Vec::new();
    for run in source {
        if run.scalar_start >= run.scalar_end {
            return Err("render typography contains an empty/reversed scalar run".to_owned());
        }
        let mut boundaries = vec![run.scalar_start, run.scalar_end];
        for segments in [bold, italic].into_iter().flatten() {
            for segment in segments {
                if run.scalar_start < segment.start_scalar && segment.start_scalar < run.scalar_end
                {
                    boundaries.push(segment.start_scalar);
                }
                if run.scalar_start < segment.end_scalar && segment.end_scalar < run.scalar_end {
                    boundaries.push(segment.end_scalar);
                }
            }
        }
        boundaries.sort_unstable();
        boundaries.dedup();

        for pair in boundaries.windows(2) {
            let scalar_start = pair[0];
            let scalar_end = pair[1];
            if scalar_start >= scalar_end {
                continue;
            }
            let mut next = run.clone();
            next.scalar_start = scalar_start;
            next.scalar_end = scalar_end;
            if let Some(value) = boolean_value_for_subrange_v1(bold, scalar_start, scalar_end)? {
                next.bold = Some(value);
            }
            if let Some(value) = boolean_value_for_subrange_v1(italic, scalar_start, scalar_end)? {
                next.italic = Some(value);
            }
            out.push(next);
        }
    }
    Ok(out)
}

/// Overlay current durable EditorSession Bold/Italic state onto the source-neutral
/// render-plan typography carrier. Source Reader typography remains the base.
///
/// This intentionally does not resolve or synthesize a physical styled face.
/// That is a separate environment/resource admission step. The purpose here is
/// to make the current edited boolean style observable to shaping/render
/// consumers without requiring unrelated Color/FontSize authority.
pub(super) fn apply_editor_current_boolean_typography_v1(
    plan: &mut PageRenderPlanV1,
    editor: &EditorSession,
) -> Result<(), String> {
    for fragment in plan.nodes.iter_mut().filter_map(|node| node.text.as_mut()) {
        if fragment.scalar_start >= fragment.scalar_end {
            continue;
        }
        let bold = current_edited_boolean_segments_v1(
            editor,
            fragment.story_id,
            FormatPropertyV1::Bold,
            fragment.scalar_start,
            fragment.scalar_end,
        )?;
        let italic = current_edited_boolean_segments_v1(
            editor,
            fragment.story_id,
            FormatPropertyV1::Italic,
            fragment.scalar_start,
            fragment.scalar_end,
        )?;
        if bold.is_none() && italic.is_none() {
            continue;
        }
        if fragment.typography.is_empty() {
            return Err(format!(
                "edited Bold/Italic Story {:?} has no source-neutral render typography carrier",
                fragment.story_id
            ));
        }
        fragment.typography = split_render_typography_for_current_booleans_v1(
            &fragment.typography,
            bold.as_deref(),
            italic.as_deref(),
        )?;
    }
    Ok(())
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
    use pub_editor::{FormatValueV1, Sha256Digest, open_mature_0x2c_editor};
    use sha2::{Digest, Sha256};
    use std::{env, fs, path::PathBuf};

    fn bool_value(segment: &EffectivePropertySegmentV1) -> bool {
        match &segment.value {
            FormatValueV1::Bool(value) => *value,
            _ => panic!("boolean property segment must carry a boolean value"),
        }
    }

    #[test]
    fn real_pub_color_blocked_bold_reaches_current_render_typography_input() {
        let Some(root) = env::var_os("CHAPTERA_TEXT_FORMAT_FIXTURES_DIR") else {
            eprintln!(
                "CHAPTERA_TEXT_FORMAT_FIXTURES_DIR not set; dedicated render-consume gate owns real evidence"
            );
            return;
        };
        let path = PathBuf::from(root).join("51318.pub");
        let original = fs::read(&path).expect("read pinned 51318.pub");
        assert_eq!(
            format!("{:x}", Sha256::digest(&original)),
            "3ab75a6a9196e0a51fc9b0aa759459501c71d313030c06652aabffbae0a2ab09"
        );
        let digest = Sha256::digest(&original);
        let mut digest_bytes = [0_u8; 32];
        digest_bytes.copy_from_slice(&digest);
        let source_hash = Sha256Digest::from_bytes(digest_bytes);

        let visual = pub_viewer::open_mature_0x2c_geometry(
            &original,
            pub_viewer::viewer_geometry_environment_v0_1(),
        )
        .expect("open pinned real PUB Viewer");
        let mut editor =
            open_mature_0x2c_editor(&original, source_hash).expect("open pinned real PUB Editor");

        let mut witness = None;
        'pages: for page_index in 0..visual.document.pages.len() {
            let plan = build_desktop_page_render_plan(&visual, page_index).expect("source plan");
            for node in &plan.nodes {
                let Some(fragment) = node.text.as_ref().filter(|fragment| {
                    fragment.scalar_start < fragment.scalar_end
                        && !fragment.typography.is_empty()
                        && fragment.typography.iter().any(|run| run.bold.is_some())
                }) else {
                    continue;
                };
                let full_error = match editor.current_text_format_overlay_v1(fragment.story_id) {
                    Ok(_) => continue,
                    Err(error) => error.to_string(),
                };
                if !full_error.contains("bounded effective direct-RGB text color is unavailable") {
                    continue;
                }
                let Ok(segments) = editor.current_text_format_property_segments_v1(
                    fragment.story_id,
                    FormatPropertyV1::Bold,
                    fragment.scalar_start,
                    fragment.scalar_end,
                ) else {
                    continue;
                };
                if segments.is_empty() {
                    continue;
                }
                witness = Some((
                    page_index,
                    node.node_id,
                    fragment.story_id,
                    fragment.scalar_start,
                    fragment.scalar_end,
                    fragment.text.clone(),
                    fragment.typography.clone(),
                    segments,
                    full_error,
                ));
                break 'pages;
            }
        }

        let (
            page_index,
            node_id,
            story_id,
            scalar_start,
            scalar_end,
            source_fragment_text,
            source_typography,
            before_segments,
            full_error,
        ) = witness.expect(
            "51318.pub must expose a rendered Story whose full overlay is color-blocked but Bold is property-scoped",
        );

        assert!(
            source_typography.iter().any(|run| run.bold.is_some()),
            "Reader-proven source Bold must survive pub-viewer -> render-plan before any Editor override"
        );
        let all_true = before_segments.iter().all(bool_value);
        let expected = !all_true;
        let state_hash = editor
            .current_text_format_property_state_hash_v1(story_id, FormatPropertyV1::Bold)
            .expect("current Bold property hash");
        editor
            .set_text_format_property_scoped_v1(
                story_id,
                scalar_start,
                scalar_end,
                FormatPropertyV1::Bold,
                FormatValueV1::Bool(expected),
                &state_hash,
            )
            .expect("set scoped Bold for rendered range");

        assert!(
            editor
                .current_text_format_overlay_v1(story_id)
                .expect_err("scoped Bold must not invent unresolved Color")
                .to_string()
                .contains("bounded effective direct-RGB text color is unavailable")
        );

        let mut edited =
            build_desktop_page_render_plan(&visual, page_index).expect("edited source plan");
        apply_editor_current_boolean_typography_v1(&mut edited, &editor)
            .expect("apply current scoped Bold to render input");
        let edited_fragment = edited
            .nodes
            .iter()
            .find(|node| node.node_id == node_id)
            .and_then(|node| node.text.as_ref())
            .expect("edited witness remains in render plan");
        let edited_runs = edited_fragment
            .typography
            .iter()
            .filter(|run| run.scalar_end > scalar_start && run.scalar_start < scalar_end)
            .collect::<Vec<_>>();
        assert!(!edited_runs.is_empty());
        assert!(
            edited_runs.iter().all(|run| run.bold == Some(expected)),
            "current scoped Bold must reach every source-neutral render-typography subrun in the edited range"
        );
        assert_eq!(edited_fragment.text, source_fragment_text);

        editor.undo().expect("undo scoped Bold");
        let mut undone =
            build_desktop_page_render_plan(&visual, page_index).expect("undo source plan");
        apply_editor_current_boolean_typography_v1(&mut undone, &editor)
            .expect("undo render overlay");
        let undone_fragment = undone
            .nodes
            .iter()
            .find(|node| node.node_id == node_id)
            .and_then(|node| node.text.as_ref())
            .expect("undo witness");
        assert_eq!(undone_fragment.typography, source_typography);

        assert_eq!(fs::read(&path).expect("re-read source PUB"), original);
        eprintln!(
            "render-consume witness={} story={story_id:?} range={scalar_start}..{scalar_end} legacy_full_overlay_error={full_error}",
            path.file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("<non-utf8>")
        );
    }
}
