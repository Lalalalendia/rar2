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

#[cfg(not(feature = "reader-only"))]
#[test]
#[ignore = "runtime GUI evidence requires pinned CHAPTERA_SAMPLE_NEWSLETTER"]
fn gui_add_page_at_end_projects_membership_and_replays_on_real_pub() {
    use egui_kittest::{Harness, kittest::Queryable};

    let fixture_source = std::env::var_os("CHAPTERA_SAMPLE_NEWSLETTER")
        .map(PathBuf::from)
        .expect("CHAPTERA_SAMPLE_NEWSLETTER must point to the pinned Apache POI fixture");
    let original = fs::read(&fixture_source).expect("read pinned SampleNewsletter fixture");
    let root =
        std::env::temp_dir().join(format!("chaptera-gui-page-append-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("create page-append GUI temp directory");
    let fixture = root.join("SampleNewsletter.pub");
    fs::write(&fixture, &original).expect("write page-append GUI PUB fixture");

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
    harness.step();

    let (source_order, last_source_size, operations_before) = {
        let app = harness.state();
        let visual = app.visual.as_ref().expect("visual loaded");
        assert!(
            !visual.document.pages.is_empty(),
            "fixture must expose at least one customer-visible page"
        );
        let source_order = visual
            .document
            .pages
            .iter()
            .map(|page| page.id)
            .collect::<Vec<_>>();
        assert_eq!(
            app.source_customer_page_ids, source_order,
            "Desktop baseline must capture source-qualified membership before lifecycle replay"
        );
        let last_page_id = *source_order.last().expect("last source customer page");
        let last_source_size = app
            .editor
            .as_ref()
            .expect("editor loaded")
            .graph()
            .pages
            .get(&last_page_id)
            .expect("last source page in editor graph")
            .size;
        (
            source_order,
            last_source_size,
            app.editor
                .as_ref()
                .expect("editor loaded")
                .operations()
                .len(),
        )
    };

    let source_hash_before = harness
        .state()
        .visual
        .as_ref()
        .expect("visual loaded")
        .document
        .source
        .source_hash;
    let mismatched_source_hash =
        if source_hash_before == pub_editor::Sha256Digest::from_bytes([0x5a; 32]) {
            pub_editor::Sha256Digest::from_bytes([0xa5; 32])
        } else {
            pub_editor::Sha256Digest::from_bytes([0x5a; 32])
        };
    harness
        .state_mut()
        .visual
        .as_mut()
        .expect("visual loaded")
        .document
        .source
        .source_hash = mismatched_source_hash;

    let rejected_add = harness.get_by_label("Add Page at End");
    assert!(
        !rejected_add.is_disabled(),
        "source-identity mismatch is detected by transactional preflight, not capability gating"
    );
    rejected_add.click();
    harness.step();
    harness.step();
    {
        let app = harness.state();
        assert_eq!(
            app.editor.as_ref().expect("editor").operations().len(),
            operations_before,
            "failed Viewer membership preflight must not commit an Append revision"
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
            source_order,
            "failed Viewer membership preflight must leave visible membership unchanged"
        );
        assert!(
            app.edit_status
                .as_deref()
                .is_some_and(|status| status.contains("before commit")),
            "failed preflight must report the rejection before any durable append"
        );
    }
    harness
        .state_mut()
        .visual
        .as_mut()
        .expect("visual loaded")
        .document
        .source
        .source_hash = source_hash_before;
    harness.step();

    let add = harness.get_by_label("Add Page at End");
    assert!(
        !add.is_disabled(),
        "existing customer document admits append"
    );
    add.click();
    harness.step();
    harness.step();

    let appended_page_id = {
        let app = harness.state();
        let editor = app.editor.as_ref().expect("editor after append");
        assert_eq!(
            editor.operations().len(),
            operations_before + 1,
            "one Add Page click must append exactly one lifecycle operation"
        );
        let appended_page_id = match editor.operations().last() {
            Some(pub_editor::EditOperation::AppendBlankPageV1 { transition }) => {
                transition.identity.page_id
            }
            other => panic!("expected AppendBlankPageV1, got {other:?}"),
        };
        let appended_page = editor
            .graph()
            .pages
            .get(&appended_page_id)
            .expect("appended page in editor graph");
        assert_eq!(appended_page.size, last_source_size);
        assert!(appended_page.children.is_empty());
        assert!(appended_page.extensions.is_empty());

        let visual = app.visual.as_ref().expect("visual after append");
        let mut expected = source_order.clone();
        expected.push(appended_page_id);
        assert_eq!(
            visual
                .document
                .pages
                .iter()
                .map(|page| page.id)
                .collect::<Vec<_>>(),
            expected,
            "Viewer membership must include authored page at publication end"
        );
        assert_eq!(
            visual.document.pages[app.selected_page].id, appended_page_id,
            "accepted Add Page selects the newly-created stable PageId"
        );
        let surface = visual
            .scene
            .surfaces
            .iter()
            .find(|surface| surface.origin == appended_page_id)
            .expect("appended page must have a Viewer scene surface");
        assert_eq!(surface.size, last_source_size);
        assert_eq!(
            visual
                .document
                .pages
                .last()
                .expect("last Viewer page")
                .index,
            u32::try_from(expected.len()).expect("page count fits u32")
        );
        appended_page_id
    };

    let appended_label = format!("Page {}", source_order.len() + 1);
    assert!(
        !harness.get_by_label(&appended_label).is_disabled(),
        "new page is reachable through the real Pages sidebar"
    );

    harness
        .get_all_by_label("Undo")
        .next()
        .expect("Undo page append")
        .click();
    harness.step();
    harness.step();
    {
        let app = harness.state();
        let visual = app.visual.as_ref().expect("visual after undo");
        assert_eq!(
            visual
                .document
                .pages
                .iter()
                .map(|page| page.id)
                .collect::<Vec<_>>(),
            source_order,
            "Undo removes exact authored membership"
        );
        assert!(
            visual
                .scene
                .surfaces
                .iter()
                .all(|surface| surface.origin != appended_page_id),
            "Undo removes the authored page surface"
        );
        assert_eq!(
            visual.document.pages[app.selected_page].id,
            *source_order.last().expect("last source page"),
            "when selected authored page disappears, selection falls back to the surviving neighbor"
        );
        assert_eq!(
            app.editor.as_ref().expect("editor").operations().len(),
            operations_before
        );
    }

    harness
        .get_all_by_label("Redo")
        .next()
        .expect("Redo page append")
        .click();
    harness.step();
    harness.step();
    {
        let app = harness.state();
        let visual = app.visual.as_ref().expect("visual after redo");
        assert_eq!(
            visual
                .document
                .pages
                .last()
                .expect("restored authored page")
                .id,
            appended_page_id,
            "Redo restores the same PageId"
        );
        assert!(
            visual
                .scene
                .surfaces
                .iter()
                .any(|surface| surface.origin == appended_page_id),
            "Redo restores the authored page surface"
        );
        assert_eq!(
            app.editor.as_ref().expect("editor").operations().len(),
            operations_before + 1
        );
    }

    harness.get_by_label(&appended_label).click();
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
        appended_page_id,
        "restored page remains navigable by stable identity"
    );

    harness.get_by_label("Save Project").click();
    harness.step();
    harness.step();
    harness.step();
    {
        let reopen = harness.get_by_label("Reopen Project");
        assert!(!reopen.is_disabled(), "saved AddPage project can reopen");
        reopen.click();
    }
    harness.step();
    harness.step();
    harness.step();

    {
        let app = harness.state();
        let editor = app.editor.as_ref().expect("fresh reopened editor");
        assert_eq!(editor.operations().len(), operations_before + 1);
        assert!(matches!(
            editor.operations().last(),
            Some(pub_editor::EditOperation::AppendBlankPageV1 { transition })
                if transition.identity.page_id == appended_page_id
        ));
        assert_eq!(
            app.source_customer_page_ids, source_order,
            "fresh reopen recovers source-qualified baseline independently of authored lifecycle"
        );

        let visual = app.visual.as_ref().expect("fresh reopened visual");
        let mut expected = source_order.clone();
        expected.push(appended_page_id);
        assert_eq!(
            visual
                .document
                .pages
                .iter()
                .map(|page| page.id)
                .collect::<Vec<_>>(),
            expected,
            "fresh source open plus EditorProject replay restores exact page membership/order"
        );
        assert!(
            visual
                .scene
                .surfaces
                .iter()
                .any(|surface| surface.origin == appended_page_id
                    && surface.size == last_source_size)
        );
    }

    assert_eq!(
        fs::read(&fixture).expect("re-read source PUB"),
        original,
        "Add Page sidecar lifecycle must not mutate source PUB bytes"
    );
    let _ = fs::remove_dir_all(root);
}

#[cfg(not(feature = "reader-only"))]
#[test]
#[ignore = "runtime GUI evidence requires pinned CHAPTERA_SAMPLE_NEWSLETTER"]
fn gui_delete_empty_authored_page_projects_membership_and_replays_on_real_pub() {
    use egui_kittest::{Harness, kittest::Queryable};

    let fixture_source = std::env::var_os("CHAPTERA_SAMPLE_NEWSLETTER")
        .map(PathBuf::from)
        .expect("CHAPTERA_SAMPLE_NEWSLETTER must point to the pinned Apache POI fixture");
    let original = fs::read(&fixture_source).expect("read pinned SampleNewsletter fixture");
    let root =
        std::env::temp_dir().join(format!("chaptera-gui-page-delete-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("create page-delete GUI temp directory");
    let fixture = root.join("SampleNewsletter.pub");
    fs::write(&fixture, &original).expect("write page-delete GUI PUB fixture");

    let fixture_for_app = fixture.clone();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1280.0, 820.0))
        .with_pixels_per_point(1.0)
        .with_max_steps(100)
        .build_eframe(move |cc| {
            fallback_font::install(&cc.egui_ctx)
                .expect("pinned Chaptera fallback font resource must validate");
            ViewerApp::new_with_storage(Some(fixture_for_app), cc.storage)
        });
    harness.step();
    harness.step();

    let (source_order, operations_before) = {
        let app = harness.state();
        let visual = app.visual.as_ref().expect("visual loaded");
        let source_order = visual
            .document
            .pages
            .iter()
            .map(|page| page.id)
            .collect::<Vec<_>>();
        assert!(
            !source_order.is_empty(),
            "fixture must expose source-qualified customer pages"
        );
        assert_eq!(app.source_customer_page_ids, source_order);
        (
            source_order,
            app.editor
                .as_ref()
                .expect("editor loaded")
                .operations()
                .len(),
        )
    };

    assert!(
        harness.get_by_label("Delete Empty Page").is_disabled(),
        "source-backed selected page must not advertise authored-page deletion"
    );

    harness.get_by_label("Add Page at End").click();
    harness.step();
    harness.step();

    let appended_page_id = {
        let app = harness.state();
        let editor = app.editor.as_ref().expect("editor after append");
        assert_eq!(editor.operations().len(), operations_before + 1);
        match editor.operations().last() {
            Some(pub_editor::EditOperation::AppendBlankPageV1 { transition }) => {
                transition.identity.page_id
            }
            other => panic!("expected AppendBlankPageV1, got {other:?}"),
        }
    };
    assert!(
        !harness.get_by_label("Delete Empty Page").is_disabled(),
        "selected authored blank page must expose bounded delete"
    );

    // A real canonical authored Rectangle makes the selected page nonblank.
    // The button must be disabled before click, not only fail closed at commit.
    {
        let app = harness.state_mut();
        let size = app
            .editor
            .as_ref()
            .expect("editor")
            .graph()
            .pages
            .get(&appended_page_id)
            .expect("authored page")
            .size;
        let shape_id = pub_editor::NodeId::from_canonical(pub_model::new_editor_canonical_id());
        app.editor
            .as_mut()
            .expect("editor")
            .create_shape(
                shape_id,
                appended_page_id,
                pub_editor::RectEmu::new(
                    pub_editor::LengthEmu::new(size.width.get() / 5),
                    pub_editor::LengthEmu::new(size.height.get() / 5),
                    pub_editor::LengthEmu::new(size.width.get() / 4),
                    pub_editor::LengthEmu::new(size.height.get() / 4),
                ),
                rectangle_creation::chaptera_rectangle_paint_v1(),
            )
            .expect("seed content-bearing authored Rectangle");
        app.finish_authoring_change("Seeded canonical Rectangle for Delete Empty Page negative.");
    }
    harness.step();
    assert!(
        harness.get_by_label("Delete Empty Page").is_disabled(),
        "content-bearing authored page must not advertise Delete Empty Page"
    );
    assert_eq!(
        harness
            .state()
            .editor
            .as_ref()
            .expect("editor")
            .operations()
            .len(),
        operations_before + 2,
        "disabled Delete Empty Page must not create any additional revision"
    );

    harness
        .get_all_by_label("Undo")
        .next()
        .expect("Undo canonical Rectangle")
        .click();
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
        operations_before + 1,
        "Undo of rectangle restores exactly the original blank authored page"
    );
    assert!(
        !harness.get_by_label("Delete Empty Page").is_disabled(),
        "deletion becomes available only after canonical authored content is removed"
    );

    let source_hash_before = harness
        .state()
        .visual
        .as_ref()
        .expect("visual loaded")
        .document
        .source
        .source_hash;
    let mismatched_source_hash =
        if source_hash_before == pub_editor::Sha256Digest::from_bytes([0x5a; 32]) {
            pub_editor::Sha256Digest::from_bytes([0xa5; 32])
        } else {
            pub_editor::Sha256Digest::from_bytes([0x5a; 32])
        };
    harness
        .state_mut()
        .visual
        .as_mut()
        .expect("visual loaded")
        .document
        .source
        .source_hash = mismatched_source_hash;

    harness.get_by_label("Delete Empty Page").click();
    harness.step();
    harness.step();
    {
        let app = harness.state();
        assert_eq!(
            app.editor.as_ref().expect("editor").operations().len(),
            operations_before + 1,
            "failed Viewer preflight must not commit a Delete revision"
        );
        assert!(
            app.visual
                .as_ref()
                .expect("visual")
                .document
                .pages
                .iter()
                .any(|page| page.id == appended_page_id),
            "failed Viewer preflight must leave visible membership unchanged"
        );
        assert!(
            app.edit_status
                .as_deref()
                .is_some_and(|status| status.contains("before commit")),
            "failed preflight must report rejection before durable delete"
        );
    }

    harness
        .state_mut()
        .visual
        .as_mut()
        .expect("visual loaded")
        .document
        .source
        .source_hash = source_hash_before;
    harness.step();

    harness.get_by_label("Delete Empty Page").click();
    harness.step();
    harness.step();

    {
        let app = harness.state();
        let editor = app.editor.as_ref().expect("editor after delete");
        assert_eq!(
            editor.operations().len(),
            operations_before + 2,
            "one Delete Empty Page click must append exactly one lifecycle operation"
        );
        assert!(matches!(
            editor.operations().last(),
            Some(pub_editor::EditOperation::DeleteBlankAuthoredPageV1 { transition })
                if transition.identity.page_id == appended_page_id
        ));

        let visual = app.visual.as_ref().expect("visual after delete");
        assert_eq!(
            visual
                .document
                .pages
                .iter()
                .map(|page| page.id)
                .collect::<Vec<_>>(),
            source_order,
            "accepted Delete must restore exact source-visible membership"
        );
        assert!(
            visual
                .scene
                .surfaces
                .iter()
                .all(|surface| surface.origin != appended_page_id),
            "deleted page surface must disappear"
        );
        assert_eq!(
            visual.document.pages[app.selected_page].id,
            *source_order.last().expect("last surviving source page"),
            "direct Delete must focus the previous surviving page"
        );
    }
    assert!(
        harness.get_by_label("Delete Empty Page").is_disabled(),
        "source-backed fallback page must not expose authored-page deletion"
    );

    harness
        .get_all_by_label("Undo")
        .next()
        .expect("Undo page delete")
        .click();
    harness.step();
    harness.step();
    {
        let app = harness.state();
        let visual = app.visual.as_ref().expect("visual after undo");
        assert_eq!(
            app.editor.as_ref().expect("editor").operations().len(),
            operations_before + 1
        );
        assert_eq!(
            visual
                .document
                .pages
                .last()
                .expect("restored authored page")
                .id,
            appended_page_id,
            "Undo restores exact authored PageId"
        );
        assert!(
            visual
                .scene
                .surfaces
                .iter()
                .any(|surface| surface.origin == appended_page_id),
            "Undo restores the authored page surface"
        );
    }

    let appended_label = format!("Page {}", source_order.len() + 1);
    harness.get_by_label(&appended_label).click();
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
        appended_page_id
    );

    harness
        .get_all_by_label("Redo")
        .next()
        .expect("Redo page delete")
        .click();
    harness.step();
    harness.step();
    {
        let app = harness.state();
        let visual = app.visual.as_ref().expect("visual after redo");
        assert_eq!(
            app.editor.as_ref().expect("editor").operations().len(),
            operations_before + 2
        );
        assert_eq!(
            visual
                .document
                .pages
                .iter()
                .map(|page| page.id)
                .collect::<Vec<_>>(),
            source_order,
            "Redo removes the same authored membership again"
        );
        assert!(
            visual
                .scene
                .surfaces
                .iter()
                .all(|surface| surface.origin != appended_page_id)
        );
        assert_eq!(
            visual.document.pages[app.selected_page].id,
            *source_order.last().expect("last surviving source page")
        );
    }

    harness.get_by_label("Save Project").click();
    harness.step();
    harness.step();
    harness.step();
    {
        let reopen = harness.get_by_label("Reopen Project");
        assert!(!reopen.is_disabled(), "saved DeletePage project can reopen");
        reopen.click();
    }
    harness.step();
    harness.step();
    harness.step();

    {
        let app = harness.state();
        let editor = app.editor.as_ref().expect("fresh reopened editor");
        assert_eq!(editor.operations().len(), operations_before + 2);
        assert!(matches!(
            editor.operations().last(),
            Some(pub_editor::EditOperation::DeleteBlankAuthoredPageV1 { transition })
                if transition.identity.page_id == appended_page_id
        ));
        assert_eq!(
            app.source_customer_page_ids, source_order,
            "fresh reopen must recover the source-qualified baseline independently"
        );

        let visual = app.visual.as_ref().expect("fresh reopened visual");
        assert_eq!(
            visual
                .document
                .pages
                .iter()
                .map(|page| page.id)
                .collect::<Vec<_>>(),
            source_order,
            "fresh source open plus sidecar replay preserves deletion"
        );
        assert!(
            visual
                .scene
                .surfaces
                .iter()
                .all(|surface| surface.origin != appended_page_id)
        );
    }

    assert_eq!(
        fs::read(&fixture).expect("re-read source PUB"),
        original,
        "Delete Empty Page sidecar lifecycle must not mutate source PUB bytes"
    );
    let _ = fs::remove_dir_all(root);
}

#[cfg(not(feature = "reader-only"))]
#[test]
#[ignore = "runtime GUI evidence requires pinned CHAPTERA_SAMPLE_NEWSLETTER"]
fn gui_duplicate_blank_page_projects_membership_and_replays_on_real_pub() {
    use egui_kittest::{Harness, kittest::Queryable};

    let fixture_source = std::env::var_os("CHAPTERA_SAMPLE_NEWSLETTER")
        .map(PathBuf::from)
        .expect("CHAPTERA_SAMPLE_NEWSLETTER must point to the pinned Apache POI fixture");
    let original = fs::read(&fixture_source).expect("read pinned SampleNewsletter fixture");
    let root = std::env::temp_dir().join(format!(
        "chaptera-gui-page-duplicate-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("create page-duplicate GUI temp directory");
    let fixture = root.join("SampleNewsletter.pub");
    fs::write(&fixture, &original).expect("write page-duplicate GUI PUB fixture");

    let fixture_for_app = fixture.clone();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1280.0, 820.0))
        .with_pixels_per_point(1.0)
        .with_max_steps(120)
        .build_eframe(move |cc| {
            fallback_font::install(&cc.egui_ctx)
                .expect("pinned Chaptera fallback font resource must validate");
            ViewerApp::new_with_storage(Some(fixture_for_app), cc.storage)
        });
    harness.step();
    harness.step();

    let (source_order, operations_before) = {
        let app = harness.state();
        let visual = app.visual.as_ref().expect("visual loaded");
        let source_order = visual
            .document
            .pages
            .iter()
            .map(|page| page.id)
            .collect::<Vec<_>>();
        assert!(!source_order.is_empty(), "fixture must have customer pages");
        assert_eq!(app.source_customer_page_ids, source_order);
        (
            source_order,
            app.editor
                .as_ref()
                .expect("editor loaded")
                .operations()
                .len(),
        )
    };

    harness.get_by_label("Add Page at End").click();
    harness.step();
    harness.step();
    let (source_page_id, source_page_before) = {
        let app = harness.state();
        let editor = app.editor.as_ref().expect("editor after Add Page");
        let source_id = match editor.operations().last() {
            Some(pub_editor::EditOperation::AppendBlankPageV1 { transition }) => {
                transition.identity.page_id
            }
            other => panic!("expected canonical AppendBlankPageV1, got {other:?}"),
        };
        let page = editor.graph().pages[&source_id].clone();
        (source_id, page)
    };
    assert!(
        !harness.get_by_label("Duplicate Blank Page").is_disabled(),
        "new authored blank page must admit duplication"
    );

    // Seed real authored content; empty-page capability must reject before click.
    {
        let app = harness.state_mut();
        let size = source_page_before.size;
        let node_id = pub_editor::NodeId::from_canonical(pub_model::new_editor_canonical_id());
        app.editor
            .as_mut()
            .expect("editor")
            .create_shape(
                node_id,
                source_page_id,
                pub_editor::RectEmu::new(
                    pub_editor::LengthEmu::new(size.width.get() / 5),
                    pub_editor::LengthEmu::new(size.height.get() / 5),
                    pub_editor::LengthEmu::new(size.width.get() / 4),
                    pub_editor::LengthEmu::new(size.height.get() / 4),
                ),
                rectangle_creation::chaptera_rectangle_paint_v1(),
            )
            .expect("create canonical Rectangle");
        app.finish_authoring_change("Seeded authored content for DuplicateBlank negative.");
    }
    harness.step();
    assert!(
        harness.get_by_label("Duplicate Blank Page").is_disabled(),
        "content-bearing Page must not advertise Duplicate Blank Page"
    );
    assert_eq!(
        harness
            .state()
            .editor
            .as_ref()
            .expect("editor")
            .operations()
            .len(),
        operations_before + 2
    );

    harness
        .get_all_by_label("Undo")
        .next()
        .expect("Undo canonical Rectangle")
        .click();
    harness.step();
    harness.step();
    assert!(
        !harness.get_by_label("Duplicate Blank Page").is_disabled(),
        "Undo of authored content restores DuplicateBlank admission"
    );

    let source_hash_before = harness
        .state()
        .visual
        .as_ref()
        .expect("visual loaded")
        .document
        .source
        .source_hash;
    let mismatched_source_hash =
        if source_hash_before == pub_editor::Sha256Digest::from_bytes([0x5a; 32]) {
            pub_editor::Sha256Digest::from_bytes([0xa5; 32])
        } else {
            pub_editor::Sha256Digest::from_bytes([0x5a; 32])
        };
    harness
        .state_mut()
        .visual
        .as_mut()
        .expect("visual")
        .document
        .source
        .source_hash = mismatched_source_hash;
    harness.get_by_label("Duplicate Blank Page").click();
    harness.step();
    harness.step();
    {
        let app = harness.state();
        let visual = app
            .visual
            .as_ref()
            .expect("visual after rejected DuplicateBlank");
        assert_eq!(
            app.editor.as_ref().expect("editor").operations().len(),
            operations_before + 1,
            "failed Viewer preflight must consume zero Duplicate revisions"
        );
        let mut expected_order = source_order.clone();
        expected_order.push(source_page_id);
        assert_eq!(
            visual
                .document
                .pages
                .iter()
                .map(|page| page.id)
                .collect::<Vec<_>>(),
            expected_order,
            "failed preflight must not alter Viewer membership"
        );
        assert!(
            app.edit_status
                .as_deref()
                .is_some_and(|status| status.contains("before commit")),
            "failed projection must report a transaction rejection"
        );
    }
    harness
        .state_mut()
        .visual
        .as_mut()
        .expect("visual")
        .document
        .source
        .source_hash = source_hash_before;
    harness.step();

    harness.get_by_label("Duplicate Blank Page").click();
    harness.step();
    harness.step();
    let duplicate_id = {
        let app = harness.state();
        let editor = app.editor.as_ref().expect("editor after duplicate");
        assert_eq!(editor.operations().len(), operations_before + 2);
        let duplicate_id = match editor.operations().last() {
            Some(pub_editor::EditOperation::DuplicateBlankPageV1 { transition }) => {
                assert_eq!(transition.source_page_id, source_page_id);
                assert_ne!(transition.destination_identity.page_id, source_page_id);
                transition.destination_identity.page_id
            }
            other => panic!("expected exactly one DuplicateBlankPageV1, got {other:?}"),
        };
        let source = editor
            .graph()
            .pages
            .get(&source_page_id)
            .expect("source page");
        let duplicate = editor
            .graph()
            .pages
            .get(&duplicate_id)
            .expect("duplicate page");
        assert_eq!(
            source, &source_page_before,
            "source Page state must remain exact"
        );
        assert_eq!(duplicate.size, source.size);
        assert_eq!(duplicate.bleed, source.bleed);
        assert_eq!(duplicate.margins, source.margins);
        assert!(duplicate.children.is_empty() && duplicate.extensions.is_empty());
        let mut expected_order = source_order.clone();
        expected_order.push(source_page_id);
        expected_order.push(duplicate_id);
        assert_eq!(
            editor
                .effective_customer_page_order_v1(&source_order)
                .expect("canonical customer order"),
            expected_order
        );
        let visual = app.visual.as_ref().expect("visual after duplicate");
        assert_eq!(
            visual
                .document
                .pages
                .iter()
                .map(|page| page.id)
                .collect::<Vec<_>>(),
            expected_order
        );
        assert_eq!(
            visual.document.pages[app.selected_page].id, duplicate_id,
            "accepted duplicate must select exact new PageId"
        );
        assert!(
            visual
                .scene
                .surfaces
                .iter()
                .any(|surface| surface.origin == duplicate_id),
            "new blank page must have a Viewer surface"
        );
        duplicate_id
    };
    assert!(
        !harness.get_by_label("Delete Empty Page").is_disabled(),
        "newly duplicated blank page must be deletable"
    );

    harness
        .get_all_by_label("Undo")
        .next()
        .expect("Undo duplicate")
        .click();
    harness.step();
    harness.step();
    {
        let app = harness.state();
        assert_eq!(
            app.editor.as_ref().expect("editor").operations().len(),
            operations_before + 1
        );
        assert!(
            app.visual
                .as_ref()
                .expect("visual")
                .document
                .pages
                .iter()
                .all(|page| page.id != duplicate_id),
            "Undo removes exactly the duplicated membership"
        );
    }
    harness
        .get_all_by_label("Redo")
        .next()
        .expect("Redo duplicate")
        .click();
    harness.step();
    harness.step();
    {
        let app = harness.state();
        assert_eq!(
            app.editor.as_ref().expect("editor").operations().len(),
            operations_before + 2
        );
        assert!(
            app.visual
                .as_ref()
                .expect("visual")
                .document
                .pages
                .iter()
                .any(|page| page.id == duplicate_id),
            "Redo restores the same PageId"
        );
    }

    harness.get_by_label("Save Project").click();
    harness.step();
    harness.step();
    harness.step();
    harness.get_by_label("Reopen Project").click();
    harness.step();
    harness.step();
    harness.step();
    {
        let app = harness.state();
        let editor = app.editor.as_ref().expect("fresh reopened editor");
        assert_eq!(app.source_customer_page_ids, source_order);
        assert_eq!(editor.operations().len(), operations_before + 2);
        assert!(matches!(
            editor.operations().last(),
            Some(pub_editor::EditOperation::DuplicateBlankPageV1 { transition })
                if transition.destination_identity.page_id == duplicate_id
        ));
        let mut expected_order = source_order.clone();
        expected_order.push(source_page_id);
        expected_order.push(duplicate_id);
        assert_eq!(
            app.visual
                .as_ref()
                .expect("fresh reopened Viewer")
                .document
                .pages
                .iter()
                .map(|page| page.id)
                .collect::<Vec<_>>(),
            expected_order
        );
    }
    // Page thumbnails beyond the visible sidebar viewport are not a reliable
    // pointer target in a headless GUI. Navigate through real page shortcuts,
    // then assert the exact canonical PageId before testing Delete.
    {
        let app = harness.state();
        let editor = app.editor.as_ref().expect("reopened Editor");
        let source_customer_page_ids = &app.source_customer_page_ids;
        let mut delete_candidate = editor.clone();
        let delete_rejection = delete_candidate
            .delete_blank_authored_page_v1(source_customer_page_ids.clone(), duplicate_id)
            .err();
        assert!(
            editor.can_delete_blank_authored_page_v1(source_customer_page_ids, duplicate_id),
            "core DeleteBlank admission rejects replayed duplicate: {delete_rejection:?}"
        );
    }
    let (current_index, target_index) = {
        let app = harness.state();
        let pages = &app.visual.as_ref().expect("reopened Viewer").document.pages;
        let target = pages
            .iter()
            .position(|page| page.id == duplicate_id)
            .expect("replayed destination remains in Viewer membership");
        (app.selected_page, target)
    };
    if current_index < target_index {
        for _ in current_index..target_index {
            harness.press_key_modifiers(egui::Modifiers::CTRL, egui::Key::PageDown);
            harness.step();
        }
    } else {
        for _ in target_index..current_index {
            harness.press_key_modifiers(egui::Modifiers::CTRL, egui::Key::PageUp);
            harness.step();
        }
    }
    assert_eq!(
        harness
            .state()
            .visual
            .as_ref()
            .expect("visual after Page navigation")
            .document
            .pages[harness.state().selected_page]
            .id,
        duplicate_id,
        "GUI must focus duplicated PageId before DeleteBlank command"
    );
    assert!(
        !harness.get_by_label("Delete Empty Page").is_disabled(),
        "duplicate must participate in merged DeleteBlank lifecycle"
    );
    harness.get_by_label("Delete Empty Page").click();
    harness.step();
    harness.step();
    {
        let app = harness.state();
        let editor = app
            .editor
            .as_ref()
            .expect("editor after deleting duplicate");
        assert_eq!(editor.operations().len(), operations_before + 3);
        assert!(matches!(
            editor.operations().last(),
            Some(pub_editor::EditOperation::DeleteBlankAuthoredPageV1 { transition })
                if transition.identity.page_id == duplicate_id
        ));
        assert!(
            app.visual
                .as_ref()
                .expect("visual after deleting duplicate")
                .document
                .pages
                .iter()
                .all(|page| page.id != duplicate_id)
        );
    }
    assert_eq!(
        fs::read(&fixture).expect("re-read source PUB"),
        original,
        "bounded Desktop DuplicateBlank/DeleteBlank never mutates source PUB bytes"
    );
    let _ = fs::remove_dir_all(root);
}
