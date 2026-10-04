//! Existing pinned-PUB GUI acceptance for the direct text-session owner.

use super::*;

#[cfg(not(feature = "reader-only"))]
#[test]
#[ignore = "runtime GUI evidence requires pinned CHAPTERA_SAMPLE_NEWSLETTER"]
fn gui_direct_text_session_edits_real_story_without_inspector_apply() {
    use egui_kittest::{Harness, kittest::Queryable};

    let fixture_source = std::env::var_os("CHAPTERA_SAMPLE_NEWSLETTER")
        .map(PathBuf::from)
        .expect("CHAPTERA_SAMPLE_NEWSLETTER must point to the pinned Apache POI fixture");
    let original = fs::read(&fixture_source).expect("read pinned SampleNewsletter fixture");
    let root =
        std::env::temp_dir().join(format!("chaptera-gui-direct-text-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("create direct-text GUI temp directory");
    let fixture = root.join("SampleNewsletter.pub");
    fs::write(&fixture, &original).expect("write direct-text GUI PUB fixture");

    let fixture_for_app = fixture.clone();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1280.0, 820.0))
        .with_pixels_per_point(1.0)
        .with_max_steps(32)
        .build_eframe(move |cc| {
            fallback_font::install(&cc.egui_ctx)
                .expect("pinned Chaptera fallback font resource must validate");
            ViewerApp::new_with_storage(Some(fixture_for_app), cc.storage)
        });
    harness.step();

    let (page_label, target_document_point, target_story_id, target_frame_id, before_fragment) = {
        let app = harness.state();
        let visual = app.visual.as_ref().expect("visual loaded");
        let editor = app.editor.as_ref().expect("editor loaded");
        visual
            .text_fragments
            .iter()
            .find_map(|fragment| {
                let explicit_mode = text_session::enter_explicit_text_mode(
                    editor,
                    fragment.story_id,
                    fragment.frame_id,
                )
                .ok()?;
                let node = visual
                    .scene
                    .nodes
                    .iter()
                    .find(|node| node.origin == fragment.frame_id)?;
                let page = visual
                    .document
                    .pages
                    .iter()
                    .find(|page| node.parent_origin == page.id.into_canonical())?;
                let page_id_text = page.id.as_canonical().to_string();
                let frame_id_text = fragment.frame_id.as_canonical().to_string();
                let page_nodes = visual
                    .scene
                    .nodes
                    .iter()
                    .filter(|candidate| candidate.parent_origin == page.id.into_canonical())
                    .collect::<Vec<_>>();
                let hit_index = SceneHitTestIndex::new(
                    page_nodes
                        .iter()
                        .enumerate()
                        .filter_map(|(paint_order, candidate)| {
                            let instance =
                                direct_scene_instance(editor, &page_id_text, candidate.origin)?;
                            Some(SceneHitEntry {
                                instance_id: instance.instance_id,
                                node_id: candidate.origin,
                                bounds: candidate.bounds,
                                z_order: 0,
                                paint_order: u32::try_from(paint_order).unwrap_or(u32::MAX),
                            })
                        })
                        .collect(),
                );
                let point = explicit_mode
                    .layout
                    .caret_map
                    .caret_stops
                    .iter()
                    .filter(|caret| {
                        caret.page_id == page_id_text && caret.frame_id == frame_id_text
                    })
                    .find_map(|caret| {
                        let point = pub_interaction::DocumentPoint::new(
                            pub_editor::LengthEmu::new(caret.page_x_emu),
                            pub_editor::LengthEmu::new(
                                caret.page_y_top_emu
                                    + (caret.page_y_bottom_emu - caret.page_y_top_emu) / 2,
                            ),
                        );
                        let admitted = strict_document_rect_interior(&node.bounds, point)
                            && hit_index
                                .topmost_at(point)
                                .is_some_and(|top| top.node_id == fragment.frame_id);
                        admitted.then_some(point)
                    })?;
                Some((
                    format!("Page {}", page.index),
                    point,
                    fragment.story_id,
                    fragment.frame_id,
                    fragment.text.clone(),
                ))
            })
            .expect("real fixture exposes one topmost capability-safe TextFrame")
    };

    harness.get_by_label(&page_label).click();
    harness.step();

    let frame_center = {
        let canvas = harness
            .get_by_label("Document canvas")
            .raw_bounds()
            .expect("document canvas has screen bounds");
        let app = harness.state();
        let visual = app.visual.as_ref().expect("visual loaded");
        let page = visual
            .document
            .pages
            .get(app.selected_page)
            .expect("selected direct-text page remains available");
        let surface = visual
            .scene
            .surfaces
            .iter()
            .find(|surface| surface.origin == page.id)
            .expect("selected direct-text page has a scene surface");
        let viewport = egui::vec2(
            (canvas.x1 - canvas.x0) as f32,
            (canvas.y1 - canvas.y0) as f32,
        );
        let fit_scale = fitted_scale(
            surface.size.width.get(),
            surface.size.height.get(),
            viewport,
        )
        .expect("selected direct-text page has valid fit scale");
        let scene_scale = fit_scale * app.zoom;
        let page_width = surface.size.width.get() as f32 * scene_scale;
        let page_height = surface.size.height.get() as f32 * scene_scale;
        let page_left = ((canvas.x0 + canvas.x1) as f32 - page_width) / 2.0;
        let page_top = ((canvas.y0 + canvas.y1) as f32 - page_height) / 2.0;
        egui::pos2(
            page_left + target_document_point.x.get() as f32 * scene_scale,
            page_top + target_document_point.y.get() as f32 * scene_scale,
        )
    };

    harness.input_mut().events.extend([
        egui::Event::PointerMoved(frame_center),
        egui::Event::PointerButton {
            pos: frame_center,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::default(),
        },
        egui::Event::PointerButton {
            pos: frame_center,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::default(),
        },
    ]);
    harness.step();
    harness.step();
    let operations_before = harness
        .state()
        .editor
        .as_ref()
        .expect("editor")
        .operations()
        .len();

    assert_eq!(
        harness
            .state()
            .text_mode
            .as_ref()
            .expect("strict TextFrame interior click enters a session")
            .story_id,
        target_story_id
    );
    assert_eq!(
        harness
            .state()
            .text_mode
            .as_ref()
            .expect("direct text mode")
            .frame_id,
        target_frame_id
    );
    assert_eq!(
        harness
            .state()
            .editor
            .as_ref()
            .expect("editor")
            .operations()
            .len(),
        operations_before,
        "entering direct text mode is transient"
    );

    harness.input_mut().events.extend([
        egui::Event::PointerMoved(frame_center),
        egui::Event::PointerButton {
            pos: frame_center,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::default(),
        },
        egui::Event::PointerButton {
            pos: frame_center,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::default(),
        },
    ]);
    harness.step();
    harness
        .input_mut()
        .events
        .push(egui::Event::Text("X".to_owned()));
    harness.step();
    harness.step();

    {
        let app = harness.state();
        let editor = app.editor.as_ref().expect("editor");
        assert_eq!(editor.operations().len(), operations_before + 1);
        assert!(matches!(
            editor.operations().last(),
            Some(pub_editor::EditOperation::ReplaceStoryRange { .. })
        ));
        let after_fragment = app
            .visual
            .as_ref()
            .expect("visual")
            .text_fragments
            .iter()
            .find(|fragment| fragment.frame_id == target_frame_id)
            .expect("edited frame remains projected");
        assert_ne!(
            after_fragment.text, before_fragment,
            "canvas paint projection must reflect the current Story after direct typing"
        );
    }

    harness.input_mut().events.push(egui::Event::Key {
        key: egui::Key::Escape,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::default(),
    });
    harness.step();
    assert!(harness.state().text_mode.is_none());
    assert_eq!(
        harness
            .state()
            .editor
            .as_ref()
            .expect("editor")
            .operations()
            .len(),
        operations_before + 1,
        "Escape exits without a second document mutation"
    );
    assert_eq!(
        fs::read(&fixture).expect("re-read source PUB"),
        original,
        "GUI direct text editing must not mutate source PUB bytes"
    );

    let _ = fs::remove_dir_all(root);
}
