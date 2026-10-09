//! Windows GUI acceptance: real on-canvas selection, raw resize control and Ctrl/Shift toggles.

use crate::*;

#[cfg(not(feature = "reader-only"))]
#[test]
#[ignore = "runtime GUI evidence requires pinned CHAPTERA_SAMPLE_NEWSLETTER"]
fn gui_resize_handle_commits_one_resize_node() {
    use egui_kittest::{Harness, kittest::Queryable};

    let fixture = std::env::var_os("CHAPTERA_SAMPLE_NEWSLETTER")
        .map(PathBuf::from)
        .expect("CHAPTERA_SAMPLE_NEWSLETTER must point to the pinned Apache POI fixture");
    let original = fs::read(&fixture).expect("read pinned SampleNewsletter fixture");

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

    let (page_label, target_document_point) = {
        let app = harness.state();
        let visual = app.visual.as_ref().expect("visual loaded");
        let editor = app.editor.as_ref().expect("editor loaded");
        visual
            .document
            .pages
            .iter()
            .find_map(|page| {
                let page_origin = page.id.into_canonical();
                let page_id_text = page.id.as_canonical().to_string();
                let page_nodes = visual
                    .scene
                    .nodes
                    .iter()
                    .filter(|node| node.parent_origin == page_origin)
                    .collect::<Vec<_>>();
                let hit_index = SceneHitTestIndex::new(
                    page_nodes
                        .iter()
                        .enumerate()
                        .filter_map(|(paint_order, node)| {
                            let instance =
                                direct_scene_instance(editor, &page_id_text, node.origin)?;
                            Some(SceneHitEntry {
                                instance_id: instance.instance_id,
                                node_id: node.origin,
                                bounds: node.bounds,
                                z_order: 0,
                                paint_order: u32::try_from(paint_order).unwrap_or(u32::MAX),
                            })
                        })
                        .collect(),
                );

                hit_index.entries.iter().rev().find_map(|hit| {
                    let instance = direct_scene_instance(editor, &page_id_text, hit.node_id)?;
                    let admission =
                        admit_object_mutation_v1(&instance, ObjectMutationKindV1::ResizeNode);
                    if !admission.admitted
                        || admission.origin_node_id.as_deref()
                            != Some(hit.node_id.as_canonical().to_string().as_str())
                        || editor.can_resize_node(hit.node_id).is_err()
                    {
                        return None;
                    }
                    let point = pub_interaction::DocumentPoint::new(
                        pub_editor::LengthEmu::new(hit.bounds.x.get() + hit.bounds.width.get() / 2),
                        pub_editor::LengthEmu::new(
                            hit.bounds.y.get() + hit.bounds.height.get() / 2,
                        ),
                    );
                    hit_index
                        .topmost_at(point)
                        .filter(|top| top.instance_id == hit.instance_id)
                        .map(|_| (format!("Page {}", page.index), point))
                })
            })
            .expect("real fixture exposes a topmost ResizeNode-admitted object")
    };

    harness.get_by_label(&page_label).click();
    harness.step();

    let object_center = {
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
            .expect("selected resize page remains available");
        let surface = visual
            .scene
            .surfaces
            .iter()
            .find(|surface| surface.origin == page.id)
            .expect("selected resize page has a scene surface");
        let viewport = egui::vec2(
            (canvas.x1 - canvas.x0) as f32,
            (canvas.y1 - canvas.y0) as f32,
        );
        let fit_scale = fitted_scale(
            surface.size.width.get(),
            surface.size.height.get(),
            viewport,
        )
        .expect("selected resize page has valid fit scale");
        let scene_scale = fit_scale * app.zoom;
        let page_width = surface.size.width.get() as f32 * scene_scale;
        let page_height = surface.size.height.get() as f32 * scene_scale;
        let page_left = ((canvas.x0 + canvas.x1) as f32 - page_width) / 2.0;
        let page_top = ((canvas.y0 + canvas.y1) as f32 - page_height) / 2.0;
        egui::pos2(
            page_left + target_document_point.x.get() as f32 * scene_scale,
            page_top + target_document_point.y.get() as f32 * scene_scale,
        )
    };
    harness.input_mut().events.extend([
        egui::Event::PointerMoved(object_center),
        egui::Event::PointerButton {
            pos: object_center,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::default(),
        },
        egui::Event::PointerButton {
            pos: object_center,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::default(),
        },
    ]);
    harness.step();
    harness.step();

    let handle_bounds = harness
        .get_by_label("Resize bottom-right handle")
        .raw_bounds()
        .expect("selected resizable object exposes bottom-right handle");
    let start = egui::pos2(
        ((handle_bounds.x0 + handle_bounds.x1) / 2.0) as f32,
        ((handle_bounds.y0 + handle_bounds.y1) / 2.0) as f32,
    );
    let end = start + egui::vec2(18.0, 12.0);

    harness.input_mut().events.extend([
        egui::Event::PointerMoved(start),
        egui::Event::PointerButton {
            pos: start,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::default(),
        },
    ]);
    harness.step();
    assert_eq!(
        harness
            .state()
            .editor
            .as_ref()
            .expect("editor")
            .operations()
            .len(),
        0,
        "resize pointer-down/preview must not emit an Editor operation"
    );

    harness
        .input_mut()
        .events
        .push(egui::Event::PointerMoved(end));
    harness.step();
    assert_eq!(
        harness
            .state()
            .editor
            .as_ref()
            .expect("editor")
            .operations()
            .len(),
        0,
        "resize pointer motion must remain transient"
    );

    harness.input_mut().events.push(egui::Event::PointerButton {
        pos: end,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::default(),
    });
    harness.step();
    harness.step();

    let (node_id, before, after) = {
        let editor = harness.state().editor.as_ref().expect("editor");
        assert_eq!(
            editor.operations().len(),
            1,
            "handle release must emit exactly one ResizeNode"
        );
        match editor.operations().last().expect("resize operation") {
            pub_editor::EditOperation::ResizeNode {
                node_id,
                before,
                after,
            } => (*node_id, *before, *after),
            other => panic!("resize handle emitted unexpected operation: {other:?}"),
        }
    };
    assert_ne!(before.width, after.width);
    assert_ne!(before.height, after.height);

    harness
        .get_all_by_label("Undo")
        .next()
        .expect("Undo command")
        .click();
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
        "GUI Undo restores exact pre-resize bounds"
    );

    harness
        .get_all_by_label("Redo")
        .next()
        .expect("Redo command")
        .click();
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
        "GUI Redo restores exact resized bounds"
    );
    assert_eq!(
        fs::read(&fixture).expect("read immutable source after resize"),
        original,
        "GUI resize must not mutate source PUB bytes"
    );
}

#[cfg(not(feature = "reader-only"))]
#[test]
#[ignore = "runtime GUI evidence requires pinned CHAPTERA_SAMPLE_NEWSLETTER"]
fn gui_resize_modifiers_toggle_mid_drag_and_commit_exact_constraint_plan() {
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

    let (page_label, target_document_point, node_id, source_bounds) = {
        let app = harness.state();
        let visual = app.visual.as_ref().expect("visual loaded");
        let editor = app.editor.as_ref().expect("editor loaded");
        visual
            .document
            .pages
            .iter()
            .find_map(|page| {
                let page_origin = page.id.into_canonical();
                let page_id_text = page.id.as_canonical().to_string();
                let page_nodes = visual
                    .scene
                    .nodes
                    .iter()
                    .filter(|node| node.parent_origin == page_origin)
                    .collect::<Vec<_>>();
                let hit_index = SceneHitTestIndex::new(
                    page_nodes
                        .iter()
                        .enumerate()
                        .filter_map(|(paint_order, node)| {
                            let instance =
                                direct_scene_instance(editor, &page_id_text, node.origin)?;
                            Some(SceneHitEntry {
                                instance_id: instance.instance_id,
                                node_id: node.origin,
                                bounds: node.bounds,
                                z_order: 0,
                                paint_order: u32::try_from(paint_order).unwrap_or(u32::MAX),
                            })
                        })
                        .collect(),
                );

                hit_index.entries.iter().rev().find_map(|hit| {
                    let instance = direct_scene_instance(editor, &page_id_text, hit.node_id)?;
                    let admission =
                        admit_object_mutation_v1(&instance, ObjectMutationKindV1::ResizeNode);
                    if !admission.admitted
                        || admission.origin_node_id.as_deref()
                            != Some(hit.node_id.as_canonical().to_string().as_str())
                        || editor.can_resize_node(hit.node_id).is_err()
                    {
                        return None;
                    }
                    let point = pub_interaction::DocumentPoint::new(
                        pub_editor::LengthEmu::new(hit.bounds.x.get() + hit.bounds.width.get() / 2),
                        pub_editor::LengthEmu::new(
                            hit.bounds.y.get() + hit.bounds.height.get() / 2,
                        ),
                    );
                    hit_index
                        .topmost_at(point)
                        .filter(|top| top.instance_id == hit.instance_id)
                        .map(|_| {
                            (
                                format!("Page {}", page.index),
                                point,
                                hit.node_id,
                                hit.bounds,
                            )
                        })
                })
            })
            .expect("real fixture exposes a topmost ResizeNode-admitted object")
    };

    harness.get_by_label(&page_label).click();
    harness.step();

    let object_center = {
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
            .expect("selected resize page remains available");
        let surface = visual
            .scene
            .surfaces
            .iter()
            .find(|surface| surface.origin == page.id)
            .expect("selected resize page has a scene surface");
        let viewport = egui::vec2(
            (canvas.x1 - canvas.x0) as f32,
            (canvas.y1 - canvas.y0) as f32,
        );
        let fit_scale = fitted_scale(
            surface.size.width.get(),
            surface.size.height.get(),
            viewport,
        )
        .expect("selected resize page has valid fit scale");
        let scene_scale = fit_scale * app.zoom;
        let page_width = surface.size.width.get() as f32 * scene_scale;
        let page_height = surface.size.height.get() as f32 * scene_scale;
        let page_left = ((canvas.x0 + canvas.x1) as f32 - page_width) / 2.0;
        let page_top = ((canvas.y0 + canvas.y1) as f32 - page_height) / 2.0;
        egui::pos2(
            page_left + target_document_point.x.get() as f32 * scene_scale,
            page_top + target_document_point.y.get() as f32 * scene_scale,
        )
    };
    harness.input_mut().events.extend([
        egui::Event::PointerMoved(object_center),
        egui::Event::PointerButton {
            pos: object_center,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::default(),
        },
        egui::Event::PointerButton {
            pos: object_center,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::default(),
        },
    ]);
    harness.step();
    harness.step();

    let handle_bounds = harness
        .get_by_label("Resize bottom-right handle")
        .raw_bounds()
        .expect("selected authored Rectangle exposes bottom-right handle");
    let start = egui::pos2(
        ((handle_bounds.x0 + handle_bounds.x1) / 2.0) as f32,
        ((handle_bounds.y0 + handle_bounds.y1) / 2.0) as f32,
    );
    let intermediate = start + egui::vec2(10.0, 6.0);
    let end = start + egui::vec2(24.0, 14.0);

    let (page_rect, scene_scale) = {
        let canvas = harness
            .get_by_label("Document canvas")
            .raw_bounds()
            .expect("canvas bounds");
        let app = harness.state();
        let visual = app.visual.as_ref().expect("visual");
        let page = visual.document.pages.get(app.selected_page).expect("page");
        let surface = visual
            .scene
            .surfaces
            .iter()
            .find(|surface| surface.origin == page.id)
            .expect("surface");
        let viewport = egui::vec2(
            (canvas.x1 - canvas.x0) as f32,
            (canvas.y1 - canvas.y0) as f32,
        );
        let scene_scale = fitted_scale(
            surface.size.width.get(),
            surface.size.height.get(),
            viewport,
        )
        .expect("fit scale")
            * app.zoom;
        let page_width = surface.size.width.get() as f32 * scene_scale;
        let page_height = surface.size.height.get() as f32 * scene_scale;
        let page_left = ((canvas.x0 + canvas.x1) as f32 - page_width) / 2.0;
        let page_top = ((canvas.y0 + canvas.y1) as f32 - page_height) / 2.0;
        (
            egui::Rect::from_min_size(
                egui::pos2(page_left, page_top),
                egui::vec2(page_width, page_height),
            ),
            scene_scale,
        )
    };
    let start_document =
        canvas_document_point(page_rect, scene_scale, start).expect("start document point");
    let end_document =
        canvas_document_point(page_rect, scene_scale, end).expect("end document point");
    let mut expected_raw = ResizeTransaction::begin(
        node_id,
        source_bounds,
        ResizeHandle::BottomRight,
        start_document,
    )
    .expect("expected raw transaction");
    let ResizeUpdate::Preview(raw_target) = expected_raw
        .update(end_document)
        .expect("expected raw target")
    else {
        panic!("final GUI pointer must produce a valid raw resize target")
    };
    let ctrl_shift_mask = ResizeModifierMaskV1 {
        centered: true,
        aspect_lock: true,
    };
    let expected = pub_interaction::plan_resize_constraint_v1(
        source_bounds,
        ResizeHandle::BottomRight,
        raw_target,
        ctrl_shift_mask,
    )
    .expect("exact constrained plan")
    .constrained_rect;

    let operations_before = harness
        .state()
        .editor
        .as_ref()
        .expect("editor")
        .operations()
        .len();

    harness.input_mut().modifiers = egui::Modifiers::default();
    harness.input_mut().events.extend([
        egui::Event::PointerMoved(start),
        egui::Event::PointerButton {
            pos: start,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::default(),
        },
    ]);
    harness.step();
    harness
        .input_mut()
        .events
        .push(egui::Event::PointerMoved(intermediate));
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
        "unmodified mid-drag preview remains transient"
    );

    let ctrl_shift = egui::Modifiers {
        ctrl: true,
        shift: true,
        ..Default::default()
    };
    harness.input_mut().modifiers = ctrl_shift;
    harness
        .input_mut()
        .events
        .push(egui::Event::PointerMoved(end));
    harness.step();
    assert_eq!(
        harness
            .state()
            .canvas_resize
            .expect("active constrained resize")
            .preview_bounds(),
        Some(expected),
        "mid-drag modifier toggle must recompute exact constrained preview"
    );

    harness.input_mut().events.push(egui::Event::PointerButton {
        pos: end,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: ctrl_shift,
    });
    harness.step();
    harness.step();
    harness.input_mut().modifiers = egui::Modifiers::default();

    let after = {
        let editor = harness.state().editor.as_ref().expect("editor");
        assert_eq!(
            editor.operations().len(),
            operations_before + 1,
            "modifier resize release emits exactly one ResizeNode"
        );
        match editor.operations().last().expect("resize operation") {
            pub_editor::EditOperation::ResizeNode {
                node_id: actual_node_id,
                before,
                after,
            } => {
                assert_eq!(*actual_node_id, node_id);
                assert_eq!(*before, source_bounds);
                assert_eq!(*after, expected);
                *after
            }
            other => panic!("modifier resize emitted unexpected operation: {other:?}"),
        }
    };

    harness.get_by_label("Undo").click();
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
        source_bounds,
        "Undo restores exact pre-modifier bounds"
    );
    harness.get_by_label("Redo").click();
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
        "Redo restores exact constrained bounds"
    );
    assert_eq!(
        fs::read(&fixture).expect("re-read source PUB"),
        original,
        "modifier resize must not mutate source PUB bytes"
    );
}
