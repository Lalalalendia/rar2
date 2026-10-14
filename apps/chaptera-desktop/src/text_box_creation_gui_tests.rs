//! Real-PUB GUI acceptance for Text Box creation shell.

use crate::*;

#[cfg(not(feature = "reader-only"))]
#[test]
#[ignore = "runtime GUI evidence requires pinned CHAPTERA_SAMPLE_NEWSLETTER"]
fn gui_textbox_tool_creates_focuses_types_and_replays_one_new_story() {
    use egui_kittest::{Harness, kittest::Queryable};

    let fixture_source = std::env::var_os("CHAPTERA_SAMPLE_NEWSLETTER")
        .map(PathBuf::from)
        .expect("CHAPTERA_SAMPLE_NEWSLETTER must point to the pinned Apache POI fixture");
    let original = fs::read(&fixture_source).expect("read pinned SampleNewsletter fixture");
    let root = std::env::temp_dir().join(format!(
        "chaptera-gui-textbox-create-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("create TextBox GUI temp directory");
    let fixture = root.join("SampleNewsletter.pub");
    fs::write(&fixture, &original).expect("write TextBox GUI PUB fixture");

    let fixture_for_app = fixture.clone();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1280.0, 820.0))
        .with_pixels_per_point(1.0)
        .with_max_steps(80)
        .build_eframe(move |cc| {
            fallback_font::install(&cc.egui_ctx)
                .expect("pinned Chaptera fallback font resource must validate");
            ViewerApp::new_with_storage(Some(fixture_for_app), cc.storage)
        });
    harness.step();

    let operations_before = harness
        .state()
        .editor
        .as_ref()
        .expect("editor loaded")
        .operations()
        .len();

    let (zero_point, drag_start, drag_end) = {
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
            .expect("selected page");
        let surface = visual
            .scene
            .surfaces
            .iter()
            .find(|surface| surface.origin == page.id)
            .expect("selected page has surface");
        let viewport = egui::vec2(
            (canvas.x1 - canvas.x0) as f32,
            (canvas.y1 - canvas.y0) as f32,
        );
        let scene_scale = fitted_scale(
            surface.size.width.get(),
            surface.size.height.get(),
            viewport,
        )
        .expect("selected page fits");
        let page_width = surface.size.width.get() as f32 * scene_scale;
        let page_height = surface.size.height.get() as f32 * scene_scale;
        let page_left = ((canvas.x0 + canvas.x1) as f32 - page_width) / 2.0;
        let page_top = ((canvas.y0 + canvas.y1) as f32 - page_height) / 2.0;

        let doc_start_x = surface.size.width.get() / 5;
        let doc_start_y = surface.size.height.get() / 5;
        let doc_end_x = doc_start_x + surface.size.width.get() / 4;
        let doc_end_y = doc_start_y + surface.size.height.get() / 8;
        let start = egui::pos2(
            page_left + doc_start_x as f32 * scene_scale,
            page_top + doc_start_y as f32 * scene_scale,
        );
        let end = egui::pos2(
            page_left + doc_end_x as f32 * scene_scale,
            page_top + doc_end_y as f32 * scene_scale,
        );
        (start, start, end)
    };

    // A zero-size release is an explicit one-shot no-op and must return to Select.
    harness.get_by_label("Text Box").click();
    harness.step();
    assert!(harness.state().text_box_creation.active());
    harness.input_mut().events.extend([
        egui::Event::PointerMoved(zero_point),
        egui::Event::PointerButton {
            pos: zero_point,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::default(),
        },
    ]);
    harness.step();
    harness.input_mut().events.push(egui::Event::PointerButton {
        pos: zero_point,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::default(),
    });
    harness.step();
    harness.step();
    assert_eq!(
        harness
            .state()
            .editor
            .as_ref()
            .expect("editor")
            .operations()
            .len(),
        operations_before,
        "zero-size TextBox gesture must not create a document revision"
    );
    assert!(
        !harness.state().text_box_creation.active(),
        "zero-size one-shot must return to Select"
    );
    assert!(harness.state().text_box_creation.gesture_token.is_none());

    // Positive drag commits exactly one CreateTextBox and immediately focuses its empty Story.
    harness.get_by_label("Text Box").click();
    harness.step();
    harness.input_mut().events.extend([
        egui::Event::PointerMoved(drag_start),
        egui::Event::PointerButton {
            pos: drag_start,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::default(),
        },
    ]);
    harness.step();
    harness
        .input_mut()
        .events
        .push(egui::Event::PointerMoved(drag_end));
    harness.step();
    harness.input_mut().events.push(egui::Event::PointerButton {
        pos: drag_end,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::default(),
    });
    harness.step();
    harness.step();

    let (created_node_id, created_story_id) = {
        let app = harness.state();
        let editor = app.editor.as_ref().expect("editor");
        assert_eq!(editor.operations().len(), operations_before + 1);
        let (node_id, story_id) = match editor.operations().last() {
            Some(pub_editor::EditOperation::CreateTextBox {
                node_id, story_id, ..
            }) => (*node_id, *story_id),
            other => panic!("expected one CreateTextBox operation, got {other:?}"),
        };
        assert_eq!(editor.graph().stories[&story_id].text, "");
        assert!(
            app.visual
                .as_ref()
                .expect("visual")
                .scene
                .nodes
                .iter()
                .any(|node| node.origin == node_id),
            "accepted CreateTextBox must be materialized in the current Viewer scene"
        );
        let mode = app
            .text_mode
            .as_ref()
            .expect("accepted TextBox enters direct text mode");
        assert_eq!(mode.story_id, story_id);
        assert_eq!(mode.frame_id, node_id);
        assert_eq!(mode.session.selection.focus_scalar, 0);
        assert!(
            !app.text_box_creation.active(),
            "accepted one-shot must return to Select"
        );
        assert!(app.text_box_creation.gesture_token.is_none());
        (node_id, story_id)
    };

    harness
        .input_mut()
        .events
        .push(egui::Event::Text("Hello".to_owned()));
    harness.step();
    harness.step();

    {
        let app = harness.state();
        let editor = app.editor.as_ref().expect("editor");
        assert_eq!(editor.operations().len(), operations_before + 2);
        assert!(matches!(
            editor.operations().last(),
            Some(pub_editor::EditOperation::ReplaceStoryRange {
                story_id,
                replacement_text,
                ..
            }) if *story_id == created_story_id && replacement_text == "Hello"
        ));
        assert_eq!(editor.graph().stories[&created_story_id].text, "Hello");
        assert!(
            app.visual
                .as_ref()
                .expect("visual")
                .scene
                .nodes
                .iter()
                .any(|node| node.origin == created_node_id)
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

    harness
        .get_all_by_label("Undo")
        .next()
        .expect("Undo created Story text")
        .click();
    harness.step();
    {
        let editor = harness.state().editor.as_ref().expect("editor");
        assert_eq!(editor.graph().stories[&created_story_id].text, "");
        assert!(editor.graph().nodes.contains_key(&created_node_id));
    }

    harness
        .get_all_by_label("Undo")
        .next()
        .expect("Undo created TextBox")
        .click();
    harness.step();
    {
        let app = harness.state();
        let editor = app.editor.as_ref().expect("editor");
        assert!(
            !editor.graph().nodes.contains_key(&created_node_id),
            "second Undo removes the created TextFrame"
        );
        assert!(
            !editor.graph().stories.contains_key(&created_story_id),
            "second Undo removes its created Story atomically"
        );
        assert!(
            !app.visual
                .as_ref()
                .expect("visual")
                .scene
                .nodes
                .iter()
                .any(|node| node.origin == created_node_id),
            "Scene removes the undone created TextBox"
        );
    }

    harness
        .get_all_by_label("Redo")
        .next()
        .expect("Redo created TextBox")
        .click();
    harness.step();
    {
        let app = harness.state();
        let editor = app.editor.as_ref().expect("editor");
        assert!(editor.graph().nodes.contains_key(&created_node_id));
        assert_eq!(editor.graph().stories[&created_story_id].text, "");
        assert!(
            app.visual
                .as_ref()
                .expect("visual")
                .scene
                .nodes
                .iter()
                .any(|node| node.origin == created_node_id),
            "Redo restores the same TextBox identity in Scene"
        );
    }

    harness
        .get_all_by_label("Redo")
        .next()
        .expect("Redo created Story text")
        .click();
    harness.step();
    assert_eq!(
        harness
            .state()
            .editor
            .as_ref()
            .expect("editor")
            .graph()
            .stories[&created_story_id]
            .text,
        "Hello"
    );

    harness.get_by_label("Save Project").click();
    harness.step();
    harness.step();
    harness.step();
    {
        let reopen = harness.get_by_label("Reopen Project");
        assert!(!reopen.is_disabled(), "saved TextBox project can reopen");
        reopen.click();
    }
    harness.step();
    harness.step();
    {
        let app = harness.state();
        let editor = app.editor.as_ref().expect("fresh reopened editor");
        assert!(editor.graph().nodes.contains_key(&created_node_id));
        assert_eq!(
            editor.graph().stories[&created_story_id].text,
            "Hello",
            "fresh replay restores the same created Story identity and content"
        );
        assert!(
            app.visual
                .as_ref()
                .expect("visual")
                .scene
                .nodes
                .iter()
                .any(|node| node.origin == created_node_id),
            "fresh replay restores created TextBox Scene visibility"
        );
    }

    assert_eq!(
        fs::read(&fixture).expect("re-read source PUB"),
        original,
        "TextBox authoring must not mutate source PUB bytes"
    );
    let _ = fs::remove_dir_all(root);
}
