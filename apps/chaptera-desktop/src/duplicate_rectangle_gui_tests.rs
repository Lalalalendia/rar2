//! Real-PUB GUI acceptance for Duplicate Rectangle.

use crate::*;

#[cfg(not(feature = "reader-only"))]
#[test]
#[ignore = "runtime GUI evidence requires pinned CHAPTERA_SAMPLE_NEWSLETTER"]
fn gui_duplicate_button_commits_one_create_shape_and_selects_duplicate_on_real_pub() {
    use egui_kittest::{Harness, kittest::Queryable};

    let fixture = std::env::var_os("CHAPTERA_SAMPLE_NEWSLETTER")
        .map(PathBuf::from)
        .expect("CHAPTERA_SAMPLE_NEWSLETTER must point to the pinned Apache POI fixture");
    let original = fs::read(&fixture).expect("read pinned SampleNewsletter fixture");
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
    harness.step();

    let (page_index, page_id, source_bounds) = {
        let app = harness.state();
        let visual = app.visual.as_ref().expect("visual loaded");
        let page = visual
            .document
            .pages
            .first()
            .expect("real fixture exposes a first page");
        let surface = visual
            .scene
            .surfaces
            .iter()
            .find(|surface| surface.origin == page.id)
            .expect("first page has a scene surface");
        let width = (surface.size.width.get() / 5).max(127_000);
        let height = (surface.size.height.get() / 8).max(127_000);
        (
            0,
            page.id,
            pub_editor::RectEmu::new(
                pub_editor::LengthEmu::new(surface.size.width.get() / 4),
                pub_editor::LengthEmu::new(surface.size.height.get() / 4),
                pub_editor::LengthEmu::new(width),
                pub_editor::LengthEmu::new(height),
            ),
        )
    };

    let source_node_id = pub_editor::NodeId::from_canonical(pub_model::new_editor_canonical_id());
    {
        let app = harness.state_mut();
        app.selected_page = page_index;
        app.editor
            .as_mut()
            .expect("editor")
            .create_shape(
                source_node_id,
                page_id,
                source_bounds,
                rectangle_creation::chaptera_rectangle_paint_v1(),
            )
            .expect("seed authored Rectangle through canonical CreateShape");
        app.finish_authoring_change("Seeded Duplicate GUI witness.");
        let instance = direct_page_local_instance_v1(
            &source_node_id.as_canonical().to_string(),
            &page_id.as_canonical().to_string(),
        )
        .expect("canonical source instance");
        app.canvas_selection.select_only(instance.instance_id);
    }
    harness.step();

    let operations_before = harness
        .state()
        .editor
        .as_ref()
        .expect("editor")
        .operations()
        .len();
    harness.get_by_label("Duplicate").click();
    harness.step();
    harness.step();

    let duplicate_node_id = {
        let app = harness.state();
        let editor = app.editor.as_ref().expect("editor");
        assert_eq!(
            editor.operations().len(),
            operations_before + 1,
            "one Duplicate click must append exactly one document operation"
        );
        let Some(pub_editor::EditOperation::CreateShape { node_id, .. }) =
            editor.operations().last()
        else {
            panic!("Duplicate must persist as CreateShape")
        };
        assert_ne!(*node_id, source_node_id);
        let source = editor
            .authored_shape(source_node_id)
            .expect("source authored shape");
        let duplicate = editor
            .authored_shape(*node_id)
            .expect("duplicate authored shape");
        assert_eq!(duplicate.paint, source.paint);
        assert_eq!(duplicate.bounds.width, source.bounds.width);
        assert_eq!(duplicate.bounds.height, source.bounds.height);
        assert_eq!(
            duplicate.bounds.x.get(),
            source.bounds.x.get() + pub_editor::DUPLICATE_OFFSET_EMU_V1
        );
        assert_eq!(
            duplicate.bounds.y.get(),
            source.bounds.y.get() + pub_editor::DUPLICATE_OFFSET_EMU_V1
        );
        let expected_instance = direct_page_local_instance_v1(
            &node_id.as_canonical().to_string(),
            &page_id.as_canonical().to_string(),
        )
        .expect("canonical duplicate instance");
        assert_eq!(
            app.canvas_selection.primary(),
            Some(expected_instance.instance_id.as_str()),
            "accepted Duplicate must select the durable duplicate"
        );
        *node_id
    };

    harness.get_by_label("Undo").click();
    harness.step();
    assert!(
        harness
            .state()
            .editor
            .as_ref()
            .expect("editor")
            .authored_shape(duplicate_node_id)
            .is_none(),
        "Undo must remove the duplicate"
    );

    harness.get_by_label("Redo").click();
    harness.step();
    let app = harness.state();
    assert!(
        app.editor
            .as_ref()
            .expect("editor")
            .authored_shape(duplicate_node_id)
            .is_some(),
        "Redo must restore the same duplicate identity"
    );
    assert_eq!(
        fs::read(&fixture).expect("re-read source PUB"),
        original,
        "Duplicate must never mutate source PUB bytes"
    );
}
