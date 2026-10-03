use pub_interaction::{
    RESIZE_CONSTRAINT_MAX_SAFE_EMU_V1, ResizeAspectControlAxisV1, ResizeConstraintErrorV1,
    ResizeHandle, ResizeModifierMaskV1, plan_resize_constraint_v1,
};
use pub_model::{LengthEmu, RectEmu};

fn rect(x: i64, y: i64, width: i64, height: i64) -> RectEmu {
    RectEmu::new(
        LengthEmu::new(x),
        LengthEmu::new(y),
        LengthEmu::new(width),
        LengthEmu::new(height),
    )
}

#[test]
fn no_modifier_east_preserves_base_left_and_inactive_y() {
    let base = rect(100, 200, 80, 40);
    let raw = rect(100, 999, 120, 7);
    let plan = plan_resize_constraint_v1(
        base,
        ResizeHandle::Right,
        raw,
        ResizeModifierMaskV1::default(),
    )
    .expect("plain east");
    assert_eq!(plan.constrained_rect, rect(100, 200, 120, 40));
    assert!(!plan.centered_applied);
    assert!(!plan.aspect_applied);
    assert_eq!(plan.aspect_control_axis, None);
}

#[test]
fn aspect_lock_is_inert_on_edge_handle_v1() {
    let base = rect(100, 200, 80, 40);
    let raw = rect(100, 200, 120, 100);
    let plain = plan_resize_constraint_v1(
        base,
        ResizeHandle::Right,
        raw,
        ResizeModifierMaskV1::default(),
    )
    .expect("plain");
    let shifted = plan_resize_constraint_v1(
        base,
        ResizeHandle::Right,
        raw,
        ResizeModifierMaskV1 {
            centered: false,
            aspect_lock: true,
        },
    )
    .expect("shifted edge");
    assert_eq!(plain.constrained_rect, shifted.constrained_rect);
    assert!(!shifted.aspect_applied);
}

#[test]
fn centered_east_mirrors_across_exact_doubled_center() {
    let base = rect(100, 200, 80, 40);
    let raw = rect(100, 200, 100, 40);
    let plan = plan_resize_constraint_v1(
        base,
        ResizeHandle::Right,
        raw,
        ResizeModifierMaskV1 {
            centered: true,
            aspect_lock: false,
        },
    )
    .expect("centered east");
    assert_eq!(plan.constrained_rect, rect(80, 200, 120, 40));
    assert_eq!(
        base.x.get() + base.right().unwrap().get(),
        plan.constrained_rect.x.get() + plan.constrained_rect.right().unwrap().get()
    );
    assert_eq!(base.y, plan.constrained_rect.y);
    assert_eq!(base.height, plan.constrained_rect.height);
}

#[test]
fn aspect_corner_x_control_uses_exact_rational_rounding() {
    let plan = plan_resize_constraint_v1(
        rect(0, 0, 3, 2),
        ResizeHandle::BottomRight,
        rect(0, 0, 4, 2),
        ResizeModifierMaskV1 {
            centered: false,
            aspect_lock: true,
        },
    )
    .expect("aspect x");
    assert_eq!(
        plan.aspect_control_axis,
        Some(ResizeAspectControlAxisV1::X)
    );
    assert!(plan.aspect_applied);
    assert_eq!(plan.constrained_rect, rect(0, 0, 4, 3));
}

#[test]
fn normalized_change_tie_chooses_x() {
    let plan = plan_resize_constraint_v1(
        rect(10, 20, 100, 50),
        ResizeHandle::BottomRight,
        rect(10, 20, 150, 75),
        ResizeModifierMaskV1 {
            centered: false,
            aspect_lock: true,
        },
    )
    .expect("normalized tie");
    assert_eq!(
        plan.aspect_control_axis,
        Some(ResizeAspectControlAxisV1::X)
    );
    assert_eq!(plan.constrained_rect, rect(10, 20, 150, 75));
}

#[test]
fn aspect_y_control_can_expand_other_axis_beyond_raw() {
    let plan = plan_resize_constraint_v1(
        rect(0, 0, 100, 50),
        ResizeHandle::BottomRight,
        rect(0, 0, 150, 100),
        ResizeModifierMaskV1 {
            centered: false,
            aspect_lock: true,
        },
    )
    .expect("aspect y");
    assert_eq!(
        plan.aspect_control_axis,
        Some(ResizeAspectControlAxisV1::Y)
    );
    assert_eq!(plan.constrained_rect, rect(0, 0, 200, 100));
}

#[test]
fn centered_aspect_nearest_required_parity_tie_chooses_larger() {
    let base = rect(0, 0, 4, 2);
    let plan = plan_resize_constraint_v1(
        base,
        ResizeHandle::BottomRight,
        rect(0, 0, 7, 2),
        ResizeModifierMaskV1 {
            centered: true,
            aspect_lock: true,
        },
    )
    .expect("centered aspect");
    assert_eq!(
        plan.aspect_control_axis,
        Some(ResizeAspectControlAxisV1::X)
    );
    assert_eq!(plan.constrained_rect, rect(-3, -2, 10, 6));
    assert_eq!(
        base.x.get() + base.right().unwrap().get(),
        plan.constrained_rect.x.get() + plan.constrained_rect.right().unwrap().get()
    );
    assert_eq!(
        base.y.get() + base.bottom().unwrap().get(),
        plan.constrained_rect.y.get() + plan.constrained_rect.bottom().unwrap().get()
    );
}

#[test]
fn centered_odd_base_extent_preserves_required_parity() {
    let plan = plan_resize_constraint_v1(
        rect(0, 0, 5, 5),
        ResizeHandle::BottomRight,
        rect(0, 0, 6, 6),
        ResizeModifierMaskV1 {
            centered: true,
            aspect_lock: false,
        },
    )
    .expect("odd centered");
    assert_eq!(plan.constrained_rect.width.get(), 7);
    assert_eq!(plan.constrained_rect.height.get(), 7);
    assert_eq!(plan.constrained_rect.width.get() % 2, 1);
    assert_eq!(plan.constrained_rect.height.get() % 2, 1);
}

#[test]
fn modifier_recompute_is_stateless_from_same_base_and_raw() {
    let base = rect(100, 100, 80, 40);
    let raw = rect(100, 100, 140, 90);
    let plain1 = plan_resize_constraint_v1(
        base,
        ResizeHandle::BottomRight,
        raw,
        ResizeModifierMaskV1::default(),
    )
    .expect("plain1");
    let _ = plan_resize_constraint_v1(
        base,
        ResizeHandle::BottomRight,
        raw,
        ResizeModifierMaskV1 {
            centered: true,
            aspect_lock: true,
        },
    )
    .expect("modified");
    let plain2 = plan_resize_constraint_v1(
        base,
        ResizeHandle::BottomRight,
        raw,
        ResizeModifierMaskV1::default(),
    )
    .expect("plain2");
    assert_eq!(plain1, plain2);
}

#[test]
fn crossing_fixed_edge_or_center_fails_closed() {
    let base = rect(100, 100, 80, 40);
    assert!(matches!(
        plan_resize_constraint_v1(
            base,
            ResizeHandle::Left,
            rect(190, 100, 10, 40),
            ResizeModifierMaskV1::default(),
        ),
        Err(ResizeConstraintErrorV1::CrossedAxis("horizontal"))
    ));
    assert!(matches!(
        plan_resize_constraint_v1(
            base,
            ResizeHandle::Right,
            rect(0, 100, 130, 40),
            ResizeModifierMaskV1 {
                centered: true,
                aspect_lock: false,
            },
        ),
        Err(ResizeConstraintErrorV1::CrossedAxis("horizontal"))
    ));
}

#[test]
fn large_safe_coordinates_do_not_require_center_when_uncentered() {
    let max = RESIZE_CONSTRAINT_MAX_SAFE_EMU_V1;
    let plan = plan_resize_constraint_v1(
        rect(max - 1000, 0, 400, 100),
        ResizeHandle::Right,
        rect(max - 1000, 0, 500, 100),
        ResizeModifierMaskV1::default(),
    )
    .expect("large plain");
    assert_eq!(plan.constrained_rect.width.get(), 500);
}

#[test]
fn centered_mode_rejects_unsafe_doubled_center() {
    let max = RESIZE_CONSTRAINT_MAX_SAFE_EMU_V1;
    assert!(matches!(
        plan_resize_constraint_v1(
            rect(max - 1000, 0, 400, 100),
            ResizeHandle::Right,
            rect(max - 1000, 0, 500, 100),
            ResizeModifierMaskV1 {
                centered: true,
                aspect_lock: false,
            },
        ),
        Err(ResizeConstraintErrorV1::UnsafeEmu("base.center2_x"))
    ));
}

#[test]
fn changed_flag_is_explicit() {
    let base = rect(0, 0, 10, 10);
    let same = plan_resize_constraint_v1(
        base,
        ResizeHandle::BottomRight,
        base,
        ResizeModifierMaskV1::default(),
    )
    .expect("same");
    assert!(!same.changed);

    let changed = plan_resize_constraint_v1(
        base,
        ResizeHandle::BottomRight,
        rect(0, 0, 20, 20),
        ResizeModifierMaskV1::default(),
    )
    .expect("changed");
    assert!(changed.changed);
}

#[test]
fn all_eight_typed_handles_are_accepted_without_string_dispatch() {
    let base = rect(100, 100, 80, 40);
    let raw = rect(90, 90, 100, 60);
    for handle in ResizeHandle::ALL {
        let result =
            plan_resize_constraint_v1(base, handle, raw, ResizeModifierMaskV1::default());
        assert!(result.is_ok(), "typed handle {handle:?} should be admitted");
    }
}
