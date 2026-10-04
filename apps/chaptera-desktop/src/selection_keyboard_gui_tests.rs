//! Existing pinned-PUB GUI acceptance for the selection-keyboard owner.

use super::*;

#[cfg(not(feature = "reader-only"))]
#[test]
#[ignore = "runtime GUI evidence requires pinned CHAPTERA_SAMPLE_NEWSLETTER"]
fn gui_canvas_keyboard_nudge_select_all_escape_and_delete_fail_closed_on_real_pub() {
    use egui_kittest::Harness;

    let fixture = std::env::var_os("CHAPTERA_SAMPLE_NEWSLETTER")
        .map(PathBuf::from)
        .expect("CHAPTERA_SAMPLE_NEWSLETTER must point to the pinned Apache POI fixture");
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1280.0, 820.0))
        .with_pixels_per_point(1.0)
        .with_max_steps(24)
        .build_eframe(move |cc| {
            fallback_font::install(&cc.egui_ctx)
                .expect("pinned Chaptera fallback font resource must validate");
            ViewerApp::new_with_storage(Some(fixture), cc.storage)
        });
    harness.step();
    harness.step();

    let (page_index, instance_id, node_id, before) = {
        let app = harness.state();
        let visual = app.visual.as_ref().expect("visual loaded");
        let editor = app.editor.as_ref().expect("editor loaded");
        visual
            .document
            .pages
            .iter()
            .enumerate()
            .find_map(|(page_index, page)| {
                let page_origin = page.id.into_canonical();
                let page_id_text = page.id.as_canonical().to_string();
                visual
                    .scene
                    .nodes
                    .iter()
                    .filter(|node| node.parent_origin == page_origin)
                    .find_map(|node| {
                        let instance =
                            direct_scene_instance(editor, &page_id_text, node.origin)?;
                        let admission =
                            admit_object_mutation_v1(&instance, ObjectMutationKindV1::MoveNode);
                        if !admission.admitted
                            || admission.origin_node_id.as_deref()
                                != Some(node.origin.as_canonical().to_string().as_str())
                        {
                            return None;
                        }
                        let bounds = editor.graph().nodes.get(&node.origin)?.header.bounds;
                        editor
                            .can_move_node_to(node.origin, bounds.x, bounds.y)
                            .ok()?;
                        Some((page_index, instance.instance_id, node.origin, bounds))
                    })
            })
            .expect("real fixture exposes one direct movable object")
    };

    {
        let app = harness.state_mut();
        app.selected_page = page_index;
        app.canvas_selection.select_only(instance_id);
    }
    let operations_before = harness
        .state()
        .editor
        .as_ref()
        .expect("editor")
        .operations()
        .len();

    harness.input_mut().events.push(egui::Event::Key {
        key: egui::Key::ArrowRight,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::default(),
    });
    harness.step();
    harness.step();

    let after = harness
        .state()
        .editor
        .as_ref()
        .expect("editor")
        .graph()
        .nodes[&node_id]
        .header
        .bounds;
    assert_eq!(
        after.x.get() - before.x.get(),
        selection_keyboard::BASE_NUDGE_EMU
    );
    assert_eq!(after.y, before.y);
    assert_eq!(
        harness
            .state()
            .editor
            .as_ref()
            .expect("editor")
            .operations()
            .len(),
        operations_before + 1,
        "one Arrow key must emit exactly one MoveNode"
    );
    assert!(matches!(
        harness
            .state()
            .editor
            .as_ref()
            .expect("editor")
            .operations()
            .last(),
        Some(pub_editor::EditOperation::MoveNode { node_id: moved, .. }) if *moved == node_id
    ));

    let expected_select_all = {
        let app = harness.state();
        let visual = app.visual.as_ref().expect("visual");
        let editor = app.editor.as_ref().expect("editor");
        let page = &visual.document.pages[page_index];
        let page_origin = page.id.into_canonical();
        let page_id_text = page.id.as_canonical().to_string();
        visual
            .scene
            .nodes
            .iter()
            .filter(|node| node.parent_origin == page_origin)
            .filter_map(|node| direct_scene_instance(editor, &page_id_text, node.origin))
            .map(|instance| instance.instance_id)
            .collect::<BTreeSet<_>>()
    };
    let operations_after_nudge = operations_before + 1;
    harness.input_mut().events.push(egui::Event::Key {
        key: egui::Key::A,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers {
            ctrl: true,
            ..Default::default()
        },
    });
    harness.step();
    assert_eq!(
        harness
            .state()
            .canvas_selection
            .iter()
            .map(str::to_owned)
            .collect::<BTreeSet<_>>(),
        expected_select_all
    );
    assert_eq!(
        harness
            .state()
            .editor
            .as_ref()
            .expect("editor")
            .operations()
            .len(),
        operations_after_nudge,
        "canvas Ctrl+A must remain transient"
    );

    harness.input_mut().events.push(egui::Event::Key {
        key: egui::Key::Delete,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::default(),
    });
    harness.step();
    assert_eq!(
        harness
            .state()
            .editor
            .as_ref()
            .expect("editor")
            .operations()
            .len(),
        operations_after_nudge,
        "object Delete remains deliberately unavailable in this slice"
    );

    harness.input_mut().events.push(egui::Event::Key {
        key: egui::Key::Escape,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::default(),
    });
    harness.step();
    assert_eq!(harness.state().canvas_selection.len(), 0);
    assert_eq!(
        harness
            .state()
            .editor
            .as_ref()
            .expect("editor")
            .operations()
            .len(),
        operations_after_nudge,
        "Escape selection clear must not create a document revision"
    );
}

#[cfg(not(feature = "reader-only"))]
#[test]
#[ignore = "runtime GUI evidence requires pinned CHAPTERA_SAMPLE_NEWSLETTER"]
fn gui_escape_cancels_active_object_gesture_before_clearing_selection_on_real_pub() {
    use egui_kittest::Harness;

    let fixture = std::env::var_os("CHAPTERA_SAMPLE_NEWSLETTER")
        .map(PathBuf::from)
        .expect("CHAPTERA_SAMPLE_NEWSLETTER must point to the pinned Apache POI fixture");
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1280.0, 820.0))
        .with_pixels_per_point(1.0)
        .with_max_steps(16)
        .build_eframe(move |cc| {
            fallback_font::install(&cc.egui_ctx)
                .expect("pinned Chaptera fallback font resource must validate");
            ViewerApp::new_with_storage(Some(fixture), cc.storage)
        });
    harness.step();
    harness.step();

    let (page_index, instance_id, node_id, before) = {
        let app = harness.state();
        let visual = app.visual.as_ref().expect("visual loaded");
        let editor = app.editor.as_ref().expect("editor loaded");
        visual
            .document
            .pages
            .iter()
            .enumerate()
            .find_map(|(page_index, page)| {
                let page_origin = page.id.into_canonical();
                let page_id_text = page.id.as_canonical().to_string();
                visual
                    .scene
                    .nodes
                    .iter()
                    .filter(|node| node.parent_origin == page_origin)
                    .find_map(|node| {
                        let instance =
                            direct_scene_instance(editor, &page_id_text, node.origin)?;
                        let admission =
                            admit_object_mutation_v1(&instance, ObjectMutationKindV1::MoveNode);
                        if !admission.admitted {
                            return None;
                        }
                        let bounds = editor.graph().nodes.get(&node.origin)?.header.bounds;
                        editor
                            .can_move_node_to(node.origin, bounds.x, bounds.y)
                            .ok()?;
                        Some((page_index, instance.instance_id, node.origin, bounds))
                    })
            })
            .expect("real fixture exposes one direct movable object")
    };

    let pointer_start = pub_interaction::DocumentPoint::new(
        pub_editor::LengthEmu::ZERO,
        pub_editor::LengthEmu::ZERO,
    );
    {
        let app = harness.state_mut();
        app.selected_page = page_index;
        app.canvas_selection.select_only(instance_id);
        app.canvas_drag = Some(
            MoveTransaction::begin(node_id, before, pointer_start)
                .expect("valid transient move gesture"),
        );
    }
    let operations_before = harness
        .state()
        .editor
        .as_ref()
        .expect("editor")
        .operations()
        .len();

    harness.input_mut().events.push(egui::Event::Key {
        key: egui::Key::Escape,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::default(),
    });
    harness.step();

    assert!(
        harness.state().canvas_drag.is_none(),
        "first Escape must cancel the active object gesture"
    );
    assert_eq!(
        harness.state().canvas_selection.len(),
        1,
        "gesture-owning Escape must not also clear selection"
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
        "gesture cancellation is transient"
    );

    harness.input_mut().events.push(egui::Event::Key {
        key: egui::Key::Escape,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::default(),
    });
    harness.step();

    assert_eq!(
        harness.state().canvas_selection.len(),
        0,
        "second Escape reaches the final selection-clearing fallback"
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
        "selection clearing is transient"
    );
}

#[cfg(not(feature = "reader-only"))]
#[test]
#[ignore = "runtime GUI evidence requires pinned CHAPTERA_SAMPLE_NEWSLETTER"]
fn gui_story_keyboard_keeps_arrows_text_owned_and_alt_nudges_owner_on_real_pub() {
    use egui_kittest::Harness;

    let fixture = std::env::var_os("CHAPTERA_SAMPLE_NEWSLETTER")
        .map(PathBuf::from)
        .expect("CHAPTERA_SAMPLE_NEWSLETTER must point to the pinned Apache POI fixture");
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1280.0, 820.0))
        .with_pixels_per_point(1.0)
        .with_max_steps(24)
        .build_eframe(move |cc| {
            fallback_font::install(&cc.egui_ctx)
                .expect("pinned Chaptera fallback font resource must validate");
            ViewerApp::new_with_storage(Some(fixture), cc.storage)
        });
    harness.step();
    harness.step();

    let (page_index, instance_id, story_id, frame_id, before) = {
        let app = harness.state();
        let visual = app.visual.as_ref().expect("visual loaded");
        let editor = app.editor.as_ref().expect("editor loaded");
        visual
            .text_fragments
            .iter()
            .find_map(|fragment| {
                text_session::enter_explicit_text_mode(
                    editor,
                    fragment.story_id,
                    fragment.frame_id,
                )
                .ok()?;
                let node = visual
                    .scene
                    .nodes
                    .iter()
                    .find(|node| node.origin == fragment.frame_id)?;
                let (page_index, page) = visual
                    .document
                    .pages
                    .iter()
                    .enumerate()
                    .find(|(_, page)| node.parent_origin == page.id.into_canonical())?;
                let page_id_text = page.id.as_canonical().to_string();
                let instance = direct_scene_instance(editor, &page_id_text, fragment.frame_id)?;
                let admission =
                    admit_object_mutation_v1(&instance, ObjectMutationKindV1::MoveNode);
                if !admission.admitted
                    || admission.origin_node_id.as_deref()
                        != Some(fragment.frame_id.as_canonical().to_string().as_str())
                {
                    return None;
                }
                let bounds = editor.graph().nodes.get(&fragment.frame_id)?.header.bounds;
                editor
                    .can_move_node_to(fragment.frame_id, bounds.x, bounds.y)
                    .ok()?;
                Some((
                    page_index,
                    instance.instance_id,
                    fragment.story_id,
                    fragment.frame_id,
                    bounds,
                ))
            })
            .expect("real fixture exposes one editable movable direct TextFrame")
    };

    {
        let app = harness.state_mut();
        app.selected_page = page_index;
        app.canvas_selection.select_only(instance_id);
        app.enter_canvas_text_mode(story_id, frame_id);
    }
    assert!(harness.state().text_mode.is_some());
    let operations_before = harness
        .state()
        .editor
        .as_ref()
        .expect("editor")
        .operations()
        .len();

    harness.input_mut().events.push(egui::Event::Key {
        key: egui::Key::ArrowLeft,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::default(),
    });
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
        "ordinary Story Arrow must stay transient text navigation"
    );
    assert_eq!(
        harness
            .state()
            .editor
            .as_ref()
            .expect("editor")
            .graph()
            .nodes[&frame_id]
            .header
            .bounds,
        before,
        "ordinary Story Arrow must not move the owning object"
    );

    harness.input_mut().events.push(egui::Event::Key {
        key: egui::Key::ArrowRight,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers {
            alt: true,
            ..Default::default()
        },
    });
    harness.step();
    harness.step();

    let after = harness
        .state()
        .editor
        .as_ref()
        .expect("editor")
        .graph()
        .nodes[&frame_id]
        .header
        .bounds;
    assert_eq!(
        after.x.get() - before.x.get(),
        selection_keyboard::BASE_NUDGE_EMU
    );
    assert_eq!(after.y, before.y);
    assert_eq!(
        harness
            .state()
            .editor
            .as_ref()
            .expect("editor")
            .operations()
            .len(),
        operations_before + 1,
        "Alt+Arrow must emit exactly one MoveNode for the owning TextFrame"
    );
    {
        let app = harness.state();
        let editor = app.editor.as_ref().expect("editor");
        let mode = app.text_mode.as_ref().expect("Story mode remains active");
        assert_eq!(
            mode.session.revision_id,
            editor.project().state_id_v1(),
            "active Story selection must rebind to the post-MoveNode revision"
        );
    }

    let operations_after_nudge = operations_before + 1;
    harness.input_mut().events.push(egui::Event::Key {
        key: egui::Key::A,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers {
            ctrl: true,
            ..Default::default()
        },
    });
    harness.step();
    {
        let app = harness.state();
        let mode = app.text_mode.as_ref().expect("Story mode");
        assert_eq!(mode.session.selection.anchor_scalar, 0);
        assert_eq!(
            mode.session.selection.focus_scalar,
            mode.domain.raw_scalar_len
        );
        assert_eq!(
            app.editor.as_ref().expect("editor").operations().len(),
            operations_after_nudge,
            "Story Ctrl+A must remain transient"
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
    assert_eq!(
        harness.state().canvas_selection.len(),
        1,
        "the same Escape that exits Story mode must not also clear object selection"
    );
    assert_eq!(
        harness
            .state()
            .editor
            .as_ref()
            .expect("editor")
            .operations()
            .len(),
        operations_after_nudge
    );

    harness.input_mut().events.push(egui::Event::Key {
        key: egui::Key::Escape,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::default(),
    });
    harness.step();
    assert_eq!(harness.state().canvas_selection.len(), 0);
    assert_eq!(
        harness
            .state()
            .editor
            .as_ref()
            .expect("editor")
            .operations()
            .len(),
        operations_after_nudge
    );
}
