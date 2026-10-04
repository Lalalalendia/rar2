//! Bounded real-PUB GUI acceptance for Desktop history shortcuts.

use super::*;
use std::fs;
use std::path::PathBuf;

#[cfg(not(feature = "reader-only"))]
#[test]
#[ignore = "runtime GUI evidence requires pinned CHAPTERA_SAMPLE_NEWSLETTER"]
fn gui_history_shortcuts_undo_redo_real_pub() {
    use egui_kittest::Harness;

    let fixture_source = std::env::var_os("CHAPTERA_SAMPLE_NEWSLETTER")
        .map(PathBuf::from)
        .expect("CHAPTERA_SAMPLE_NEWSLETTER must point to the pinned Apache POI fixture");
    let original = fs::read(&fixture_source).expect("read pinned SampleNewsletter fixture");
    let root =
        std::env::temp_dir().join(format!("chaptera-gui-history-shortcuts-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("create history GUI temp directory");
    let fixture = root.join("SampleNewsletter.pub");
    fs::write(&fixture, &original).expect("write history GUI PUB fixture");

    let fixture_for_app = fixture.clone();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1280.0, 820.0))
        .with_pixels_per_point(1.0)
        .with_max_steps(24)
        .build_eframe(move |cc| {
            fallback_font::install(&cc.egui_ctx)
                .expect("pinned Chaptera fallback font resource must validate");
            ViewerApp::new_with_storage(Some(fixture_for_app), cc.storage)
        });
    harness.step();
    harness.step();

    let (node_id, before, target_x, target_y, operations_before) = {
        let app = harness.state();
        let editor = app.editor.as_ref().expect("editor loaded");
        let candidate = editor
            .graph()
            .nodes
            .iter()
            .find_map(|(node_id, node)| {
                [118_872_i64, -118_872_i64].into_iter().find_map(|dx| {
                    let x = node
                        .header
                        .bounds
                        .x
                        .checked_add(pub_editor::LengthEmu::new(dx))?;
                    let y = node.header.bounds.y;
                    editor
                        .can_move_node_to(node_id.clone(), x, y)
                        .ok()
                        .map(|_| (node_id.clone(), node.header.bounds, x, y))
                })
            })
            .expect("real fixture exposes one movable authored object");
        (
            candidate.0,
            candidate.1,
            candidate.2,
            candidate.3,
            editor.operations().len(),
        )
    };

    {
        let app = harness.state_mut();
        app.editor
            .as_mut()
            .expect("editor")
            .move_node_to(node_id.clone(), target_x, target_y)
            .expect("seed one canonical MoveNode operation");
    }
    let after = harness
        .state()
        .editor
        .as_ref()
        .expect("editor")
        .graph()
        .nodes[&node_id]
        .header
        .bounds;
    assert_ne!(after, before, "seed MoveNode must change geometry");
    assert_eq!(
        harness
            .state()
            .editor
            .as_ref()
            .expect("editor")
            .operations()
            .len(),
        operations_before + 1
    );

    harness.press_key_modifiers(egui::Modifiers::CTRL, egui::Key::Z);
    harness.step();
    assert_eq!(
        harness
            .state()
            .editor
            .as_ref()
            .expect("editor")
            .graph()
            .nodes[&node_id]
            .header
            .bounds,
        before,
        "Ctrl+Z must route to canonical Undo geometry"
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
        "Ctrl+Z must not append a hidden operation"
    );

    harness.press_key_modifiers(egui::Modifiers::CTRL, egui::Key::Y);
    harness.step();
    assert_eq!(
        harness
            .state()
            .editor
            .as_ref()
            .expect("editor")
            .graph()
            .nodes[&node_id]
            .header
            .bounds,
        after,
        "Ctrl+Y must route to canonical Redo geometry"
    );
    assert_eq!(
        harness
            .state()
            .editor
            .as_ref()
            .expect("editor")
            .operations()
            .len(),
        operations_before + 1,
        "Ctrl+Y must restore the same operation without appending another one"
    );

    assert_eq!(
        fs::read(&fixture).expect("re-read source PUB"),
        original,
        "History shortcuts must not mutate source PUB bytes"
    );

    let _ = fs::remove_dir_all(root);
}
