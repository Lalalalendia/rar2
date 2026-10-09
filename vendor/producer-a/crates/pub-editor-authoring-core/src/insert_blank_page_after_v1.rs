//! Canonical insertion of one new blank customer Page after any admitted anchor.
//!
//! Unlike DuplicateBlankPageV1, the source anchor may own Stories, Nodes,
//! authored stack members or other page-local content. None is copied.
//! Unlike AppendBlankPageV1, the insertion anchor need not be the final
//! customer Page. Both prior wire protocols retain their original meaning.

use crate::{AuthoredPageIdentityV1, validate_authored_page_identity_v1};
use pub_model::{DocumentId, Page, PageId};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub const INSERT_BLANK_PAGE_AFTER_PROTOCOL_V1: &str = "chaptera.insert-blank-page-after.v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InsertBlankPageAfterTransitionV1 {
    pub document_id: DocumentId,
    pub anchor_page_id: PageId,
    pub identity: AuthoredPageIdentityV1,
    pub page: Page,
    pub before_customer_page_ids: Vec<PageId>,
    pub after_customer_page_ids: Vec<PageId>,
    pub insertion_index: usize,
    pub before_document_state_id: String,
    pub after_document_state_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InsertBlankPageAfterErrorV1 {
    CustomerPagesEmpty,
    DuplicateCustomerPage { page_id: PageId },
    CustomerPageMissing { page_id: PageId },
    CustomerPageRepeated { page_id: PageId },
    CurrentCustomerOrderMismatch,
    AnchorNotCustomer { page_id: PageId },
    AnchorPageMissing { page_id: PageId },
    AnchorPageRepeated { page_id: PageId },
    IdentityInvalid,
    IdentityCollision { page_id: PageId },
    PageIdentityMismatch,
    InvalidPage,
    NonBlankPage,
    InvalidTransition,
    DocumentMismatch,
    BeforeStateMismatch,
    AfterStateMismatch,
    InsertionSlotMismatch,
    PageStateMismatch,
}

pub fn insert_blank_page_after_document_state_id_v1(
    document_id: DocumentId,
    document_pages: &[PageId],
) -> String {
    let payload = serde_json::json!({
        "protocol_version": INSERT_BLANK_PAGE_AFTER_PROTOCOL_V1,
        "document_id": document_id,
        "document_pages": document_pages,
    });
    let bytes = serde_json::to_vec(&payload)
        .expect("canonical insert-page document state serialization cannot fail");
    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;
        write!(&mut hex, "{byte:02x}").expect("hex encoding into String cannot fail");
    }
    format!("sha256:{hex}")
}

fn customer_set_v1(
    page_ids: &[PageId],
) -> Result<BTreeSet<PageId>, InsertBlankPageAfterErrorV1> {
    if page_ids.is_empty() {
        return Err(InsertBlankPageAfterErrorV1::CustomerPagesEmpty);
    }
    let mut seen = BTreeSet::new();
    for page_id in page_ids {
        if !seen.insert(*page_id) {
            return Err(InsertBlankPageAfterErrorV1::DuplicateCustomerPage {
                page_id: *page_id,
            });
        }
    }
    Ok(seen)
}

fn current_customer_order_v1(
    document_pages: &[PageId],
    customer_page_ids: &[PageId],
) -> Result<Vec<PageId>, InsertBlankPageAfterErrorV1> {
    let customer = customer_set_v1(customer_page_ids)?;
    let mut seen = BTreeSet::new();
    let mut order = Vec::with_capacity(customer.len());
    for page_id in document_pages {
        if customer.contains(page_id) {
            if !seen.insert(*page_id) {
                return Err(InsertBlankPageAfterErrorV1::CustomerPageRepeated {
                    page_id: *page_id,
                });
            }
            order.push(*page_id);
        }
    }
    if seen != customer {
        let missing = customer
            .difference(&seen)
            .next()
            .copied()
            .expect("different customer sets imply one absent member");
        return Err(InsertBlankPageAfterErrorV1::CustomerPageMissing { page_id: missing });
    }
    Ok(order)
}

fn validate_blank_destination_v1(
    identity: AuthoredPageIdentityV1,
    page: &Page,
) -> Result<(), InsertBlankPageAfterErrorV1> {
    if validate_authored_page_identity_v1(&identity).is_err() {
        return Err(InsertBlankPageAfterErrorV1::IdentityInvalid);
    }
    if page.id != identity.page_id {
        return Err(InsertBlankPageAfterErrorV1::PageIdentityMismatch);
    }
    if page.validate().is_err() {
        return Err(InsertBlankPageAfterErrorV1::InvalidPage);
    }
    if !page.children.is_empty() || !page.extensions.is_empty() {
        return Err(InsertBlankPageAfterErrorV1::NonBlankPage);
    }
    Ok(())
}

fn validate_transition_v1(
    transition: &InsertBlankPageAfterTransitionV1,
) -> Result<(), InsertBlankPageAfterErrorV1> {
    validate_blank_destination_v1(transition.identity, &transition.page)?;
    let before = customer_set_v1(&transition.before_customer_page_ids)?;
    let after = customer_set_v1(&transition.after_customer_page_ids)?;
    if !before.contains(&transition.anchor_page_id) {
        return Err(InsertBlankPageAfterErrorV1::AnchorNotCustomer {
            page_id: transition.anchor_page_id,
        });
    }
    if before.contains(&transition.identity.page_id)
        || !after.contains(&transition.identity.page_id)
        || after.len() != before.len() + 1
        || transition.insertion_index == 0
    {
        return Err(InsertBlankPageAfterErrorV1::InvalidTransition);
    }
    let anchor_customer_index = transition
        .before_customer_page_ids
        .iter()
        .position(|page_id| *page_id == transition.anchor_page_id)
        .ok_or(InsertBlankPageAfterErrorV1::AnchorNotCustomer {
            page_id: transition.anchor_page_id,
        })?;
    let mut expected_after = transition.before_customer_page_ids.clone();
    expected_after.insert(anchor_customer_index + 1, transition.identity.page_id);
    if transition.after_customer_page_ids != expected_after {
        return Err(InsertBlankPageAfterErrorV1::InvalidTransition);
    }
    Ok(())
}

pub fn plan_insert_blank_page_after_v1(
    document_id: DocumentId,
    document_pages: &[PageId],
    pages: &BTreeMap<PageId, Page>,
    current_customer_page_ids: &[PageId],
    anchor_page_id: PageId,
    identity: AuthoredPageIdentityV1,
    page: Page,
) -> Result<InsertBlankPageAfterTransitionV1, InsertBlankPageAfterErrorV1> {
    validate_blank_destination_v1(identity, &page)?;
    if pages.contains_key(&identity.page_id) {
        return Err(InsertBlankPageAfterErrorV1::IdentityCollision {
            page_id: identity.page_id,
        });
    }
    if !pages.contains_key(&anchor_page_id) {
        return Err(InsertBlankPageAfterErrorV1::AnchorPageMissing {
            page_id: anchor_page_id,
        });
    }
    let current = current_customer_order_v1(document_pages, current_customer_page_ids)?;
    if current != current_customer_page_ids {
        return Err(InsertBlankPageAfterErrorV1::CurrentCustomerOrderMismatch);
    }
    let anchor_customer_index = current
        .iter()
        .position(|id| *id == anchor_page_id)
        .ok_or(InsertBlankPageAfterErrorV1::AnchorNotCustomer {
            page_id: anchor_page_id,
        })?;
    let anchors = document_pages
        .iter()
        .enumerate()
        .filter_map(|(index, id)| (*id == anchor_page_id).then_some(index))
        .collect::<Vec<_>>();
    let [anchor_raw_index] = anchors.as_slice() else {
        return Err(InsertBlankPageAfterErrorV1::AnchorPageRepeated {
            page_id: anchor_page_id,
        });
    };
    let insertion_index = anchor_raw_index + 1;

    let mut after_document_pages = document_pages.to_vec();
    after_document_pages.insert(insertion_index, identity.page_id);
    let mut after_customer_page_ids = current.clone();
    after_customer_page_ids.insert(anchor_customer_index + 1, identity.page_id);

    Ok(InsertBlankPageAfterTransitionV1 {
        document_id,
        anchor_page_id,
        identity,
        page,
        before_customer_page_ids: current,
        after_customer_page_ids,
        insertion_index,
        before_document_state_id: insert_blank_page_after_document_state_id_v1(
            document_id,
            document_pages,
        ),
        after_document_state_id: insert_blank_page_after_document_state_id_v1(
            document_id,
            &after_document_pages,
        ),
    })
}

pub fn apply_insert_blank_page_after_forward_v1(
    document_id: DocumentId,
    document_pages: &mut Vec<PageId>,
    pages: &mut BTreeMap<PageId, Page>,
    transition: &InsertBlankPageAfterTransitionV1,
) -> Result<(), InsertBlankPageAfterErrorV1> {
    validate_transition_v1(transition)?;
    if document_id != transition.document_id {
        return Err(InsertBlankPageAfterErrorV1::DocumentMismatch);
    }
    if insert_blank_page_after_document_state_id_v1(document_id, document_pages)
        != transition.before_document_state_id
    {
        return Err(InsertBlankPageAfterErrorV1::BeforeStateMismatch);
    }
    if current_customer_order_v1(document_pages, &transition.before_customer_page_ids)?
        != transition.before_customer_page_ids
    {
        return Err(InsertBlankPageAfterErrorV1::CurrentCustomerOrderMismatch);
    }
    if !pages.contains_key(&transition.anchor_page_id) {
        return Err(InsertBlankPageAfterErrorV1::AnchorPageMissing {
            page_id: transition.anchor_page_id,
        });
    }
    if pages.contains_key(&transition.identity.page_id) {
        return Err(InsertBlankPageAfterErrorV1::IdentityCollision {
            page_id: transition.identity.page_id,
        });
    }
    if document_pages
        .get(transition.insertion_index.checked_sub(1).ok_or(
            InsertBlankPageAfterErrorV1::InsertionSlotMismatch,
        )?)
        .copied()
        != Some(transition.anchor_page_id)
    {
        return Err(InsertBlankPageAfterErrorV1::InsertionSlotMismatch);
    }

    let mut candidate_document_pages = document_pages.clone();
    let mut candidate_pages = pages.clone();
    candidate_document_pages.insert(transition.insertion_index, transition.identity.page_id);
    candidate_pages.insert(transition.identity.page_id, transition.page.clone());

    if insert_blank_page_after_document_state_id_v1(document_id, &candidate_document_pages)
        != transition.after_document_state_id
        || current_customer_order_v1(
            &candidate_document_pages,
            &transition.after_customer_page_ids,
        )? != transition.after_customer_page_ids
    {
        return Err(InsertBlankPageAfterErrorV1::AfterStateMismatch);
    }
    *document_pages = candidate_document_pages;
    *pages = candidate_pages;
    Ok(())
}

pub fn apply_insert_blank_page_after_inverse_v1(
    document_id: DocumentId,
    document_pages: &mut Vec<PageId>,
    pages: &mut BTreeMap<PageId, Page>,
    transition: &InsertBlankPageAfterTransitionV1,
) -> Result<(), InsertBlankPageAfterErrorV1> {
    validate_transition_v1(transition)?;
    if document_id != transition.document_id {
        return Err(InsertBlankPageAfterErrorV1::DocumentMismatch);
    }
    if insert_blank_page_after_document_state_id_v1(document_id, document_pages)
        != transition.after_document_state_id
        || current_customer_order_v1(document_pages, &transition.after_customer_page_ids)?
            != transition.after_customer_page_ids
    {
        return Err(InsertBlankPageAfterErrorV1::AfterStateMismatch);
    }
    if pages.get(&transition.identity.page_id) != Some(&transition.page) {
        return Err(InsertBlankPageAfterErrorV1::PageStateMismatch);
    }
    if !pages.contains_key(&transition.anchor_page_id) {
        return Err(InsertBlankPageAfterErrorV1::AnchorPageMissing {
            page_id: transition.anchor_page_id,
        });
    }
    if document_pages.get(transition.insertion_index).copied()
        != Some(transition.identity.page_id)
        || document_pages
            .get(transition.insertion_index.checked_sub(1).ok_or(
                InsertBlankPageAfterErrorV1::InsertionSlotMismatch,
            )?)
            .copied()
            != Some(transition.anchor_page_id)
    {
        return Err(InsertBlankPageAfterErrorV1::InsertionSlotMismatch);
    }

    let mut candidate_document_pages = document_pages.clone();
    let mut candidate_pages = pages.clone();
    candidate_document_pages.remove(transition.insertion_index);
    candidate_pages.remove(&transition.identity.page_id);
    if insert_blank_page_after_document_state_id_v1(document_id, &candidate_document_pages)
        != transition.before_document_state_id
        || current_customer_order_v1(
            &candidate_document_pages,
            &transition.before_customer_page_ids,
        )? != transition.before_customer_page_ids
    {
        return Err(InsertBlankPageAfterErrorV1::BeforeStateMismatch);
    }
    *document_pages = candidate_document_pages;
    *pages = candidate_pages;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AuthoredEntityProvenanceV1;
    use pub_model::{CanonicalId, LengthEmu, NodeId, Size2D};

    fn page_id(value: &str) -> PageId {
        serde_json::from_str(&format!("\"{value}\"")).expect("valid PageId")
    }

    fn document_id() -> DocumentId {
        serde_json::from_str("\"33000000-0000-4000-8000-000000000001\"")
            .expect("valid DocumentId")
    }

    fn page(id: PageId, width: i64) -> Page {
        Page {
            id,
            size: Size2D::new(LengthEmu::new(width), LengthEmu::new(2_000_000)),
            bleed: None,
            margins: None,
            children: Vec::new(),
            extensions: Vec::new(),
        }
    }

    fn identity() -> AuthoredPageIdentityV1 {
        AuthoredPageIdentityV1 {
            page_id: page_id("01890f4f-1234-7abc-8def-0123456789ab"),
            provenance: AuthoredEntityProvenanceV1::AuthorCreated,
        }
    }

    #[test]
    fn inserts_after_content_bearing_nonfinal_anchor_and_preserves_raw_slots() {
        let master = page_id("11111111-1111-4111-8111-111111111111");
        let a = page_id("22222222-2222-4222-8222-222222222222");
        let service = page_id("33333333-3333-4333-8333-333333333333");
        let b = page_id("44444444-4444-4444-8444-444444444444");
        let carrier = page_id("55555555-5555-4555-8555-555555555555");
        let before = vec![master, a, service, b, carrier];
        let mut pages = before
            .iter()
            .copied()
            .map(|id| (id, page(id, 1_000_000)))
            .collect::<BTreeMap<_, _>>();
        let child = NodeId::from_canonical(CanonicalId::from_bytes([0x33; 16]));
        pages.get_mut(&a).expect("anchor Page").children.push(child);
        let original = pages.clone();
        let new_page = page(identity().page_id, 1_500_000);
        let transition = plan_insert_blank_page_after_v1(
            document_id(), &before, &pages, &[a, b], a, identity(), new_page.clone(),
        )
        .expect("admitted content-bearing anchor");
        assert_eq!(transition.insertion_index, 2);
        assert_eq!(transition.after_customer_page_ids, vec![a, identity().page_id, b]);

        let mut order = before.clone();
        apply_insert_blank_page_after_forward_v1(
            document_id(), &mut order, &mut pages, &transition,
        )
        .expect("forward");
        assert_eq!(order, vec![master, a, identity().page_id, service, b, carrier]);
        assert_eq!(pages.get(&a), original.get(&a), "content anchor was not changed");
        assert_eq!(pages.get(&identity().page_id), Some(&new_page));

        apply_insert_blank_page_after_inverse_v1(
            document_id(), &mut order, &mut pages, &transition,
        )
        .expect("inverse");
        assert_eq!(order, before);
        assert_eq!(pages, original);
    }

    #[test]
    fn rejects_noncustomer_anchor_or_identity_collision() {
        let a = page_id("22222222-2222-4222-8222-222222222222");
        let service = page_id("33333333-3333-4333-8333-333333333333");
        let before = vec![a, service];
        let mut pages = before
            .iter()
            .copied()
            .map(|id| (id, page(id, 1_000_000)))
            .collect::<BTreeMap<_, _>>();
        let requested = page(identity().page_id, 1_000_000);
        assert_eq!(
            plan_insert_blank_page_after_v1(
                document_id(), &before, &pages, &[a], service, identity(), requested.clone(),
            ),
            Err(InsertBlankPageAfterErrorV1::AnchorNotCustomer { page_id: service })
        );
        pages.insert(identity().page_id, requested.clone());
        assert_eq!(
            plan_insert_blank_page_after_v1(
                document_id(), &before, &pages, &[a], a, identity(), requested,
            ),
            Err(InsertBlankPageAfterErrorV1::IdentityCollision { page_id: identity().page_id })
        );
    }

    #[test]
    fn tampered_state_and_contentful_destination_fail_atomically() {
        let a = page_id("22222222-2222-4222-8222-222222222222");
        let before = vec![a];
        let original = [(a, page(a, 1_000_000))].into_iter().collect::<BTreeMap<_, _>>();
        let mut transition = plan_insert_blank_page_after_v1(
            document_id(), &before, &original, &[a], a, identity(),
            page(identity().page_id, 1_000_000),
        )
        .expect("plan");

        let mut order = before.clone();
        let mut pages = original.clone();
        transition.after_document_state_id = "sha256:tampered".to_owned();
        assert_eq!(
            apply_insert_blank_page_after_forward_v1(
                document_id(), &mut order, &mut pages, &transition,
            ),
            Err(InsertBlankPageAfterErrorV1::AfterStateMismatch)
        );
        assert_eq!(order, before);
        assert_eq!(pages, original);

        transition = plan_insert_blank_page_after_v1(
            document_id(), &before, &original, &[a], a, identity(),
            page(identity().page_id, 1_000_000),
        )
        .expect("plan again");
        apply_insert_blank_page_after_forward_v1(
            document_id(), &mut order, &mut pages, &transition,
        )
        .expect("forward");
        pages.get_mut(&identity().page_id).expect("destination").children.push(
            NodeId::from_canonical(CanonicalId::from_bytes([0x55; 16]))
        );
        let before_inverse = pages.clone();
        assert_eq!(
            apply_insert_blank_page_after_inverse_v1(
                document_id(), &mut order, &mut pages, &transition,
            ),
            Err(InsertBlankPageAfterErrorV1::PageStateMismatch)
        );
        assert_eq!(pages, before_inverse);
        assert_eq!(order, vec![a, identity().page_id]);
    }
}
