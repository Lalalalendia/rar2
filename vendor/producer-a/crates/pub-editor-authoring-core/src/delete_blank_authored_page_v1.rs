//! Canonical removal transition for one empty authored customer Page.
//!
//! This module owns only exact page state + raw document membership mutation.
//! Session-wide proof that the Page owns no nested/runtime content belongs to
//! pub-editor before this pure transition is planned.

use crate::{AuthoredPageIdentityV1, validate_authored_page_identity_v1};
use pub_model::{DocumentId, Page, PageId};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub const DELETE_BLANK_AUTHORED_PAGE_PROTOCOL_V1: &str =
    "chaptera.delete-blank-authored-page.v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeleteBlankAuthoredPageTransitionV1 {
    pub document_id: DocumentId,
    pub identity: AuthoredPageIdentityV1,
    pub page: Page,
    pub before_customer_page_ids: Vec<PageId>,
    pub after_customer_page_ids: Vec<PageId>,
    pub removal_index: usize,
    pub before_document_state_id: String,
    pub after_document_state_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeleteBlankAuthoredPageErrorV1 {
    CustomerPagesTooSmall,
    DuplicateCustomerPage { page_id: PageId },
    CustomerPageMissing { page_id: PageId },
    CustomerPageRepeated { page_id: PageId },
    CurrentCustomerOrderMismatch,
    IdentityInvalid,
    TargetNotCustomer { page_id: PageId },
    PageIdentityMismatch,
    InvalidPage,
    NonBlankPage,
    DocumentPageRepeated { page_id: PageId },
    InvalidTransition,
    DocumentMismatch,
    BeforeStateMismatch,
    AfterStateMismatch,
    PageMissing { page_id: PageId },
    PageStateMismatch,
    RemovalSlotMismatch,
    PageCollision { page_id: PageId },
}

pub fn delete_blank_authored_page_document_state_id_v1(
    document_id: DocumentId,
    document_pages: &[PageId],
) -> String {
    let payload = serde_json::json!({
        "protocol_version": DELETE_BLANK_AUTHORED_PAGE_PROTOCOL_V1,
        "document_id": document_id,
        "document_pages": document_pages,
    });
    let bytes = serde_json::to_vec(&payload)
        .expect("canonical delete-page document state JSON serialization cannot fail");
    let digest = Sha256::digest(bytes);
    let mut encoded = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;
        write!(&mut encoded, "{byte:02x}").expect("writing lowercase hex into String cannot fail");
    }
    format!("sha256:{encoded}")
}

fn customer_set_v1(
    page_ids: &[PageId],
) -> Result<BTreeSet<PageId>, DeleteBlankAuthoredPageErrorV1> {
    if page_ids.len() < 2 {
        return Err(DeleteBlankAuthoredPageErrorV1::CustomerPagesTooSmall);
    }
    let mut set = BTreeSet::new();
    for page_id in page_ids {
        if !set.insert(*page_id) {
            return Err(DeleteBlankAuthoredPageErrorV1::DuplicateCustomerPage {
                page_id: *page_id,
            });
        }
    }
    Ok(set)
}

fn current_customer_order_v1(
    document_pages: &[PageId],
    customer_page_ids: &[PageId],
) -> Result<Vec<PageId>, DeleteBlankAuthoredPageErrorV1> {
    let customer = customer_set_v1(customer_page_ids)?;
    let mut seen = BTreeSet::new();
    let mut ordered = Vec::with_capacity(customer.len());
    for page_id in document_pages {
        if !customer.contains(page_id) {
            continue;
        }
        if !seen.insert(*page_id) {
            return Err(DeleteBlankAuthoredPageErrorV1::CustomerPageRepeated {
                page_id: *page_id,
            });
        }
        ordered.push(*page_id);
    }
    if seen.len() != customer.len() {
        let missing = customer
            .difference(&seen)
            .next()
            .copied()
            .expect("different set sizes imply missing customer PageId");
        return Err(DeleteBlankAuthoredPageErrorV1::CustomerPageMissing { page_id: missing });
    }
    Ok(ordered)
}

fn validate_page_v1(
    identity: AuthoredPageIdentityV1,
    page: &Page,
) -> Result<(), DeleteBlankAuthoredPageErrorV1> {
    if validate_authored_page_identity_v1(&identity).is_err() {
        return Err(DeleteBlankAuthoredPageErrorV1::IdentityInvalid);
    }
    if page.id != identity.page_id {
        return Err(DeleteBlankAuthoredPageErrorV1::PageIdentityMismatch);
    }
    if page.validate().is_err() {
        return Err(DeleteBlankAuthoredPageErrorV1::InvalidPage);
    }
    if !page.children.is_empty() || !page.extensions.is_empty() {
        return Err(DeleteBlankAuthoredPageErrorV1::NonBlankPage);
    }
    Ok(())
}

fn validate_transition_v1(
    transition: &DeleteBlankAuthoredPageTransitionV1,
) -> Result<(), DeleteBlankAuthoredPageErrorV1> {
    validate_page_v1(transition.identity, &transition.page)?;
    let before = customer_set_v1(&transition.before_customer_page_ids)?;
    if !before.contains(&transition.identity.page_id)
        || transition.after_customer_page_ids.len() + 1
            != transition.before_customer_page_ids.len()
    {
        return Err(DeleteBlankAuthoredPageErrorV1::InvalidTransition);
    }
    let expected_after = transition
        .before_customer_page_ids
        .iter()
        .copied()
        .filter(|page_id| *page_id != transition.identity.page_id)
        .collect::<Vec<_>>();
    if expected_after != transition.after_customer_page_ids || expected_after.is_empty() {
        return Err(DeleteBlankAuthoredPageErrorV1::InvalidTransition);
    }
    Ok(())
}

pub fn plan_delete_blank_authored_page_v1(
    document_id: DocumentId,
    document_pages: &[PageId],
    pages: &BTreeMap<PageId, Page>,
    current_customer_page_ids: &[PageId],
    identity: AuthoredPageIdentityV1,
) -> Result<DeleteBlankAuthoredPageTransitionV1, DeleteBlankAuthoredPageErrorV1> {
    if validate_authored_page_identity_v1(&identity).is_err() {
        return Err(DeleteBlankAuthoredPageErrorV1::IdentityInvalid);
    }
    let page = pages
        .get(&identity.page_id)
        .cloned()
        .ok_or(DeleteBlankAuthoredPageErrorV1::PageMissing {
            page_id: identity.page_id,
        })?;
    validate_page_v1(identity, &page)?;

    let current = current_customer_order_v1(document_pages, current_customer_page_ids)?;
    if current != current_customer_page_ids {
        return Err(DeleteBlankAuthoredPageErrorV1::CurrentCustomerOrderMismatch);
    }
    if !current.contains(&identity.page_id) {
        return Err(DeleteBlankAuthoredPageErrorV1::TargetNotCustomer {
            page_id: identity.page_id,
        });
    }

    let positions = document_pages
        .iter()
        .enumerate()
        .filter_map(|(index, page_id)| (*page_id == identity.page_id).then_some(index))
        .collect::<Vec<_>>();
    let [removal_index] = positions.as_slice() else {
        return Err(DeleteBlankAuthoredPageErrorV1::DocumentPageRepeated {
            page_id: identity.page_id,
        });
    };

    let mut after_document_pages = document_pages.to_vec();
    after_document_pages.remove(*removal_index);
    let after_customer_page_ids = current
        .iter()
        .copied()
        .filter(|page_id| *page_id != identity.page_id)
        .collect::<Vec<_>>();
    if after_customer_page_ids.is_empty() {
        return Err(DeleteBlankAuthoredPageErrorV1::CustomerPagesTooSmall);
    }

    Ok(DeleteBlankAuthoredPageTransitionV1 {
        document_id,
        identity,
        page,
        before_customer_page_ids: current,
        after_customer_page_ids,
        removal_index: *removal_index,
        before_document_state_id: delete_blank_authored_page_document_state_id_v1(
            document_id,
            document_pages,
        ),
        after_document_state_id: delete_blank_authored_page_document_state_id_v1(
            document_id,
            &after_document_pages,
        ),
    })
}

pub fn apply_delete_blank_authored_page_forward_v1(
    document_id: DocumentId,
    document_pages: &mut Vec<PageId>,
    pages: &mut BTreeMap<PageId, Page>,
    transition: &DeleteBlankAuthoredPageTransitionV1,
) -> Result<(), DeleteBlankAuthoredPageErrorV1> {
    validate_transition_v1(transition)?;
    if document_id != transition.document_id {
        return Err(DeleteBlankAuthoredPageErrorV1::DocumentMismatch);
    }
    if delete_blank_authored_page_document_state_id_v1(document_id, document_pages)
        != transition.before_document_state_id
    {
        return Err(DeleteBlankAuthoredPageErrorV1::BeforeStateMismatch);
    }
    if document_pages.get(transition.removal_index).copied()
        != Some(transition.identity.page_id)
    {
        return Err(DeleteBlankAuthoredPageErrorV1::RemovalSlotMismatch);
    }
    if pages.get(&transition.identity.page_id) != Some(&transition.page) {
        return Err(DeleteBlankAuthoredPageErrorV1::PageStateMismatch);
    }

    let mut next_document_pages = document_pages.clone();
    next_document_pages.remove(transition.removal_index);
    if delete_blank_authored_page_document_state_id_v1(document_id, &next_document_pages)
        != transition.after_document_state_id
    {
        return Err(DeleteBlankAuthoredPageErrorV1::AfterStateMismatch);
    }

    let removed = pages
        .remove(&transition.identity.page_id)
        .ok_or(DeleteBlankAuthoredPageErrorV1::PageMissing {
            page_id: transition.identity.page_id,
        })?;
    if removed != transition.page {
        pages.insert(transition.identity.page_id, removed);
        return Err(DeleteBlankAuthoredPageErrorV1::PageStateMismatch);
    }
    *document_pages = next_document_pages;
    Ok(())
}

pub fn apply_delete_blank_authored_page_inverse_v1(
    document_id: DocumentId,
    document_pages: &mut Vec<PageId>,
    pages: &mut BTreeMap<PageId, Page>,
    transition: &DeleteBlankAuthoredPageTransitionV1,
) -> Result<(), DeleteBlankAuthoredPageErrorV1> {
    validate_transition_v1(transition)?;
    if document_id != transition.document_id {
        return Err(DeleteBlankAuthoredPageErrorV1::DocumentMismatch);
    }
    if delete_blank_authored_page_document_state_id_v1(document_id, document_pages)
        != transition.after_document_state_id
    {
        return Err(DeleteBlankAuthoredPageErrorV1::AfterStateMismatch);
    }
    if pages.contains_key(&transition.identity.page_id) {
        return Err(DeleteBlankAuthoredPageErrorV1::PageCollision {
            page_id: transition.identity.page_id,
        });
    }
    if transition.removal_index > document_pages.len() {
        return Err(DeleteBlankAuthoredPageErrorV1::RemovalSlotMismatch);
    }

    let mut previous_document_pages = document_pages.clone();
    previous_document_pages.insert(transition.removal_index, transition.identity.page_id);
    if delete_blank_authored_page_document_state_id_v1(document_id, &previous_document_pages)
        != transition.before_document_state_id
    {
        return Err(DeleteBlankAuthoredPageErrorV1::BeforeStateMismatch);
    }

    pages.insert(transition.identity.page_id, transition.page.clone());
    *document_pages = previous_document_pages;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AuthoredEntityProvenanceV1;
    use pub_model::{CanonicalId, LengthEmu, Size2D};

    fn page_id(last: u8) -> PageId {
        let mut bytes = [0x22; 16];
        bytes[15] = last;
        PageId::from_canonical(CanonicalId::from_bytes(bytes))
    }

    fn authored_page_id() -> PageId {
        let mut bytes = [0x33; 16];
        bytes[6] = 0x70;
        bytes[8] = 0x80;
        PageId::from_canonical(CanonicalId::from_bytes(bytes))
    }

    fn identity() -> AuthoredPageIdentityV1 {
        AuthoredPageIdentityV1 {
            page_id: authored_page_id(),
            provenance: AuthoredEntityProvenanceV1::AuthorCreated,
        }
    }

    fn blank_page(id: PageId) -> Page {
        Page {
            id,
            size: Size2D::new(LengthEmu::new(200), LengthEmu::new(300)),
            bleed: None,
            margins: None,
            children: Vec::new(),
            extensions: Vec::new(),
        }
    }

    fn document_id() -> DocumentId {
        DocumentId::from_canonical(CanonicalId::from_bytes([0x44; 16]))
    }

    #[test]
    fn delete_and_inverse_restore_exact_page_and_raw_slot() {
        let source = page_id(1);
        let authored = authored_page_id();
        let trailing_service = page_id(9);
        let mut document_pages = vec![source, authored, trailing_service];
        let mut pages = BTreeMap::from([
            (source, blank_page(source)),
            (authored, blank_page(authored)),
            (trailing_service, blank_page(trailing_service)),
        ]);
        let before_pages = document_pages.clone();
        let before_map = pages.clone();

        let transition = plan_delete_blank_authored_page_v1(
            document_id(),
            &document_pages,
            &pages,
            &[source, authored],
            identity(),
        )
        .expect("plan delete");

        apply_delete_blank_authored_page_forward_v1(
            document_id(),
            &mut document_pages,
            &mut pages,
            &transition,
        )
        .expect("delete");
        assert_eq!(document_pages, vec![source, trailing_service]);
        assert!(!pages.contains_key(&authored));

        apply_delete_blank_authored_page_inverse_v1(
            document_id(),
            &mut document_pages,
            &mut pages,
            &transition,
        )
        .expect("inverse");
        assert_eq!(document_pages, before_pages);
        assert_eq!(pages, before_map);
    }

    #[test]
    fn delete_rejects_last_customer_page() {
        let authored = authored_page_id();
        let pages = BTreeMap::from([(authored, blank_page(authored))]);
        assert_eq!(
            plan_delete_blank_authored_page_v1(
                document_id(),
                &[authored],
                &pages,
                &[authored],
                identity(),
            ),
            Err(DeleteBlankAuthoredPageErrorV1::CustomerPagesTooSmall)
        );
    }

    #[test]
    fn delete_rejects_nonblank_page() {
        let source = page_id(1);
        let authored = authored_page_id();
        let mut page = blank_page(authored);
        page.children.push(crate::NodeId::from_canonical(CanonicalId::from_bytes([0x55; 16])));
        let pages = BTreeMap::from([(source, blank_page(source)), (authored, page)]);
        assert_eq!(
            plan_delete_blank_authored_page_v1(
                document_id(),
                &[source, authored],
                &pages,
                &[source, authored],
                identity(),
            ),
            Err(DeleteBlankAuthoredPageErrorV1::NonBlankPage)
        );
    }

    #[test]
    fn stale_forward_rejects_without_mutation() {
        let source = page_id(1);
        let authored = authored_page_id();
        let service = page_id(9);
        let pages = BTreeMap::from([
            (source, blank_page(source)),
            (authored, blank_page(authored)),
            (service, blank_page(service)),
        ]);
        let transition = plan_delete_blank_authored_page_v1(
            document_id(),
            &[source, authored, service],
            &pages,
            &[source, authored],
            identity(),
        )
        .expect("plan");

        let mut stale_document_pages = vec![service, source, authored];
        let mut stale_pages = pages.clone();
        let before_document = stale_document_pages.clone();
        let before_pages = stale_pages.clone();

        assert_eq!(
            apply_delete_blank_authored_page_forward_v1(
                document_id(),
                &mut stale_document_pages,
                &mut stale_pages,
                &transition,
            ),
            Err(DeleteBlankAuthoredPageErrorV1::BeforeStateMismatch)
        );
        assert_eq!(stale_document_pages, before_document);
        assert_eq!(stale_pages, before_pages);
    }
}
