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

#[cfg(not(feature = "reader-only"))]
#[test]
#[ignore = "runtime GUI evidence requires pinned CHAPTERA_SAMPLE_NEWSLETTER"]
fn gui_move_page_command_commits_one_reorder_and_replays_on_real_pub() {
    use egui_kittest::{Harness, kittest::Queryable};

    let fixture_source = std::env::var_os("CHAPTERA_SAMPLE_NEWSLETTER")
        .map(PathBuf::from)
        .expect("CHAPTERA_SAMPLE_NEWSLETTER must point to the pinned Apache POI fixture");
    let original = fs::read(&fixture_source).expect("read pinned SampleNewsletter fixture");
    let root =
        std::env::temp_dir().join(format!("chaptera-gui-page-reorder-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("create page-reorder GUI temp directory");
    let fixture = root.join("SampleNewsletter.pub");
    fs::write(&fixture, &original).expect("write page-reorder GUI PUB fixture");

    let fixture_for_app = fixture.clone();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1280.0, 820.0))
        .with_pixels_per_point(1.0)
        .with_max_steps(60)
        .build_eframe(move |cc| {
            fallback_font::install(&cc.egui_ctx)
                .expect("pinned Chaptera fallback font resource must validate");
            ViewerApp::new_with_storage(Some(fixture_for_app), cc.storage)
        });
    harness.step();
    harness.step();

    let (original_order, operations_before) = {
        let app = harness.state();
        let visual = app.visual.as_ref().expect("visual loaded");
        assert!(
            visual.document.pages.len() >= 2,
            "fixture must expose at least two customer-visible pages"
        );
        (
            visual
                .document
                .pages
                .iter()
                .map(|page| page.id)
                .collect::<Vec<_>>(),
            app.editor
                .as_ref()
                .expect("editor loaded")
                .operations()
                .len(),
        )
    };
    let moved_page_id = original_order[1];

    harness.get_by_label("Page 2").click();
    harness.step();
    assert_eq!(
        harness
            .state()
            .visual
            .as_ref()
            .expect("visual")
            .document
            .pages[harness.state().selected_page]
            .id,
        moved_page_id,
        "navigation selects the second stable PageId before reorder"
    );

    harness.get_by_label("Move Page Up").click();
    harness.step();
    harness.step();

    let reordered = {
        let app = harness.state();
        let editor = app.editor.as_ref().expect("editor");
        assert_eq!(
            editor.operations().len(),
            operations_before + 1,
            "one Move Page click must append exactly one document operation"
        );
        assert!(matches!(
            editor.operations().last(),
            Some(pub_editor::EditOperation::ReorderPagesV1 { .. })
        ));
        let visual = app.visual.as_ref().expect("visual");
        let page_ids = visual
            .document
            .pages
            .iter()
            .map(|page| page.id)
            .collect::<Vec<_>>();
        assert_eq!(page_ids[0], moved_page_id);
        assert_eq!(page_ids[1], original_order[0]);
        assert_eq!(
            visual.document.pages[app.selected_page].id, moved_page_id,
            "selected PageId must survive its index change"
        );
        assert_eq!(app.selected_page, 0);
        assert_eq!(
            visual
                .document
                .pages
                .iter()
                .map(|page| page.index)
                .collect::<Vec<_>>(),
            (1..=u32::try_from(page_ids.len()).expect("page count fits u32")).collect::<Vec<_>>(),
            "Viewer page numbers follow canonical publication order"
        );
        assert_eq!(
            editor
                .current_qualified_page_order_v1(&page_ids)
                .expect("canonical current customer-page order"),
            page_ids
        );
        page_ids
    };

    assert!(
        harness.get_by_label("Move Page Up").is_disabled(),
        "first canonical page cannot be moved further up"
    );

    harness
        .get_all_by_label("Undo")
        .next()
        .expect("Undo page reorder")
        .click();
    harness.step();
    {
        let app = harness.state();
        assert_eq!(
            app.editor.as_ref().expect("editor").operations().len(),
            operations_before
        );
        assert_eq!(
            app.visual
                .as_ref()
                .expect("visual")
                .document
                .pages
                .iter()
                .map(|page| page.id)
                .collect::<Vec<_>>(),
            original_order,
            "Undo restores exact publication order"
        );
        assert_eq!(
            app.visual.as_ref().expect("visual").document.pages[app.selected_page].id,
            moved_page_id,
            "Undo preserves selected PageId while its index moves back"
        );
    }

    harness
        .get_all_by_label("Redo")
        .next()
        .expect("Redo page reorder")
        .click();
    harness.step();
    {
        let app = harness.state();
        assert_eq!(
            app.editor.as_ref().expect("editor").operations().len(),
            operations_before + 1
        );
        assert_eq!(
            app.visual
                .as_ref()
                .expect("visual")
                .document
                .pages
                .iter()
                .map(|page| page.id)
                .collect::<Vec<_>>(),
            reordered,
            "Redo restores exact reordered publication order"
        );
        assert_eq!(
            app.visual.as_ref().expect("visual").document.pages[app.selected_page].id,
            moved_page_id
        );
    }

    harness.get_by_label("Save Project").click();
    harness.step();
    harness.step();
    harness.step();
    {
        let reopen = harness.get_by_label("Reopen Project");
        assert!(!reopen.is_disabled(), "saved page-order project can reopen");
        reopen.click();
    }
    harness.step();
    harness.step();
    {
        let app = harness.state();
        assert_eq!(
            app.editor
                .as_ref()
                .expect("fresh reopened editor")
                .operations()
                .len(),
            operations_before + 1
        );
        assert_eq!(
            app.visual
                .as_ref()
                .expect("fresh reopened visual")
                .document
                .pages
                .iter()
                .map(|page| page.id)
                .collect::<Vec<_>>(),
            reordered,
            "fresh source reopen plus EditorProject replay restores page order"
        );
    }

    assert_eq!(
        fs::read(&fixture).expect("re-read source PUB"),
        original,
        "page reorder must never mutate source PUB bytes"
    );
    let _ = fs::remove_dir_all(root);
}
