//! Canonical duplication transition for one blank customer Page.
//!
//! This module owns only exact Page-state + raw document-membership mutation.
//! Session-wide proof that the source Page owns no resolved/runtime content
//! belongs to pub-editor before this pure transition is planned.

use crate::{AuthoredPageIdentityV1, validate_authored_page_identity_v1};
use pub_model::{DocumentId, Page, PageId};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub const DUPLICATE_BLANK_PAGE_PROTOCOL_V1: &str = "chaptera.duplicate-blank-page.v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DuplicateBlankPageTransitionV1 {
    pub document_id: DocumentId,
    pub source_page_id: PageId,
    pub source_page: Page,
    pub destination_identity: AuthoredPageIdentityV1,
    pub destination_page: Page,
    pub before_customer_page_ids: Vec<PageId>,
    pub after_customer_page_ids: Vec<PageId>,
    pub insertion_index: usize,
    pub before_document_state_id: String,
    pub after_document_state_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DuplicateBlankPageErrorV1 {
    CustomerPagesEmpty,
    DuplicateCustomerPage { page_id: PageId },
    CustomerPageMissing { page_id: PageId },
    CustomerPageRepeated { page_id: PageId },
    CurrentCustomerOrderMismatch,
    SourceNotCustomer { page_id: PageId },
    SourcePageMissing { page_id: PageId },
    SourcePageRepeated { page_id: PageId },
    SourcePageInvalid,
    SourcePageNonBlank,
    SourcePageStateMismatch,
    IdentityInvalid,
    IdentityCollision { page_id: PageId },
    DestinationMatchesSource,
    DestinationPageIdentityMismatch,
    DestinationPageInvalid,
    DestinationPageNonBlank,
    DestinationPageCopyMismatch,
    InvalidTransition,
    DocumentMismatch,
    BeforeStateMismatch,
    AfterStateMismatch,
    InsertionSlotMismatch,
    DestinationPageStateMismatch,
}

pub fn duplicate_blank_page_document_state_id_v1(
    document_id: DocumentId,
    document_pages: &[PageId],
) -> String {
    let payload = serde_json::json!({
        "protocol_version": DUPLICATE_BLANK_PAGE_PROTOCOL_V1,
        "document_id": document_id,
        "document_pages": document_pages,
    });
    let bytes = serde_json::to_vec(&payload)
        .expect("canonical duplicate-page document state JSON serialization cannot fail");
    let digest = Sha256::digest(bytes);
    let mut encoded = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;
        write!(&mut encoded, "{byte:02x}").expect("writing lowercase hex into String cannot fail");
    }
    format!("sha256:{encoded}")
}

fn customer_set_v1(page_ids: &[PageId]) -> Result<BTreeSet<PageId>, DuplicateBlankPageErrorV1> {
    if page_ids.is_empty() {
        return Err(DuplicateBlankPageErrorV1::CustomerPagesEmpty);
    }
    let mut set = BTreeSet::new();
    for page_id in page_ids {
        if !set.insert(*page_id) {
            return Err(DuplicateBlankPageErrorV1::DuplicateCustomerPage { page_id: *page_id });
        }
    }
    Ok(set)
}

fn current_customer_order_v1(
    document_pages: &[PageId],
    customer_page_ids: &[PageId],
) -> Result<Vec<PageId>, DuplicateBlankPageErrorV1> {
    let customer = customer_set_v1(customer_page_ids)?;
    let mut seen = BTreeSet::new();
    let mut ordered = Vec::with_capacity(customer.len());
    for page_id in document_pages {
        if !customer.contains(page_id) {
            continue;
        }
        if !seen.insert(*page_id) {
            return Err(DuplicateBlankPageErrorV1::CustomerPageRepeated { page_id: *page_id });
        }
        ordered.push(*page_id);
    }
    if seen.len() != customer.len() {
        let missing = customer
            .difference(&seen)
            .next()
            .copied()
            .expect("different set sizes imply missing customer PageId");
        return Err(DuplicateBlankPageErrorV1::CustomerPageMissing { page_id: missing });
    }
    Ok(ordered)
}

fn validate_blank_source_page_v1(page: &Page) -> Result<(), DuplicateBlankPageErrorV1> {
    if page.validate().is_err() {
        return Err(DuplicateBlankPageErrorV1::SourcePageInvalid);
    }
    if !page.children.is_empty() || !page.extensions.is_empty() {
        return Err(DuplicateBlankPageErrorV1::SourcePageNonBlank);
    }
    Ok(())
}

fn duplicate_page_from_source_v1(
    source: &Page,
    destination_identity: AuthoredPageIdentityV1,
) -> Page {
    Page {
        id: destination_identity.page_id,
        size: source.size,
        bleed: source.bleed,
        margins: source.margins,
        children: Vec::new(),
        extensions: Vec::new(),
    }
}

fn validate_destination_page_v1(
    identity: AuthoredPageIdentityV1,
    page: &Page,
) -> Result<(), DuplicateBlankPageErrorV1> {
    if validate_authored_page_identity_v1(&identity).is_err() {
        return Err(DuplicateBlankPageErrorV1::IdentityInvalid);
    }
    if page.id != identity.page_id {
        return Err(DuplicateBlankPageErrorV1::DestinationPageIdentityMismatch);
    }
    if page.validate().is_err() {
        return Err(DuplicateBlankPageErrorV1::DestinationPageInvalid);
    }
    if !page.children.is_empty() || !page.extensions.is_empty() {
        return Err(DuplicateBlankPageErrorV1::DestinationPageNonBlank);
    }
    Ok(())
}

fn validate_transition_v1(
    transition: &DuplicateBlankPageTransitionV1,
) -> Result<(), DuplicateBlankPageErrorV1> {
    if transition.source_page.id != transition.source_page_id {
        return Err(DuplicateBlankPageErrorV1::SourcePageStateMismatch);
    }
    validate_blank_source_page_v1(&transition.source_page)?;
    validate_destination_page_v1(
        transition.destination_identity,
        &transition.destination_page,
    )?;
    if transition.source_page_id == transition.destination_identity.page_id {
        return Err(DuplicateBlankPageErrorV1::DestinationMatchesSource);
    }
    if transition.destination_page
        != duplicate_page_from_source_v1(&transition.source_page, transition.destination_identity)
    {
        return Err(DuplicateBlankPageErrorV1::DestinationPageCopyMismatch);
    }

    let before = customer_set_v1(&transition.before_customer_page_ids)?;
    let after = customer_set_v1(&transition.after_customer_page_ids)?;
    if !before.contains(&transition.source_page_id)
        || before.contains(&transition.destination_identity.page_id)
        || !after.contains(&transition.source_page_id)
        || !after.contains(&transition.destination_identity.page_id)
        || after.len() != before.len() + 1
    {
        return Err(DuplicateBlankPageErrorV1::InvalidTransition);
    }
    let source_customer_index = transition
        .before_customer_page_ids
        .iter()
        .position(|page_id| *page_id == transition.source_page_id)
        .ok_or(DuplicateBlankPageErrorV1::SourceNotCustomer {
            page_id: transition.source_page_id,
        })?;
    let mut expected_after = transition.before_customer_page_ids.clone();
    expected_after.insert(
        source_customer_index + 1,
        transition.destination_identity.page_id,
    );
    if expected_after != transition.after_customer_page_ids {
        return Err(DuplicateBlankPageErrorV1::InvalidTransition);
    }
    Ok(())
}

pub fn plan_duplicate_blank_page_v1(
    document_id: DocumentId,
    document_pages: &[PageId],
    pages: &BTreeMap<PageId, Page>,
    current_customer_page_ids: &[PageId],
    source_page_id: PageId,
    destination_identity: AuthoredPageIdentityV1,
) -> Result<DuplicateBlankPageTransitionV1, DuplicateBlankPageErrorV1> {
    if validate_authored_page_identity_v1(&destination_identity).is_err() {
        return Err(DuplicateBlankPageErrorV1::IdentityInvalid);
    }
    if source_page_id == destination_identity.page_id {
        return Err(DuplicateBlankPageErrorV1::DestinationMatchesSource);
    }
    if pages.contains_key(&destination_identity.page_id) {
        return Err(DuplicateBlankPageErrorV1::IdentityCollision {
            page_id: destination_identity.page_id,
        });
    }

    let source_page = pages.get(&source_page_id).cloned().ok_or(
        DuplicateBlankPageErrorV1::SourcePageMissing {
            page_id: source_page_id,
        },
    )?;
    validate_blank_source_page_v1(&source_page)?;

    let current = current_customer_order_v1(document_pages, current_customer_page_ids)?;
    if current != current_customer_page_ids {
        return Err(DuplicateBlankPageErrorV1::CurrentCustomerOrderMismatch);
    }
    let source_customer_index = current
        .iter()
        .position(|page_id| *page_id == source_page_id)
        .ok_or(DuplicateBlankPageErrorV1::SourceNotCustomer {
            page_id: source_page_id,
        })?;

    let source_positions = document_pages
        .iter()
        .enumerate()
        .filter_map(|(index, page_id)| (*page_id == source_page_id).then_some(index))
        .collect::<Vec<_>>();
    let [source_index] = source_positions.as_slice() else {
        return Err(DuplicateBlankPageErrorV1::SourcePageRepeated {
            page_id: source_page_id,
        });
    };
    let insertion_index = source_index + 1;

    let destination_page = duplicate_page_from_source_v1(&source_page, destination_identity);
    let mut after_document_pages = document_pages.to_vec();
    after_document_pages.insert(insertion_index, destination_identity.page_id);
    let mut after_customer_page_ids = current.clone();
    after_customer_page_ids.insert(source_customer_index + 1, destination_identity.page_id);

    Ok(DuplicateBlankPageTransitionV1 {
        document_id,
        source_page_id,
        source_page,
        destination_identity,
        destination_page,
        before_customer_page_ids: current,
        after_customer_page_ids,
        insertion_index,
        before_document_state_id: duplicate_blank_page_document_state_id_v1(
            document_id,
            document_pages,
        ),
        after_document_state_id: duplicate_blank_page_document_state_id_v1(
            document_id,
            &after_document_pages,
        ),
    })
}

pub fn apply_duplicate_blank_page_forward_v1(
    document_id: DocumentId,
    document_pages: &mut Vec<PageId>,
    pages: &mut BTreeMap<PageId, Page>,
    transition: &DuplicateBlankPageTransitionV1,
) -> Result<(), DuplicateBlankPageErrorV1> {
    validate_transition_v1(transition)?;
    if document_id != transition.document_id {
        return Err(DuplicateBlankPageErrorV1::DocumentMismatch);
    }
    if duplicate_blank_page_document_state_id_v1(document_id, document_pages)
        != transition.before_document_state_id
    {
        return Err(DuplicateBlankPageErrorV1::BeforeStateMismatch);
    }
    if current_customer_order_v1(document_pages, &transition.before_customer_page_ids)?
        != transition.before_customer_page_ids
    {
        return Err(DuplicateBlankPageErrorV1::CurrentCustomerOrderMismatch);
    }
    if pages.get(&transition.source_page_id) != Some(&transition.source_page) {
        return Err(DuplicateBlankPageErrorV1::SourcePageStateMismatch);
    }
    if pages.contains_key(&transition.destination_identity.page_id) {
        return Err(DuplicateBlankPageErrorV1::IdentityCollision {
            page_id: transition.destination_identity.page_id,
        });
    }
    if transition.insertion_index == 0
        || document_pages.get(transition.insertion_index - 1).copied()
            != Some(transition.source_page_id)
    {
        return Err(DuplicateBlankPageErrorV1::InsertionSlotMismatch);
    }

    let mut candidate_document_pages = document_pages.clone();
    let mut candidate_pages = pages.clone();
    candidate_document_pages.insert(
        transition.insertion_index,
        transition.destination_identity.page_id,
    );
    candidate_pages.insert(
        transition.destination_identity.page_id,
        transition.destination_page.clone(),
    );

    if duplicate_blank_page_document_state_id_v1(document_id, &candidate_document_pages)
        != transition.after_document_state_id
        || current_customer_order_v1(
            &candidate_document_pages,
            &transition.after_customer_page_ids,
        )? != transition.after_customer_page_ids
        || candidate_pages.get(&transition.source_page_id) != Some(&transition.source_page)
    {
        return Err(DuplicateBlankPageErrorV1::AfterStateMismatch);
    }

    *document_pages = candidate_document_pages;
    *pages = candidate_pages;
    Ok(())
}

pub fn apply_duplicate_blank_page_inverse_v1(
    document_id: DocumentId,
    document_pages: &mut Vec<PageId>,
    pages: &mut BTreeMap<PageId, Page>,
    transition: &DuplicateBlankPageTransitionV1,
) -> Result<(), DuplicateBlankPageErrorV1> {
    validate_transition_v1(transition)?;
    if document_id != transition.document_id {
        return Err(DuplicateBlankPageErrorV1::DocumentMismatch);
    }
    if duplicate_blank_page_document_state_id_v1(document_id, document_pages)
        != transition.after_document_state_id
        || current_customer_order_v1(document_pages, &transition.after_customer_page_ids)?
            != transition.after_customer_page_ids
    {
        return Err(DuplicateBlankPageErrorV1::AfterStateMismatch);
    }
    if pages.get(&transition.source_page_id) != Some(&transition.source_page) {
        return Err(DuplicateBlankPageErrorV1::SourcePageStateMismatch);
    }
    if pages.get(&transition.destination_identity.page_id) != Some(&transition.destination_page) {
        return Err(DuplicateBlankPageErrorV1::DestinationPageStateMismatch);
    }
    if document_pages.get(transition.insertion_index).copied()
        != Some(transition.destination_identity.page_id)
        || transition.insertion_index == 0
        || document_pages.get(transition.insertion_index - 1).copied()
            != Some(transition.source_page_id)
    {
        return Err(DuplicateBlankPageErrorV1::InsertionSlotMismatch);
    }

    let mut candidate_document_pages = document_pages.clone();
    let mut candidate_pages = pages.clone();
    candidate_document_pages.remove(transition.insertion_index);
    candidate_pages.remove(&transition.destination_identity.page_id);

    if duplicate_blank_page_document_state_id_v1(document_id, &candidate_document_pages)
        != transition.before_document_state_id
        || current_customer_order_v1(
            &candidate_document_pages,
            &transition.before_customer_page_ids,
        )? != transition.before_customer_page_ids
        || candidate_pages.get(&transition.source_page_id) != Some(&transition.source_page)
    {
        return Err(DuplicateBlankPageErrorV1::BeforeStateMismatch);
    }

    *document_pages = candidate_document_pages;
    *pages = candidate_pages;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AuthoredEntityProvenanceV1;
    use pub_model::{BoxEdges, CanonicalId, LengthEmu, Size2D};

    fn page_id(last: u8) -> PageId {
        let mut bytes = [0x22; 16];
        bytes[15] = last;
        PageId::from_canonical(CanonicalId::from_bytes(bytes))
    }

    fn destination_page_id() -> PageId {
        let mut bytes = [0x33; 16];
        bytes[6] = 0x70;
        bytes[8] = 0x80;
        PageId::from_canonical(CanonicalId::from_bytes(bytes))
    }

    fn destination_identity() -> AuthoredPageIdentityV1 {
        AuthoredPageIdentityV1 {
            page_id: destination_page_id(),
            provenance: AuthoredEntityProvenanceV1::AuthorCreated,
        }
    }

    fn document_id() -> DocumentId {
        DocumentId::from_canonical(CanonicalId::from_bytes([0x44; 16]))
    }

    fn blank_page(id: PageId, width: i64, height: i64) -> Page {
        Page {
            id,
            size: Size2D::new(LengthEmu::new(width), LengthEmu::new(height)),
            bleed: Some(BoxEdges {
                top: LengthEmu::new(1),
                right: LengthEmu::new(2),
                bottom: LengthEmu::new(3),
                left: LengthEmu::new(4),
            }),
            margins: Some(BoxEdges {
                top: LengthEmu::new(10),
                right: LengthEmu::new(20),
                bottom: LengthEmu::new(30),
                left: LengthEmu::new(40),
            }),
            children: Vec::new(),
            extensions: Vec::new(),
        }
    }

    #[test]
    fn duplicate_middle_customer_page_preserves_raw_carriers_and_inverse() {
        let a = page_id(1);
        let service = page_id(2);
        let b = page_id(3);
        let carrier = page_id(4);
        let c = page_id(5);
        let destination = destination_page_id();
        let before = vec![a, service, b, carrier, c];
        let mut document_pages = before.clone();
        let mut pages = BTreeMap::from([
            (a, blank_page(a, 100, 200)),
            (service, blank_page(service, 110, 210)),
            (b, blank_page(b, 300, 400)),
            (carrier, blank_page(carrier, 120, 220)),
            (c, blank_page(c, 500, 600)),
        ]);

        let transition = plan_duplicate_blank_page_v1(
            document_id(),
            &document_pages,
            &pages,
            &[a, b, c],
            b,
            destination_identity(),
        )
        .expect("plan duplicate");

        assert_eq!(
            transition.after_customer_page_ids,
            vec![a, b, destination, c]
        );
        assert_eq!(transition.insertion_index, 3);
        assert_eq!(transition.destination_page.id, destination);
        assert_eq!(transition.destination_page.size, pages[&b].size);
        assert_eq!(transition.destination_page.bleed, pages[&b].bleed);
        assert_eq!(transition.destination_page.margins, pages[&b].margins);
        assert!(transition.destination_page.children.is_empty());
        assert!(transition.destination_page.extensions.is_empty());

        let before_pages = pages.clone();
        apply_duplicate_blank_page_forward_v1(
            document_id(),
            &mut document_pages,
            &mut pages,
            &transition,
        )
        .expect("duplicate forward");
        assert_eq!(document_pages, vec![a, service, b, destination, carrier, c]);
        assert_eq!(pages.get(&destination), Some(&transition.destination_page));
        assert_eq!(pages.get(&b), Some(&transition.source_page));

        apply_duplicate_blank_page_inverse_v1(
            document_id(),
            &mut document_pages,
            &mut pages,
            &transition,
        )
        .expect("duplicate inverse");
        assert_eq!(document_pages, before);
        assert_eq!(pages, before_pages);
    }

    #[test]
    fn duplicate_rejects_nonblank_source() {
        let source = page_id(1);
        let child = pub_model::NodeId::from_canonical(CanonicalId::from_bytes([0x55; 16]));
        let mut source_page = blank_page(source, 100, 200);
        source_page.children.push(child);
        let pages = BTreeMap::from([(source, source_page)]);

        assert_eq!(
            plan_duplicate_blank_page_v1(
                document_id(),
                &[source],
                &pages,
                &[source],
                source,
                destination_identity(),
            ),
            Err(DuplicateBlankPageErrorV1::SourcePageNonBlank)
        );
    }

    #[test]
    fn duplicate_rejects_source_outside_customer_membership() {
        let customer = page_id(1);
        let source = page_id(2);
        let pages = BTreeMap::from([
            (customer, blank_page(customer, 100, 200)),
            (source, blank_page(source, 300, 400)),
        ]);

        assert_eq!(
            plan_duplicate_blank_page_v1(
                document_id(),
                &[customer, source],
                &pages,
                &[customer],
                source,
                destination_identity(),
            ),
            Err(DuplicateBlankPageErrorV1::SourceNotCustomer { page_id: source })
        );
    }

    #[test]
    fn duplicate_rejects_destination_identity_collision() {
        let source = page_id(1);
        let destination = destination_page_id();
        let pages = BTreeMap::from([
            (source, blank_page(source, 100, 200)),
            (destination, blank_page(destination, 300, 400)),
        ]);

        assert_eq!(
            plan_duplicate_blank_page_v1(
                document_id(),
                &[source],
                &pages,
                &[source],
                source,
                destination_identity(),
            ),
            Err(DuplicateBlankPageErrorV1::IdentityCollision {
                page_id: destination,
            })
        );
    }

    #[test]
    fn forward_rejects_stale_source_page_state_before_mutation() {
        let source = page_id(1);
        let mut pages = BTreeMap::from([(source, blank_page(source, 100, 200))]);
        let transition = plan_duplicate_blank_page_v1(
            document_id(),
            &[source],
            &pages,
            &[source],
            source,
            destination_identity(),
        )
        .expect("plan duplicate");
        pages.get_mut(&source).expect("source").size =
            Size2D::new(LengthEmu::new(101), LengthEmu::new(200));
        let mut document_pages = vec![source];
        let before_document = document_pages.clone();
        let before_pages = pages.clone();

        assert_eq!(
            apply_duplicate_blank_page_forward_v1(
                document_id(),
                &mut document_pages,
                &mut pages,
                &transition,
            ),
            Err(DuplicateBlankPageErrorV1::SourcePageStateMismatch)
        );
        assert_eq!(document_pages, before_document);
        assert_eq!(pages, before_pages);
    }

    #[test]
    fn forward_rejects_tampered_destination_copy_before_mutation() {
        let source = page_id(1);
        let mut pages = BTreeMap::from([(source, blank_page(source, 100, 200))]);
        let mut transition = plan_duplicate_blank_page_v1(
            document_id(),
            &[source],
            &pages,
            &[source],
            source,
            destination_identity(),
        )
        .expect("plan duplicate");
        transition.destination_page.size = Size2D::new(LengthEmu::new(999), LengthEmu::new(200));
        let mut document_pages = vec![source];
        let before_document = document_pages.clone();
        let before_pages = pages.clone();

        assert_eq!(
            apply_duplicate_blank_page_forward_v1(
                document_id(),
                &mut document_pages,
                &mut pages,
                &transition,
            ),
            Err(DuplicateBlankPageErrorV1::DestinationPageCopyMismatch)
        );
        assert_eq!(document_pages, before_document);
        assert_eq!(pages, before_pages);
    }
}
