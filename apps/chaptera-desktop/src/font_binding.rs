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
    use chaptera_viewer_render_plan::{RenderTextLayoutDispositionV1, build_page_render_plan_v1};
    use pub_editor::{
        EDITOR_PROJECT_VERSION_V0_16, FormatPropertyV1, FormatValueV1, NodeId, Sha256Digest,
        open_mature_0x2c_editor,
    };
    use pub_model::{CanonicalId, StoryId};
    use sha2::{Digest, Sha256};
    use std::{env, fs, path::PathBuf};

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

    fn text_for_node(plan: &PageRenderPlanV1, node_id: NodeId) -> Option<&RenderTextFragmentV1> {
        plan.nodes
            .iter()
            .find(|node| node.node_id == node_id)
            .and_then(|node| node.text.as_ref())
    }

    fn resource_for_scalar(fragment: &RenderTextFragmentV1, scalar: u32) -> Option<&str> {
        let layout = fragment.layout.as_ref()?;
        for line in &layout.lines {
            if !(line.scalar_start <= scalar && scalar < line.scalar_end) {
                continue;
            }
            if let Some(span) = line
                .spans
                .iter()
                .find(|span| span.scalar_start <= scalar && scalar < span.scalar_end)
            {
                return span.font_resource_id.as_deref();
            }
            if let RenderTextLayoutDispositionV1::SharedResolved {
                font_resource_id, ..
            } = &layout.disposition
            {
                return Some(font_resource_id.as_str());
            }
        }
        None
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_real_pub_scoped_style_changes_exact_resource_and_roundtrips() {
        struct Witness {
            path: PathBuf,
            page_index: usize,
            node_id: NodeId,
            story_id: StoryId,
            property: FormatPropertyV1,
            toggled_value: bool,
            edit_start: u32,
            edit_end: u32,
            outside_scalar: u32,
            before_resource_id: String,
            after_resource_id: String,
        }

        let root = env::var_os("CHAPTERA_TEXT_FORMAT_FIXTURES_DIR")
            .map(PathBuf::from)
            .expect("CHAPTERA_TEXT_FORMAT_FIXTURES_DIR");
        let mut paths = fs::read_dir(&root)
            .expect("read pinned text-format corpus")
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| {
                path.extension()
                    .and_then(|value| value.to_str())
                    .is_some_and(|value| value.eq_ignore_ascii_case("pub"))
            })
            .collect::<Vec<_>>();
        paths.sort();

        let mut selected = None;
        'files: for path in paths {
            let original = fs::read(&path).expect("read candidate PUB");
            let digest = Sha256::digest(&original);
            let mut digest_bytes = [0_u8; 32];
            digest_bytes.copy_from_slice(&digest);
            let source_hash = Sha256Digest::from_bytes(digest_bytes);
            let Ok(editor) = open_mature_0x2c_editor(&original, source_hash) else {
                continue;
            };
            let Ok(visual) = crate::diagnostic_sweep::open_for_product(&original) else {
                continue;
            };
            let mut registry = source_font::DesktopSourceFontRegistry::new();
            registry.ensure_visual_fonts(&visual);

            for page_index in 0..visual.document.pages.len() {
                let Ok(base_plan) = build_page_render_plan_v1(&visual, page_index) else {
                    continue;
                };
                for node in &base_plan.nodes {
                    let Some(fragment) = node.text.as_ref() else {
                        continue;
                    };
                    if !fragment.text.is_ascii() || fragment.scalar_end <= fragment.scalar_start + 1 {
                        continue;
                    }
                    let Some(story) = editor.graph().stories.get(&fragment.story_id) else {
                        continue;
                    };
                    let Ok(story_len) = u32::try_from(story.text.chars().count()) else {
                        continue;
                    };
                    if fragment.scalar_start != 0
                        || fragment.scalar_end != story_len
                        || fragment.text != story.text
                    {
                        continue;
                    }

                    let Ok(Some(current_typography)) =
                        current_fragment_typography_v1(&editor, fragment)
                    else {
                        continue;
                    };

                    for run in current_typography {
                        if run.scalar_end.saturating_sub(run.scalar_start) < 2
                            || run.source_font_name.trim().is_empty()
                        {
                            continue;
                        }
                        let (Some(source_bold), Some(source_italic)) = (run.bold, run.italic) else {
                            continue;
                        };
                        let Some(before_font) = registry.resource_for_typography_run(&run) else {
                            continue;
                        };
                        let before_resource_id = before_font.resource_id.to_owned();

                        let mut property = FormatPropertyV1::Bold;
                        let mut toggled_value = !source_bold;
                        let mut toggled = run.clone();
                        toggled.bold = Some(toggled_value);
                        let mut after_resource_id = registry
                            .resource_for_typography_run(&toggled)
                            .map(|font| font.resource_id.to_owned());

                        if after_resource_id.as_deref() == Some(before_resource_id.as_str())
                            || after_resource_id.is_none()
                        {
                            property = FormatPropertyV1::Italic;
                            toggled_value = !source_italic;
                            toggled = run.clone();
                            toggled.italic = Some(toggled_value);
                            after_resource_id = registry
                                .resource_for_typography_run(&toggled)
                                .map(|font| font.resource_id.to_owned());
                        }

                        let Some(after_resource_id) = after_resource_id else {
                            continue;
                        };
                        if after_resource_id == before_resource_id {
                            continue;
                        }

                        selected = Some(Witness {
                            path: path.clone(),
                            page_index,
                            node_id: node.node_id,
                            story_id: fragment.story_id,
                            property,
                            toggled_value,
                            edit_start: run.scalar_start,
                            edit_end: run.scalar_start + 1,
                            outside_scalar: run.scalar_start + 1,
                            before_resource_id,
                            after_resource_id,
                        });
                        break 'files;
                    }
                }
            }
        }

        let witness = selected.expect(
            "pinned text-format corpus must expose a placed Story with one uniquely resolvable exact Bold/Italic style transition",
        );
        let original = fs::read(&witness.path).expect("read selected witness");
        let digest = Sha256::digest(&original);
        let mut digest_bytes = [0_u8; 32];
        digest_bytes.copy_from_slice(&digest);
        let source_hash = Sha256Digest::from_bytes(digest_bytes);
        let visual =
            crate::diagnostic_sweep::open_for_product(&original).expect("open witness visual");
        let mut registry = source_font::DesktopSourceFontRegistry::new();
        registry.ensure_visual_fonts(&visual);

        let paint_fonts = registry.egui_fonts();
        let ctx = egui::Context::default();
        crate::fallback_font::install_with_additional(&ctx, &paint_fonts)
            .expect("exact styled resources must register in egui");

        let mut editor =
            open_mature_0x2c_editor(&original, source_hash).expect("open witness editor");
        let source_text = editor.graph().stories[&witness.story_id].text.clone();

        let before_plan = build_desktop_page_render_plan_with_current_source_fonts(
            &visual,
            witness.page_index,
            &registry,
            &editor,
        )
        .expect("source/base render plan");
        let before_text =
            text_for_node(&before_plan, witness.node_id).expect("source/base text node");
        assert_eq!(
            resource_for_scalar(before_text, witness.edit_start),
            Some(witness.before_resource_id.as_str())
        );

        let state_hash = editor
            .current_text_format_property_state_hash_v1(witness.story_id, witness.property)
            .expect("current scoped property hash");
        editor
            .set_text_format_property_scoped_v1(
                witness.story_id,
                witness.edit_start,
                witness.edit_end,
                witness.property,
                FormatValueV1::Bool(witness.toggled_value),
                &state_hash,
            )
            .expect("commit scoped style");

        let styled_plan = build_desktop_page_render_plan_with_current_source_fonts(
            &visual,
            witness.page_index,
            &registry,
            &editor,
        )
        .expect("styled render plan");
        let styled_text = text_for_node(&styled_plan, witness.node_id).expect("styled text node");
        assert_eq!(
            resource_for_scalar(styled_text, witness.edit_start),
            Some(witness.after_resource_id.as_str())
        );
        assert_eq!(
            resource_for_scalar(styled_text, witness.outside_scalar),
            Some(witness.before_resource_id.as_str()),
            "exact styled resource may change only inside the edited scalar range"
        );

        editor.undo().expect("undo scoped style");
        let undo_plan = build_desktop_page_render_plan_with_current_source_fonts(
            &visual,
            witness.page_index,
            &registry,
            &editor,
        )
        .expect("undo render plan");
        assert_eq!(
            resource_for_scalar(
                text_for_node(&undo_plan, witness.node_id).expect("undo text node"),
                witness.edit_start,
            ),
            Some(witness.before_resource_id.as_str())
        );

        editor.redo().expect("redo scoped style");
        let redo_plan = build_desktop_page_render_plan_with_current_source_fonts(
            &visual,
            witness.page_index,
            &registry,
            &editor,
        )
        .expect("redo render plan");
        assert_eq!(
            resource_for_scalar(
                text_for_node(&redo_plan, witness.node_id).expect("redo text node"),
                witness.edit_start,
            ),
            Some(witness.after_resource_id.as_str())
        );

        let project = editor.project();
        assert_eq!(project.schema_version, EDITOR_PROJECT_VERSION_V0_16);
        let serialized = serde_json::to_vec(&project).expect("serialize v0.16 project");
        let project_roundtrip = serde_json::from_slice(&serialized).expect("deserialize project");
        let mut reopened =
            open_mature_0x2c_editor(&original, source_hash).expect("fresh reopen source");
        reopened
            .apply_project(&project_roundtrip)
            .expect("replay scoped styled project");
        let reopened_plan = build_desktop_page_render_plan_with_current_source_fonts(
            &visual,
            witness.page_index,
            &registry,
            &reopened,
        )
        .expect("reopened styled render plan");
        assert_eq!(
            resource_for_scalar(
                text_for_node(&reopened_plan, witness.node_id).expect("reopened text node"),
                witness.edit_start,
            ),
            Some(witness.after_resource_id.as_str())
        );

        let clear_hash = reopened
            .current_text_format_property_state_hash_v1(witness.story_id, witness.property)
            .expect("clear scoped property hash");
        reopened
            .clear_text_format_property_override_scoped_v1(
                witness.story_id,
                witness.edit_start,
                witness.edit_end,
                witness.property,
                &clear_hash,
            )
            .expect("clear scoped style");
        let cleared_plan = build_desktop_page_render_plan_with_current_source_fonts(
            &visual,
            witness.page_index,
            &registry,
            &reopened,
        )
        .expect("cleared render plan");
        assert_eq!(
            resource_for_scalar(
                text_for_node(&cleared_plan, witness.node_id).expect("cleared text node"),
                witness.edit_start,
            ),
            Some(witness.before_resource_id.as_str())
        );

        assert_eq!(reopened.graph().stories[&witness.story_id].text, source_text);
        assert_eq!(reopened.source_hash(), source_hash);
        assert_eq!(
            fs::read(&witness.path).expect("re-read source PUB"),
            original,
            "styled resource execution must not mutate source PUB bytes"
        );

        let receipt = serde_json::json!({
            "schema": "chaptera.desktop-styled-font-real-pub.v1",
            "fixture": witness.path.file_name().and_then(|value| value.to_str()),
            "story_id": witness.story_id,
            "node_id": witness.node_id,
            "page_index": witness.page_index,
            "property": format!("{:?}", witness.property),
            "edited_range": [witness.edit_start, witness.edit_end],
            "source_resource_id": witness.before_resource_id,
            "styled_resource_id": witness.after_resource_id,
            "resource_change_scoped_to_range": true,
            "undo_restores_source": true,
            "redo_restores_styled": true,
            "v016_reopen_restores_styled": true,
            "clear_restores_source": true,
            "story_text_unchanged": true,
            "source_pub_bytes_unchanged": true
        });
        if let Ok(path) = env::var("CHAPTERA_STYLED_FONT_REAL_PUB_RECEIPT") {
            fs::write(
                path,
                serde_json::to_vec_pretty(&receipt).expect("serialize styled-font receipt"),
            )
            .expect("write styled-font receipt");
        }
        println!("{}", serde_json::to_string(&receipt).expect("receipt json"));
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
