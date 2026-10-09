//! Pure atomic deletion of one authored Rectangle together with its authored Page.
//!
//! A Rectangle is an authored overlay, not a Page.children entry. Reusing the
//! bare DeleteBlank planner without checking authored shapes/stack would accept
//! a page that is visibly populated. This bounded contract composes the three
//! independently owned authorities without weakening their existing protocols.
//!
//! Caller must separately prove absence of resolved-source membership and of
//! other authored entity types; those authorities do not live in this crate.

use crate::{
    AuthoredPageIdentityV1, AuthoredShapeRuntimeV1, AuthoredStackLifecycleErrorV1,
    AuthoredStackLifecycleTransitionV1, AuthoredStackV1, DeleteBlankAuthoredPageErrorV1,
    DeleteBlankAuthoredPageTransitionV1, NodeId, PageId,
    apply_authored_stack_transition_forward_v1, apply_authored_stack_transition_inverse_v1,
    apply_delete_blank_authored_page_forward_v1, apply_delete_blank_authored_page_inverse_v1,
    plan_delete_blank_authored_page_v1, plan_delete_shape_remove_v1,
    validate_authored_shape_runtime_v1, validate_authored_stack_v1,
};
use pub_model::{DocumentId, Page};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

pub const DELETE_AUTHORED_RECTANGLE_PAGE_PROTOCOL_V1: &str =
    "chaptera.delete-authored-rectangle-page.v1";

/// Candidate-owned source-neutral state. This does NOT claim to be a complete
/// EditorSession; the runtime must derive every field from its live authorities.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeleteAuthoredRectanglePageStateV1 {
    pub document_pages: Vec<PageId>,
    pub pages: BTreeMap<PageId, Page>,
    pub authored_shapes: BTreeMap<NodeId, AuthoredShapeRuntimeV1>,
    pub authored_stack: AuthoredStackV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeleteAuthoredRectanglePageTransitionV1 {
    pub page: DeleteBlankAuthoredPageTransitionV1,
    pub shape_before: AuthoredShapeRuntimeV1,
    pub stack: AuthoredStackLifecycleTransitionV1,
    pub before_state_id: String,
    pub after_state_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeleteAuthoredRectanglePageErrorV1 {
    ForeignOrUnprovenMembership,
    InvalidAuthoredStack,
    IncorrectStackMembership,
    ShapeCountMismatch,
    InvalidAuthoredShape,
    ShapeRegistryMismatch,
    TransitionMismatch,
    BeforeStateMismatch,
    AfterStateMismatch,
    IdentityCollision,
    Page(DeleteBlankAuthoredPageErrorV1),
    Stack(AuthoredStackLifecycleErrorV1),
}

fn state_id_v1(
    document_id: DocumentId,
    document_pages: &[PageId],
    customer_page_ids: &[PageId],
    target_page: Option<&Page>,
    target_shape: Option<&AuthoredShapeRuntimeV1>,
    stack: &AuthoredStackV1,
) -> String {
    let payload = serde_json::json!({
        "protocol": DELETE_AUTHORED_RECTANGLE_PAGE_PROTOCOL_V1,
        "document_id": document_id,
        "document_pages": document_pages,
        "customer_page_ids": customer_page_ids,
        "page": target_page,
        "shape": target_shape,
        "authored_stack": stack,
    });
    let bytes = serde_json::to_vec(&payload)
        .expect("canonical authored rectangle page state JSON cannot fail");
    let digest = Sha256::digest(bytes);
    let mut encoded = String::with_capacity(71);
    encoded.push_str("sha256:");
    for byte in digest {
        use std::fmt::Write as _;
        write!(&mut encoded, "{byte:02x}").expect("hex into String cannot fail");
    }
    encoded
}

/// Requires an externally checked absence of resolved/source and non-Rectangle
/// entities. In production, pass true for ANY unproven page-local membership.
pub fn plan_delete_authored_rectangle_page_v1(
    document_id: DocumentId,
    state: &DeleteAuthoredRectanglePageStateV1,
    customer_page_ids: &[PageId],
    identity: AuthoredPageIdentityV1,
    has_foreign_or_unproven_membership: bool,
) -> Result<DeleteAuthoredRectanglePageTransitionV1, DeleteAuthoredRectanglePageErrorV1> {
    use DeleteAuthoredRectanglePageErrorV1 as Error;

    if has_foreign_or_unproven_membership {
        return Err(Error::ForeignOrUnprovenMembership);
    }
    validate_authored_stack_v1(&state.authored_stack).map_err(Error::Stack)?;
    if state.authored_stack.page_id != identity.page_id {
        return Err(Error::InvalidAuthoredStack);
    }

    let on_page = state
        .authored_shapes
        .iter()
        .filter(|(_, shape)| {
            shape.page_id == identity.page_id || shape.parent_id == identity.page_id
        })
        .collect::<Vec<_>>();
    let [(key, shape)] = on_page.as_slice() else {
        return Err(Error::ShapeCountMismatch);
    };
    if **key != shape.node_id {
        return Err(Error::ShapeRegistryMismatch);
    }
    validate_authored_shape_runtime_v1(shape).map_err(|_| Error::InvalidAuthoredShape)?;
    if shape.page_id != identity.page_id || shape.parent_id != identity.page_id {
        return Err(Error::InvalidAuthoredShape);
    }
    if state.authored_stack.members.as_slice() != [shape.node_id] {
        return Err(Error::IncorrectStackMembership);
    }

    let page = plan_delete_blank_authored_page_v1(
        document_id,
        &state.document_pages,
        &state.pages,
        customer_page_ids,
        identity,
    )
    .map_err(Error::Page)?;
    let stack = plan_delete_shape_remove_v1(&state.authored_stack, shape)
        .map_err(Error::Stack)?;
    if !stack.after.members.is_empty() {
        return Err(Error::IncorrectStackMembership);
    }

    let mut after_document_pages = state.document_pages.clone();
    after_document_pages.remove(page.removal_index);

    let before_state_id = state_id_v1(
        document_id,
        &state.document_pages,
        customer_page_ids,
        state.pages.get(&identity.page_id),
        Some(shape),
        &state.authored_stack,
    );
    let after_state_id = state_id_v1(
        document_id,
        &after_document_pages,
        &page.after_customer_page_ids,
        None,
        None,
        &stack.after,
    );

    Ok(DeleteAuthoredRectanglePageTransitionV1 {
        page,
        shape_before: (**shape).clone(),
        stack,
        before_state_id,
        after_state_id,
    })
}

/// All validation and mutations happen on a candidate. Live state changes once
/// and only after page, shape, lane and combined-state proofs all succeed.
pub fn apply_delete_authored_rectangle_page_forward_v1(
    document_id: DocumentId,
    state: &mut DeleteAuthoredRectanglePageStateV1,
    current_customer_page_ids: &[PageId],
    has_foreign_or_unproven_membership: bool,
    transition: &DeleteAuthoredRectanglePageTransitionV1,
) -> Result<(), DeleteAuthoredRectanglePageErrorV1> {
    use DeleteAuthoredRectanglePageErrorV1 as Error;

    let planned = plan_delete_authored_rectangle_page_v1(
        document_id,
        state,
        current_customer_page_ids,
        transition.page.identity,
        has_foreign_or_unproven_membership,
    )?;
    if planned != *transition {
        return Err(Error::TransitionMismatch);
    }

    let mut next = state.clone();
    apply_delete_blank_authored_page_forward_v1(
        document_id,
        &mut next.document_pages,
        &mut next.pages,
        &transition.page,
    )
    .map_err(Error::Page)?;

    let removed = next.authored_shapes.remove(&transition.shape_before.node_id);
    if removed.as_ref() != Some(&transition.shape_before) {
        return Err(Error::BeforeStateMismatch);
    }
    next.authored_stack =
        apply_authored_stack_transition_forward_v1(&next.authored_stack, &transition.stack)
            .map_err(Error::Stack)?;

    if state_id_v1(
        document_id,
        &next.document_pages,
        &transition.page.after_customer_page_ids,
        None,
        None,
        &next.authored_stack,
    ) != transition.after_state_id
    {
        return Err(Error::AfterStateMismatch);
    }
    *state = next;
    Ok(())
}

/// Inverse restores exact old identities and order; it cannot allocate a new
/// PageId or NodeId and refuses a colliding/tampered post-delete state.
pub fn apply_delete_authored_rectangle_page_inverse_v1(
    document_id: DocumentId,
    state: &mut DeleteAuthoredRectanglePageStateV1,
    current_customer_page_ids: &[PageId],
    transition: &DeleteAuthoredRectanglePageTransitionV1,
) -> Result<(), DeleteAuthoredRectanglePageErrorV1> {
    use DeleteAuthoredRectanglePageErrorV1 as Error;

    let page_id = transition.page.identity.page_id;
    let node_id = transition.shape_before.node_id;
    if current_customer_page_ids != transition.page.after_customer_page_ids.as_slice() {
        return Err(Error::AfterStateMismatch);
    }
    if transition.shape_before.page_id != page_id
        || transition.shape_before.parent_id != page_id
        || validate_authored_shape_runtime_v1(&transition.shape_before).is_err()
    {
        return Err(Error::InvalidAuthoredShape);
    }
    if transition.stack.before.page_id != page_id
        || transition.stack.after.page_id != page_id
        || transition.stack.before.members.as_slice() != [node_id]
        || !transition.stack.after.members.is_empty()
        || plan_delete_shape_remove_v1(&transition.stack.before, &transition.shape_before)
            .map_err(Error::Stack)?
            != transition.stack
    {
        return Err(Error::TransitionMismatch);
    }
    if state.pages.contains_key(&page_id)
        || state.authored_shapes.contains_key(&node_id)
        || state
            .authored_shapes
            .values()
            .any(|shape| shape.page_id == page_id || shape.parent_id == page_id)
    {
        return Err(Error::IdentityCollision);
    }
    if state.authored_stack != transition.stack.after {
        return Err(Error::AfterStateMismatch);
    }
    if state_id_v1(
        document_id,
        &state.document_pages,
        current_customer_page_ids,
        None,
        None,
        &state.authored_stack,
    ) != transition.after_state_id
    {
        return Err(Error::AfterStateMismatch);
    }

    let mut previous = state.clone();
    apply_delete_blank_authored_page_inverse_v1(
        document_id,
        &mut previous.document_pages,
        &mut previous.pages,
        &transition.page,
    )
    .map_err(Error::Page)?;
    previous.authored_stack =
        apply_authored_stack_transition_inverse_v1(&previous.authored_stack, &transition.stack)
            .map_err(Error::Stack)?;
    if previous
        .authored_shapes
        .insert(node_id, transition.shape_before.clone())
        .is_some()
    {
        return Err(Error::IdentityCollision);
    }

    if state_id_v1(
        document_id,
        &previous.document_pages,
        &transition.page.before_customer_page_ids,
        previous.pages.get(&page_id),
        previous.authored_shapes.get(&node_id),
        &previous.authored_stack,
    ) != transition.before_state_id
    {
        return Err(Error::BeforeStateMismatch);
    }
    *state = previous;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AuthoredEntityProvenanceV1, AuthoredShapeKindV1, AuthoredShapePaintV1,
        AuthoredShapeTransformV1, AuthoredSolidFillV1, AuthoredSolidStrokeV1, Srgb8V1,
    };
    use pub_model::{CanonicalId, LengthEmu, RectEmu, Size2D};

    fn page_id(n: u8) -> PageId {
        let mut bytes = [n; 16];
        bytes[6] = 0x40;
        bytes[8] = 0x80;
        PageId::from_canonical(CanonicalId::from_bytes(bytes))
    }

    fn authored_page_id() -> PageId {
        let mut bytes = [0x77; 16];
        bytes[6] = 0x70;
        bytes[8] = 0x80;
        PageId::from_canonical(CanonicalId::from_bytes(bytes))
    }

    fn shape_id(n: u8) -> NodeId {
        let mut bytes = [n; 16];
        bytes[6] = 0x70;
        bytes[8] = 0x80;
        NodeId::from_canonical(CanonicalId::from_bytes(bytes))
    }

    fn page(id: PageId) -> Page {
        Page {
            id,
            size: Size2D::new(LengthEmu::new(2_000_000), LengthEmu::new(3_000_000)),
            bleed: None,
            margins: None,
            children: Vec::new(),
            extensions: Vec::new(),
        }
    }

    fn shape(id: NodeId, owner: PageId) -> AuthoredShapeRuntimeV1 {
        let color = Srgb8V1 { r: 20, g: 40, b: 60 };
        AuthoredShapeRuntimeV1 {
            node_id: id,
            page_id: owner,
            parent_id: owner,
            shape_kind: AuthoredShapeKindV1::Rectangle,
            bounds: RectEmu::new(
                LengthEmu::new(1_000),
                LengthEmu::new(2_000),
                LengthEmu::new(300_000),
                LengthEmu::new(400_000),
            ),
            transform: AuthoredShapeTransformV1::Identity,
            paint: AuthoredShapePaintV1 {
                fill: AuthoredSolidFillV1 { visible: true, color },
                stroke: AuthoredSolidStrokeV1 {
                    visible: true,
                    color,
                    width_emu: 12_700,
                },
                provenance: AuthoredEntityProvenanceV1::AuthorCreated,
            },
            provenance: AuthoredEntityProvenanceV1::AuthorCreated,
        }
    }

    fn fixture() -> (
        DocumentId,
        DeleteAuthoredRectanglePageStateV1,
        Vec<PageId>,
        AuthoredPageIdentityV1,
        NodeId,
    ) {
        let document_id = DocumentId::from_canonical(CanonicalId::from_bytes([0xaa; 16]));
        let source = page_id(0x22);
        let service = page_id(0x33);
        let authored = authored_page_id();
        let other = page_id(0x44);
        let node = shape_id(0x55);
        let state = DeleteAuthoredRectanglePageStateV1 {
            document_pages: vec![source, service, authored, other],
            pages: BTreeMap::from([
                (source, page(source)),
                (service, page(service)),
                (authored, page(authored)),
                (other, page(other)),
            ]),
            authored_shapes: BTreeMap::from([(node, shape(node, authored))]),
            authored_stack: AuthoredStackV1 {
                page_id: authored,
                members: vec![node],
            },
        };
        let identity = AuthoredPageIdentityV1 {
            page_id: authored,
            provenance: AuthoredEntityProvenanceV1::AuthorCreated,
        };
        (document_id, state, vec![source, authored, other], identity, node)
    }

    #[test]
    fn one_rectangle_delete_inverse_restores_exact_raw_slots_shape_and_lane() {
        let (document_id, mut state, customers, identity, node) = fixture();
        let before = state.clone();
        let transition =
            plan_delete_authored_rectangle_page_v1(document_id, &state, &customers, identity, false)
                .expect("bounded plan");
        apply_delete_authored_rectangle_page_forward_v1(
            document_id, &mut state, &customers, false, &transition,
        )
        .expect("atomic delete");
        assert_eq!(state.document_pages, vec![page_id(0x22), page_id(0x33), page_id(0x44)]);
        assert!(!state.pages.contains_key(&identity.page_id));
        assert!(!state.authored_shapes.contains_key(&node));
        assert!(state.authored_stack.members.is_empty());
        apply_delete_authored_rectangle_page_inverse_v1(
            document_id, &mut state, &transition.page.after_customer_page_ids, &transition,
        )
        .expect("atomic undo");
        assert_eq!(state, before);
    }

    #[test]
    fn foreign_membership_extra_shape_and_invalid_lane_fail_closed() {
        let (document_id, state, customers, identity, node) = fixture();
        assert_eq!(
            plan_delete_authored_rectangle_page_v1(
                document_id, &state, &customers, identity, true,
            ),
            Err(DeleteAuthoredRectanglePageErrorV1::ForeignOrUnprovenMembership)
        );

        let mut extra = state.clone();
        let second = shape_id(0x66);
        extra.authored_shapes.insert(second, shape(second, identity.page_id));
        assert_eq!(
            plan_delete_authored_rectangle_page_v1(
                document_id, &extra, &customers, identity, false,
            ),
            Err(DeleteAuthoredRectanglePageErrorV1::ShapeCountMismatch)
        );

        let mut stale_lane = state.clone();
        stale_lane.authored_stack.members.clear();
        assert_eq!(
            plan_delete_authored_rectangle_page_v1(
                document_id, &stale_lane, &customers, identity, false,
            ),
            Err(DeleteAuthoredRectanglePageErrorV1::IncorrectStackMembership)
        );

        let mut source_shape = state.clone();
        source_shape.authored_shapes.get_mut(&node).unwrap().provenance =
            AuthoredEntityProvenanceV1::SourceBacked;
        assert_eq!(
            plan_delete_authored_rectangle_page_v1(
                document_id, &source_shape, &customers, identity, false,
            ),
            Err(DeleteAuthoredRectanglePageErrorV1::InvalidAuthoredShape)
        );
    }

    #[test]
    fn page_content_last_customer_and_source_identity_rejected() {
        let (document_id, state, customers, identity, node) = fixture();
        let mut with_raw_child = state.clone();
        with_raw_child.pages.get_mut(&identity.page_id).unwrap().children.push(node);
        assert_eq!(
            plan_delete_authored_rectangle_page_v1(
                document_id, &with_raw_child, &customers, identity, false,
            ),
            Err(DeleteAuthoredRectanglePageErrorV1::Page(
                DeleteBlankAuthoredPageErrorV1::NonBlankPage,
            ))
        );
        assert!(plan_delete_authored_rectangle_page_v1(
            document_id, &state, &[identity.page_id], identity, false,
        ).is_err());
        let invalid = AuthoredPageIdentityV1 {
            page_id: identity.page_id,
            provenance: AuthoredEntityProvenanceV1::SourceBacked,
        };
        assert!(plan_delete_authored_rectangle_page_v1(
            document_id, &state, &customers, invalid, false,
        ).is_err());
    }

    #[test]
    fn stale_forward_and_colliding_inverse_have_zero_partial_mutation() {
        let (document_id, state, customers, identity, node) = fixture();
        let transition =
            plan_delete_authored_rectangle_page_v1(document_id, &state, &customers, identity, false)
                .expect("plan");

        let mut stale = state.clone();
        stale.document_pages.swap(0, 1);
        let before = stale.clone();
        assert!(apply_delete_authored_rectangle_page_forward_v1(
            document_id, &mut stale, &customers, false, &transition,
        ).is_err());
        assert_eq!(stale, before);

        let mut tampered = state.clone();
        tampered.authored_shapes.get_mut(&node).unwrap().paint.stroke.width_emu += 1;
        let before = tampered.clone();
        assert!(apply_delete_authored_rectangle_page_forward_v1(
            document_id, &mut tampered, &customers, false, &transition,
        ).is_err());
        assert_eq!(tampered, before);

        let mut deleted = state.clone();
        apply_delete_authored_rectangle_page_forward_v1(
            document_id, &mut deleted, &customers, false, &transition,
        )
        .expect("forward");
        deleted.authored_shapes.insert(node, shape(node, identity.page_id));
        let before = deleted.clone();
        assert_eq!(
            apply_delete_authored_rectangle_page_inverse_v1(
                document_id, &mut deleted, &transition.page.after_customer_page_ids, &transition,
            ),
            Err(DeleteAuthoredRectanglePageErrorV1::IdentityCollision)
        );
        assert_eq!(deleted, before);
    }
}
