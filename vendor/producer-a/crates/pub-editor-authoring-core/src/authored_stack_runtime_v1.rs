//! Production AuthoredStack reorder contract for pub-editor.
//!
//! Create/Delete membership is owned by authored_stack_lifecycle_v1. This
//! module adds only explicit authored-lane reordering and exact replay
//! evidence. Imported/source-backed Publisher stacking is intentionally out of
//! scope.

use crate::{
    AuthoredStackV1, NodeId, PageId, authored_stack_state_id_v1, validate_authored_stack_v1,
};
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthoredStackReorderModeV1 {
    StepForward,
    StepBackward,
    ToFront,
    ToBack,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthoredStackReorderTransitionV1 {
    pub page_id: PageId,
    pub node_id: NodeId,
    pub mode: AuthoredStackReorderModeV1,
    pub before_index: usize,
    pub after_index: usize,
    pub before: AuthoredStackV1,
    pub after: AuthoredStackV1,
    pub before_state_id: String,
    pub after_state_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthoredStackReorderErrorV1 {
    InvalidStack,
    PageMismatch,
    MissingMember { node_id: NodeId },
    NoChange { node_id: NodeId },
    BeforeStateMismatch,
    AfterStateMismatch,
    TransitionMismatch,
}

impl fmt::Display for AuthoredStackReorderErrorV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidStack => formatter.write_str("authored stack is invalid"),
            Self::PageMismatch => formatter.write_str("authored-stack reorder page mismatch"),
            Self::MissingMember { node_id } => {
                write!(formatter, "authored stack does not contain {node_id:?}")
            }
            Self::NoChange { node_id } => {
                write!(
                    formatter,
                    "authored-stack reorder for {node_id:?} is a no-op"
                )
            }
            Self::BeforeStateMismatch => {
                formatter.write_str("authored-stack reorder before state is stale")
            }
            Self::AfterStateMismatch => {
                formatter.write_str("authored-stack reorder after state is stale")
            }
            Self::TransitionMismatch => {
                formatter.write_str("authored-stack reorder transition is non-canonical")
            }
        }
    }
}

impl std::error::Error for AuthoredStackReorderErrorV1 {}

pub fn plan_reorder_authored_stack_v1(
    stack: &AuthoredStackV1,
    node_id: NodeId,
    mode: AuthoredStackReorderModeV1,
) -> Result<AuthoredStackReorderTransitionV1, AuthoredStackReorderErrorV1> {
    validate_authored_stack_v1(stack).map_err(|_| AuthoredStackReorderErrorV1::InvalidStack)?;
    let Some(before_index) = stack.members.iter().position(|member| *member == node_id) else {
        return Err(AuthoredStackReorderErrorV1::MissingMember { node_id });
    };

    let after_index = match mode {
        AuthoredStackReorderModeV1::StepForward => before_index
            .checked_add(1)
            .filter(|index| *index < stack.members.len())
            .ok_or(AuthoredStackReorderErrorV1::NoChange { node_id })?,
        AuthoredStackReorderModeV1::StepBackward => before_index
            .checked_sub(1)
            .ok_or(AuthoredStackReorderErrorV1::NoChange { node_id })?,
        AuthoredStackReorderModeV1::ToFront => {
            let index = stack.members.len().saturating_sub(1);
            if index == before_index {
                return Err(AuthoredStackReorderErrorV1::NoChange { node_id });
            }
            index
        }
        AuthoredStackReorderModeV1::ToBack => {
            if before_index == 0 {
                return Err(AuthoredStackReorderErrorV1::NoChange { node_id });
            }
            0
        }
    };

    let before = stack.clone();
    let mut after = before.clone();
    let moved = after.members.remove(before_index);
    after.members.insert(after_index, moved);

    Ok(AuthoredStackReorderTransitionV1 {
        page_id: stack.page_id,
        node_id,
        mode,
        before_index,
        after_index,
        before_state_id: authored_stack_state_id_v1(&before),
        after_state_id: authored_stack_state_id_v1(&after),
        before,
        after,
    })
}

fn validate_transition_v1(
    transition: &AuthoredStackReorderTransitionV1,
) -> Result<(), AuthoredStackReorderErrorV1> {
    validate_authored_stack_v1(&transition.before)
        .map_err(|_| AuthoredStackReorderErrorV1::InvalidStack)?;
    validate_authored_stack_v1(&transition.after)
        .map_err(|_| AuthoredStackReorderErrorV1::InvalidStack)?;
    if transition.before.page_id != transition.page_id
        || transition.after.page_id != transition.page_id
    {
        return Err(AuthoredStackReorderErrorV1::PageMismatch);
    }
    if authored_stack_state_id_v1(&transition.before) != transition.before_state_id {
        return Err(AuthoredStackReorderErrorV1::BeforeStateMismatch);
    }
    if authored_stack_state_id_v1(&transition.after) != transition.after_state_id {
        return Err(AuthoredStackReorderErrorV1::AfterStateMismatch);
    }

    let canonical =
        plan_reorder_authored_stack_v1(&transition.before, transition.node_id, transition.mode)?;
    if canonical.before_index != transition.before_index
        || canonical.after_index != transition.after_index
        || canonical.after != transition.after
        || canonical.after_state_id != transition.after_state_id
    {
        return Err(AuthoredStackReorderErrorV1::TransitionMismatch);
    }
    Ok(())
}

pub fn apply_authored_stack_reorder_forward_v1(
    current: &AuthoredStackV1,
    transition: &AuthoredStackReorderTransitionV1,
) -> Result<AuthoredStackV1, AuthoredStackReorderErrorV1> {
    validate_transition_v1(transition)?;
    if current != &transition.before
        || authored_stack_state_id_v1(current) != transition.before_state_id
    {
        return Err(AuthoredStackReorderErrorV1::BeforeStateMismatch);
    }
    Ok(transition.after.clone())
}

pub fn apply_authored_stack_reorder_inverse_v1(
    current: &AuthoredStackV1,
    transition: &AuthoredStackReorderTransitionV1,
) -> Result<AuthoredStackV1, AuthoredStackReorderErrorV1> {
    validate_transition_v1(transition)?;
    if current != &transition.after
        || authored_stack_state_id_v1(current) != transition.after_state_id
    {
        return Err(AuthoredStackReorderErrorV1::AfterStateMismatch);
    }
    Ok(transition.before.clone())
}

// Canonical visible-page ordering. Membership remains an external page-role authority.
use pub_model::DocumentId;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

pub const PAGE_ORDER_PROTOCOL_V1: &str = "chaptera.page-order.v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageOrderTransitionV1 {
    pub document_id: DocumentId,
    pub before: Vec<PageId>,
    pub after: Vec<PageId>,
    pub before_state_id: String,
    pub after_state_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PageOrderErrorV1 {
    QualifiedPagesEmpty,
    DuplicateQualifiedPage { page_id: PageId },
    PageMissingFromDocument { page_id: PageId },
    DocumentPageRepeated { page_id: PageId },
    CurrentOrderMismatch,
    TargetSetMismatch,
    NoChange,
    DocumentMismatch,
    BeforeStateMismatch,
    AfterStateMismatch,
}

impl fmt::Display for PageOrderErrorV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::QualifiedPagesEmpty => {
                formatter.write_str("qualified customer-page set is empty")
            }
            Self::DuplicateQualifiedPage { page_id } => write!(
                formatter,
                "qualified customer-page set repeats {}",
                page_id.as_canonical()
            ),
            Self::PageMissingFromDocument { page_id } => write!(
                formatter,
                "qualified customer page {} is absent from the current document order",
                page_id.as_canonical()
            ),
            Self::DocumentPageRepeated { page_id } => write!(
                formatter,
                "current document order repeats qualified page {}",
                page_id.as_canonical()
            ),
            Self::CurrentOrderMismatch => formatter.write_str(
                "expected qualified page order does not match the current document order",
            ),
            Self::TargetSetMismatch => formatter.write_str(
                "target page order is not an exact permutation of the admitted customer pages",
            ),
            Self::NoChange => formatter.write_str("target page order does not change the document"),
            Self::DocumentMismatch => {
                formatter.write_str("page-order transition targets a different document")
            }
            Self::BeforeStateMismatch => formatter.write_str(
                "current qualified page-order state does not match the transition before state",
            ),
            Self::AfterStateMismatch => formatter.write_str(
                "resulting qualified page-order state does not match the transition after state",
            ),
        }
    }
}

impl std::error::Error for PageOrderErrorV1 {}

fn canonical_page_set_v1(page_ids: &[PageId]) -> Result<BTreeSet<PageId>, PageOrderErrorV1> {
    if page_ids.is_empty() {
        return Err(PageOrderErrorV1::QualifiedPagesEmpty);
    }

    let mut set = BTreeSet::new();
    for page_id in page_ids {
        if !set.insert(*page_id) {
            return Err(PageOrderErrorV1::DuplicateQualifiedPage { page_id: *page_id });
        }
    }
    Ok(set)
}

pub fn page_order_state_id_v1(document_id: DocumentId, page_ids: &[PageId]) -> String {
    let payload = serde_json::json!({
        "protocol_version": PAGE_ORDER_PROTOCOL_V1,
        "document_id": document_id,
        "page_ids": page_ids,
    });
    let bytes = serde_json::to_vec(&payload)
        .expect("canonical page-order state JSON serialization cannot fail");
    let digest = Sha256::digest(bytes);
    let mut encoded = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;
        write!(&mut encoded, "{byte:02x}")
            .expect("writing lowercase hex into String cannot fail");
    }
    format!("sha256:{encoded}")
}

/// Return the current order of one already-qualified customer-page set.
///
/// Membership is external authority (currently the Viewer customer-page
/// projection). This function owns only ordering and therefore never tries to
/// classify raw PAGE records itself.
pub fn qualified_page_order_v1(
    document_pages: &[PageId],
    qualified_page_ids: &[PageId],
) -> Result<Vec<PageId>, PageOrderErrorV1> {
    let qualified = canonical_page_set_v1(qualified_page_ids)?;
    let mut found = BTreeSet::new();
    let mut ordered = Vec::with_capacity(qualified.len());

    for page_id in document_pages {
        if !qualified.contains(page_id) {
            continue;
        }
        if !found.insert(*page_id) {
            return Err(PageOrderErrorV1::DocumentPageRepeated { page_id: *page_id });
        }
        ordered.push(*page_id);
    }

    if found.len() != qualified.len() {
        let missing = qualified
            .difference(&found)
            .next()
            .copied()
            .expect("different set sizes imply one missing qualified page");
        return Err(PageOrderErrorV1::PageMissingFromDocument { page_id: missing });
    }

    Ok(ordered)
}

pub fn plan_page_order_transition_v1(
    document_id: DocumentId,
    document_pages: &[PageId],
    expected_before: &[PageId],
    requested_after: &[PageId],
) -> Result<PageOrderTransitionV1, PageOrderErrorV1> {
    let before_set = canonical_page_set_v1(expected_before)?;
    let after_set = canonical_page_set_v1(requested_after)?;
    if before_set != after_set {
        return Err(PageOrderErrorV1::TargetSetMismatch);
    }

    let current = qualified_page_order_v1(document_pages, expected_before)?;
    if current != expected_before {
        return Err(PageOrderErrorV1::CurrentOrderMismatch);
    }
    if expected_before == requested_after {
        return Err(PageOrderErrorV1::NoChange);
    }

    Ok(PageOrderTransitionV1 {
        document_id,
        before: expected_before.to_vec(),
        after: requested_after.to_vec(),
        before_state_id: page_order_state_id_v1(document_id, expected_before),
        after_state_id: page_order_state_id_v1(document_id, requested_after),
    })
}

fn replace_qualified_slots_v1(
    document_pages: &mut [PageId],
    membership: &BTreeSet<PageId>,
    ordered_pages: &[PageId],
) {
    let mut replacement = ordered_pages.iter().copied();
    for page_id in document_pages {
        if membership.contains(page_id) {
            *page_id = replacement
                .next()
                .expect("qualified slot count was validated before replacement");
        }
    }
    debug_assert!(replacement.next().is_none());
}

fn apply_page_order_transition_v1(
    document_id: DocumentId,
    document_pages: &mut [PageId],
    transition: &PageOrderTransitionV1,
    forward: bool,
) -> Result<(), PageOrderErrorV1> {
    if document_id != transition.document_id {
        return Err(PageOrderErrorV1::DocumentMismatch);
    }

    let (before, after, before_state_id, after_state_id) = if forward {
        (
            transition.before.as_slice(),
            transition.after.as_slice(),
            transition.before_state_id.as_str(),
            transition.after_state_id.as_str(),
        )
    } else {
        (
            transition.after.as_slice(),
            transition.before.as_slice(),
            transition.after_state_id.as_str(),
            transition.before_state_id.as_str(),
        )
    };

    let before_set = canonical_page_set_v1(before)?;
    let after_set = canonical_page_set_v1(after)?;
    if before_set != after_set {
        return Err(PageOrderErrorV1::TargetSetMismatch);
    }

    let current = qualified_page_order_v1(document_pages, before)?;
    if current != before || page_order_state_id_v1(document_id, &current) != before_state_id {
        return Err(PageOrderErrorV1::BeforeStateMismatch);
    }

    replace_qualified_slots_v1(document_pages, &before_set, after);

    let current_after = qualified_page_order_v1(document_pages, after)?;
    if current_after != after
        || page_order_state_id_v1(document_id, &current_after) != after_state_id
    {
        return Err(PageOrderErrorV1::AfterStateMismatch);
    }

    Ok(())
}

pub fn apply_page_order_transition_forward_v1(
    document_id: DocumentId,
    document_pages: &mut [PageId],
    transition: &PageOrderTransitionV1,
) -> Result<(), PageOrderErrorV1> {
    apply_page_order_transition_v1(document_id, document_pages, transition, true)
}

pub fn apply_page_order_transition_inverse_v1(
    document_id: DocumentId,
    document_pages: &mut [PageId],
    transition: &PageOrderTransitionV1,
) -> Result<(), PageOrderErrorV1> {
    apply_page_order_transition_v1(document_id, document_pages, transition, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(value: &str) -> PageId {
        serde_json::from_str(&format!("\"{value}\"")).expect("valid test PageId")
    }

    fn document(value: &str) -> DocumentId {
        serde_json::from_str(&format!("\"{value}\"")).expect("valid test DocumentId")
    }

    #[test]
    fn reorder_changes_only_qualified_raw_slots() {
        let master = page("11111111-1111-4111-8111-111111111111");
        let a = page("22222222-2222-4222-8222-222222222222");
        let service = page("33333333-3333-4333-8333-333333333333");
        let b = page("44444444-4444-4444-8444-444444444444");
        let c = page("55555555-5555-4555-8555-555555555555");
        let document_id = document("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa");

        let before_raw = vec![master, a, service, b, c];
        let mut current = before_raw.clone();
        let transition =
            plan_page_order_transition_v1(document_id, &current, &[a, b, c], &[c, a, b])
                .expect("plan bounded page reorder");

        apply_page_order_transition_forward_v1(document_id, &mut current, &transition)
            .expect("apply bounded page reorder");
        assert_eq!(current, vec![master, c, service, a, b]);
        assert_eq!(
            qualified_page_order_v1(&current, &[a, b, c]).unwrap(),
            vec![c, a, b]
        );

        apply_page_order_transition_inverse_v1(document_id, &mut current, &transition)
            .expect("undo bounded page reorder");
        assert_eq!(current, before_raw);
    }

    #[test]
    fn plan_rejects_duplicate_missing_stale_and_changed_membership() {
        let a = page("22222222-2222-4222-8222-222222222222");
        let b = page("44444444-4444-4444-8444-444444444444");
        let c = page("55555555-5555-4555-8555-555555555555");
        let outsider = page("66666666-6666-4666-8666-666666666666");
        let document_id = document("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa");
        let raw = vec![a, b, c];

        assert!(matches!(
            plan_page_order_transition_v1(document_id, &raw, &[a, a], &[a, a]),
            Err(PageOrderErrorV1::DuplicateQualifiedPage { .. })
        ));
        assert!(matches!(
            plan_page_order_transition_v1(document_id, &raw, &[a, outsider], &[outsider, a]),
            Err(PageOrderErrorV1::PageMissingFromDocument { .. })
        ));
        assert!(matches!(
            plan_page_order_transition_v1(document_id, &raw, &[b, a, c], &[a, b, c]),
            Err(PageOrderErrorV1::CurrentOrderMismatch)
        ));
        assert!(matches!(
            plan_page_order_transition_v1(document_id, &raw, &[a, b, c], &[a, b, outsider]),
            Err(PageOrderErrorV1::TargetSetMismatch)
        ));
    }

    #[test]
    fn apply_rejects_stale_before_state() {
        let a = page("22222222-2222-4222-8222-222222222222");
        let b = page("44444444-4444-4444-8444-444444444444");
        let c = page("55555555-5555-4555-8555-555555555555");
        let document_id = document("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa");
        let transition =
            plan_page_order_transition_v1(document_id, &[a, b, c], &[a, b, c], &[c, a, b])
                .unwrap();
        let mut stale = vec![b, a, c];

        assert!(matches!(
            apply_page_order_transition_forward_v1(document_id, &mut stale, &transition),
            Err(PageOrderErrorV1::BeforeStateMismatch)
        ));
        assert_eq!(stale, vec![b, a, c]);
    }
}
