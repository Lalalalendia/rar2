use pub_editor_authoring_core::{
    AuthoredStackReorderModeV1 as Mode, AuthoredStackSetReorderErrorV1 as Error,
    AuthoredStackSetReorderTransitionV1, AuthoredStackV1,
    apply_authored_stack_set_reorder_forward_v1, apply_authored_stack_set_reorder_inverse_v1,
    authored_stack_state_id_v1, plan_reorder_authored_stack_set_v1,
};
use pub_model::{NodeId, PageId};

fn page_id() -> PageId {
    serde_json::from_str("\"11000000-0000-4000-8000-000000000001\"").unwrap()
}

fn other_page_id() -> PageId {
    serde_json::from_str("\"11000000-0000-4000-8000-000000000002\"").unwrap()
}

fn ids() -> Vec<NodeId> {
    (0..7)
        .map(|n| {
            serde_json::from_str(&format!("\"01890f47-0d0{n}-7abc-8def-0123456789ab\""))
                .unwrap()
        })
        .collect()
}

fn stack(members: Vec<NodeId>) -> AuthoredStackV1 {
    AuthoredStackV1 {
        page_id: page_id(),
        members,
    }
}

fn plan(
    before: &AuthoredStackV1,
    selected: &[NodeId],
    mode: Mode,
) -> Result<AuthoredStackSetReorderTransitionV1, Error> {
    plan_reorder_authored_stack_set_v1(before, page_id(), selected, mode)
}

#[test]
fn to_front_and_to_back_are_stable_partitions_in_before_lane_order() {
    let a = ids();
    let before = stack(a.clone());
    let picked = [a[5], a[1], a[3]]; // caller order is deliberately reversed
    let front = plan(&before, &picked, Mode::ToFront).expect("front");
    assert_eq!(front.selected_node_ids, vec![a[1], a[3], a[5]]);
    assert_eq!(front.after.members, vec![a[0], a[2], a[4], a[6], a[1], a[3], a[5]]);
    let back = plan(&before, &picked, Mode::ToBack).expect("back");
    assert_eq!(back.after.members, vec![a[1], a[3], a[5], a[0], a[2], a[4], a[6]]);
    assert_eq!(
        plan(&before, &[a[3], a[5], a[1]], Mode::ToFront).unwrap(),
        front,
        "enumeration order must never own z-order"
    );
}

#[test]
fn step_forward_moves_each_maximal_selected_block_by_one_unselected_neighbor() {
    let a = ids();
    let before = stack(a.clone());
    let forward = plan(&before, &[a[4], a[2], a[1]], Mode::StepForward).unwrap();
    assert_eq!(forward.after.members, vec![a[0], a[3], a[1], a[2], a[5], a[4], a[6]]);
    assert_eq!(forward.selected_node_ids, vec![a[1], a[2], a[4]]);
    assert_eq!(
        apply_authored_stack_set_reorder_inverse_v1(&forward.after, &forward).unwrap(),
        before
    );
}

#[test]
fn step_backward_moves_each_block_without_crossing_selected_members() {
    let a = ids();
    let before = stack(a.clone());
    let backward = plan(&before, &[a[4], a[2], a[1]], Mode::StepBackward).unwrap();
    assert_eq!(backward.after.members, vec![a[1], a[2], a[0], a[4], a[3], a[5], a[6]]);
    assert_eq!(
        apply_authored_stack_set_reorder_forward_v1(&before, &backward).unwrap(),
        backward.after
    );
}

#[test]
fn selected_block_touching_edge_stays_while_other_blocks_move() {
    let a = ids();
    let before = stack(a.clone());
    let forward = plan(&before, &[a[1], a[5], a[6]], Mode::StepForward).unwrap();
    assert_eq!(forward.after.members, vec![a[0], a[2], a[1], a[3], a[4], a[5], a[6]]);
    let backward = plan(&before, &[a[0], a[1], a[4]], Mode::StepBackward).unwrap();
    assert_eq!(backward.after.members, vec![a[0], a[1], a[2], a[4], a[3], a[5], a[6]]);
}

#[test]
fn boundaries_and_all_selected_are_nondurable_no_change() {
    let a = ids();
    let before = stack(a.clone());
    assert_eq!(
        plan(&before, &[a[5], a[6]], Mode::StepForward),
        Err(Error::NoChange)
    );
    assert_eq!(plan(&before, &[a[0], a[1]], Mode::ToBack), Err(Error::NoChange));
    assert_eq!(plan(&before, &[a[5], a[6]], Mode::ToFront), Err(Error::NoChange));
    for mode in [Mode::StepForward, Mode::StepBackward, Mode::ToBack, Mode::ToFront] {
        assert_eq!(plan(&before, &a, mode), Err(Error::NoChange));
    }
}

#[test]
fn duplicate_missing_wrong_page_and_corrupt_lane_all_reject_without_mutation() {
    let a = ids();
    let before = stack(a.clone());
    assert_eq!(plan(&before, &[a[1]], Mode::ToFront), Err(Error::TooFewSelected));
    assert_eq!(
        plan(&before, &[a[1], a[1]], Mode::ToFront),
        Err(Error::DuplicateSelected { node_id: a[1] })
    );
    let foreign: NodeId = serde_json::from_str("\"01890f47-0dff-7abc-8def-0123456789ab\"").unwrap();
    assert_eq!(
        plan(&before, &[a[1], foreign], Mode::ToFront),
        Err(Error::MissingMember { node_id: foreign })
    );
    assert_eq!(
        plan_reorder_authored_stack_set_v1(&before, other_page_id(), &[a[1], a[3]], Mode::ToFront),
        Err(Error::PageMismatch)
    );
    let malformed = stack(vec![a[0], a[0], a[1]]);
    assert_eq!(plan(&malformed, &[a[0], a[1]], Mode::ToFront), Err(Error::InvalidStack));
    assert_eq!(before.members, a);
}

#[test]
fn exact_forward_inverse_reject_stale_current_states() {
    let a = ids();
    let before = stack(a.clone());
    let t = plan(&before, &[a[1], a[4]], Mode::ToFront).unwrap();
    assert_eq!(
        apply_authored_stack_set_reorder_forward_v1(&before, &t).unwrap(),
        t.after
    );
    assert_eq!(
        apply_authored_stack_set_reorder_inverse_v1(&t.after, &t).unwrap(),
        before
    );
    let stale = stack(vec![a[6], a[5], a[4], a[3], a[2], a[1], a[0]]);
    assert_eq!(
        apply_authored_stack_set_reorder_forward_v1(&stale, &t),
        Err(Error::BeforeStateMismatch)
    );
    assert_eq!(
        apply_authored_stack_set_reorder_inverse_v1(&stale, &t),
        Err(Error::AfterStateMismatch)
    );
}

#[test]
fn forged_transition_is_rejected_even_if_hashes_are_recomputed() {
    let a = ids();
    let before = stack(a.clone());
    let t = plan(&before, &[a[1], a[4]], Mode::ToFront).unwrap();
    let mut forged = t.clone();
    forged.after.members.swap(0, 1);
    forged.after_state_id = authored_stack_state_id_v1(&forged.after);
    assert_eq!(
        apply_authored_stack_set_reorder_forward_v1(&before, &forged),
        Err(Error::TransitionMismatch)
    );
    let mut forged = t.clone();
    forged.selected_node_ids.reverse();
    assert_eq!(
        apply_authored_stack_set_reorder_forward_v1(&before, &forged),
        Err(Error::TransitionMismatch)
    );
    let mut forged = t.clone();
    forged.protocol_version = "chaptera.authored-stack-set-reorder.v0".to_owned();
    assert_eq!(
        apply_authored_stack_set_reorder_forward_v1(&before, &forged),
        Err(Error::ProtocolMismatch)
    );
    let mut forged = t.clone();
    forged.after_state_id = "sha256:forged".to_owned();
    assert_eq!(
        apply_authored_stack_set_reorder_forward_v1(&before, &forged),
        Err(Error::AfterStateMismatch)
    );
}

#[test]
fn json_transition_roundtrip_preserves_exact_canonical_evidence() {
    let a = ids();
    let before = stack(a.clone());
    let t = plan(&before, &[a[5], a[1], a[3]], Mode::StepForward).unwrap();
    let bytes = serde_json::to_vec(&t).unwrap();
    let decoded: AuthoredStackSetReorderTransitionV1 = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(decoded, t);
    assert_eq!(apply_authored_stack_set_reorder_forward_v1(&before, &decoded).unwrap(), t.after);
}

#[test]
fn exhaustive_selections_preserve_both_relative_orders_and_replay() {
    let a = ids();
    let before = stack(a[..5].to_vec());
    for mask in 0usize..(1 << 5) {
        let selected = before.members.iter().enumerate()
            .filter_map(|(index, &id)| ((mask & (1 << index)) != 0).then_some(id))
            .collect::<Vec<_>>();
        if selected.len() < 2 {
            continue;
        }
        let selected_set: std::collections::BTreeSet<_> = selected.iter().copied().collect();
        for mode in [Mode::StepForward, Mode::StepBackward, Mode::ToFront, Mode::ToBack] {
            match plan(&before, &selected, mode) {
                Ok(t) => {
                    assert_ne!(t.after.members, before.members);
                    assert_eq!(t.selected_node_ids, selected);
                    let original_selected: Vec<_> = before.members.iter().copied()
                        .filter(|id| selected_set.contains(id)).collect();
                    let original_unselected: Vec<_> = before.members.iter().copied()
                        .filter(|id| !selected_set.contains(id)).collect();
                    let after_selected: Vec<_> = t.after.members.iter().copied()
                        .filter(|id| selected_set.contains(id)).collect();
                    let after_unselected: Vec<_> = t.after.members.iter().copied()
                        .filter(|id| !selected_set.contains(id)).collect();
                    assert_eq!(after_selected, original_selected);
                    assert_eq!(after_unselected, original_unselected);
                    assert_eq!(
        apply_authored_stack_set_reorder_forward_v1(&before, &t).unwrap(),
        t.after
    );
                    assert_eq!(
        apply_authored_stack_set_reorder_inverse_v1(&t.after, &t).unwrap(),
        before
    );
                }
                Err(Error::NoChange) => {}
                Err(error) => panic!("mask={mask} mode={mode:?}: {error}"),
            }
        }
    }
}
