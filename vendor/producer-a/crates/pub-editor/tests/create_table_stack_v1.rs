use pub_editor::{
    apply_authored_stack_transition_forward_v1, plan_create_table_append_v1, AuthoredStackV1,
    NodeId, PageId,
};

fn canonical_id<T: serde::de::DeserializeOwned>(value: &str) -> T {
    serde_json::from_str(&format!("\"{}\"", value)).expect("canonical typed id")
}

fn page_id() -> PageId {
    canonical_id("11000000-0000-4000-8000-000000000001")
}

fn other_page_id() -> PageId {
    canonical_id("11000000-0000-4000-8000-000000000002")
}

fn table_id() -> NodeId {
    canonical_id("01890f47-0f00-7abc-8def-0123456789ab")
}

#[test]
fn table_append_reuses_exact_generic_authored_stack_transition() {
    let before = AuthoredStackV1::empty(page_id());
    let transition =
        plan_create_table_append_v1(&before, table_id(), page_id()).expect("plan table append");

    assert_eq!(transition.page_id, page_id());
    assert_eq!(transition.node_id, table_id());
    assert_eq!(transition.member_index, 0);
    assert_eq!(transition.before, before);
    assert_eq!(transition.after.members, vec![table_id()]);

    let after =
        apply_authored_stack_transition_forward_v1(&before, &transition).expect("apply append");
    assert_eq!(after, transition.after);
}

#[test]
fn table_append_fails_closed_on_page_mismatch_and_duplicate_membership() {
    let wrong_page = AuthoredStackV1::empty(other_page_id());
    assert!(plan_create_table_append_v1(&wrong_page, table_id(), page_id()).is_err());

    let occupied = AuthoredStackV1 {
        page_id: page_id(),
        members: vec![table_id()],
    };
    assert!(plan_create_table_append_v1(&occupied, table_id(), page_id()).is_err());
}
