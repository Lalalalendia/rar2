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
    assert!(
        harness.get_by_label("Delete Rectangle Page").is_disabled(),
        "source-backed selected page must never advertise Rectangle-page deletion"
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
    let authored_rectangle_id = {
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
        shape_id
    };
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

    // The same page is now admitted exclusively by the new bounded operation.
    assert!(
        !harness.get_by_label("Delete Rectangle Page").is_disabled(),
        "one direct AuthorCreated Rectangle page must enable its own command"
    );
    // Reject a cascade: a second authored Rectangle disables Page deletion.
    let extra_rectangle_id = {
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
        let node_id = pub_editor::NodeId::from_canonical(pub_model::new_editor_canonical_id());
        app.editor
            .as_mut()
            .expect("editor")
            .create_shape(
                node_id,
                appended_page_id,
                pub_editor::RectEmu::new(
                    pub_editor::LengthEmu::new(size.width.get() / 3),
                    pub_editor::LengthEmu::new(size.height.get() / 3),
                    pub_editor::LengthEmu::new(size.width.get() / 5),
                    pub_editor::LengthEmu::new(size.height.get() / 5),
                ),
                rectangle_creation::chaptera_rectangle_paint_v1(),
            )
            .expect("create second authored Rectangle");
        app.finish_authoring_change("Seeded extra Rectangle to prove fail-closed admission.");
        node_id
    };
    harness.step();
    assert!(
        harness.get_by_label("Delete Rectangle Page").is_disabled(),
        "a two-Rectangle Page must not enable one-Rectangle DeletePage"
    );
    let two_shape_history = harness.state().editor.as_ref().unwrap().operations().len();
    harness
        .get_all_by_label("Undo")
        .next()
        .expect("remove second Rectangle")
        .click();
    harness.step();
    harness.step();
    assert_eq!(
        harness.state().editor.as_ref().unwrap().operations().len() + 1,
        two_shape_history
    );
    assert!(
        harness
            .state()
            .editor
            .as_ref()
            .unwrap()
            .authored_shape(extra_rectangle_id)
            .is_none()
    );
    assert!(
        !harness.get_by_label("Delete Rectangle Page").is_disabled(),
        "one-Rectangle admission must return after undo of extra Rectangle"
    );

    let shape_before = harness
        .state()
        .editor
        .as_ref()
        .expect("editor")
        .authored_shape(authored_rectangle_id)
        .expect("authored shape")
        .clone();
    let before_rectangle_delete = harness
        .state()
        .editor
        .as_ref()
        .expect("editor")
        .operations()
        .len();

    // Candidate Viewer projection must fail before commit on source mismatch.
    let original_source_hash = harness
        .state()
        .visual
        .as_ref()
        .expect("visual")
        .document
        .source
        .source_hash;
    let mismatched_source_hash =
        if original_source_hash == pub_editor::Sha256Digest::from_bytes([0x39; 32]) {
            pub_editor::Sha256Digest::from_bytes([0x93; 32])
        } else {
            pub_editor::Sha256Digest::from_bytes([0x39; 32])
        };
    harness
        .state_mut()
        .visual
        .as_mut()
        .expect("visual")
        .document
        .source
        .source_hash = mismatched_source_hash;
    harness.get_by_label("Delete Rectangle Page").click();
    harness.step();
    harness.step();
    {
        let app = harness.state();
        assert_eq!(
            app.editor.as_ref().expect("editor").operations().len(),
            before_rectangle_delete,
            "failed Viewer preflight must not add a delete revision"
        );
        assert!(
            app.visual
                .as_ref()
                .expect("visual")
                .document
                .pages
                .iter()
                .any(|page| page.id == appended_page_id),
            "rejected candidate must not remove the visible authored page"
        );
        assert!(
            app.edit_status
                .as_deref()
                .is_some_and(|status| status.contains("before commit")),
            "rejected Viewer projection must produce an explicit error"
        );
    }
    harness
        .state_mut()
        .visual
        .as_mut()
        .expect("visual")
        .document
        .source
        .source_hash = original_source_hash;
    harness.step();

    harness.get_by_label("Delete Rectangle Page").click();
    harness.step();
    harness.step();
    {
        let app = harness.state();
        let editor = app
            .editor
            .as_ref()
            .expect("editor after Rectangle-page delete");
        assert_eq!(editor.operations().len(), before_rectangle_delete + 1);
        assert!(matches!(
            editor.operations().last(),
            Some(pub_editor::EditOperation::DeleteAuthoredRectanglePageV1 { transition })
                if transition.page.identity.page_id == appended_page_id
                    && transition.shape_before.node_id == authored_rectangle_id
                    && transition.shape_before == shape_before
        ));
        assert!(editor.authored_shape(authored_rectangle_id).is_none());
        assert!(editor.authored_stack(appended_page_id).is_none());
        assert_eq!(
            app.visual
                .as_ref()
                .expect("visual")
                .document
                .pages
                .iter()
                .map(|page| page.id)
                .collect::<Vec<_>>(),
            source_order
        );
        assert!(
            app.visual
                .as_ref()
                .expect("visual")
                .scene
                .surfaces
                .iter()
                .all(|surface| surface.origin != appended_page_id)
        );
        assert_eq!(
            app.visual.as_ref().expect("visual").document.pages[app.selected_page].id,
            *source_order.last().expect("surviving source page"),
            "deletion must select the preceding admitted customer page"
        );
    }
    harness
        .get_all_by_label("Undo")
        .next()
        .expect("Undo authored Rectangle-page deletion")
        .click();
    harness.step();
    harness.step();
    {
        let app = harness.state();
        let editor = app.editor.as_ref().expect("editor after Undo");
        assert_eq!(editor.operations().len(), before_rectangle_delete);
        assert_eq!(
            editor.authored_shape(authored_rectangle_id),
            Some(&shape_before)
        );
        assert_eq!(
            editor
                .authored_stack(appended_page_id)
                .expect("restored authored lane")
                .members,
            vec![authored_rectangle_id]
        );
        assert_eq!(
            app.visual
                .as_ref()
                .expect("visual")
                .document
                .pages
                .last()
                .unwrap()
                .id,
            appended_page_id,
            "Undo restores exactly the same authored PageId"
        );
    }
    // The restored Page thumbnail can be clipped by the scrollable sidebar.
    // Navigate with an actual supported keyboard shortcut instead of clicking
    // an off-viewport label and accidentally retaining the source-page focus.
    assert_eq!(
        harness.state().selected_page,
        source_order.len() - 1,
        "Undo should keep the preceding surviving customer Page selected"
    );
    harness.press_key_modifiers(egui::Modifiers::CTRL, egui::Key::PageDown);
    harness.step();
    harness.step();
    {
        let app = harness.state();
        let visual = app
            .visual
            .as_ref()
            .expect("Viewer after selecting restored Page");
        let selected_page_id = visual
            .document
            .pages
            .get(app.selected_page)
            .map(|page| page.id);
        assert_eq!(
            selected_page_id,
            Some(appended_page_id),
            "real Pages sidebar click must select the restored PageId"
        );
        let editor = app
            .editor
            .as_ref()
            .expect("Editor after selecting restored Page");
        let mut debug_candidate = editor.clone();
        let rejection = debug_candidate
            .delete_authored_rectangle_page_v1(
                app.source_customer_page_ids.clone(),
                appended_page_id,
            )
            .err();
        assert!(
            editor.can_delete_authored_rectangle_page_v1(
                &app.source_customer_page_ids,
                appended_page_id,
            ),
            "canonical runtime must re-admit the exact restored Page/Rectangle after Undo: {rejection:?}"
        );
    }
    assert!(
        !harness.get_by_label("Delete Rectangle Page").is_disabled(),
        "Undo plus explicit Pages selection must restore Delete Rectangle Page capability"
    );

    harness
        .get_all_by_label("Redo")
        .next()
        .expect("Redo authored Rectangle-page deletion")
        .click();
    harness.step();
    harness.step();
    assert_eq!(
        harness
            .state()
            .editor
            .as_ref()
            .expect("editor after Redo")
            .operations()
            .len(),
        before_rectangle_delete + 1
    );
    assert!(
        harness
            .state()
            .editor
            .as_ref()
            .unwrap()
            .authored_shape(authored_rectangle_id)
            .is_none()
    );
    // Persist the v0.29 *deleted* state and reload from the original PUB:
    // reopening a later Delete Empty project would not test this protocol.
    harness.get_by_label("Save Project").click();
    harness.step();
    harness.step();
    harness.step();
    {
        let reopen = harness.get_by_label("Reopen Project");
        assert!(
            !reopen.is_disabled(),
            "v0.29 Rectangle-page project must reopen"
        );
        reopen.click();
    }
    harness.step();
    harness.step();
    harness.step();
    {
        let app = harness.state();
        let editor = app.editor.as_ref().expect("fresh v0.29 Editor");
        assert_eq!(editor.operations().len(), before_rectangle_delete + 1);
        assert!(matches!(
            editor.operations().last(),
            Some(pub_editor::EditOperation::DeleteAuthoredRectanglePageV1 { transition })
                if transition.page.identity.page_id == appended_page_id
                    && transition.shape_before.node_id == authored_rectangle_id
        ));
        assert!(editor.authored_shape(authored_rectangle_id).is_none());
        assert_eq!(app.source_customer_page_ids, source_order);
        assert_eq!(
            app.visual
                .as_ref()
                .expect("fresh v0.29 Viewer")
                .document
                .pages
                .iter()
                .map(|page| page.id)
                .collect::<Vec<_>>(),
            source_order
        );
        assert_eq!(
            fs::read(&fixture).expect("read source after v0.29 reopen"),
            original,
            "native PUB bytes must not change"
        );
    }

    harness
        .get_all_by_label("Undo")
        .next()
        .expect("restore authored Rectangle page for the old blank-page regression")
        .click();
    harness.step();
    harness.step();
    assert_eq!(
        harness
            .state()
            .editor
            .as_ref()
            .expect("restored authored editor")
            .authored_shape(authored_rectangle_id),
        Some(&shape_before)
    );

    // The existing Delete Empty Page acceptance continues from exactly the
    // same authored Rectangle state and unchanged operation history.
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
    // Fresh project reopen starts on a surviving imported Page. Undo restores
    // the authored PageId without stealing focus from that selected source.
    // Navigate to the now-empty authored Page explicitly before testing its
    // separate Delete Empty Page capability and the original regression.
    let (current_index, target_index, current_page_id) = {
        let app = harness.state();
        let visual = app.visual.as_ref().expect("visual after two Undo");
        let target_index = visual
            .document
            .pages
            .iter()
            .position(|page| page.id == appended_page_id)
            .expect("restored blank authored Page remains in Viewer membership");
        (
            app.selected_page,
            target_index,
            visual.document.pages[app.selected_page].id,
        )
    };
    assert!(
        source_order.contains(&current_page_id),
        "fresh reopen plus Undo must keep focus on a surviving source-backed Page"
    );
    assert!(
        current_index < target_index,
        "restored authored Page must remain after the selected source-backed Page"
    );
    for _ in current_index..target_index {
        harness.press_key_modifiers(egui::Modifiers::CTRL, egui::Key::PageDown);
        harness.step();
        harness.step();
    }
    {
        let app = harness.state();
        assert_eq!(
            app.visual.as_ref().expect("visual").document.pages[app.selected_page].id,
            appended_page_id,
            "real keyboard navigation must reselect restored blank Page"
        );
        let editor = app.editor.as_ref().expect("editor");
        let mut candidate = editor.clone();
        let rejection = candidate
            .delete_blank_authored_page_v1(app.source_customer_page_ids.clone(), appended_page_id)
            .err();
        assert!(
            editor.can_delete_blank_authored_page_v1(
                &app.source_customer_page_ids,
                appended_page_id,
            ),
            "canonical Delete Empty admission must recover after v0.29 Undo: {rejection:?}"
        );
    }
    assert!(
        !harness.get_by_label("Delete Empty Page").is_disabled(),
        "deletion becomes available after restored empty Page is selected"
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

#[cfg(not(feature = "reader-only"))]
#[test]
#[ignore = "runtime GUI evidence requires pinned CHAPTERA_SAMPLE_NEWSLETTER"]
fn gui_insert_blank_after_content_bearing_customer_preserves_source_and_replays() {
    use egui_kittest::{Harness, kittest::Queryable};

    let fixture_source = std::env::var_os("CHAPTERA_SAMPLE_NEWSLETTER")
        .map(PathBuf::from)
        .expect("CHAPTERA_SAMPLE_NEWSLETTER must point to pinned Apache POI fixture");
    let original = fs::read(&fixture_source).expect("read pinned source PUB");
    let root =
        std::env::temp_dir().join(format!("chaptera-gui-insert-after-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("create insert-after GUI directory");
    let fixture = root.join("SampleNewsletter.pub");
    fs::write(&fixture, &original).expect("copy immutable source fixture");

    let path = fixture.clone();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1280.0, 820.0))
        .with_pixels_per_point(1.0)
        .with_max_steps(140)
        .build_eframe(move |cc| {
            fallback_font::install(&cc.egui_ctx)
                .expect("pinned fallback font resource must validate");
            ViewerApp::new_with_storage(Some(path), cc.storage)
        });
    harness.step();
    harness.step();

    let (source_order, anchor, anchor_page, operations_before) = {
        let app = harness.state();
        let visual = app.visual.as_ref().expect("real PUB visual loaded");
        let source_order = visual
            .document
            .pages
            .iter()
            .map(|page| page.id)
            .collect::<Vec<_>>();
        assert!(
            source_order.len() >= 2,
            "must prove insertion is not append"
        );
        assert_eq!(app.source_customer_page_ids, source_order);
        assert_eq!(app.selected_page, 0);
        let anchor = source_order[0];
        let editor = app.editor.as_ref().expect("real PUB editor loaded");
        (
            source_order,
            anchor,
            editor.graph().pages[&anchor].clone(),
            editor.operations().len(),
        )
    };
    assert!(
        !harness
            .get_by_label("Insert Blank After Selected")
            .is_disabled(),
        "admitted source-backed customer page must enable InsertAfter"
    );

    // Add authored content to the selected real-PUB page. This intentionally
    // distinguishes InsertAfter from DuplicateBlank's empty-source requirement.
    let shape_id = pub_editor::NodeId::from_canonical(pub_model::new_editor_canonical_id());
    {
        let app = harness.state_mut();
        let size = anchor_page.size;
        app.editor
            .as_mut()
            .expect("editor")
            .create_shape(
                shape_id,
                anchor,
                pub_editor::RectEmu::new(
                    pub_editor::LengthEmu::new(size.width.get() / 5),
                    pub_editor::LengthEmu::new(size.height.get() / 5),
                    pub_editor::LengthEmu::new(size.width.get() / 4),
                    pub_editor::LengthEmu::new(size.height.get() / 4),
                ),
                rectangle_creation::chaptera_rectangle_paint_v1(),
            )
            .expect("make selected source page content-bearing");
        app.finish_authoring_change("Seeded canonical Rectangle for InsertAfter.");
    }
    harness.step();
    let shape_operation = harness
        .state()
        .editor
        .as_ref()
        .expect("editor")
        .operations()
        .last()
        .expect("authored shape operation")
        .clone();
    assert!(
        matches!(
            shape_operation,
            pub_editor::EditOperation::CreateShape { .. }
        ),
        "real authored content is tracked by canonical Editor history"
    );
    assert!(
        harness.get_by_label("Duplicate Blank Page").is_disabled(),
        "content-bearing source cannot be duplicated through blank-only command"
    );
    assert!(
        !harness
            .get_by_label("Insert Blank After Selected")
            .is_disabled(),
        "content-bearing source is a valid insertion anchor"
    );

    // Fail-closed UI projection: corrupt only candidate viewer identity.
    // Durable EditorSession and visible membership must not be touched.
    let source_hash = harness
        .state()
        .visual
        .as_ref()
        .expect("visual")
        .document
        .source
        .source_hash;
    let wrong_hash = if source_hash == pub_editor::Sha256Digest::from_bytes([0x37; 32]) {
        pub_editor::Sha256Digest::from_bytes([0x73; 32])
    } else {
        pub_editor::Sha256Digest::from_bytes([0x37; 32])
    };
    harness
        .state_mut()
        .visual
        .as_mut()
        .expect("visual")
        .document
        .source
        .source_hash = wrong_hash;
    harness.get_by_label("Insert Blank After Selected").click();
    harness.step();
    harness.step();
    {
        let app = harness.state();
        assert_eq!(
            app.editor.as_ref().expect("editor").operations().len(),
            operations_before + 1,
            "rejected Viewer preflight consumes zero InsertAfter revisions"
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
            "rejected candidate leaves visible customer membership unchanged"
        );
        assert!(
            app.edit_status
                .as_deref()
                .is_some_and(|status| status.contains("before commit")),
            "failure must report pre-commit rejection"
        );
    }
    harness
        .state_mut()
        .visual
        .as_mut()
        .expect("visual")
        .document
        .source
        .source_hash = source_hash;
    harness.step();

    harness.get_by_label("Insert Blank After Selected").click();
    harness.step();
    harness.step();
    let inserted = {
        let app = harness.state();
        let editor = app.editor.as_ref().expect("editor after InsertAfter");
        assert_eq!(editor.operations().len(), operations_before + 2);
        assert_eq!(editor.operations()[operations_before], shape_operation);
        let inserted = match editor.operations().last() {
            Some(pub_editor::EditOperation::InsertBlankPageAfterV1 { transition }) => {
                assert_eq!(transition.anchor_page_id, anchor);
                assert_eq!(transition.before_customer_page_ids, source_order);
                assert_eq!(
                    transition.after_customer_page_ids.len(),
                    source_order.len() + 1
                );
                assert_ne!(transition.identity.page_id, anchor);
                transition.identity.page_id
            }
            other => panic!("one click must produce exactly one InsertAfter, got {other:?}"),
        };
        assert_eq!(editor.graph().pages[&anchor], anchor_page);
        let dest = &editor.graph().pages[&inserted];
        assert_eq!(dest.size, anchor_page.size);
        assert_eq!(dest.bleed, anchor_page.bleed);
        assert_eq!(dest.margins, anchor_page.margins);
        assert!(dest.children.is_empty() && dest.extensions.is_empty());
        let mut expected = source_order.clone();
        expected.insert(1, inserted);
        assert_eq!(
            editor
                .effective_customer_page_order_v1(&source_order)
                .expect("order"),
            expected
        );
        let visual = app.visual.as_ref().expect("Viewer after insertion");
        assert_eq!(
            visual
                .document
                .pages
                .iter()
                .map(|page| page.id)
                .collect::<Vec<_>>(),
            expected
        );
        assert_eq!(app.selected_page, 1);
        assert_eq!(visual.document.pages[app.selected_page].id, inserted);
        assert!(
            visual
                .scene
                .surfaces
                .iter()
                .any(|surface| surface.origin == inserted),
            "new blank customer page gets a real Viewer scene surface"
        );
        inserted
    };
    assert!(
        !harness.get_by_label("Delete Empty Page").is_disabled(),
        "inserted blank page must admit lifecycle Delete"
    );

    harness
        .get_all_by_label("Undo")
        .next()
        .expect("Undo insert")
        .click();
    harness.step();
    harness.step();
    {
        let app = harness.state();
        let editor = app.editor.as_ref().expect("editor after undo");
        assert_eq!(editor.operations().len(), operations_before + 1);
        assert_eq!(editor.operations()[operations_before], shape_operation);
        assert_eq!(editor.graph().pages[&anchor], anchor_page);
        let visual = app.visual.as_ref().expect("Viewer after undo");
        assert_eq!(
            visual
                .document
                .pages
                .iter()
                .map(|page| page.id)
                .collect::<Vec<_>>(),
            source_order,
            "Undo must restore original customer order"
        );
        assert!(
            visual
                .scene
                .surfaces
                .iter()
                .all(|surface| surface.origin != inserted)
        );
    }

    harness
        .get_all_by_label("Redo")
        .next()
        .expect("Redo insert")
        .click();
    harness.step();
    harness.step();
    {
        let app = harness.state();
        let visual = app.visual.as_ref().expect("Viewer after redo");
        let mut expected = source_order.clone();
        expected.insert(1, inserted);
        assert_eq!(
            visual
                .document
                .pages
                .iter()
                .map(|page| page.id)
                .collect::<Vec<_>>(),
            expected,
            "Redo restores exactly the same inserted PageId and order"
        );
        assert_eq!(
            app.editor.as_ref().expect("editor").operations().len(),
            operations_before + 2
        );
    }

    harness.get_by_label("Save Project").click();
    harness.step();
    harness.step();
    harness.step();
    assert!(
        !harness.get_by_label("Reopen Project").is_disabled(),
        "saved v0.28 EditorProject must be reopenable"
    );
    harness.get_by_label("Reopen Project").click();
    harness.step();
    harness.step();
    harness.step();
    {
        let app = harness.state();
        let editor = app.editor.as_ref().expect("fresh v0.28 editor");
        assert_eq!(
            editor.project().schema_version,
            pub_editor::EDITOR_PROJECT_VERSION_V0_28
        );
        assert_eq!(editor.operations().len(), operations_before + 2);
        assert_eq!(editor.operations()[operations_before], shape_operation);
        assert!(matches!(
            editor.operations().last(),
            Some(pub_editor::EditOperation::InsertBlankPageAfterV1 { transition })
                if transition.anchor_page_id == anchor
                    && transition.identity.page_id == inserted
        ));
        assert_eq!(app.source_customer_page_ids, source_order);
        assert_eq!(editor.graph().pages[&anchor], anchor_page);
        let mut expected = source_order.clone();
        expected.insert(1, inserted);
        let visual = app.visual.as_ref().expect("reopened Viewer");
        assert_eq!(
            visual
                .document
                .pages
                .iter()
                .map(|page| page.id)
                .collect::<Vec<_>>(),
            expected,
            "reopen restores inserted PageId before previous second page"
        );
        assert!(
            visual
                .scene
                .surfaces
                .iter()
                .any(|surface| surface.origin == inserted)
        );
    }

    assert_eq!(
        fs::read(&fixture).expect("source fixture remains available"),
        original,
        "InsertAfter and v0.28 project replay must not rewrite source PUB bytes"
    );
    let _ = fs::remove_dir_all(root);
}

#[cfg(not(feature = "reader-only"))]
#[test]
#[ignore = "runtime GUI evidence requires pinned CHAPTERA_SAMPLE_NEWSLETTER"]
fn gui_duplicate_authored_rectangle_page_v030_click_undo_redo_reopen_real_pub() {
    use egui_kittest::{Harness, kittest::Queryable};

    let fixture_source = std::env::var_os("CHAPTERA_SAMPLE_NEWSLETTER")
        .map(PathBuf::from)
        .expect("CHAPTERA_SAMPLE_NEWSLETTER must name the pinned Apache POI fixture");
    let original = fs::read(&fixture_source).expect("read pinned SampleNewsletter source");
    let root = std::env::temp_dir().join(format!(
        "chaptera-gui-duplicate-rectangle-page-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("create GUI duplicate temp directory");
    let fixture = root.join("SampleNewsletter.pub");
    fs::write(&fixture, &original).expect("copy immutable source PUB");

    let path = fixture.clone();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1280.0, 820.0))
        .with_pixels_per_point(1.0)
        .with_max_steps(140)
        .build_eframe(move |cc| {
            fallback_font::install(&cc.egui_ctx)
                .expect("pinned Chaptera fallback font must validate");
            ViewerApp::new_with_storage(Some(path), cc.storage)
        });
    harness.step();
    harness.step();
    let source_pages = harness.state().source_customer_page_ids.clone();
    assert!(
        !source_pages.is_empty(),
        "real PUB must expose customer Pages"
    );
    assert!(
        harness
            .get_by_label("Duplicate Rectangle Page")
            .is_disabled(),
        "imported source-backed Page must fail admission at the real button"
    );

    harness.get_by_label("Add Page at End").click();
    harness.step();
    harness.step();
    let authored_page_id = {
        let app = harness.state();
        match app.editor.as_ref().expect("editor").operations().last() {
            Some(pub_editor::EditOperation::AppendBlankPageV1 { transition }) => {
                transition.identity.page_id
            }
            other => panic!("expected canonical AppendBlankPageV1: {other:?}"),
        }
    };
    assert!(
        harness
            .get_by_label("Duplicate Rectangle Page")
            .is_disabled(),
        "blank authored Page is handled by the separate Duplicate Blank Page command"
    );

    let authored_node_id = pub_editor::NodeId::from_canonical(pub_model::new_editor_canonical_id());
    {
        let app = harness.state_mut();
        let size = app.editor.as_ref().expect("editor").graph().pages[&authored_page_id].size;
        app.editor
            .as_mut()
            .expect("editor")
            .create_shape(
                authored_node_id,
                authored_page_id,
                pub_editor::RectEmu::new(
                    pub_editor::LengthEmu::new(size.width.get() / 5),
                    pub_editor::LengthEmu::new(size.height.get() / 5),
                    pub_editor::LengthEmu::new(size.width.get() / 4),
                    pub_editor::LengthEmu::new(size.height.get() / 4),
                ),
                rectangle_creation::chaptera_rectangle_paint_v1(),
            )
            .expect("one direct AuthorCreated Rectangle");
        app.finish_authoring_change("Seeded one canonical authored Rectangle.");
    }
    harness.step();
    assert!(
        !harness
            .get_by_label("Duplicate Rectangle Page")
            .is_disabled(),
        "one authored Rectangle enables the actual Duplicate Rectangle Page command"
    );
    assert!(
        harness.get_by_label("Duplicate Blank Page").is_disabled(),
        "content-bearing Page must not pass the blank-only command"
    );

    // A second authored object must remove the GUI capability before any click.
    let second_id = pub_editor::NodeId::from_canonical(pub_model::new_editor_canonical_id());
    {
        let app = harness.state_mut();
        let size = app.editor.as_ref().unwrap().graph().pages[&authored_page_id].size;
        app.editor
            .as_mut()
            .unwrap()
            .create_shape(
                second_id,
                authored_page_id,
                pub_editor::RectEmu::new(
                    pub_editor::LengthEmu::new(size.width.get() / 3),
                    pub_editor::LengthEmu::new(size.height.get() / 3),
                    pub_editor::LengthEmu::new(size.width.get() / 5),
                    pub_editor::LengthEmu::new(size.height.get() / 5),
                ),
                rectangle_creation::chaptera_rectangle_paint_v1(),
            )
            .expect("add second authored Rectangle");
        app.finish_authoring_change("Seeded additional Rectangle to test fail-closed.");
    }
    harness.step();
    assert!(
        harness
            .get_by_label("Duplicate Rectangle Page")
            .is_disabled(),
        "multi-object Page cannot advertise one-Rectangle duplicate"
    );
    harness
        .get_all_by_label("Undo")
        .next()
        .expect("Undo extra Rectangle")
        .click();
    harness.step();
    harness.step();
    assert!(
        !harness
            .get_by_label("Duplicate Rectangle Page")
            .is_disabled(),
        "Undo restores one-Rectangle admission"
    );

    let (history_before, source_shape, source_page, source_hash) = {
        let app = harness.state();
        let editor = app.editor.as_ref().expect("editor");
        (
            editor.operations().len(),
            editor
                .authored_shape(authored_node_id)
                .expect("source authored shape")
                .clone(),
            editor.graph().pages[&authored_page_id].clone(),
            app.visual.as_ref().unwrap().document.source.source_hash,
        )
    };

    // A bad Viewer authority must reject the real click before committing
    // either a PageId or a duplicated Rectangle NodeId.
    let wrong_hash = if source_hash == pub_editor::Sha256Digest::from_bytes([0x79; 32]) {
        pub_editor::Sha256Digest::from_bytes([0x97; 32])
    } else {
        pub_editor::Sha256Digest::from_bytes([0x79; 32])
    };
    harness
        .state_mut()
        .visual
        .as_mut()
        .unwrap()
        .document
        .source
        .source_hash = wrong_hash;
    harness.get_by_label("Duplicate Rectangle Page").click();
    harness.step();
    harness.step();
    assert_eq!(
        harness.state().editor.as_ref().unwrap().operations().len(),
        history_before,
        "Viewer projection mismatch must consume no history operation"
    );
    assert_eq!(
        harness
            .state()
            .visual
            .as_ref()
            .unwrap()
            .document
            .pages
            .len(),
        source_pages.len() + 1,
        "rejected transaction must preserve visible customer membership"
    );
    harness
        .state_mut()
        .visual
        .as_mut()
        .unwrap()
        .document
        .source
        .source_hash = source_hash;
    harness.step();

    harness.get_by_label("Duplicate Rectangle Page").click();
    harness.step();
    harness.step();

    let (destination_page_id, destination_node_id, after_history_len) = {
        let app = harness.state();
        let editor = app.editor.as_ref().expect("post-click editor");
        assert_eq!(editor.operations().len(), history_before + 1);
        let transition = match editor.operations().last() {
            Some(pub_editor::EditOperation::DuplicateAuthoredRectanglePageV1 { transition }) => {
                transition
            }
            other => panic!("real GUI click did not create canonical v0.30 operation: {other:?}"),
        };
        assert_eq!(transition.page.source_page_id, authored_page_id);
        assert_eq!(transition.source_shape.node_id, authored_node_id);
        let destination_page_id = transition.page.destination_identity.page_id;
        let destination_node_id = transition.destination_shape.node_id;
        assert_ne!(destination_page_id, authored_page_id);
        assert_ne!(destination_node_id, authored_node_id);
        assert_eq!(editor.graph().pages[&authored_page_id], source_page);
        assert_eq!(
            editor.authored_shape(authored_node_id),
            Some(&source_shape),
            "original Rectangle remains unchanged"
        );
        let duplicated = editor
            .authored_shape(destination_node_id)
            .expect("destination authored Rectangle");
        assert_eq!(duplicated.page_id, destination_page_id);
        assert_eq!(duplicated.parent_id, destination_page_id);
        assert_eq!(duplicated.bounds, source_shape.bounds);
        assert_eq!(duplicated.paint, source_shape.paint);
        assert_eq!(
            editor
                .authored_stack(destination_page_id)
                .expect("destination lane")
                .members,
            vec![destination_node_id]
        );
        let mut expected = source_pages.clone();
        expected.push(authored_page_id);
        expected.push(destination_page_id);
        assert_eq!(
            editor
                .effective_customer_page_order_v1(&source_pages)
                .expect("canonical membership"),
            expected
        );
        let visual = app.visual.as_ref().expect("post-click Viewer");
        assert_eq!(
            visual
                .document
                .pages
                .iter()
                .map(|p| p.id)
                .collect::<Vec<_>>(),
            expected
        );
        assert_eq!(
            visual.document.pages[app.selected_page].id,
            destination_page_id
        );
        assert!(
            visual
                .scene
                .surfaces
                .iter()
                .any(|surface| surface.origin == destination_page_id),
            "new Page must have a real Viewer surface"
        );
        (
            destination_page_id,
            destination_node_id,
            editor.operations().len(),
        )
    };
    // The authority test alone would pass if the copy had become an
    // invisible object. This is the exact frame work consumed by the canvas
    // and Page thumbnails: test fill, stroke, hit index and Page membership.
    let assert_copied_rectangle_is_painted = |app: &ViewerApp| {
        let visual = app.visual.as_ref().expect("real PUB Viewer");
        let destination_index = visual
            .document
            .pages
            .iter()
            .position(|page| page.id == destination_page_id)
            .expect("visible cloned customer Page");
        let frame = app
            .build_page_frame_work(destination_index)
            .expect("production Desktop canvas render plan");
        assert_eq!(frame.render_plan.page_id, destination_page_id);
        let rendered = frame
            .render_plan
            .nodes
            .iter()
            .filter(|node| node.node_id == destination_node_id)
            .collect::<Vec<_>>();
        assert_eq!(
            rendered.len(),
            1,
            "the cloned authored Rectangle must appear exactly once in the painted Page lane"
        );
        let painted = rendered[0];
        assert_eq!(painted.bounds, source_shape.bounds);
        assert_eq!(
            painted.solid_fill_rgb,
            source_shape.paint.fill.visible.then_some([
                source_shape.paint.fill.color.r,
                source_shape.paint.fill.color.g,
                source_shape.paint.fill.color.b,
            ])
        );
        assert_eq!(
            painted
                .solid_line
                .as_ref()
                .map(|line| (line.rgb, line.width_emu)),
            source_shape.paint.stroke.visible.then_some((
                [
                    source_shape.paint.stroke.color.r,
                    source_shape.paint.stroke.color.g,
                    source_shape.paint.stroke.color.b,
                ],
                source_shape.paint.stroke.width_emu,
            ))
        );
        let instance_id = frame
            .hit_index
            .instance_for_node(destination_node_id)
            .expect("the visible Rectangle must be selectable by canvas hit testing");
        let hit = frame
            .hit_index
            .entry_for_instance(instance_id)
            .expect("canvas instance must resolve to one hit entry");
        assert_eq!(hit.node_id, destination_node_id);
        assert_eq!(hit.bounds, source_shape.bounds);
    };
    assert_copied_rectangle_is_painted(harness.state());

    assert!(
        !harness.get_by_label("Delete Rectangle Page").is_disabled(),
        "cloned authored content must remain an actionable Page"
    );

    harness
        .get_all_by_label("Undo")
        .next()
        .expect("Undo one Page+Rectangle duplicate")
        .click();
    harness.step();
    harness.step();
    {
        let editor = harness.state().editor.as_ref().unwrap();
        assert_eq!(editor.operations().len(), history_before);
        assert!(!editor.graph().pages.contains_key(&destination_page_id));
        assert!(editor.authored_shape(destination_node_id).is_none());
        assert!(editor.authored_stack(destination_page_id).is_none());
        assert_eq!(editor.authored_shape(authored_node_id), Some(&source_shape));
    }
    harness
        .get_all_by_label("Redo")
        .next()
        .expect("Redo one Page+Rectangle duplicate")
        .click();
    harness.step();
    harness.step();
    {
        let editor = harness.state().editor.as_ref().unwrap();
        assert_eq!(editor.operations().len(), after_history_len);
        assert!(editor.graph().pages.contains_key(&destination_page_id));
        assert!(editor.authored_shape(destination_node_id).is_some());
    }

    harness.get_by_label("Save Project").click();
    harness.step();
    harness.step();
    {
        let reopen = harness.get_by_label("Reopen Project");
        assert!(!reopen.is_disabled(), "v0.30 project must be reopenable");
        reopen.click();
    }
    harness.step();
    harness.step();
    harness.step();
    {
        let app = harness.state();
        let editor = app.editor.as_ref().expect("fresh v0.30 session");
        assert_eq!(
            editor.project().schema_version,
            pub_editor::EDITOR_PROJECT_VERSION_V0_30
        );
        assert_eq!(editor.operations().len(), after_history_len);
        assert!(matches!(
            editor.operations().last(),
            Some(pub_editor::EditOperation::DuplicateAuthoredRectanglePageV1 { transition })
                if transition.page.destination_identity.page_id == destination_page_id
                    && transition.destination_shape.node_id == destination_node_id
        ));
        assert_eq!(editor.authored_shape(authored_node_id), Some(&source_shape));
        assert_eq!(
            editor
                .authored_shape(destination_node_id)
                .map(|s| (s.bounds, s.paint.clone())),
            Some((source_shape.bounds, source_shape.paint.clone()))
        );
        assert_eq!(
            app.visual
                .as_ref()
                .expect("fresh Viewer")
                .document
                .pages
                .iter()
                .map(|page| page.id)
                .next_back(),
            Some(destination_page_id)
        );
    }

    // A fresh project replay must paint the copied Rectangle again, not
    // merely deserialize Page/Node state without a renderable authored lane.
    assert_copied_rectangle_is_painted(harness.state());

    // Export must bind the *current* complete revision, never silently
    // materialize a source-only copy under an unrelated preview receipt.
    harness.get_by_label("Export").click();
    harness.step();
    harness
        .get_all_by_label("Preview IDML")
        .last()
        .expect("actual Export menu Preview IDML")
        .click();
    harness.step();
    {
        let app = harness.state();
        if let Some(preview) = app.export_preview.as_ref() {
            assert_eq!(preview.operation_count, after_history_len);
            assert_eq!(preview.target, pub_editor::EditorEditableTarget::Idml);
        } else {
            assert!(
                app.edit_status
                    .as_deref()
                    .is_some_and(|status| status.contains("Could not preview")),
                "unsupported export must report a concrete error, not silently succeed"
            );
        }
    }
    assert_eq!(
        fs::read(&fixture).expect("read original PUB after Save/Reopen/Preview"),
        original,
        "all Desktop v0.30 Page duplication operations must preserve native PUB bytes"
    );
    let _ = fs::remove_dir_all(root);
}

#[cfg(not(feature = "reader-only"))]
#[test]
#[ignore = "runtime GUI evidence requires pinned CHAPTERA_SAMPLE_NEWSLETTER"]
fn gui_duplicate_authored_rectangles_page_v031_click_undo_redo_reopen_real_pub() {
    use egui_kittest::{Harness, kittest::Queryable};

    let fixture_source = std::env::var_os("CHAPTERA_SAMPLE_NEWSLETTER")
        .map(PathBuf::from)
        .expect("CHAPTERA_SAMPLE_NEWSLETTER must name the pinned Apache POI fixture");
    let original = fs::read(&fixture_source).expect("read pinned SampleNewsletter source");
    let root = std::env::temp_dir().join(format!(
        "chaptera-gui-duplicate-multi-rectangle-page-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("create GUI multi-duplicate temp directory");
    let fixture = root.join("SampleNewsletter.pub");
    fs::write(&fixture, &original).expect("copy immutable source PUB");

    let path = fixture.clone();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1280.0, 820.0))
        .with_pixels_per_point(1.0)
        .with_max_steps(180)
        .build_eframe(move |cc| {
            fallback_font::install(&cc.egui_ctx)
                .expect("pinned Chaptera fallback font must validate");
            ViewerApp::new_with_storage(Some(path), cc.storage)
        });
    harness.step();
    harness.step();

    let source_pages = harness.state().source_customer_page_ids.clone();
    assert!(
        !source_pages.is_empty(),
        "real PUB must expose customer Pages"
    );
    assert!(
        harness
            .get_by_label("Duplicate Multi-Rectangle Page")
            .is_disabled(),
        "imported source-backed Page must fail multi-Rectangle admission"
    );

    harness.get_by_label("Add Page at End").click();
    harness.step();
    harness.step();
    let authored_page_id = {
        let app = harness.state();
        match app.editor.as_ref().expect("editor").operations().last() {
            Some(pub_editor::EditOperation::AppendBlankPageV1 { transition }) => {
                transition.identity.page_id
            }
            other => panic!("expected canonical AppendBlankPageV1: {other:?}"),
        }
    };
    assert!(
        harness
            .get_by_label("Duplicate Multi-Rectangle Page")
            .is_disabled(),
        "blank Page must not pass multi-Rectangle duplicate"
    );

    let source_node_a = pub_editor::NodeId::from_canonical(pub_model::new_editor_canonical_id());
    let source_node_b = pub_editor::NodeId::from_canonical(pub_model::new_editor_canonical_id());
    {
        let app = harness.state_mut();
        let size = app.editor.as_ref().expect("editor").graph().pages[&authored_page_id].size;
        let paint_a = rectangle_creation::chaptera_rectangle_paint_v1();
        app.editor
            .as_mut()
            .expect("editor")
            .create_shape(
                source_node_a,
                authored_page_id,
                pub_editor::RectEmu::new(
                    pub_editor::LengthEmu::new(size.width.get() / 7),
                    pub_editor::LengthEmu::new(size.height.get() / 7),
                    pub_editor::LengthEmu::new(size.width.get() / 5),
                    pub_editor::LengthEmu::new(size.height.get() / 5),
                ),
                paint_a,
            )
            .expect("first direct AuthorCreated Rectangle");
        app.finish_authoring_change("Seeded first canonical authored Rectangle.");
    }
    harness.step();
    assert!(
        !harness
            .get_by_label("Duplicate Rectangle Page")
            .is_disabled(),
        "one Rectangle still belongs to the v0.30 command"
    );
    assert!(
        harness
            .get_by_label("Duplicate Multi-Rectangle Page")
            .is_disabled(),
        "one Rectangle must not activate the plural command"
    );

    {
        let app = harness.state_mut();
        let size = app.editor.as_ref().expect("editor").graph().pages[&authored_page_id].size;
        let mut paint_b = rectangle_creation::chaptera_rectangle_paint_v1();
        paint_b.fill.color = pub_editor::Srgb8V1 {
            r: 34,
            g: 132,
            b: 211,
        };
        paint_b.stroke.color = pub_editor::Srgb8V1 {
            r: 190,
            g: 61,
            b: 44,
        };
        paint_b.stroke.width_emu = 25_400;
        app.editor
            .as_mut()
            .expect("editor")
            .create_shape(
                source_node_b,
                authored_page_id,
                pub_editor::RectEmu::new(
                    pub_editor::LengthEmu::new(size.width.get() / 2),
                    pub_editor::LengthEmu::new(size.height.get() / 3),
                    pub_editor::LengthEmu::new(size.width.get() / 6),
                    pub_editor::LengthEmu::new(size.height.get() / 4),
                ),
                paint_b,
            )
            .expect("second direct AuthorCreated Rectangle");
        app.finish_authoring_change("Seeded second canonical authored Rectangle.");
    }
    harness.step();
    assert!(
        harness
            .get_by_label("Duplicate Rectangle Page")
            .is_disabled(),
        "the one-Rectangle command must remain fail-closed on multi-object Page"
    );
    assert!(
        !harness
            .get_by_label("Duplicate Multi-Rectangle Page")
            .is_disabled(),
        "two independent authored Rectangles must enable the plural command"
    );

    let (history_before, source_shapes, source_stack, source_page, source_hash) = {
        let app = harness.state();
        let editor = app.editor.as_ref().expect("editor");
        (
            editor.operations().len(),
            vec![
                editor
                    .authored_shape(source_node_a)
                    .expect("first source shape")
                    .clone(),
                editor
                    .authored_shape(source_node_b)
                    .expect("second source shape")
                    .clone(),
            ],
            editor
                .authored_stack(authored_page_id)
                .expect("source authored stack")
                .members
                .clone(),
            editor.graph().pages[&authored_page_id].clone(),
            app.visual
                .as_ref()
                .expect("Viewer")
                .document
                .source
                .source_hash,
        )
    };
    assert_eq!(source_stack, vec![source_node_a, source_node_b]);

    let wrong_hash = if source_hash == pub_editor::Sha256Digest::from_bytes([0x79; 32]) {
        pub_editor::Sha256Digest::from_bytes([0x97; 32])
    } else {
        pub_editor::Sha256Digest::from_bytes([0x79; 32])
    };
    harness
        .state_mut()
        .visual
        .as_mut()
        .expect("Viewer")
        .document
        .source
        .source_hash = wrong_hash;
    harness
        .get_by_label("Duplicate Multi-Rectangle Page")
        .click();
    harness.step();
    harness.step();
    assert_eq!(
        harness.state().editor.as_ref().unwrap().operations().len(),
        history_before,
        "Viewer preflight mismatch must consume no plural history operation"
    );
    harness
        .state_mut()
        .visual
        .as_mut()
        .expect("Viewer")
        .document
        .source
        .source_hash = source_hash;
    harness.step();

    harness
        .get_by_label("Duplicate Multi-Rectangle Page")
        .click();
    harness.step();
    harness.step();

    let (destination_page_id, destination_node_ids, after_history_len) = {
        let app = harness.state();
        let editor = app.editor.as_ref().expect("post-click editor");
        assert_eq!(editor.operations().len(), history_before + 1);
        let transition = match editor.operations().last() {
            Some(pub_editor::EditOperation::DuplicateAuthoredRectanglesPageV1 { transition }) => {
                transition
            }
            other => panic!("real GUI click did not create canonical v0.31 operation: {other:?}"),
        };
        assert_eq!(transition.page.source_page_id, authored_page_id);
        assert_eq!(
            transition
                .source_shapes
                .iter()
                .map(|shape| shape.node_id)
                .collect::<Vec<_>>(),
            vec![source_node_a, source_node_b]
        );
        assert_eq!(transition.destination_shapes.len(), 2);
        let destination_page_id = transition.page.destination_identity.page_id;
        let destination_node_ids = transition
            .destination_shapes
            .iter()
            .map(|shape| shape.node_id)
            .collect::<Vec<_>>();
        assert_ne!(destination_page_id, authored_page_id);
        assert_ne!(destination_node_ids[0], source_node_a);
        assert_ne!(destination_node_ids[1], source_node_b);
        assert_ne!(destination_node_ids[0], destination_node_ids[1]);
        assert_eq!(editor.graph().pages[&authored_page_id], source_page);
        for ((source_id, source_shape), destination_id) in [
            (source_node_a, &source_shapes[0]),
            (source_node_b, &source_shapes[1]),
        ]
        .into_iter()
        .zip(&destination_node_ids)
        {
            assert_eq!(
                editor.authored_shape(source_id),
                Some(source_shape),
                "source Rectangle remains unchanged"
            );
            let duplicated = editor
                .authored_shape(*destination_id)
                .expect("destination authored Rectangle");
            assert_eq!(duplicated.page_id, destination_page_id);
            assert_eq!(duplicated.parent_id, destination_page_id);
            assert_eq!(duplicated.bounds, source_shape.bounds);
            assert_eq!(duplicated.paint, source_shape.paint);
        }
        assert_eq!(
            editor
                .authored_stack(destination_page_id)
                .expect("destination authored stack")
                .members,
            destination_node_ids
        );
        let mut expected = source_pages.clone();
        expected.push(authored_page_id);
        expected.push(destination_page_id);
        assert_eq!(
            editor
                .effective_customer_page_order_v1(&source_pages)
                .expect("canonical membership"),
            expected
        );
        let visual = app.visual.as_ref().expect("post-click Viewer");
        assert_eq!(
            visual
                .document
                .pages
                .iter()
                .map(|page| page.id)
                .collect::<Vec<_>>(),
            expected
        );
        assert_eq!(
            visual.document.pages[app.selected_page].id,
            destination_page_id
        );
        assert!(
            visual
                .scene
                .surfaces
                .iter()
                .any(|surface| surface.origin == destination_page_id),
            "new Page must have a real Viewer surface"
        );
        (
            destination_page_id,
            destination_node_ids,
            editor.operations().len(),
        )
    };

    let assert_copied_rectangles_are_painted = |app: &ViewerApp| {
        let visual = app.visual.as_ref().expect("real PUB Viewer");
        let destination_index = visual
            .document
            .pages
            .iter()
            .position(|page| page.id == destination_page_id)
            .expect("visible cloned customer Page");
        let frame = app
            .build_page_frame_work(destination_index)
            .expect("production Desktop canvas render plan");
        assert_eq!(frame.render_plan.page_id, destination_page_id);
        for (destination_node_id, source_shape) in destination_node_ids.iter().zip(&source_shapes) {
            let rendered = frame
                .render_plan
                .nodes
                .iter()
                .filter(|node| node.node_id == *destination_node_id)
                .collect::<Vec<_>>();
            assert_eq!(
                rendered.len(),
                1,
                "each cloned authored Rectangle must appear exactly once in the painted Page lane"
            );
            let painted = rendered[0];
            assert_eq!(painted.bounds, source_shape.bounds);
            assert_eq!(
                painted.solid_fill_rgb,
                source_shape.paint.fill.visible.then_some([
                    source_shape.paint.fill.color.r,
                    source_shape.paint.fill.color.g,
                    source_shape.paint.fill.color.b,
                ])
            );
            assert_eq!(
                painted
                    .solid_line
                    .as_ref()
                    .map(|line| (line.rgb, line.width_emu)),
                source_shape.paint.stroke.visible.then_some((
                    [
                        source_shape.paint.stroke.color.r,
                        source_shape.paint.stroke.color.g,
                        source_shape.paint.stroke.color.b,
                    ],
                    source_shape.paint.stroke.width_emu,
                ))
            );
            let instance_id = frame
                .hit_index
                .instance_for_node(*destination_node_id)
                .expect("visible cloned Rectangle must be selectable by canvas hit testing");
            let hit = frame
                .hit_index
                .entry_for_instance(instance_id)
                .expect("canvas instance must resolve to one hit entry");
            assert_eq!(hit.node_id, *destination_node_id);
            assert_eq!(hit.bounds, source_shape.bounds);
        }
    };
    assert_copied_rectangles_are_painted(harness.state());

    harness
        .get_all_by_label("Undo")
        .next()
        .expect("Undo one Page+multi-Rectangle duplicate")
        .click();
    harness.step();
    harness.step();
    {
        let editor = harness.state().editor.as_ref().expect("editor after Undo");
        assert_eq!(editor.operations().len(), history_before);
        assert!(!editor.graph().pages.contains_key(&destination_page_id));
        for destination_node_id in &destination_node_ids {
            assert!(editor.authored_shape(*destination_node_id).is_none());
        }
        assert!(editor.authored_stack(destination_page_id).is_none());
        assert_eq!(
            editor
                .authored_stack(authored_page_id)
                .expect("source stack survives")
                .members,
            source_stack
        );
    }

    harness
        .get_all_by_label("Redo")
        .next()
        .expect("Redo one Page+multi-Rectangle duplicate")
        .click();
    harness.step();
    harness.step();
    {
        let editor = harness.state().editor.as_ref().expect("editor after Redo");
        assert_eq!(editor.operations().len(), after_history_len);
        assert!(editor.graph().pages.contains_key(&destination_page_id));
        for destination_node_id in &destination_node_ids {
            assert!(editor.authored_shape(*destination_node_id).is_some());
        }
    }
    assert_copied_rectangles_are_painted(harness.state());

    harness.get_by_label("Save Project").click();
    harness.step();
    harness.step();
    {
        let reopen = harness.get_by_label("Reopen Project");
        assert!(!reopen.is_disabled(), "v0.31 project must be reopenable");
        reopen.click();
    }
    harness.step();
    harness.step();
    harness.step();

    {
        let app = harness.state();
        let editor = app.editor.as_ref().expect("fresh v0.31 session");
        assert_eq!(
            editor.project().schema_version,
            pub_editor::EDITOR_PROJECT_VERSION_V0_31
        );
        assert_eq!(editor.operations().len(), after_history_len);
        assert!(matches!(
            editor.operations().last(),
            Some(pub_editor::EditOperation::DuplicateAuthoredRectanglesPageV1 { transition })
                if transition.page.destination_identity.page_id == destination_page_id
                    && transition
                        .destination_shapes
                        .iter()
                        .map(|shape| shape.node_id)
                        .collect::<Vec<_>>()
                        == destination_node_ids
        ));
        for ((source_id, source_shape), destination_node_id) in [
            (source_node_a, &source_shapes[0]),
            (source_node_b, &source_shapes[1]),
        ]
        .into_iter()
        .zip(&destination_node_ids)
        {
            assert_eq!(editor.authored_shape(source_id), Some(source_shape));
            assert_eq!(
                editor
                    .authored_shape(*destination_node_id)
                    .map(|shape| (shape.bounds, shape.paint.clone())),
                Some((source_shape.bounds, source_shape.paint.clone()))
            );
        }
        assert_eq!(
            app.visual
                .as_ref()
                .expect("fresh Viewer")
                .document
                .pages
                .iter()
                .map(|page| page.id)
                .next_back(),
            Some(destination_page_id)
        );
    }
    assert_copied_rectangles_are_painted(harness.state());

    assert_eq!(
        fs::read(&fixture).expect("read original PUB after v0.31 Save/Reopen"),
        original,
        "Desktop v0.31 multi-Rectangle Page duplication must preserve native PUB bytes"
    );
    let _ = fs::remove_dir_all(root);
}
