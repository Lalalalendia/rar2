//! Real-PUB GUI acceptance for canonical page navigation shortcuts.

use crate::*;

#[cfg(not(feature = "reader-only"))]
#[test]
#[ignore = "runtime GUI evidence requires pinned CHAPTERA_SAMPLE_NEWSLETTER"]
fn gui_ctrl_page_shortcuts_reuse_canonical_page_navigation_without_revision() {
    use egui_kittest::Harness;

    let fixture_source = std::env::var_os("CHAPTERA_SAMPLE_NEWSLETTER")
        .map(PathBuf::from)
        .expect("CHAPTERA_SAMPLE_NEWSLETTER must point to the pinned Apache POI fixture");
    let original = fs::read(&fixture_source).expect("read pinned SampleNewsletter fixture");
    let root = std::env::temp_dir().join(format!(
        "chaptera-gui-page-nav-shortcuts-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("create page-nav GUI temp directory");
    let fixture = root.join("SampleNewsletter.pub");
    fs::write(&fixture, &original).expect("write page-nav GUI PUB fixture");

    let fixture_for_app = fixture.clone();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1280.0, 820.0))
        .with_pixels_per_point(1.0)
        .with_max_steps(40)
        .build_eframe(move |cc| {
            fallback_font::install(&cc.egui_ctx)
                .expect("pinned Chaptera fallback font resource must validate");
            ViewerApp::new_with_storage(Some(fixture_for_app), cc.storage)
        });
    harness.step();

    let (page_count, operations_before) = {
        let app = harness.state();
        let page_count = app
            .visual
            .as_ref()
            .expect("visual loaded")
            .document
            .pages
            .len();
        assert!(
            page_count >= 2,
            "fixture must expose multiple canonical pages"
        );
        assert_eq!(app.selected_page, 0);
        let operations = app
            .editor
            .as_ref()
            .expect("editor loaded")
            .operations()
            .len();
        (page_count, operations)
    };

    harness
        .state_mut()
        .canvas_selection
        .select_only("synthetic-page-nav-selection".to_owned());
    harness.press_key_modifiers(egui::Modifiers::CTRL, egui::Key::PageUp);
    harness.step();
    assert_eq!(
        harness.state().selected_page,
        0,
        "Ctrl+PageUp on first page must not wrap"
    );
    assert_eq!(
        harness.state().canvas_selection.primary(),
        Some("synthetic-page-nav-selection"),
        "boundary no-op must not invent a page transition"
    );

    harness.press_key_modifiers(egui::Modifiers::CTRL, egui::Key::PageDown);
    harness.step();
    {
        let app = harness.state();
        assert_eq!(app.selected_page, 1, "Ctrl+PageDown moves exactly one page");
        assert_eq!(
            app.canvas_selection.len(),
            0,
            "page change clears selection"
        );
        assert!(app.canvas_drag.is_none());
        assert!(app.canvas_resize.is_none());
        assert_eq!(
            app.editor.as_ref().expect("editor").operations().len(),
            operations_before,
            "page navigation must not append an Editor operation"
        );
    }

    harness.press_key_modifiers(egui::Modifiers::CTRL, egui::Key::PageUp);
    harness.step();
    assert_eq!(
        harness.state().selected_page,
        0,
        "Ctrl+PageUp returns to the previous canonical page"
    );

    {
        let app = harness.state_mut();
        assert!(
            app.navigate_to_page_index(page_count - 1),
            "canonical page authority reaches the last page"
        );
        app.canvas_selection
            .select_only("synthetic-last-page-selection".to_owned());
    }
    harness.step();
    harness.press_key_modifiers(egui::Modifiers::CTRL, egui::Key::PageDown);
    harness.step();
    {
        let app = harness.state();
        assert_eq!(
            app.selected_page,
            page_count - 1,
            "Ctrl+PageDown on last page must not wrap"
        );
        assert_eq!(
            app.canvas_selection.primary(),
            Some("synthetic-last-page-selection"),
            "last-page no-op must not synthesize a navigation reset"
        );
        assert_eq!(
            app.editor.as_ref().expect("editor").operations().len(),
            operations_before
        );
    }

    assert_eq!(
        fs::read(&fixture).expect("re-read source PUB"),
        original,
        "page navigation must not mutate source PUB bytes"
    );
    let _ = fs::remove_dir_all(root);
}
