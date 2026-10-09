use pub_interaction::{
    DocumentPoint, ResizeHandle, ResizeInvalidReason, ResizeModifierMaskV1, ResizeTransaction,
    ResizeTransactionError, ResizeUpdate,
};
use pub_model::{CanonicalId, LengthEmu, NodeId, RectEmu};

fn node_id(byte: u8) -> NodeId {
    NodeId::from_canonical(CanonicalId::from_bytes([byte; 16]))
}

fn rect(x: i64, y: i64, width: i64, height: i64) -> RectEmu {
    RectEmu::new(
        LengthEmu::new(x),
        LengthEmu::new(y),
        LengthEmu::new(width),
        LengthEmu::new(height),
    )
}

fn point(x: i64, y: i64) -> DocumentPoint {
    DocumentPoint::new(LengthEmu::new(x), LengthEmu::new(y))
}

#[test]
fn no_modifiers_match_existing_raw_resize_semantics() {
    let before = rect(0, 0, 100, 50);
    let mut raw = ResizeTransaction::begin(
        node_id(1),
        before,
        ResizeHandle::BottomRight,
        point(100, 50),
    )
    .expect("raw transaction");
    let mut constrained = ResizeTransaction::begin(
        node_id(2),
        before,
        ResizeHandle::BottomRight,
        point(100, 50),
    )
    .expect("constrained transaction");

    let raw_update = raw.update(point(150, 75)).expect("raw update");
    let constrained_update = constrained
        .update_constrained(point(150, 75), ResizeModifierMaskV1::default())
        .expect("constrained update");

    assert_eq!(raw_update, constrained_update);
    assert_eq!(
        raw.commit().unwrap().after,
        constrained.commit().unwrap().after
    );
}

#[test]
fn ctrl_centered_resize_preserves_exact_center() {
    let before = rect(0, 0, 100, 50);
    let mut transaction =
        ResizeTransaction::begin(node_id(3), before, ResizeHandle::Right, point(100, 25))
            .expect("transaction");

    assert_eq!(
        transaction
            .update_constrained(
                point(120, 25),
                ResizeModifierMaskV1 {
                    centered: true,
                    aspect_lock: false,
                },
            )
            .unwrap(),
        ResizeUpdate::Preview(rect(-20, 0, 140, 50))
    );
    let after = transaction.commit().unwrap().after;
    assert_eq!(
        before.x.get() + before.right().unwrap().get(),
        after.x.get() + after.right().unwrap().get()
    );
}

#[test]
fn shift_corner_resize_preserves_base_aspect_ratio() {
    let before = rect(0, 0, 100, 50);
    let mut transaction = ResizeTransaction::begin(
        node_id(4),
        before,
        ResizeHandle::BottomRight,
        point(100, 50),
    )
    .expect("transaction");

    assert_eq!(
        transaction
            .update_constrained(
                point(150, 50),
                ResizeModifierMaskV1 {
                    centered: false,
                    aspect_lock: true,
                },
            )
            .unwrap(),
        ResizeUpdate::Preview(rect(0, 0, 150, 75))
    );
}

#[test]
fn ctrl_shift_combines_center_and_aspect() {
    let before = rect(0, 0, 100, 50);
    let mut transaction = ResizeTransaction::begin(
        node_id(5),
        before,
        ResizeHandle::BottomRight,
        point(100, 50),
    )
    .expect("transaction");

    assert_eq!(
        transaction
            .update_constrained(
                point(120, 60),
                ResizeModifierMaskV1 {
                    centered: true,
                    aspect_lock: true,
                },
            )
            .unwrap(),
        ResizeUpdate::Preview(rect(-20, -10, 140, 70))
    );
}

#[test]
fn shift_on_edge_handle_is_inert() {
    let before = rect(10, 20, 100, 50);
    let mut plain =
        ResizeTransaction::begin(node_id(6), before, ResizeHandle::Right, point(110, 45))
            .expect("plain");
    let mut shifted =
        ResizeTransaction::begin(node_id(7), before, ResizeHandle::Right, point(110, 45))
            .expect("shifted");

    let expected = plain.update(point(140, 45)).unwrap();
    let actual = shifted
        .update_constrained(
            point(140, 45),
            ResizeModifierMaskV1 {
                centered: false,
                aspect_lock: true,
            },
        )
        .unwrap();
    assert_eq!(actual, expected);
}

#[test]
fn toggling_modifiers_mid_drag_recomputes_from_original_base_and_pointer() {
    let before = rect(0, 0, 100, 50);
    let mut transaction =
        ResizeTransaction::begin(node_id(8), before, ResizeHandle::Right, point(100, 25))
            .expect("transaction");
    let pointer = point(120, 25);

    assert_eq!(
        transaction
            .update_constrained(pointer, ResizeModifierMaskV1::default())
            .unwrap(),
        ResizeUpdate::Preview(rect(0, 0, 120, 50))
    );
    assert_eq!(
        transaction
            .update_constrained(
                pointer,
                ResizeModifierMaskV1 {
                    centered: true,
                    aspect_lock: false,
                },
            )
            .unwrap(),
        ResizeUpdate::Preview(rect(-20, 0, 140, 50))
    );
    assert_eq!(
        transaction
            .update_constrained(pointer, ResizeModifierMaskV1::default())
            .unwrap(),
        ResizeUpdate::Preview(rect(0, 0, 120, 50))
    );
}

#[test]
fn terminal_constraint_invalid_state_cannot_commit_stale_preview() {
    let before = rect(0, 0, 100, 50);
    let mut transaction =
        ResizeTransaction::begin(node_id(9), before, ResizeHandle::Right, point(100, 25))
            .expect("transaction");

    assert_eq!(
        transaction
            .update_constrained(
                point(120, 25),
                ResizeModifierMaskV1 {
                    centered: true,
                    aspect_lock: false,
                },
            )
            .unwrap(),
        ResizeUpdate::Preview(rect(-20, 0, 140, 50))
    );

    assert_eq!(
        transaction
            .update_constrained(
                point(40, 25),
                ResizeModifierMaskV1 {
                    centered: true,
                    aspect_lock: false,
                },
            )
            .unwrap(),
        ResizeUpdate::Invalid {
            reason: ResizeInvalidReason::Constraint,
            last_valid: rect(-20, 0, 140, 50),
        }
    );
    assert_eq!(
        transaction.commit(),
        Err(ResizeTransactionError::LatestCandidateInvalid)
    );
}
