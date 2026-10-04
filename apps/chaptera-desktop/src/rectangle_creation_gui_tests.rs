//! Pinned real-PUB GUI acceptance for the Rectangle creation shell.

use super::*;
use std::fs;
use std::path::PathBuf;

#[cfg(not(feature = "reader-only"))]
#[test]
#[ignore = "runtime GUI evidence requires pinned CHAPTERA_SAMPLE_NEWSLETTER"]
fn gui_rectangle_tool_creates_one_shape_and_selects_it() {
    use egui_kittest::{Harness, kittest::Queryable};

    let fixture_source = std::env::var_os("CHAPTERA_SAMPLE_NEWSLETTER")
        .map(PathBuf::from)
        .expect("CHAPTERA_SAMPLE_NEWSLETTER must point to the pinned Apache POI fixture");
    let original = fs::read(&fixture_source).expect("read pinned SampleNewsletter fixture");
    let root = std::env::temp_dir().join(format!(
        "chaptera-gui-rectangle-create-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("create Rectangle GUI temp directory");
    let fixture = root.join("SampleNewsletter.pub");
    fs::write(&fixture, &original).expect("write Rectangle GUI PUB fixture");

    let fixture_for_app = fixture.clone();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1280.0, 820.0))
        .with_pixels_per_point(1.0)
        .with_max_steps(48)
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

    let (drag_start, drag_end, page_id) = {
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
        (
            egui::pos2(
                page_left + doc_start_x as f32 * scene_scale,
                page_top + doc_start_y as f32 * scene_scale,
            ),
            egui::pos2(
                page_left + doc_end_x as f32 * scene_scale,
                page_top + doc_end_y as f32 * scene_scale,
            ),
            page.id,
        )
    };

    harness.get_by_label("Rectangle").click();
    harness.step();
    assert!(harness.state().rectangle_creation.active());

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

    assert_eq!(
        harness
            .state()
            .editor
            .as_ref()
            .expect("editor")
            .operations()
            .len(),
        operations_before,
        "Rectangle preview must remain transient"
    );
    assert!(matches!(
        harness.state().rectangle_creation.preview(),
        Ok(rectangle_creation::RectangleCreatePreviewV1::Bounds(_))
    ));

    harness.input_mut().events.push(egui::Event::PointerButton {
        pos: drag_end,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::default(),
    });
    harness.step();
    harness.step();

    let created_node_id = {
        let app = harness.state();
        let editor = app.editor.as_ref().expect("editor");
        assert_eq!(editor.operations().len(), operations_before + 1);
        let node_id = match editor.operations().last() {
            Some(pub_editor::EditOperation::CreateShape { node_id, page_id: created_page, .. }) => {
                assert_eq!(*created_page, page_id);
                *node_id
            }
            other => panic!("expected one CreateShape operation, got {other:?}"),
        };
        assert!(
            app.visual
                .as_ref()
                .expect("visual")
                .scene
                .nodes
                .iter()
                .any(|node| node.origin == node_id),
            "accepted CreateShape must be materialized in the current Viewer scene"
        );
        let expected_instance = direct_page_local_instance_v1(
            &node_id.as_canonical().to_string(),
            &page_id.as_canonical().to_string(),
        )
        .expect("canonical created Rectangle instance");
        assert_eq!(
            app.canvas_selection.primary(),
            Some(expected_instance.instance_id.as_str()),
            "accepted Rectangle must select its durable direct-page instance"
        );
        assert!(
            !app.rectangle_creation.active(),
            "accepted one-shot Rectangle must return to Select"
        );
        assert!(app.rectangle_creation.gesture_token.is_none());
        node_id
    };

    assert!(
        harness
            .state()
            .editor
            .as_ref()
            .expect("editor")
            .authored_shape(created_node_id)
            .is_some()
    );
    assert_eq!(
        fs::read(&fixture).expect("re-read source PUB"),
        original,
        "Rectangle creation must not mutate source PUB bytes"
    );

    let _ = fs::remove_dir_all(root);
}
