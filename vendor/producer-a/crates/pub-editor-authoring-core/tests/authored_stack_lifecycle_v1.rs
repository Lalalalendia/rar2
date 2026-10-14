use pub_editor_authoring_core::{
    AuthoredEntityProvenanceV1, AuthoredShapeKindV1, AuthoredShapePaintV1, AuthoredShapeRuntimeV1,
    AuthoredShapeTransformV1, AuthoredSolidFillV1, AuthoredSolidStrokeV1,
    AuthoredStackLifecycleErrorV1, AuthoredStackLifecycleKindV1, AuthoredStackV1, Srgb8V1,
    apply_authored_stack_transition_forward_v1, apply_authored_stack_transition_inverse_v1,
    authored_stack_state_id_v1, plan_create_shape_append_v1, plan_delete_shape_remove_v1,
};
use pub_model::{LengthEmu, NodeId, PageId, RectEmu};

fn canonical_id<T: serde::de::DeserializeOwned>(value: &str) -> T {
    serde_json::from_str(&format!("\"{value}\"")).expect("canonical typed id")
}

fn page_id() -> PageId {
    canonical_id("11000000-0000-4000-8000-000000000001")
}

fn other_page_id() -> PageId {
    canonical_id("11000000-0000-4000-8000-000000000002")
}

fn node_a() -> NodeId {
    canonical_id("01890f47-0d00-7abc-8def-0123456789ab")
}

fn node_b() -> NodeId {
    canonical_id("01890f47-0d01-7abc-8def-0123456789ab")
}

fn rect() -> RectEmu {
    RectEmu::new(
        LengthEmu::new(10),
        LengthEmu::new(20),
        LengthEmu::new(300),
        LengthEmu::new(400),
    )
}

fn shape(node_id: NodeId, page_id: PageId) -> AuthoredShapeRuntimeV1 {
    AuthoredShapeRuntimeV1 {
        node_id,
        page_id,
        parent_id: page_id,
        shape_kind: AuthoredShapeKindV1::Rectangle,
        bounds: rect(),
        transform: AuthoredShapeTransformV1::Identity,
        paint: AuthoredShapePaintV1 {
            fill: AuthoredSolidFillV1 {
                visible: true,
                color: Srgb8V1 { r: 1, g: 2, b: 3 },
            },
            stroke: AuthoredSolidStrokeV1 {
                visible: true,
                color: Srgb8V1 { r: 4, g: 5, b: 6 },
                width_emu: 12_700,
            },
            provenance: AuthoredEntityProvenanceV1::AuthorCreated,
        },
        provenance: AuthoredEntityProvenanceV1::AuthorCreated,
    }
}

#[test]
fn create_appends_at_authored_front_and_inverse_removes_exact_membership() {
    let empty = AuthoredStackV1::empty(page_id());
    let first = plan_create_shape_append_v1(&empty, &shape(node_a(), page_id())).expect("append A");
    assert_eq!(first.kind, AuthoredStackLifecycleKindV1::AppendCreated);
    assert_eq!(first.member_index, 0);
    assert_eq!(first.before.members, Vec::<NodeId>::new());
    assert_eq!(first.after.members, vec![node_a()]);

    let one = apply_authored_stack_transition_forward_v1(&empty, &first).expect("apply A");
    let second = plan_create_shape_append_v1(&one, &shape(node_b(), page_id())).expect("append B");
    assert_eq!(second.member_index, 1);
    assert_eq!(second.after.members, vec![node_a(), node_b()]);

    let two = apply_authored_stack_transition_forward_v1(&one, &second).expect("apply B");
    assert_eq!(
        two.members.last(),
        Some(&node_b()),
        "new create is authored front/top"
    );

    let undone = apply_authored_stack_transition_inverse_v1(&two, &second).expect("undo B");
    assert_eq!(undone, one);
}

#[test]
fn delete_removes_exact_middle_position_and_inverse_restores_same_order() {
    let before = AuthoredStackV1 {
        page_id: page_id(),
        members: vec![node_a(), node_b()],
    };
    let delete =
        plan_delete_shape_remove_v1(&before, &shape(node_a(), page_id())).expect("delete A");
    assert_eq!(delete.kind, AuthoredStackLifecycleKindV1::RemoveDeleted);
    assert_eq!(delete.member_index, 0);
    assert_eq!(delete.after.members, vec![node_b()]);

    let after = apply_authored_stack_transition_forward_v1(&before, &delete).expect("apply delete");
    let restored =
        apply_authored_stack_transition_inverse_v1(&after, &delete).expect("inverse delete");
    assert_eq!(restored, before);
    assert_eq!(restored.members, vec![node_a(), node_b()]);
}

#[test]
fn replay_is_exact_and_rejects_stale_current_lane_without_mutation() {
    let before = AuthoredStackV1 {
        page_id: page_id(),
        members: vec![node_a()],
    };
    let append = plan_create_shape_append_v1(&before, &shape(node_b(), page_id())).expect("append");

    let stale = AuthoredStackV1 {
        page_id: page_id(),
        members: Vec::new(),
    };
    assert!(matches!(
        apply_authored_stack_transition_forward_v1(&stale, &append),
        Err(AuthoredStackLifecycleErrorV1::BeforeStateMismatch)
    ));
    assert!(stale.members.is_empty());

    let after = apply_authored_stack_transition_forward_v1(&before, &append).expect("forward");
    assert_eq!(
        apply_authored_stack_transition_inverse_v1(&after, &append).expect("inverse"),
        before
    );
}

#[test]
fn duplicate_missing_page_and_non_author_created_cases_fail_closed() {
    let present = AuthoredStackV1 {
        page_id: page_id(),
        members: vec![node_a()],
    };
    assert!(matches!(
        plan_create_shape_append_v1(&present, &shape(node_a(), page_id())),
        Err(AuthoredStackLifecycleErrorV1::DuplicateMembership { .. })
    ));

    let empty = AuthoredStackV1::empty(page_id());
    assert!(matches!(
        plan_delete_shape_remove_v1(&empty, &shape(node_a(), page_id())),
        Err(AuthoredStackLifecycleErrorV1::MissingMembership { .. })
    ));

    assert!(matches!(
        plan_create_shape_append_v1(&empty, &shape(node_a(), other_page_id())),
        Err(AuthoredStackLifecycleErrorV1::StackPageMismatch { .. })
    ));

    let mut source_backed = shape(node_a(), page_id());
    source_backed.provenance = AuthoredEntityProvenanceV1::SourceBacked;
    assert!(matches!(
        plan_create_shape_append_v1(&empty, &source_backed),
        Err(AuthoredStackLifecycleErrorV1::InvalidAuthoredShape)
    ));
}

#[test]
fn transition_tamper_is_rejected_instead_of_recomputing_from_ui_order() {
    let before = AuthoredStackV1 {
        page_id: page_id(),
        members: vec![node_a()],
    };
    let mut append =
        plan_create_shape_append_v1(&before, &shape(node_b(), page_id())).expect("append");
    append.member_index = 0;

    assert!(matches!(
        apply_authored_stack_transition_forward_v1(&before, &append),
        Err(AuthoredStackLifecycleErrorV1::TransitionShapeMismatch)
    ));
}

#[test]
fn state_identity_is_deterministic_and_order_sensitive() {
    let a_then_b = AuthoredStackV1 {
        page_id: page_id(),
        members: vec![node_a(), node_b()],
    };
    let b_then_a = AuthoredStackV1 {
        page_id: page_id(),
        members: vec![node_b(), node_a()],
    };
    assert_eq!(
        authored_stack_state_id_v1(&a_then_b),
        authored_stack_state_id_v1(&a_then_b)
    );
    assert_ne!(
        authored_stack_state_id_v1(&a_then_b),
        authored_stack_state_id_v1(&b_then_a)
    );
}
