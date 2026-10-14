//! Real-PUB GUI acceptance for canonical document history shortcuts.

use crate::*;

#[test]
#[ignore = "runtime GUI evidence requires pinned CHAPTERA_SAMPLE_NEWSLETTER"]
fn gui_ctrl_history_shortcuts_reuse_canonical_undo_redo_without_hidden_operation() {
    use egui_kittest::Harness;

    let fixture_source = std::env::var_os("CHAPTERA_SAMPLE_NEWSLETTER")
        .map(PathBuf::from)
        .expect("CHAPTERA_SAMPLE_NEWSLETTER must point to the pinned Apache POI fixture");
    let original = fs::read(&fixture_source).expect("read pinned SampleNewsletter fixture");
    let root = std::env::temp_dir().join(format!(
        "chaptera-gui-history-shortcuts-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("create history GUI temp directory");
    let fixture = root.join("SampleNewsletter.pub");
    fs::write(&fixture, &original).expect("write history GUI PUB fixture");

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

    let (node_id, before) = {
        let app = harness.state();
        let editor = app.editor.as_ref().expect("editor loaded");
        app.visual
            .as_ref()
            .expect("visual loaded")
            .scene
            .nodes
            .iter()
            .find_map(|scene_node| {
                let authored = editor.graph().nodes.get(&scene_node.origin)?;
                let bounds = authored.header.bounds;
                editor
                    .can_move_node_to(scene_node.origin, bounds.x, bounds.y)
                    .ok()
                    .map(|_| (scene_node.origin, bounds))
            })
            .expect("fixture should expose one movable node")
    };

    let after = {
        let app = harness.state_mut();
        let editor = app.editor.as_mut().expect("editor loaded");
        assert_eq!(
            editor.operations().len(),
            0,
            "fresh fixture starts with no authoring operations"
        );
        let x = before
            .x
            .checked_add(pub_editor::LengthEmu::new(127_000))
            .expect("bounded x");
        let y = before
            .y
            .checked_add(pub_editor::LengthEmu::new(254_000))
            .expect("bounded y");
        editor
            .move_node_to(node_id, x, y)
            .expect("bounded MoveNode");
        let after = editor.graph().nodes[&node_id].header.bounds;
        assert_eq!(
            editor.operations().len(),
            1,
            "setup emits exactly one durable operation"
        );
        app.sync_visual_geometry_from_editor();
        after
    };
    harness.step();

    harness.press_key_modifiers(egui::Modifiers::CTRL, egui::Key::Z);
    harness.step();
    {
        let app = harness.state();
        let editor = app.editor.as_ref().expect("editor loaded");
        assert_eq!(editor.graph().nodes[&node_id].header.bounds, before);
        assert_eq!(
            editor.operations().len(),
            0,
            "Ctrl+Z moves the one operation to redo"
        );
        assert_eq!(
            app.visual
                .as_ref()
                .expect("visual loaded")
                .scene
                .nodes
                .iter()
                .find(|node| node.origin == node_id)
                .expect("node remains visible")
                .bounds,
            before,
            "Ctrl+Z must synchronize Viewer geometry through canonical apply_undo"
        );
    }

    harness.press_key_modifiers(egui::Modifiers::CTRL, egui::Key::Y);
    harness.step();
    {
        let app = harness.state();
        let editor = app.editor.as_ref().expect("editor loaded");
        assert_eq!(editor.graph().nodes[&node_id].header.bounds, after);
        assert_eq!(
            editor.operations().len(),
            1,
            "Ctrl+Y restores the original operation without appending a hidden operation"
        );
        assert_eq!(
            app.visual
                .as_ref()
                .expect("visual loaded")
                .scene
                .nodes
                .iter()
                .find(|node| node.origin == node_id)
                .expect("node remains visible")
                .bounds,
            after,
            "Ctrl+Y must synchronize Viewer geometry through canonical apply_redo"
        );
    }

    assert_eq!(
        fs::read(&fixture).expect("re-read source PUB"),
        original,
        "history shortcuts must not mutate source PUB bytes"
    );
    let _ = fs::remove_dir_all(root);
}
