//! Source-neutral atomic duplicate of an authored Page holding one authored Rectangle.
//!
//! The Page has no native children: the Rectangle lives in Chaptera's authored
//! overlay. Reusing DuplicateBlankPage alone would silently drop that visible
//! content. This bounded planner combines existing Page, shape and stack laws.
//! The EditorSession caller MUST independently prove there are no resolved
//! descendants, linked Stories, authored lines/tables, foreign stack references
//! or other membership outside the authorities passed here.
//!
//! This module deliberately does not provide an EditorSession command, native
//! Publisher serialization or a customer-facing Duplicate Page action.

use crate::{
    AuthoredPageIdentityV1, AuthoredShapeKindV1, AuthoredShapeRuntimeV1,
    AuthoredStackLifecycleErrorV1, AuthoredStackLifecycleTransitionV1, AuthoredStackV1,
    DuplicateBlankPageErrorV1, DuplicateBlankPageTransitionV1, NodeId, PageId,
    apply_authored_stack_transition_forward_v1, apply_authored_stack_transition_inverse_v1,
    apply_duplicate_blank_page_forward_v1, apply_duplicate_blank_page_inverse_v1,
    is_editor_created_uuid_v7_node_id, plan_create_shape_append_v1, plan_duplicate_blank_page_v1,
    validate_authored_page_identity_v1, validate_authored_shape_runtime_v1,
    validate_authored_stack_v1,
};
use pub_model::{DocumentId, Page};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

pub const DUPLICATE_AUTHORED_RECTANGLE_PAGE_PROTOCOL_V1: &str =
    "chaptera.duplicate-authored-rectangle-page.v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DuplicateAuthoredRectanglePageStateV1 {
    /// Independently evidenced AuthorCreated identity; the EditorSession caller
    /// must verify this against durable page-identity history.
    pub source_identity: AuthoredPageIdentityV1,
    pub document_pages: Vec<PageId>,
    pub pages: BTreeMap<PageId, Page>,
    pub authored_shapes: BTreeMap<NodeId, AuthoredShapeRuntimeV1>,
    pub source_stack: AuthoredStackV1,
    pub destination_stack: AuthoredStackV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DuplicateAuthoredRectanglePageTransitionV1 {
    pub page: DuplicateBlankPageTransitionV1,
    pub source_shape: AuthoredShapeRuntimeV1,
    pub destination_shape: AuthoredShapeRuntimeV1,
    pub stack: AuthoredStackLifecycleTransitionV1,
    pub before_state_id: String,
    pub after_state_id: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum DuplicateAuthoredRectanglePageErrorV1 {
    ForeignOrUnprovenMembership,
    SourceIdentityInvalid,
    SourceShapeCountMismatch,
    SourceShapeInvalid,
    SourceStackMismatch,
    DestinationStackNotEmpty,
    DestinationNodeInvalid,
    DestinationNodeCollision,
    DestinationPageAlreadyOwned,
    TransitionMismatch,
    BeforeStateMismatch,
    AfterStateMismatch,
    Page(DuplicateBlankPageErrorV1),
    Stack(AuthoredStackLifecycleErrorV1),
}

fn state_id_v1(
    document_id: DocumentId,
    state: &DuplicateAuthoredRectanglePageStateV1,
    customer_page_ids: &[PageId],
) -> String {
    // Bind all supplied authorities, not just the raw Page order, to defeat
    // stale shape/paint, destination membership and insertion replays.
    let payload = serde_json::json!({
        "protocol": DUPLICATE_AUTHORED_RECTANGLE_PAGE_PROTOCOL_V1,
        "document_id": document_id,
        "document_pages": state.document_pages,
        "source_identity": state.source_identity,
        "pages": state.pages,
        "authored_shapes": state.authored_shapes,
        "source_stack": state.source_stack,
        "destination_stack": state.destination_stack,
        "customer_page_ids": customer_page_ids,
    });
    let bytes = serde_json::to_vec(&payload)
        .expect("canonical authored-rectangle duplicate state serialization cannot fail");
    let digest = Sha256::digest(bytes);
    let mut encoded = String::with_capacity(71);
    encoded.push_str("sha256:");
    for byte in digest {
        use std::fmt::Write as _;
        write!(&mut encoded, "{byte:02x}").expect("hex into String cannot fail");
    }
    encoded
}

fn apply_candidate_forward_v1(
    document_id: DocumentId,
    state: &DuplicateAuthoredRectanglePageStateV1,
    page: &DuplicateBlankPageTransitionV1,
    shape: &AuthoredShapeRuntimeV1,
    stack: &AuthoredStackLifecycleTransitionV1,
) -> Result<DuplicateAuthoredRectanglePageStateV1, DuplicateAuthoredRectanglePageErrorV1> {
    use DuplicateAuthoredRectanglePageErrorV1 as Error;

    let mut candidate = state.clone();
    apply_duplicate_blank_page_forward_v1(
        document_id,
        &mut candidate.document_pages,
        &mut candidate.pages,
        page,
    )
    .map_err(Error::Page)?;
    if candidate
        .authored_shapes
        .insert(shape.node_id, shape.clone())
        .is_some()
    {
        return Err(Error::DestinationNodeCollision);
    }
    candidate.destination_stack =
        apply_authored_stack_transition_forward_v1(&candidate.destination_stack, stack)
            .map_err(Error::Stack)?;
    Ok(candidate)
}

/// The caller owns source-graph and Story reachability. This pure planner
/// refuses the request whenever that independent proof is absent.
pub fn plan_duplicate_authored_rectangle_page_v1(
    document_id: DocumentId,
    state: &DuplicateAuthoredRectanglePageStateV1,
    customer_page_ids: &[PageId],
    source_page_id: PageId,
    destination_identity: AuthoredPageIdentityV1,
    destination_node_id: NodeId,
    has_foreign_or_unproven_membership: bool,
) -> Result<DuplicateAuthoredRectanglePageTransitionV1, DuplicateAuthoredRectanglePageErrorV1> {
    use DuplicateAuthoredRectanglePageErrorV1 as Error;

    if has_foreign_or_unproven_membership {
        return Err(Error::ForeignOrUnprovenMembership);
    }
    if state.source_identity.page_id != source_page_id
        || validate_authored_page_identity_v1(&state.source_identity).is_err()
    {
        return Err(Error::SourceIdentityInvalid);
    }
    validate_authored_stack_v1(&state.source_stack).map_err(Error::Stack)?;
    validate_authored_stack_v1(&state.destination_stack).map_err(Error::Stack)?;
    if state.source_stack.page_id != source_page_id {
        return Err(Error::SourceStackMismatch);
    }
    let source_shapes = state
        .authored_shapes
        .values()
        .filter(|shape| shape.page_id == source_page_id || shape.parent_id == source_page_id)
        .collect::<Vec<_>>();
    let [source_shape] = source_shapes.as_slice() else {
        return Err(Error::SourceShapeCountMismatch);
    };
    if source_shape.page_id != source_page_id
        || source_shape.parent_id != source_page_id
        || source_shape.shape_kind != AuthoredShapeKindV1::Rectangle
        || validate_authored_shape_runtime_v1(source_shape).is_err()
    {
        return Err(Error::SourceShapeInvalid);
    }
    // The shape registry key is also authority; a foreign key must not
    // smuggle a copied NodeId into an apparently valid source lane.
    if state.authored_shapes.get(&source_shape.node_id) != Some(*source_shape) {
        return Err(Error::SourceShapeInvalid);
    }
    if state.source_stack.members.as_slice() != [source_shape.node_id] {
        return Err(Error::SourceStackMismatch);
    }
    if state.destination_stack != AuthoredStackV1::empty(destination_identity.page_id) {
        return Err(Error::DestinationStackNotEmpty);
    }
    if state.authored_shapes.contains_key(&destination_node_id)
        || destination_node_id == source_shape.node_id
        || !is_editor_created_uuid_v7_node_id(destination_node_id)
    {
        return Err(Error::DestinationNodeInvalid);
    }
    if state.authored_shapes.values().any(|shape| {
        shape.page_id == destination_identity.page_id
            || shape.parent_id == destination_identity.page_id
    }) {
        return Err(Error::DestinationPageAlreadyOwned);
    }

    // The source raw Page is blank; its single Rectangle is in the overlay.
    // The existing planner preserves source size/bleed/margins and inserts
    // directly after its exact raw Page slot, leaving service carriers intact.
    let page = plan_duplicate_blank_page_v1(
        document_id,
        &state.document_pages,
        &state.pages,
        customer_page_ids,
        source_page_id,
        destination_identity,
    )
    .map_err(Error::Page)?;
    let destination_shape = AuthoredShapeRuntimeV1 {
        node_id: destination_node_id,
        page_id: destination_identity.page_id,
        parent_id: destination_identity.page_id,
        ..(**source_shape).clone()
    };
    validate_authored_shape_runtime_v1(&destination_shape)
        .map_err(|_| Error::SourceShapeInvalid)?;
    let stack = plan_create_shape_append_v1(&state.destination_stack, &destination_shape)
        .map_err(Error::Stack)?;

    let after = apply_candidate_forward_v1(document_id, state, &page, &destination_shape, &stack)?;
    Ok(DuplicateAuthoredRectanglePageTransitionV1 {
        page: page.clone(),
        source_shape: (**source_shape).clone(),
        destination_shape,
        stack,
        before_state_id: state_id_v1(document_id, state, customer_page_ids),
        after_state_id: state_id_v1(document_id, &after, &page.after_customer_page_ids),
    })
}

/// Transactional: all checks and changes occur on a candidate clone.
pub fn apply_duplicate_authored_rectangle_page_forward_v1(
    document_id: DocumentId,
    state: &mut DuplicateAuthoredRectanglePageStateV1,
    customer_page_ids: &[PageId],
    has_foreign_or_unproven_membership: bool,
    transition: &DuplicateAuthoredRectanglePageTransitionV1,
) -> Result<(), DuplicateAuthoredRectanglePageErrorV1> {
    use DuplicateAuthoredRectanglePageErrorV1 as Error;

    let expected = plan_duplicate_authored_rectangle_page_v1(
        document_id,
        state,
        customer_page_ids,
        transition.page.source_page_id,
        transition.page.destination_identity,
        transition.destination_shape.node_id,
        has_foreign_or_unproven_membership,
    )?;
    if expected != *transition {
        return Err(Error::TransitionMismatch);
    }
    if state_id_v1(document_id, state, customer_page_ids) != transition.before_state_id {
        return Err(Error::BeforeStateMismatch);
    }
    let next = apply_candidate_forward_v1(
        document_id,
        state,
        &transition.page,
        &transition.destination_shape,
        &transition.stack,
    )?;
    if state_id_v1(document_id, &next, &transition.page.after_customer_page_ids)
        != transition.after_state_id
    {
        return Err(Error::AfterStateMismatch);
    }
    *state = next;
    Ok(())
}

/// Exact inverse: no ID allocation, no partial state change and no silent
/// overwrite of a destination Page, shape or authored stack.
pub fn apply_duplicate_authored_rectangle_page_inverse_v1(
    document_id: DocumentId,
    state: &mut DuplicateAuthoredRectanglePageStateV1,
    customer_page_ids: &[PageId],
    transition: &DuplicateAuthoredRectanglePageTransitionV1,
) -> Result<(), DuplicateAuthoredRectanglePageErrorV1> {
    use DuplicateAuthoredRectanglePageErrorV1 as Error;

    if customer_page_ids != transition.page.after_customer_page_ids.as_slice()
        || state_id_v1(document_id, state, customer_page_ids) != transition.after_state_id
    {
        return Err(Error::AfterStateMismatch);
    }
    let mut before = state.clone();
    apply_duplicate_blank_page_inverse_v1(
        document_id,
        &mut before.document_pages,
        &mut before.pages,
        &transition.page,
    )
    .map_err(Error::Page)?;
    if before
        .authored_shapes
        .remove(&transition.destination_shape.node_id)
        .as_ref()
        != Some(&transition.destination_shape)
    {
        return Err(Error::AfterStateMismatch);
    }
    before.destination_stack =
        apply_authored_stack_transition_inverse_v1(&before.destination_stack, &transition.stack)
            .map_err(Error::Stack)?;
    if state_id_v1(
        document_id,
        &before,
        &transition.page.before_customer_page_ids,
    ) != transition.before_state_id
    {
        return Err(Error::BeforeStateMismatch);
    }
    let planned = plan_duplicate_authored_rectangle_page_v1(
        document_id,
        &before,
        &transition.page.before_customer_page_ids,
        transition.page.source_page_id,
        transition.page.destination_identity,
        transition.destination_shape.node_id,
        false,
    )?;
    if planned != *transition {
        return Err(Error::TransitionMismatch);
    }
    *state = before;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AuthoredEntityProvenanceV1, AuthoredShapePaintV1, AuthoredShapeTransformV1,
        AuthoredSolidFillV1, AuthoredSolidStrokeV1, Srgb8V1,
    };
    use pub_model::{CanonicalId, LengthEmu, RectEmu, Size2D};

    fn id_bytes(n: u8, authored: bool) -> CanonicalId {
        let mut bytes = [n; 16];
        bytes[6] = if authored { 0x70 } else { 0x40 };
        bytes[8] = 0x80;
        CanonicalId::from_bytes(bytes)
    }
    fn page(n: u8, authored: bool) -> PageId {
        PageId::from_canonical(id_bytes(n, authored))
    }
    fn node(n: u8) -> NodeId {
        NodeId::from_canonical(id_bytes(n, true))
    }
    fn identity() -> AuthoredPageIdentityV1 {
        AuthoredPageIdentityV1 {
            page_id: page(0x88, true),
            provenance: AuthoredEntityProvenanceV1::AuthorCreated,
        }
    }
    fn raw_page(id: PageId) -> Page {
        Page {
            id,
            size: Size2D::new(LengthEmu::new(2_000_000), LengthEmu::new(3_000_000)),
            bleed: None,
            margins: None,
            children: Vec::new(),
            extensions: Vec::new(),
        }
    }
    fn source_shape(owner: PageId) -> AuthoredShapeRuntimeV1 {
        let color = Srgb8V1 {
            r: 20,
            g: 40,
            b: 60,
        };
        AuthoredShapeRuntimeV1 {
            node_id: node(0x55),
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
                fill: AuthoredSolidFillV1 {
                    visible: true,
                    color,
                },
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
        DuplicateAuthoredRectanglePageStateV1,
        Vec<PageId>,
    ) {
        let document = DocumentId::from_canonical(id_bytes(0xaa, false));
        let source = page(0x77, true);
        let first = page(0x11, false);
        let service = page(0x22, false);
        let last = page(0x33, false);
        let shape = source_shape(source);
        (
            document,
            DuplicateAuthoredRectanglePageStateV1 {
                source_identity: AuthoredPageIdentityV1 {
                    page_id: source,
                    provenance: AuthoredEntityProvenanceV1::AuthorCreated,
                },
                document_pages: vec![first, service, source, last],
                pages: BTreeMap::from([
                    (first, raw_page(first)),
                    (service, raw_page(service)),
                    (source, raw_page(source)),
                    (last, raw_page(last)),
                ]),
                authored_shapes: BTreeMap::from([(shape.node_id, shape.clone())]),
                source_stack: AuthoredStackV1 {
                    page_id: source,
                    members: vec![shape.node_id],
                },
                destination_stack: AuthoredStackV1::empty(identity().page_id),
            },
            vec![first, source, last],
        )
    }
    fn plan(
        document: DocumentId,
        state: &DuplicateAuthoredRectanglePageStateV1,
        customers: &[PageId],
    ) -> Result<DuplicateAuthoredRectanglePageTransitionV1, DuplicateAuthoredRectanglePageErrorV1>
    {
        plan_duplicate_authored_rectangle_page_v1(
            document,
            state,
            customers,
            page(0x77, true),
            identity(),
            node(0x66),
            false,
        )
    }

    #[test]
    fn duplicates_exact_page_rectangle_and_stack_in_one_transaction_then_inverse() {
        let (document, mut state, customers) = fixture();
        let original = state.clone();
        let transition = plan(document, &state, &customers).expect("plan");
        assert_eq!(
            transition.page.after_customer_page_ids,
            vec![
                page(0x11, false),
                page(0x77, true),
                identity().page_id,
                page(0x33, false)
            ]
        );
        assert_eq!(transition.page.insertion_index, 3);
        assert_eq!(
            transition.destination_shape.bounds,
            transition.source_shape.bounds
        );
        assert_eq!(
            transition.destination_shape.paint,
            transition.source_shape.paint
        );
        assert_ne!(
            transition.destination_shape.node_id,
            transition.source_shape.node_id
        );
        apply_duplicate_authored_rectangle_page_forward_v1(
            document,
            &mut state,
            &customers,
            false,
            &transition,
        )
        .expect("atomic forward");
        assert_eq!(
            state.document_pages,
            vec![
                page(0x11, false),
                page(0x22, false),
                page(0x77, true),
                identity().page_id,
                page(0x33, false),
            ]
        );
        assert_eq!(state.authored_shapes.len(), 2);
        assert_eq!(state.source_stack, original.source_stack);
        assert_eq!(state.destination_stack.members, vec![node(0x66)]);
        assert_eq!(
            state.pages[&identity().page_id].size,
            state.pages[&page(0x77, true)].size
        );
        apply_duplicate_authored_rectangle_page_inverse_v1(
            document,
            &mut state,
            &transition.page.after_customer_page_ids,
            &transition,
        )
        .expect("atomic inverse");
        assert_eq!(state, original);
    }

    #[test]
    fn graph_or_story_membership_must_be_independently_proven_empty() {
        let (document, mut state, customers) = fixture();
        assert_eq!(
            plan_duplicate_authored_rectangle_page_v1(
                document,
                &state,
                &customers,
                page(0x77, true),
                identity(),
                node(0x66),
                true,
            ),
            Err(DuplicateAuthoredRectanglePageErrorV1::ForeignOrUnprovenMembership)
        );
        state
            .pages
            .get_mut(&page(0x77, true))
            .unwrap()
            .children
            .push(node(0x55));
        assert_eq!(
            plan(document, &state, &customers),
            Err(DuplicateAuthoredRectanglePageErrorV1::Page(
                DuplicateBlankPageErrorV1::SourcePageNonBlank
            ))
        );
    }

    #[test]
    fn rejects_extra_content_source_backed_shape_and_foreign_stacks() {
        let (document, state, customers) = fixture();
        let mut extra = state.clone();
        extra
            .authored_shapes
            .insert(node(0x44), source_shape(page(0x77, true)));
        assert_eq!(
            plan(document, &extra, &customers),
            Err(DuplicateAuthoredRectanglePageErrorV1::SourceShapeCountMismatch)
        );
        let mut source_backed = state.clone();
        source_backed
            .authored_shapes
            .get_mut(&node(0x55))
            .unwrap()
            .provenance = AuthoredEntityProvenanceV1::SourceBacked;
        assert_eq!(
            plan(document, &source_backed, &customers),
            Err(DuplicateAuthoredRectanglePageErrorV1::SourceShapeInvalid)
        );
        let mut wrong_stack = state.clone();
        wrong_stack.source_stack.members.clear();
        assert_eq!(
            plan(document, &wrong_stack, &customers),
            Err(DuplicateAuthoredRectanglePageErrorV1::SourceStackMismatch)
        );
        let mut occupied_destination = state.clone();
        occupied_destination
            .destination_stack
            .members
            .push(node(0x66));
        assert_eq!(
            plan(document, &occupied_destination, &customers),
            Err(DuplicateAuthoredRectanglePageErrorV1::DestinationStackNotEmpty)
        );
    }

    #[test]
    fn refuses_unproven_authored_source_and_forged_shape_registry_key() {
        let (document, state, customers) = fixture();
        let mut unproven = state.clone();
        unproven.source_identity.provenance = AuthoredEntityProvenanceV1::SourceBacked;
        assert_eq!(
            plan(document, &unproven, &customers),
            Err(DuplicateAuthoredRectanglePageErrorV1::SourceIdentityInvalid)
        );

        let mut wrong_page = state.clone();
        wrong_page.source_identity.page_id = identity().page_id;
        assert_eq!(
            plan(document, &wrong_page, &customers),
            Err(DuplicateAuthoredRectanglePageErrorV1::SourceIdentityInvalid)
        );

        let mut foreign_key = state.clone();
        foreign_key.authored_shapes = BTreeMap::from([(
            node(0x44),
            source_shape(page(0x77, true)),
        )]);
        assert_eq!(
            plan(document, &foreign_key, &customers),
            Err(DuplicateAuthoredRectanglePageErrorV1::SourceShapeInvalid)
        );
    }

    #[test]
    fn fresh_identity_and_unaliased_destination_are_required() {
        let (document, state, customers) = fixture();
        assert_eq!(
            plan_duplicate_authored_rectangle_page_v1(
                document,
                &state,
                &customers,
                page(0x77, true),
                identity(),
                node(0x55),
                false
            ),
            Err(DuplicateAuthoredRectanglePageErrorV1::DestinationNodeInvalid)
        );
        assert_eq!(
            plan_duplicate_authored_rectangle_page_v1(
                document,
                &state,
                &customers,
                page(0x77, true),
                identity(),
                NodeId::from_canonical(id_bytes(0x66, false)),
                false
            ),
            Err(DuplicateAuthoredRectanglePageErrorV1::DestinationNodeInvalid)
        );
        let mut collision = state.clone();
        collision
            .pages
            .insert(identity().page_id, raw_page(identity().page_id));
        assert_eq!(
            plan(document, &collision, &customers),
            Err(DuplicateAuthoredRectanglePageErrorV1::Page(
                DuplicateBlankPageErrorV1::IdentityCollision {
                    page_id: identity().page_id
                }
            ))
        );
    }

    #[test]
    fn stale_source_and_tampered_transition_fail_without_partial_mutation() {
        let (document, state, customers) = fixture();
        let transition = plan(document, &state, &customers).expect("plan");
        let mut stale = state.clone();
        stale
            .authored_shapes
            .get_mut(&node(0x55))
            .unwrap()
            .paint
            .stroke
            .width_emu += 1;
        let unchanged = stale.clone();
        assert_eq!(
            apply_duplicate_authored_rectangle_page_forward_v1(
                document,
                &mut stale,
                &customers,
                false,
                &transition
            ),
            Err(DuplicateAuthoredRectanglePageErrorV1::TransitionMismatch)
        );
        assert_eq!(stale, unchanged);

        let mut tampered = transition.clone();
        tampered.destination_shape.paint.stroke.width_emu += 1;
        let mut candidate = state.clone();
        assert_eq!(
            apply_duplicate_authored_rectangle_page_forward_v1(
                document,
                &mut candidate,
                &customers,
                false,
                &tampered
            ),
            Err(DuplicateAuthoredRectanglePageErrorV1::TransitionMismatch)
        );
        assert_eq!(candidate, state);
    }

    #[test]
    fn tampered_destination_blocks_inverse_with_no_partial_mutation() {
        let (document, mut state, customers) = fixture();
        let transition = plan(document, &state, &customers).expect("plan");
        apply_duplicate_authored_rectangle_page_forward_v1(
            document,
            &mut state,
            &customers,
            false,
            &transition,
        )
        .expect("forward");
        state
            .authored_shapes
            .get_mut(&node(0x66))
            .unwrap()
            .paint
            .stroke
            .width_emu += 1;
        let tampered = state.clone();
        assert_eq!(
            apply_duplicate_authored_rectangle_page_inverse_v1(
                document,
                &mut state,
                &transition.page.after_customer_page_ids,
                &transition
            ),
            Err(DuplicateAuthoredRectanglePageErrorV1::AfterStateMismatch)
        );
        assert_eq!(state, tampered);
    }
}
