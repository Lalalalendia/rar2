//! Real-PUB egui acceptance for Chaptera authored-lane Arrange.

use crate::*;

#[cfg(not(feature = "reader-only"))]
#[test]
#[ignore = "real GUI acceptance requires pinned public CHAPTERA_SAMPLE_NEWSLETTER"]
fn gui_authored_arrange_v1_select_reorder_undo_redo_save_reopen() {
    use egui_kittest::{Harness, kittest::Queryable};

    let fixture = std::env::var_os("CHAPTERA_SAMPLE_NEWSLETTER")
        .map(PathBuf::from)
        .expect("pinned public Apache POI sample needed");
    let original = fs::read(&fixture).expect("read public sample");
    let dir = std::env::temp_dir().join(format!(
        "chaptera-authored-arrange-ui-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("private isolated GUI directory");
    let source = dir.join("SampleNewsletter.pub");
    fs::write(&source, &original).expect("private disposable PUB copy");
    let source_for_app = source.clone();

    let mut harness = Harness::builder()
        .with_size(egui::vec2(1720.0, 1160.0))
        .with_pixels_per_point(1.0)
        .with_max_steps(40)
        .build_eframe(move |cc| {
            fallback_font::install(&cc.egui_ctx).expect("bundled fallback font");
            ViewerApp::new_with_storage(Some(source_for_app), cc.storage)
        });
    harness.step();
    harness.step();

    let (page_id, bounds) = {
        let app = harness.state();
        let visual = app.visual.as_ref().expect("Viewer opens pinned public PUB");
        let page = visual.document.pages.first().expect("first page");
        let surface = visual
            .scene
            .surfaces
            .iter()
            .find(|surface| surface.origin == page.id)
            .expect("first page surface");
        let width = (surface.size.width.get() / 6).max(127_000);
        let height = (surface.size.height.get() / 7).max(127_000);
        (
            page.id,
            pub_editor::RectEmu::new(
                pub_editor::LengthEmu::new(surface.size.width.get() / 3),
                pub_editor::LengthEmu::new(surface.size.height.get() / 3),
                pub_editor::LengthEmu::new(width),
                pub_editor::LengthEmu::new(height),
            ),
        )
    };
    let ids = (0..3)
        .map(|_| pub_editor::NodeId::from_canonical(pub_model::new_editor_canonical_id()))
        .collect::<Vec<_>>();
    {
        let app = harness.state_mut();
        app.selected_page = 0;
        app.zoom_mode = CanvasZoomMode::FitPage;
        for node_id in &ids {
            app.editor
                .as_mut()
                .expect("editor session")
                .create_shape(
                    *node_id,
                    page_id,
                    bounds,
                    rectangle_creation::chaptera_rectangle_paint_v1(),
                )
                .expect("one canonical authored Rectangle");
        }
        app.finish_authoring_change("Created three overlapping authored Rectangles.");
        assert_eq!(
            app.editor
                .as_ref()
                .unwrap()
                .authored_stack(page_id)
                .unwrap()
                .members,
            ids
        );
    }
    harness.step();
    harness.step();

    let point = pub_interaction::DocumentPoint::new(
        pub_editor::LengthEmu::new(bounds.x.get() + bounds.width.get() / 2),
        pub_editor::LengthEmu::new(bounds.y.get() + bounds.height.get() / 2),
    );
    let click_screen = {
        let canvas = harness
            .get_by_label("Document canvas")
            .raw_bounds()
            .expect("actual egui canvas");
        let app = harness.state();
        let visual = app.visual.as_ref().unwrap();
        let surface = visual
            .scene
            .surfaces
            .iter()
            .find(|s| s.origin == page_id)
            .expect("page surface");
        let viewport = egui::vec2(
            (canvas.x1 - canvas.x0) as f32,
            (canvas.y1 - canvas.y0) as f32,
        );
        let scale = fitted_scale(surface.size.width.get(), surface.size.height.get(), viewport)
            .expect("fit page");
        let width = surface.size.width.get() as f32 * scale;
        let height = surface.size.height.get() as f32 * scale;
        egui::pos2(
            ((canvas.x0 + canvas.x1) as f32 - width) / 2.0 + point.x.get() as f32 * scale,
            ((canvas.y0 + canvas.y1) as f32 - height) / 2.0 + point.y.get() as f32 * scale,
        )
    };
    assert_eq!(
        harness
            .state_mut()
            .page_frame_work()
            .unwrap()
            .hit_index
            .topmost_at(point)
            .unwrap()
            .node_id,
        ids[2]
    );

    // Actual pointer input, not injected selection state.
    harness.input_mut().events.extend([
        egui::Event::PointerMoved(click_screen),
        egui::Event::PointerButton {
            pos: click_screen,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::default(),
        },
    ]);
    harness.step();
    harness.input_mut().events.push(egui::Event::PointerButton {
        pos: click_screen,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::default(),
    });
    harness.step();
    harness.step();
    let original_instance = direct_page_local_instance_v1(
        &ids[2].as_canonical().to_string(),
        &page_id.as_canonical().to_string(),
    )
    .expect("selected authored scene identity")
    .instance_id;
    assert_eq!(
        harness.state().canvas_selection.primary(),
        Some(original_instance.as_str())
    );

    for (command, position, top) in [
        ("Send Backward", [ids[0], ids[2], ids[1]], ids[1]),
        ("Send to Back", [ids[2], ids[0], ids[1]], ids[1]),
        ("Bring Forward", [ids[0], ids[2], ids[1]], ids[1]),
        ("Bring to Front", [ids[0], ids[1], ids[2]], ids[2]),
    ] {
        let operations_before = harness
            .state()
            .editor
            .as_ref()
            .unwrap()
            .operations()
            .len();
        harness.get_by_label(command).click();
        harness.step();
        let expected = position.to_vec();
        let app = harness.state_mut();
        let editor = app.editor.as_ref().unwrap();
        assert_eq!(editor.operations().len(), operations_before + 1, "{command}");
        assert!(matches!(
            editor.operations().last(),
            Some(pub_editor::EditOperation::ReorderAuthoredStack { transition })
                if transition.node_id == ids[2] && transition.page_id == page_id
        ));
        assert_eq!(
            editor.authored_stack(page_id).unwrap().members,
            expected,
            "{command}"
        );
        assert_eq!(
            app.canvas_selection.primary(),
            Some(original_instance.as_str())
        );
        assert_eq!(
            app.page_frame_work()
                .unwrap()
                .hit_index
                .topmost_at(point)
                .unwrap()
                .node_id,
            top,
            "paint/hit order must refresh on {command}"
        );
    }
    let expected = ids.clone();
    let operation_count = harness
        .state()
        .editor
        .as_ref()
        .unwrap()
        .operations()
        .len();
    assert!(
        harness
            .state_mut()
            .arrange_selected_authored_shape_v1(
                pub_editor::AuthoredStackReorderModeV1::StepForward
            )
            .is_err()
    );
    assert_eq!(
        harness.state().editor.as_ref().unwrap().operations().len(),
        operation_count
    );

    harness.get_by_label("Undo").click();
    harness.step();
    assert_eq!(
        harness
            .state()
            .editor
            .as_ref()
            .unwrap()
            .authored_stack(page_id)
            .unwrap()
            .members,
        vec![ids[0], ids[2], ids[1]]
    );
    harness.get_by_label("Redo").click();
    harness.step();
    assert_eq!(
        harness
            .state()
            .editor
            .as_ref()
            .unwrap()
            .authored_stack(page_id)
            .unwrap()
            .members,
        expected
    );

    harness.get_by_label("Save Project").click();
    harness.step();
    harness.get_by_label("Reopen Project").click();
    harness.step();
    assert_eq!(
        harness
            .state()
            .editor
            .as_ref()
            .unwrap()
            .authored_stack(page_id)
            .unwrap()
            .members,
        expected,
        "saved authored order must survive fresh project replay"
    );
    assert_eq!(fs::read(&source).unwrap(), original, "never rewrite imported PUB bytes");
    let _ = fs::remove_dir_all(dir);
}
