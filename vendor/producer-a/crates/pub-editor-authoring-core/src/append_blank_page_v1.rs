//! Canonical membership-changing append operation for one blank authored Page.
//!
//! Page identity, publication membership and page order are separate laws.
//! This module owns only the bounded transition that adds one already-explicit
//! author-created identity as one blank customer publication Page. It never
//! invents Publisher-native allocation state.

use crate::{AuthoredPageIdentityV1, validate_authored_page_identity_v1};
use pub_model::{DocumentId, Page, PageId};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub const APPEND_BLANK_PAGE_PROTOCOL_V1: &str = "chaptera.append-blank-page.v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppendBlankPageTransitionV1 {
    pub document_id: DocumentId,
    pub identity: AuthoredPageIdentityV1,
    pub page: Page,
    pub before_customer_page_ids: Vec<PageId>,
    pub after_customer_page_ids: Vec<PageId>,
    pub insertion_after_page_id: PageId,
    pub before_document_state_id: String,
    pub after_document_state_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppendBlankPageErrorV1 {
    CustomerPagesEmpty,
    DuplicateCustomerPage { page_id: PageId },
    CustomerPageMissing { page_id: PageId },
    CustomerPageRepeated { page_id: PageId },
    CurrentCustomerOrderMismatch,
    IdentityInvalid,
    IdentityCollision { page_id: PageId },
    PageIdentityMismatch,
    InvalidPage,
    NonBlankPage,
    InvalidTransition,
    DocumentMismatch,
    BeforeStateMismatch,
    AfterStateMismatch,
    PageStateMismatch,
}

pub fn append_blank_page_document_state_id_v1(
    document_id: DocumentId,
    document_pages: &[PageId],
) -> String {
    let payload = serde_json::json!({
        "protocol_version": APPEND_BLANK_PAGE_PROTOCOL_V1,
        "document_id": document_id,
        "document_pages": document_pages,
    });
    let bytes = serde_json::to_vec(&payload)
        .expect("canonical append-page document state JSON serialization cannot fail");
    let digest = Sha256::digest(bytes);
    let mut encoded = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;
        write!(&mut encoded, "{byte:02x}").expect("writing lowercase hex into String cannot fail");
    }
    format!("sha256:{encoded}")
}

fn customer_set_v1(page_ids: &[PageId]) -> Result<BTreeSet<PageId>, AppendBlankPageErrorV1> {
    if page_ids.is_empty() {
        return Err(AppendBlankPageErrorV1::CustomerPagesEmpty);
    }
    let mut set = BTreeSet::new();
    for page_id in page_ids {
        if !set.insert(*page_id) {
            return Err(AppendBlankPageErrorV1::DuplicateCustomerPage { page_id: *page_id });
        }
    }
    Ok(set)
}

fn current_customer_order_v1(
    document_pages: &[PageId],
    customer_page_ids: &[PageId],
) -> Result<Vec<PageId>, AppendBlankPageErrorV1> {
    let customer = customer_set_v1(customer_page_ids)?;
    let mut seen = BTreeSet::new();
    let mut ordered = Vec::with_capacity(customer.len());
    for page_id in document_pages {
        if !customer.contains(page_id) {
            continue;
        }
        if !seen.insert(*page_id) {
            return Err(AppendBlankPageErrorV1::CustomerPageRepeated { page_id: *page_id });
        }
        ordered.push(*page_id);
    }
    if seen.len() != customer.len() {
        let missing = customer
            .difference(&seen)
            .next()
            .copied()
            .expect("different set sizes imply missing customer PageId");
        return Err(AppendBlankPageErrorV1::CustomerPageMissing { page_id: missing });
    }
    Ok(ordered)
}

fn validate_page_v1(
    identity: AuthoredPageIdentityV1,
    page: &Page,
) -> Result<(), AppendBlankPageErrorV1> {
    if validate_authored_page_identity_v1(&identity).is_err() {
        return Err(AppendBlankPageErrorV1::IdentityInvalid);
    }
    if page.id != identity.page_id {
        return Err(AppendBlankPageErrorV1::PageIdentityMismatch);
    }
    if page.validate().is_err() {
        return Err(AppendBlankPageErrorV1::InvalidPage);
    }
    if !page.children.is_empty() || !page.extensions.is_empty() {
        return Err(AppendBlankPageErrorV1::NonBlankPage);
    }
    Ok(())
}

fn validate_transition_v1(
    transition: &AppendBlankPageTransitionV1,
) -> Result<(), AppendBlankPageErrorV1> {
    validate_page_v1(transition.identity, &transition.page)?;
    let before = customer_set_v1(&transition.before_customer_page_ids)?;
    let after = customer_set_v1(&transition.after_customer_page_ids)?;
    if before.contains(&transition.identity.page_id)
        || !after.contains(&transition.identity.page_id)
        || after.len() != before.len() + 1
    {
        return Err(AppendBlankPageErrorV1::InvalidTransition);
    }
    let mut expected_after = transition.before_customer_page_ids.clone();
    expected_after.push(transition.identity.page_id);
    if expected_after != transition.after_customer_page_ids
        || transition.before_customer_page_ids.last().copied()
            != Some(transition.insertion_after_page_id)
    {
        return Err(AppendBlankPageErrorV1::InvalidTransition);
    }
    Ok(())
}

pub fn plan_append_blank_page_v1(
    document_id: DocumentId,
    document_pages: &[PageId],
    existing_page_ids: &BTreeSet<PageId>,
    current_customer_page_ids: &[PageId],
    identity: AuthoredPageIdentityV1,
    page: Page,
) -> Result<AppendBlankPageTransitionV1, AppendBlankPageErrorV1> {
    validate_page_v1(identity, &page)?;
    if existing_page_ids.contains(&identity.page_id) {
        return Err(AppendBlankPageErrorV1::IdentityCollision {
            page_id: identity.page_id,
        });
    }

    let current = current_customer_order_v1(document_pages, current_customer_page_ids)?;
    if current != current_customer_page_ids {
        return Err(AppendBlankPageErrorV1::CurrentCustomerOrderMismatch);
    }
    let insertion_after_page_id = *current
        .last()
        .ok_or(AppendBlankPageErrorV1::CustomerPagesEmpty)?;
    let anchor_index = document_pages
        .iter()
        .position(|page_id| *page_id == insertion_after_page_id)
        .ok_or(AppendBlankPageErrorV1::CustomerPageMissing {
            page_id: insertion_after_page_id,
        })?;

    let mut after_document_pages = document_pages.to_vec();
    after_document_pages.insert(anchor_index + 1, identity.page_id);
    let mut after_customer_page_ids = current.clone();
    after_customer_page_ids.push(identity.page_id);

    Ok(AppendBlankPageTransitionV1 {
        document_id,
        identity,
        page,
        before_customer_page_ids: current,
        after_customer_page_ids,
        insertion_after_page_id,
        before_document_state_id: append_blank_page_document_state_id_v1(
            document_id,
            document_pages,
        ),
        after_document_state_id: append_blank_page_document_state_id_v1(
            document_id,
            &after_document_pages,
        ),
    })
}

pub fn apply_append_blank_page_forward_v1(
    document_id: DocumentId,
    document_pages: &mut Vec<PageId>,
    pages: &mut BTreeMap<PageId, Page>,
    transition: &AppendBlankPageTransitionV1,
) -> Result<(), AppendBlankPageErrorV1> {
    validate_transition_v1(transition)?;
    if document_id != transition.document_id {
        return Err(AppendBlankPageErrorV1::DocumentMismatch);
    }
    if append_blank_page_document_state_id_v1(document_id, document_pages)
        != transition.before_document_state_id
    {
        return Err(AppendBlankPageErrorV1::BeforeStateMismatch);
    }
    if current_customer_order_v1(document_pages, &transition.before_customer_page_ids)?
        != transition.before_customer_page_ids
    {
        return Err(AppendBlankPageErrorV1::CurrentCustomerOrderMismatch);
    }
    if pages.contains_key(&transition.identity.page_id) {
        return Err(AppendBlankPageErrorV1::IdentityCollision {
            page_id: transition.identity.page_id,
        });
    }

    let anchor_index = document_pages
        .iter()
        .position(|page_id| *page_id == transition.insertion_after_page_id)
        .ok_or(AppendBlankPageErrorV1::CustomerPageMissing {
            page_id: transition.insertion_after_page_id,
        })?;

    let mut candidate_document_pages = document_pages.clone();
    let mut candidate_pages = pages.clone();
    candidate_document_pages.insert(anchor_index + 1, transition.identity.page_id);
    candidate_pages.insert(transition.identity.page_id, transition.page.clone());

    if append_blank_page_document_state_id_v1(document_id, &candidate_document_pages)
        != transition.after_document_state_id
        || current_customer_order_v1(
            &candidate_document_pages,
            &transition.after_customer_page_ids,
        )? != transition.after_customer_page_ids
    {
        return Err(AppendBlankPageErrorV1::AfterStateMismatch);
    }

    *document_pages = candidate_document_pages;
    *pages = candidate_pages;
    Ok(())
}

pub fn apply_append_blank_page_inverse_v1(
    document_id: DocumentId,
    document_pages: &mut Vec<PageId>,
    pages: &mut BTreeMap<PageId, Page>,
    transition: &AppendBlankPageTransitionV1,
) -> Result<(), AppendBlankPageErrorV1> {
    validate_transition_v1(transition)?;
    if document_id != transition.document_id {
        return Err(AppendBlankPageErrorV1::DocumentMismatch);
    }
    if append_blank_page_document_state_id_v1(document_id, document_pages)
        != transition.after_document_state_id
        || current_customer_order_v1(document_pages, &transition.after_customer_page_ids)?
            != transition.after_customer_page_ids
    {
        return Err(AppendBlankPageErrorV1::AfterStateMismatch);
    }
    if pages.get(&transition.identity.page_id) != Some(&transition.page) {
        return Err(AppendBlankPageErrorV1::PageStateMismatch);
    }

    let page_index = document_pages
        .iter()
        .position(|page_id| *page_id == transition.identity.page_id)
        .ok_or(AppendBlankPageErrorV1::CustomerPageMissing {
            page_id: transition.identity.page_id,
        })?;

    let mut candidate_document_pages = document_pages.clone();
    let mut candidate_pages = pages.clone();
    candidate_document_pages.remove(page_index);
    candidate_pages.remove(&transition.identity.page_id);

    if append_blank_page_document_state_id_v1(document_id, &candidate_document_pages)
        != transition.before_document_state_id
        || current_customer_order_v1(
            &candidate_document_pages,
            &transition.before_customer_page_ids,
        )? != transition.before_customer_page_ids
    {
        return Err(AppendBlankPageErrorV1::BeforeStateMismatch);
    }

    *document_pages = candidate_document_pages;
    *pages = candidate_pages;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AuthoredEntityProvenanceV1;
    use pub_model::{CanonicalId, LengthEmu, Size2D};

    fn page_id(value: &str) -> PageId {
        serde_json::from_str(&format!("\"{value}\"")).expect("valid PageId")
    }

    fn document_id(value: &str) -> DocumentId {
        serde_json::from_str(&format!("\"{value}\"")).expect("valid DocumentId")
    }

    fn blank_page(page_id: PageId) -> Page {
        Page {
            id: page_id,
            size: Size2D::new(LengthEmu::new(1_000_000), LengthEmu::new(2_000_000)),
            bleed: None,
            margins: None,
            children: Vec::new(),
            extensions: Vec::new(),
        }
    }

    fn authored_identity() -> AuthoredPageIdentityV1 {
        AuthoredPageIdentityV1 {
            page_id: page_id("01890f4f-1234-7abc-8def-0123456789ab"),
            provenance: AuthoredEntityProvenanceV1::AuthorCreated,
        }
    }

    #[test]
    fn append_preserves_raw_carriers_and_inverse_restores_exact_state() {
        let document_id = document_id("33000000-0000-4000-8000-000000000001");
        let master = page_id("11111111-1111-4111-8111-111111111111");
        let a = page_id("22222222-2222-4222-8222-222222222222");
        let service = page_id("33333333-3333-4333-8333-333333333333");
        let b = page_id("44444444-4444-4444-8444-444444444444");
        let carrier = page_id("55555555-5555-4555-8555-555555555555");
        let identity = authored_identity();
        let before = vec![master, a, service, b, carrier];
        let existing = before.iter().copied().collect::<BTreeSet<_>>();
        let transition = plan_append_blank_page_v1(
            document_id,
            &before,
            &existing,
            &[a, b],
            identity,
            blank_page(identity.page_id),
        )
        .expect("plan append");

        let mut document_pages = before.clone();
        let mut pages = before
            .iter()
            .copied()
            .map(|id| (id, blank_page(id)))
            .collect::<BTreeMap<_, _>>();

        apply_append_blank_page_forward_v1(
            document_id,
            &mut document_pages,
            &mut pages,
            &transition,
        )
        .expect("forward");
        assert_eq!(
            document_pages,
            vec![master, a, service, b, identity.page_id, carrier]
        );
        assert_eq!(
            transition.after_customer_page_ids,
            vec![a, b, identity.page_id]
        );
        assert_eq!(pages.get(&identity.page_id), Some(&transition.page));

        apply_append_blank_page_inverse_v1(
            document_id,
            &mut document_pages,
            &mut pages,
            &transition,
        )
        .expect("inverse");
        assert_eq!(document_pages, before);
        assert!(!pages.contains_key(&identity.page_id));
    }

    #[test]
    fn append_rejects_zero_customer_documents() {
        let identity = authored_identity();
        let result = plan_append_blank_page_v1(
            document_id("33000000-0000-4000-8000-000000000001"),
            &[],
            &BTreeSet::new(),
            &[],
            identity,
            blank_page(identity.page_id),
        );
        assert_eq!(result, Err(AppendBlankPageErrorV1::CustomerPagesEmpty));
    }

    #[test]
    fn append_rejects_existing_page_identity() {
        let existing_page = page_id("22222222-2222-4222-8222-222222222222");
        let identity = authored_identity();
        let mut existing = BTreeSet::new();
        existing.insert(existing_page);
        existing.insert(identity.page_id);
        let result = plan_append_blank_page_v1(
            document_id("33000000-0000-4000-8000-000000000001"),
            &[existing_page],
            &existing,
            &[existing_page],
            identity,
            blank_page(identity.page_id),
        );
        assert_eq!(
            result,
            Err(AppendBlankPageErrorV1::IdentityCollision {
                page_id: identity.page_id
            })
        );
    }

    #[test]
    fn append_rejects_nonblank_or_invalid_page_state() {
        let source = page_id("22222222-2222-4222-8222-222222222222");
        let identity = authored_identity();
        let existing = [source].into_iter().collect::<BTreeSet<_>>();

        let mut nonblank = blank_page(identity.page_id);
        nonblank
            .extensions
            .push(pub_model::ExtensionId::from_canonical(
                CanonicalId::from_bytes([0x44; 16]),
            ));
        assert_eq!(
            plan_append_blank_page_v1(
                document_id("33000000-0000-4000-8000-000000000001"),
                &[source],
                &existing,
                &[source],
                identity,
                nonblank,
            ),
            Err(AppendBlankPageErrorV1::NonBlankPage)
        );

        let mut invalid = blank_page(identity.page_id);
        invalid.size.width = LengthEmu::new(0);
        assert_eq!(
            plan_append_blank_page_v1(
                document_id("33000000-0000-4000-8000-000000000001"),
                &[source],
                &existing,
                &[source],
                identity,
                invalid,
            ),
            Err(AppendBlankPageErrorV1::InvalidPage)
        );
    }
    #[test]
    fn tampered_after_state_fails_without_partial_mutation() {
        let document_id = document_id("33000000-0000-4000-8000-000000000001");
        let a = page_id("22222222-2222-4222-8222-222222222222");
        let identity = authored_identity();
        let before = vec![a];
        let existing = before.iter().copied().collect::<BTreeSet<_>>();
        let mut transition = plan_append_blank_page_v1(
            document_id,
            &before,
            &existing,
            &[a],
            identity,
            blank_page(identity.page_id),
        )
        .expect("plan append");
        transition.after_document_state_id = "sha256:tampered".to_owned();

        let mut document_pages = before.clone();
        let mut pages = before
            .iter()
            .copied()
            .map(|id| (id, blank_page(id)))
            .collect::<BTreeMap<_, _>>();
        let pages_before = pages.clone();

        assert_eq!(
            apply_append_blank_page_forward_v1(
                document_id,
                &mut document_pages,
                &mut pages,
                &transition,
            ),
            Err(AppendBlankPageErrorV1::AfterStateMismatch)
        );
        assert_eq!(document_pages, before);
        assert_eq!(pages, pages_before);
    }

}
