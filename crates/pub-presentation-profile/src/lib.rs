#[cfg(feature = "canonical-page-ids")]
use pub_model::derive_pub_page_id_v1;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fmt,
};

pub const CARLTON_PRESENTATION_INPUT_SCHEMA_V1: &str =
    "chaptera.carlton-presentation-profile-input.v1";
pub const CARLTON_PRESENTATION_MANIFEST_SCHEMA_V1: &str =
    "chaptera.carlton-presentation-manifest.v1";

const MARCH_2026_SHA256: &str = "bf9cda0f632b5820ab9dbdbe1b838b2a988b2f3fdd69253c22b4fc3aef9f11c3";
const DECEMBER_2025_SHA256: &str =
    "41786e9ee564dfc3d10864a49c9e58af5e1689d31479ef0c478ffb79da16f79e";

const SAMPLE_NEWSLETTER_SHA256: &str =
    "6a825ba26ba35d6e885acdc62e859591ed37cb0ff7480b554b9cb362b644dfcf";
const SAMPLE_BROCHURE_SHA256: &str =
    "ffed034ac87e679f0bd08ff9cf74ad11c0e0e510a42b1bc1a7502415f6c29c87";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CarltonPageEvidenceV1 {
    pub document_ordinal: usize,
    pub contents_seq_num: u32,
    pub oid_dword0: Option<u32>,
    pub oid_dword1: Option<u32>,
    pub applied_master_seq_num: Option<u32>,
    pub shape_child_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CarltonPresentationProfileInputV1 {
    pub schema_version: String,
    pub source_sha256: String,
    pub pages: Vec<CarltonPageEvidenceV1>,
    #[serde(default)]
    pub carrier_page_seq_nums: Vec<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CarltonPageRoleV1 {
    Customer,
    Master,
    Carrier,
    InternalService,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CarltonPageRoleReceiptV1 {
    pub document_ordinal: usize,
    pub contents_seq_num: u32,
    pub page_id: String,
    pub oid_dword0: Option<u32>,
    pub oid_dword1: Option<u32>,
    pub applied_master_seq_num: Option<u32>,
    pub shape_child_count: usize,
    pub role: CarltonPageRoleV1,
    pub reason_codes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CarltonMasterPresentationRelationV1 {
    pub source_page_seq_num: u32,
    pub source_page_id: String,
    pub master_page_seq_num: u32,
    pub master_page_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CarltonPresentationInvariantsV1 {
    pub exact_source_admission: bool,
    pub family_scoped_oid_rule: bool,
    pub canonical_source_graph_mutated: bool,
    pub canonical_page_ids_preserved: bool,
    pub carrier_pages_exposed_as_customer_pages: bool,
    pub master_pages_exposed_as_customer_pages: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CarltonPresentationSelectionV1 {
    pub profile_id: String,
    pub source_sha256: String,
    pub raw_page_count: usize,
    pub customer_page_seq_nums: Vec<u32>,
    pub master_page_seq_nums: Vec<u32>,
    pub carrier_page_seq_nums: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CarltonPresentationManifestV1 {
    pub schema_version: String,
    pub profile_id: String,
    pub source_sha256: String,
    pub raw_page_count: usize,
    pub customer_page_count: usize,
    pub customer_page_seq_nums: Vec<u32>,
    pub customer_page_ids: Vec<String>,
    pub master_page_seq_nums: Vec<u32>,
    pub carrier_page_seq_nums: Vec<u32>,
    pub pages: Vec<CarltonPageRoleReceiptV1>,
    pub customer_master_relations: Vec<CarltonMasterPresentationRelationV1>,
    pub invariants: CarltonPresentationInvariantsV1,
}

pub const STANDARD_PRINT_SERVICE_TAIL_INPUT_SCHEMA_V1: &str =
    "chaptera.standard-print-service-tail-profile-input.v1";
pub const STANDARD_PRINT_SERVICE_TAIL_PROFILE_ID_V1: &str =
    "publisher-mature-0x2c/standard-print-service-tail/v1";
pub const STANDARD_PRINT_SERVICE_TAIL_SPECIAL_PROFILE_ID_V1: &str =
    "publisher-mature-0x2c/standard-print-service-tail-interleaved/v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StandardPrintServiceTailPageEvidenceV1 {
    pub document_ordinal: usize,
    pub contents_seq_num: u32,
    pub oid_dword0: Option<u32>,
    pub oid_dword1: Option<u32>,
    pub applied_master_seq_num: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StandardPrintServiceTailProfileInputV1 {
    pub schema_version: String,
    pub document_page_list_entry_count: usize,
    pub confirmed_page_count: usize,
    pub special_entry_count: usize,
    pub scenario_evidence_list_count: usize,
    pub observed_scenario_page_count: usize,
    pub pages: Vec<StandardPrintServiceTailPageEvidenceV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StandardPrintServiceTailSelectionV1 {
    pub profile_id: String,
    pub raw_page_count: usize,
    pub customer_page_seq_nums: Vec<u32>,
    pub master_page_seq_num: u32,
    pub service_page_seq_nums: Vec<u32>,
}

/// Admits the bounded mature-0x2C print-family profile proven by independent
/// Virginia, Kroy and rendered-sheet controls.
///
/// This is deliberately a presentation-family selector rather than generic PAGE
/// semantics. Any structural drift returns `None`, allowing the Viewer to keep
/// its generic no-loss PAGE projection.
pub fn select_standard_print_service_tail_customer_page_seq_nums_v1(
    mut input: StandardPrintServiceTailProfileInputV1,
) -> Option<StandardPrintServiceTailSelectionV1> {
    if input.schema_version != STANDARD_PRINT_SERVICE_TAIL_INPUT_SCHEMA_V1
        || input.scenario_evidence_list_count != 0
        || input.observed_scenario_page_count != 0
        || input.confirmed_page_count != input.pages.len()
        || input.document_page_list_entry_count
            != input
                .confirmed_page_count
                .checked_add(input.special_entry_count)?
        || input.special_entry_count > 1
    {
        return None;
    }

    input.pages.sort_by_key(|page| page.document_ordinal);
    let mut ordinals = BTreeSet::new();
    let mut seq_nums = BTreeSet::new();
    for page in &input.pages {
        if page.document_ordinal >= input.document_page_list_entry_count
            || !ordinals.insert(page.document_ordinal)
            || !seq_nums.insert(page.contents_seq_num)
        {
            return None;
        }
    }

    let oid_is_zero = |page: &StandardPrintServiceTailPageEvidenceV1| {
        page.oid_dword0 == Some(0) && page.oid_dword1 == Some(0)
    };
    let oid_is_nonzero = |page: &StandardPrintServiceTailPageEvidenceV1| matches!((page.oid_dword0, page.oid_dword1), (Some(d0), Some(d1)) if d0 != 0 || d1 != 0);

    let master = input.pages.first()?;
    if master.document_ordinal != 0
        || !oid_is_zero(master)
        || master.applied_master_seq_num.is_some()
    {
        return None;
    }
    let master_seq = master.contents_seq_num;

    if input.special_entry_count == 0 {
        if input.pages.len() < 6
            || input
                .pages
                .iter()
                .enumerate()
                .any(|(expected_ordinal, page)| page.document_ordinal != expected_ordinal)
        {
            return None;
        }

        let split = input.pages.len().checked_sub(4)?;
        let customer_pages = input.pages.get(1..split)?;
        let service_pages = input.pages.get(split..)?;
        if customer_pages.is_empty()
            || !customer_pages
                .iter()
                .all(|page| oid_is_nonzero(page) && page.applied_master_seq_num == Some(master_seq))
            || !service_pages
                .iter()
                .all(|page| oid_is_zero(page) && page.applied_master_seq_num == Some(master_seq))
        {
            return None;
        }

        return Some(StandardPrintServiceTailSelectionV1 {
            profile_id: STANDARD_PRINT_SERVICE_TAIL_PROFILE_ID_V1.to_owned(),
            raw_page_count: input.pages.len(),
            customer_page_seq_nums: customer_pages
                .iter()
                .map(|page| page.contents_seq_num)
                .collect(),
            master_page_seq_num: master_seq,
            service_page_seq_nums: service_pages
                .iter()
                .map(|page| page.contents_seq_num)
                .collect(),
        });
    }

    // Source-semantic interleaved standard-print shape proven first by a
    // one-page native Publisher control and then falsified across all 55 paired
    // Batch01 fixtures in #973:
    //
    //   PAGE master
    //   K>=1 PAGE customer records
    //   PAGE service
    //   raw0x59 special entry
    //   PAGE service
    //   PAGE service
    //
    // The special record remains preserved in SourceGraph; only product PAGE
    // presentation is narrowed.
    if input.special_entry_count != 1
        || input.document_page_list_entry_count != input.confirmed_page_count.checked_add(1)?
        || input.confirmed_page_count < 5
    {
        return None;
    }

    let customer_count = input.confirmed_page_count.checked_sub(4)?;
    if customer_count == 0 {
        return None;
    }
    let customer_end = customer_count.checked_add(1)?;
    let service_ordinal = customer_end;
    let special_ordinal = service_ordinal.checked_add(1)?;
    let tail_first_ordinal = special_ordinal.checked_add(1)?;
    let tail_second_ordinal = special_ordinal.checked_add(2)?;

    let customer_pages = input.pages.get(1..customer_end)?;
    let first_service = input.pages.get(customer_end)?;
    let tail_pages = input.pages.get(customer_end.checked_add(1)?..)?;

    if !customer_pages.iter().enumerate().all(|(offset, page)| {
        page.document_ordinal == offset + 1
            && oid_is_nonzero(page)
            && page.applied_master_seq_num == Some(master_seq)
    }) || first_service.document_ordinal != service_ordinal
        || !oid_is_zero(first_service)
        || first_service.applied_master_seq_num != Some(master_seq)
        || tail_pages.len() != 2
        || tail_pages
            .iter()
            .map(|page| page.document_ordinal)
            .collect::<Vec<_>>()
            != vec![tail_first_ordinal, tail_second_ordinal]
        || !tail_pages
            .iter()
            .all(|page| oid_is_zero(page) && page.applied_master_seq_num == Some(master_seq))
    {
        return None;
    }

    let mut service_page_seq_nums = Vec::with_capacity(3);
    service_page_seq_nums.push(first_service.contents_seq_num);
    service_page_seq_nums.extend(tail_pages.iter().map(|page| page.contents_seq_num));

    Some(StandardPrintServiceTailSelectionV1 {
        profile_id: STANDARD_PRINT_SERVICE_TAIL_SPECIAL_PROFILE_ID_V1.to_owned(),
        raw_page_count: input.pages.len(),
        customer_page_seq_nums: customer_pages
            .iter()
            .map(|page| page.contents_seq_num)
            .collect(),
        master_page_seq_num: master_seq,
        service_page_seq_nums,
    })
}

pub const MATURE_ZERO_LEADER_DETACHED_TAIL_PROFILE_ID_V1: &str =
    "publisher-mature-0x2c/zero-leader-detached-post-special-tail/v1";

/// Admits the zero-OID leader / detached post-special topology isolated by #979.
pub fn select_mature_zero_leader_detached_tail_customer_page_seq_nums_v1(
    mut input: StandardPrintServiceTailProfileInputV1,
) -> Option<StandardPrintServiceTailSelectionV1> {
    if input.schema_version != STANDARD_PRINT_SERVICE_TAIL_INPUT_SCHEMA_V1
        || input.confirmed_page_count != input.pages.len()
        || input.special_entry_count != 1
        || input.document_page_list_entry_count != input.confirmed_page_count.checked_add(1)?
    {
        return None;
    }

    input.pages.sort_by_key(|page| page.document_ordinal);
    let mut ordinals = BTreeSet::new();
    let mut seq_nums = BTreeSet::new();
    for page in &input.pages {
        if page.document_ordinal >= input.document_page_list_entry_count
            || !ordinals.insert(page.document_ordinal)
            || !seq_nums.insert(page.contents_seq_num)
        {
            return None;
        }
    }

    let oid_is_zero = |page: &StandardPrintServiceTailPageEvidenceV1| {
        page.oid_dword0 == Some(0) && page.oid_dword1 == Some(0)
    };
    let oid_is_nonzero = |page: &StandardPrintServiceTailPageEvidenceV1| {
        matches!(
            (page.oid_dword0, page.oid_dword1),
            (Some(d0), Some(d1)) if d0 != 0 || d1 != 0
        )
    };

    let leader = input.pages.first()?;
    if leader.document_ordinal != 0
        || !oid_is_zero(leader)
        || leader.applied_master_seq_num.is_some()
    {
        return None;
    }
    let leader_seq = leader.contents_seq_num;

    let mut customer_end = 1usize;
    while let Some(page) = input.pages.get(customer_end) {
        if !oid_is_nonzero(page) {
            break;
        }
        if page.document_ordinal != customer_end || page.applied_master_seq_num != Some(leader_seq)
        {
            return None;
        }
        customer_end = customer_end.checked_add(1)?;
    }
    if customer_end < 2 {
        return None;
    }

    let customer_pages = input.pages.get(1..customer_end)?;
    let service_page = input.pages.get(customer_end)?;
    let tail_pages = input.pages.get(customer_end.checked_add(1)?..)?;
    if !oid_is_zero(service_page)
        || service_page.applied_master_seq_num != Some(leader_seq)
        || tail_pages.len() != 2
        || !tail_pages
            .iter()
            .all(|page| oid_is_zero(page) && page.applied_master_seq_num.is_none())
    {
        return None;
    }

    let last_customer_ordinal = customer_pages.last()?.document_ordinal;
    if input
        .document_page_list_entry_count
        .checked_sub(last_customer_ordinal.checked_add(1)?)?
        != 4
        || service_page.document_ordinal != last_customer_ordinal.checked_add(1)?
        || tail_pages
            .iter()
            .map(|page| page.document_ordinal)
            .collect::<Vec<_>>()
            != vec![
                last_customer_ordinal.checked_add(3)?,
                last_customer_ordinal.checked_add(4)?,
            ]
    {
        return None;
    }

    let mut service_page_seq_nums = Vec::with_capacity(3);
    service_page_seq_nums.push(service_page.contents_seq_num);
    service_page_seq_nums.extend(tail_pages.iter().map(|page| page.contents_seq_num));

    Some(StandardPrintServiceTailSelectionV1 {
        profile_id: MATURE_ZERO_LEADER_DETACHED_TAIL_PROFILE_ID_V1.to_owned(),
        raw_page_count: input.pages.len(),
        customer_page_seq_nums: customer_pages
            .iter()
            .map(|page| page.contents_seq_num)
            .collect(),
        master_page_seq_num: leader_seq,
        service_page_seq_nums,
    })
}

pub const MATURE_DETACHED_POST_SPECIAL_TAIL_PROFILE_ID_V1: &str =
    "publisher-mature-0x2c/detached-post-special-tail/v1";

/// Admits the mature detached post-special topology isolated by #977.
///
/// The selected customer set is source-semantic: one nonzero no-master leader,
/// a contiguous nonzero block applying that leader, the final nonzero PAGE as
/// terminal service, one interleaved special entry, then two zero-OID PAGEs
/// whose applied-master relation is absent. Structural drift returns `None`.
pub fn select_mature_detached_post_special_tail_customer_page_seq_nums_v1(
    mut input: StandardPrintServiceTailProfileInputV1,
) -> Option<StandardPrintServiceTailSelectionV1> {
    if input.schema_version != STANDARD_PRINT_SERVICE_TAIL_INPUT_SCHEMA_V1
        || input.confirmed_page_count != input.pages.len()
        || input.special_entry_count != 1
        || input.document_page_list_entry_count != input.confirmed_page_count.checked_add(1)?
    {
        return None;
    }

    input.pages.sort_by_key(|page| page.document_ordinal);
    let mut ordinals = BTreeSet::new();
    let mut seq_nums = BTreeSet::new();
    for page in &input.pages {
        if page.document_ordinal >= input.document_page_list_entry_count
            || !ordinals.insert(page.document_ordinal)
            || !seq_nums.insert(page.contents_seq_num)
        {
            return None;
        }
    }

    let oid_is_zero = |page: &StandardPrintServiceTailPageEvidenceV1| {
        page.oid_dword0 == Some(0) && page.oid_dword1 == Some(0)
    };
    let oid_is_nonzero = |page: &StandardPrintServiceTailPageEvidenceV1| {
        matches!(
            (page.oid_dword0, page.oid_dword1),
            (Some(d0), Some(d1)) if d0 != 0 || d1 != 0
        )
    };

    let leader = input.pages.first()?;
    if leader.document_ordinal != 0
        || !oid_is_nonzero(leader)
        || leader.applied_master_seq_num.is_some()
    {
        return None;
    }
    let leader_seq = leader.contents_seq_num;

    let mut nonzero_end = 1usize;
    while let Some(page) = input.pages.get(nonzero_end) {
        if !oid_is_nonzero(page) {
            break;
        }
        if page.document_ordinal != nonzero_end || page.applied_master_seq_num != Some(leader_seq) {
            return None;
        }
        nonzero_end = nonzero_end.checked_add(1)?;
    }

    if nonzero_end < 3 {
        return None;
    }
    let customer_pages = input.pages.get(1..nonzero_end.checked_sub(1)?)?;
    let terminal_service = input.pages.get(nonzero_end.checked_sub(1)?)?;
    let tail_pages = input.pages.get(nonzero_end..)?;
    if customer_pages.is_empty()
        || tail_pages.len() != 2
        || !tail_pages
            .iter()
            .all(|page| oid_is_zero(page) && page.applied_master_seq_num.is_none())
    {
        return None;
    }

    let last_customer_ordinal = customer_pages.last()?.document_ordinal;
    if input
        .document_page_list_entry_count
        .checked_sub(last_customer_ordinal.checked_add(1)?)?
        != 4
        || terminal_service.document_ordinal != last_customer_ordinal.checked_add(1)?
        || tail_pages
            .iter()
            .map(|page| page.document_ordinal)
            .collect::<Vec<_>>()
            != vec![
                last_customer_ordinal.checked_add(3)?,
                last_customer_ordinal.checked_add(4)?,
            ]
    {
        return None;
    }

    let mut service_page_seq_nums = Vec::with_capacity(3);
    service_page_seq_nums.push(terminal_service.contents_seq_num);
    service_page_seq_nums.extend(tail_pages.iter().map(|page| page.contents_seq_num));

    Some(StandardPrintServiceTailSelectionV1 {
        profile_id: MATURE_DETACHED_POST_SPECIAL_TAIL_PROFILE_ID_V1.to_owned(),
        raw_page_count: input.pages.len(),
        customer_page_seq_nums: customer_pages
            .iter()
            .map(|page| page.contents_seq_num)
            .collect(),
        master_page_seq_num: leader_seq,
        service_page_seq_nums,
    })
}

pub const MATURE_TERMINAL_SERVICE_TAIL_PROFILE_ID_V1: &str =
    "publisher-mature-0x2c/terminal-nonzero-service-tail/v1";

/// Admits the mature 0x2C terminal-service topology proven by the all-55
/// Publisher-paired census in #963.
///
/// This selector is intentionally source-semantic and payload-blind. It uses
/// only DOCUMENT/PAGE order, Page.Oid zero/nonzero class, and the applied-master
/// relation. External page counts, source hashes, filenames, text/payload state,
/// and PAGE seqNum constants are not inputs. Structural drift returns `None`.
pub fn select_mature_terminal_service_tail_customer_page_seq_nums_v1(
    mut input: StandardPrintServiceTailProfileInputV1,
) -> Option<StandardPrintServiceTailSelectionV1> {
    if input.schema_version != STANDARD_PRINT_SERVICE_TAIL_INPUT_SCHEMA_V1
        || input.confirmed_page_count != input.pages.len()
        || input.document_page_list_entry_count
            != input
                .confirmed_page_count
                .checked_add(input.special_entry_count)?
        || input.special_entry_count > 1
    {
        return None;
    }

    input.pages.sort_by_key(|page| page.document_ordinal);
    let mut ordinals = BTreeSet::new();
    let mut seq_nums = BTreeSet::new();
    for page in &input.pages {
        if page.document_ordinal >= input.document_page_list_entry_count
            || !ordinals.insert(page.document_ordinal)
            || !seq_nums.insert(page.contents_seq_num)
        {
            return None;
        }
    }

    let oid_is_zero = |page: &StandardPrintServiceTailPageEvidenceV1| {
        page.oid_dword0 == Some(0) && page.oid_dword1 == Some(0)
    };
    let oid_is_nonzero = |page: &StandardPrintServiceTailPageEvidenceV1| {
        matches!(
            (page.oid_dword0, page.oid_dword1),
            (Some(d0), Some(d1)) if d0 != 0 || d1 != 0
        )
    };

    let leader = input.pages.first()?;
    if leader.document_ordinal != 0
        || !oid_is_nonzero(leader)
        || leader.applied_master_seq_num.is_some()
    {
        return None;
    }
    let leader_seq = leader.contents_seq_num;

    let mut nonzero_end = 1usize;
    while let Some(page) = input.pages.get(nonzero_end) {
        if !oid_is_nonzero(page) {
            break;
        }
        if page.applied_master_seq_num != Some(leader_seq) {
            return None;
        }
        nonzero_end = nonzero_end.checked_add(1)?;
    }

    // At least one customer PAGE plus one terminal nonzero service PAGE.
    if nonzero_end < 3 {
        return None;
    }
    let customer_pages = input.pages.get(1..nonzero_end.checked_sub(1)?)?;
    let terminal_service = input.pages.get(nonzero_end.checked_sub(1)?)?;
    let zero_tail = input.pages.get(nonzero_end..)?;
    if customer_pages.is_empty()
        || zero_tail.is_empty()
        || !zero_tail
            .iter()
            .all(|page| oid_is_zero(page) && page.applied_master_seq_num == Some(leader_seq))
    {
        return None;
    }

    // The all-55 falsifier admitted only the exact four-entry DOCUMENT suffix:
    //   PAGE service / PAGE / PAGE / PAGE
    // or
    //   PAGE service / raw0x59 / PAGE / PAGE.
    // The page-role receipt exposes the one special entry as the sole ordinal
    // absent from the PAGE sequence, so the suffix can be fenced without
    // importing payload semantics.
    let last_customer_ordinal = customer_pages.last()?.document_ordinal;
    if input
        .document_page_list_entry_count
        .checked_sub(last_customer_ordinal.checked_add(1)?)?
        != 4
        || terminal_service.document_ordinal != last_customer_ordinal.checked_add(1)?
    {
        return None;
    }

    let expected_zero_ordinals = if input.special_entry_count == 0 {
        vec![
            last_customer_ordinal.checked_add(2)?,
            last_customer_ordinal.checked_add(3)?,
            last_customer_ordinal.checked_add(4)?,
        ]
    } else {
        vec![
            last_customer_ordinal.checked_add(3)?,
            last_customer_ordinal.checked_add(4)?,
        ]
    };
    if zero_tail
        .iter()
        .map(|page| page.document_ordinal)
        .collect::<Vec<_>>()
        != expected_zero_ordinals
    {
        return None;
    }

    let mut service_page_seq_nums = Vec::with_capacity(1 + zero_tail.len());
    service_page_seq_nums.push(terminal_service.contents_seq_num);
    service_page_seq_nums.extend(zero_tail.iter().map(|page| page.contents_seq_num));

    Some(StandardPrintServiceTailSelectionV1 {
        profile_id: MATURE_TERMINAL_SERVICE_TAIL_PROFILE_ID_V1.to_owned(),
        raw_page_count: input.pages.len(),
        customer_page_seq_nums: customer_pages
            .iter()
            .map(|page| page.contents_seq_num)
            .collect(),
        master_page_seq_num: leader_seq,
        service_page_seq_nums,
    })
}

pub const LEGACY22_PAGE_LIST_PROFILE_INPUT_SCHEMA_V1: &str =
    "chaptera.legacy22-page-list-profile-input.v1";
pub const LEGACY22_NOQUILL_PAGE_PROFILE_ID_V1: &str = "publisher-legacy22/noquill-middle-pages/v1";
pub const LEGACY22_QUILL_PAGE_PROFILE_ID_V1: &str = "publisher-legacy22/quill-middle-pages/v1";

const LEGACY22_PAGE_RAW_TYPE_V1: u16 = 0x0014;
const LEGACY22_PAGE_LIST_SPECIAL_RAW_TYPE_V1: u16 = 0x0041;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Legacy22PageListDialectV1 {
    NoQuill,
    Quill,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Legacy22PageListEntryEvidenceV1 {
    pub document_ordinal: usize,
    pub raw_type: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Legacy22PageListProfileInputV1 {
    pub schema_version: String,
    pub dialect: Legacy22PageListDialectV1,
    pub document_page_list_entry_count: usize,
    pub physical_page_count: usize,
    pub entries: Vec<Legacy22PageListEntryEvidenceV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Legacy22PageListPresentationSelectionV1 {
    pub profile_id: String,
    pub raw_page_list_entry_count: usize,
    pub materialized_page_count: usize,
    pub customer_page_indices: Vec<usize>,
}

/// Admits only the old-0x22 PageList envelopes proven by the strict 650-file
/// recurrence census and independent Publisher controls.
///
/// The input is source-semantic topology only. External PDF/page counts, source
/// hashes, filenames, text/payload presence, and mature-0x2C PAGE fields are
/// deliberately absent. Any structural drift returns `None`, preserving the
/// Viewer's generic no-loss PageList projection.
pub fn select_legacy22_customer_page_indices_v1(
    mut input: Legacy22PageListProfileInputV1,
) -> Option<Legacy22PageListPresentationSelectionV1> {
    if input.schema_version != LEGACY22_PAGE_LIST_PROFILE_INPUT_SCHEMA_V1
        || input.document_page_list_entry_count != input.entries.len()
        || input.entries.is_empty()
    {
        return None;
    }

    input.entries.sort_by_key(|entry| entry.document_ordinal);
    if input
        .entries
        .iter()
        .enumerate()
        .any(|(expected, entry)| entry.document_ordinal != expected)
    {
        return None;
    }

    let entry_types = input
        .entries
        .iter()
        .map(|entry| entry.raw_type)
        .collect::<Vec<_>>();
    let materialized_page_count = entry_types
        .iter()
        .filter(|raw_type| **raw_type == LEGACY22_PAGE_RAW_TYPE_V1)
        .count();
    if input.physical_page_count != materialized_page_count.checked_add(1)? {
        return None;
    }

    match input.dialect {
        Legacy22PageListDialectV1::NoQuill => {
            if entry_types.len() < 4
                || entry_types
                    .iter()
                    .any(|raw_type| *raw_type != LEGACY22_PAGE_RAW_TYPE_V1)
            {
                return None;
            }
            let customer_end = entry_types.len().checked_sub(1)?;
            if customer_end <= 2 {
                return None;
            }
            Some(Legacy22PageListPresentationSelectionV1 {
                profile_id: LEGACY22_NOQUILL_PAGE_PROFILE_ID_V1.to_owned(),
                raw_page_list_entry_count: entry_types.len(),
                materialized_page_count,
                customer_page_indices: (2..customer_end).collect(),
            })
        }
        Legacy22PageListDialectV1::Quill => {
            if entry_types.len() < 6
                || entry_types[0] != LEGACY22_PAGE_RAW_TYPE_V1
                || entry_types[1] != LEGACY22_PAGE_RAW_TYPE_V1
            {
                return None;
            }

            let tail_start = entry_types.len().checked_sub(3)?;
            if tail_start <= 2
                || entry_types[2..tail_start]
                    .iter()
                    .any(|raw_type| *raw_type != LEGACY22_PAGE_RAW_TYPE_V1)
            {
                return None;
            }
            let tail = &entry_types[tail_start..];
            let all_page_tail = tail
                == [
                    LEGACY22_PAGE_RAW_TYPE_V1,
                    LEGACY22_PAGE_RAW_TYPE_V1,
                    LEGACY22_PAGE_RAW_TYPE_V1,
                ];
            let special_tail = tail
                == [
                    LEGACY22_PAGE_RAW_TYPE_V1,
                    LEGACY22_PAGE_LIST_SPECIAL_RAW_TYPE_V1,
                    LEGACY22_PAGE_RAW_TYPE_V1,
                ];
            if !all_page_tail && !special_tail {
                return None;
            }

            Some(Legacy22PageListPresentationSelectionV1 {
                profile_id: LEGACY22_QUILL_PAGE_PROFILE_ID_V1.to_owned(),
                raw_page_list_entry_count: entry_types.len(),
                materialized_page_count,
                customer_page_indices: (2..tail_start).collect(),
            })
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReferenceFixturePresentationSelectionV1 {
    pub profile_id: String,
    pub source_sha256: String,
    pub raw_page_count: usize,
    pub customer_page_seq_nums: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReferenceFixturePresentationError {
    RawPageSequenceMismatch {
        profile_id: &'static str,
        expected: Vec<u32>,
        observed: Vec<u32>,
    },
}

impl fmt::Display for ReferenceFixturePresentationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RawPageSequenceMismatch {
                profile_id,
                expected,
                observed,
            } => write!(
                f,
                "reference fixture {profile_id} raw PAGE sequence mismatch: expected {expected:?}, observed {observed:?}"
            ),
        }
    }
}

impl Error for ReferenceFixturePresentationError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CarltonPresentationError {
    SchemaVersionMismatch,
    UnsupportedSourceHash,
    EmptyPageSet,
    DuplicateDocumentOrdinal(usize),
    DuplicatePageSeqNum(u32),
    CustomerCountMismatch {
        expected: usize,
        observed: usize,
    },
    MissingMasterTarget,
    MultipleMasterTargets(Vec<u32>),
    MasterTargetMissingFromPages(u32),
    MasterLooksCustomerVisible(u32),
    UnknownCarrierPage(u32),
    CarrierCustomerOverlap(u32),
    CarrierMasterOverlap(u32),
    CarrierEvidenceMismatch {
        expected: Vec<u32>,
        observed: Vec<u32>,
    },
    CustomerMissingExpectedMaster {
        page_seq_num: u32,
        expected_master: u32,
    },
    IdentityDerivation {
        page_seq_num: u32,
    },
}

impl fmt::Display for CarltonPresentationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SchemaVersionMismatch => write!(f, "Carlton presentation input schema mismatch"),
            Self::UnsupportedSourceHash => {
                write!(f, "source is not an admitted Carlton family control")
            }
            Self::EmptyPageSet => write!(f, "Carlton presentation input has no PAGE evidence"),
            Self::DuplicateDocumentOrdinal(value) => {
                write!(f, "duplicate Carlton document ordinal {value}")
            }
            Self::DuplicatePageSeqNum(value) => {
                write!(f, "duplicate Carlton PAGE seqNum {value}")
            }
            Self::CustomerCountMismatch { expected, observed } => write!(
                f,
                "Carlton customer-page count mismatch: expected {expected}, observed {observed}"
            ),
            Self::MissingMasterTarget => {
                write!(f, "Carlton family evidence has no applied-master target")
            }
            Self::MultipleMasterTargets(values) => {
                write!(
                    f,
                    "Carlton family evidence has multiple master targets: {values:?}"
                )
            }
            Self::MasterTargetMissingFromPages(value) => {
                write!(
                    f,
                    "Carlton master target PAGE {value} is absent from PAGE evidence"
                )
            }
            Self::MasterLooksCustomerVisible(value) => write!(
                f,
                "Carlton master PAGE {value} unexpectedly satisfies the family customer Oid rule"
            ),
            Self::UnknownCarrierPage(value) => {
                write!(
                    f,
                    "Carlton Cmo carrier PAGE {value} is absent from PAGE evidence"
                )
            }
            Self::CarrierCustomerOverlap(value) => write!(
                f,
                "Carlton Cmo carrier PAGE {value} also satisfies the customer-page rule"
            ),
            Self::CarrierMasterOverlap(value) => {
                write!(
                    f,
                    "Carlton Cmo carrier PAGE {value} is also the master PAGE"
                )
            }
            Self::CarrierEvidenceMismatch { expected, observed } => write!(
                f,
                "Carlton carrier PAGE evidence mismatch: expected {expected:?}, observed {observed:?}"
            ),
            Self::CustomerMissingExpectedMaster {
                page_seq_num,
                expected_master,
            } => write!(
                f,
                "Carlton customer PAGE {page_seq_num} does not apply expected master PAGE {expected_master}"
            ),
            Self::IdentityDerivation { page_seq_num } => write!(
                f,
                "cannot derive canonical PageId for Carlton PAGE {page_seq_num}"
            ),
        }
    }
}

impl Error for CarltonPresentationError {}

#[derive(Debug, Clone, Copy)]
struct ReferenceFixtureProfile {
    profile_id: &'static str,
    expected_raw_page_seq_nums: &'static [u32],
    customer_page_seq_nums: &'static [u32],
}

fn reference_fixture_profile(source_sha256: &str) -> Option<ReferenceFixtureProfile> {
    match source_sha256 {
        SAMPLE_NEWSLETTER_SHA256 => Some(ReferenceFixtureProfile {
            profile_id: "apache-poi/sample-newsletter/publisher-reference/v1",
            expected_raw_page_seq_nums: &[263, 266, 323, 352, 381, 269, 273, 277],
            customer_page_seq_nums: &[266, 323, 352, 381],
        }),
        SAMPLE_BROCHURE_SHA256 => Some(ReferenceFixtureProfile {
            profile_id: "apache-poi/sample-brochure/publisher-reference/v1",
            expected_raw_page_seq_nums: &[263, 266, 334, 269, 273, 277],
            customer_page_seq_nums: &[266, 334],
        }),
        _ => None,
    }
}

/// Returns true only for exact, previously evidenced reference fixture bytes.
/// This is intentionally not a family classifier.
pub fn reference_fixture_profile_known_v1(source_sha256: &str) -> bool {
    reference_fixture_profile(source_sha256).is_some()
}

/// Applies an exact-source Publisher-backed presentation projection to the two
/// pinned Apache POI reference fixtures. Unknown hashes are not classified.
///
/// The caller must pass the recovered physical PAGE sequence in document order.
/// Any drift on an admitted hash fails closed so the product can fall back to
/// generic no-loss presentation rather than silently hiding source truth.
pub fn select_reference_fixture_customer_page_seq_nums_v1(
    source_sha256: &str,
    observed_raw_page_seq_nums: &[u32],
) -> Result<Option<ReferenceFixturePresentationSelectionV1>, ReferenceFixturePresentationError> {
    let Some(profile) = reference_fixture_profile(source_sha256) else {
        return Ok(None);
    };

    if observed_raw_page_seq_nums != profile.expected_raw_page_seq_nums {
        return Err(ReferenceFixturePresentationError::RawPageSequenceMismatch {
            profile_id: profile.profile_id,
            expected: profile.expected_raw_page_seq_nums.to_vec(),
            observed: observed_raw_page_seq_nums.to_vec(),
        });
    }

    Ok(Some(ReferenceFixturePresentationSelectionV1 {
        profile_id: profile.profile_id.to_owned(),
        source_sha256: source_sha256.to_owned(),
        raw_page_count: observed_raw_page_seq_nums.len(),
        customer_page_seq_nums: profile.customer_page_seq_nums.to_vec(),
    }))
}

#[derive(Debug, Clone, Copy)]
struct AdmittedProfile {
    profile_id: &'static str,
    expected_customer_count: usize,
    carrier_page_seq_nums: &'static [u32],
}

fn admitted_profile(source_sha256: &str) -> Result<AdmittedProfile, CarltonPresentationError> {
    match source_sha256 {
        MARCH_2026_SHA256 => Ok(AdmittedProfile {
            profile_id: "carlton-school-jotter/march-2026/v1",
            expected_customer_count: 3,
            carrier_page_seq_nums: &[279],
        }),
        DECEMBER_2025_SHA256 => Ok(AdmittedProfile {
            profile_id: "carlton-school-jotter/december-2025/v1",
            expected_customer_count: 5,
            carrier_page_seq_nums: &[279],
        }),
        _ => Err(CarltonPresentationError::UnsupportedSourceHash),
    }
}

/// Returns the previously proven PlcCmob carrier PAGE set for one exact admitted
/// Carlton family control. Unknown source hashes are not Carlton-admitted.
pub fn carlton_admitted_carrier_page_seq_nums_v1(source_sha256: &str) -> Option<&'static [u32]> {
    admitted_profile(source_sha256)
        .ok()
        .map(|profile| profile.carrier_page_seq_nums)
}

struct ValidatedCarltonPresentation {
    input: CarltonPresentationProfileInputV1,
    profile: AdmittedProfile,
    customer_seq_nums: Vec<u32>,
    master_seq: u32,
    carrier_set: BTreeSet<u32>,
}

fn validate_carlton_presentation_v1(
    mut input: CarltonPresentationProfileInputV1,
) -> Result<ValidatedCarltonPresentation, CarltonPresentationError> {
    if input.schema_version != CARLTON_PRESENTATION_INPUT_SCHEMA_V1 {
        return Err(CarltonPresentationError::SchemaVersionMismatch);
    }
    let profile = admitted_profile(&input.source_sha256)?;
    if input.pages.is_empty() {
        return Err(CarltonPresentationError::EmptyPageSet);
    }

    input.pages.sort_by_key(|page| page.document_ordinal);
    let mut ordinals = BTreeSet::new();
    let mut pages_by_seq = BTreeMap::new();
    for page in &input.pages {
        if !ordinals.insert(page.document_ordinal) {
            return Err(CarltonPresentationError::DuplicateDocumentOrdinal(
                page.document_ordinal,
            ));
        }
        if pages_by_seq.insert(page.contents_seq_num, page).is_some() {
            return Err(CarltonPresentationError::DuplicatePageSeqNum(
                page.contents_seq_num,
            ));
        }
    }

    let customer_seq_nums = input
        .pages
        .iter()
        .filter(|page| page.oid_dword0 == Some(2))
        .map(|page| page.contents_seq_num)
        .collect::<Vec<_>>();
    if customer_seq_nums.len() != profile.expected_customer_count {
        return Err(CarltonPresentationError::CustomerCountMismatch {
            expected: profile.expected_customer_count,
            observed: customer_seq_nums.len(),
        });
    }
    let customer_set = customer_seq_nums.iter().copied().collect::<BTreeSet<_>>();

    let master_targets = input
        .pages
        .iter()
        .filter_map(|page| page.applied_master_seq_num)
        .collect::<BTreeSet<_>>();
    let master_seq = match master_targets.len() {
        0 => return Err(CarltonPresentationError::MissingMasterTarget),
        1 => *master_targets.first().expect("one master"),
        _ => {
            return Err(CarltonPresentationError::MultipleMasterTargets(
                master_targets.iter().copied().collect(),
            ));
        }
    };
    let master_page = pages_by_seq.get(&master_seq).copied().ok_or(
        CarltonPresentationError::MasterTargetMissingFromPages(master_seq),
    )?;
    if master_page.oid_dword0 == Some(2) {
        return Err(CarltonPresentationError::MasterLooksCustomerVisible(
            master_seq,
        ));
    }

    let mut carrier_set = BTreeSet::new();
    for seq_num in &input.carrier_page_seq_nums {
        if !pages_by_seq.contains_key(seq_num) {
            return Err(CarltonPresentationError::UnknownCarrierPage(*seq_num));
        }
        if customer_set.contains(seq_num) {
            return Err(CarltonPresentationError::CarrierCustomerOverlap(*seq_num));
        }
        if *seq_num == master_seq {
            return Err(CarltonPresentationError::CarrierMasterOverlap(*seq_num));
        }
        carrier_set.insert(*seq_num);
    }
    let expected_carrier_set = profile
        .carrier_page_seq_nums
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    if carrier_set != expected_carrier_set {
        return Err(CarltonPresentationError::CarrierEvidenceMismatch {
            expected: expected_carrier_set.into_iter().collect(),
            observed: carrier_set.into_iter().collect(),
        });
    }

    for seq_num in &customer_seq_nums {
        let page = pages_by_seq[seq_num];
        if page.applied_master_seq_num != Some(master_seq) {
            return Err(CarltonPresentationError::CustomerMissingExpectedMaster {
                page_seq_num: *seq_num,
                expected_master: master_seq,
            });
        }
    }

    Ok(ValidatedCarltonPresentation {
        input,
        profile,
        customer_seq_nums,
        master_seq,
        carrier_set,
    })
}

/// Applies the exact Carlton family admission and PAGE-role law without
/// materializing any model-specific PageId type. This is the product-consumer
/// seam for Reader/Viewer implementations that own a different canonical model
/// crate but share the same source identity contract.
pub fn select_carlton_customer_page_seq_nums_v1(
    input: CarltonPresentationProfileInputV1,
) -> Result<CarltonPresentationSelectionV1, CarltonPresentationError> {
    let validated = validate_carlton_presentation_v1(input)?;
    Ok(CarltonPresentationSelectionV1 {
        profile_id: validated.profile.profile_id.to_owned(),
        source_sha256: validated.input.source_sha256,
        raw_page_count: validated.input.pages.len(),
        customer_page_seq_nums: validated.customer_seq_nums,
        master_page_seq_nums: vec![validated.master_seq],
        carrier_page_seq_nums: validated.carrier_set.into_iter().collect(),
    })
}

#[cfg(feature = "canonical-page-ids")]
pub fn build_carlton_presentation_manifest_v1(
    input: CarltonPresentationProfileInputV1,
) -> Result<CarltonPresentationManifestV1, CarltonPresentationError> {
    let validated = validate_carlton_presentation_v1(input)?;
    let input = validated.input;
    let profile = validated.profile;
    let customer_seq_nums = validated.customer_seq_nums;
    let master_seq = validated.master_seq;
    let carrier_set = validated.carrier_set;
    let customer_set = customer_seq_nums.iter().copied().collect::<BTreeSet<_>>();

    let page_id = |seq_num: u32| {
        derive_pub_page_id_v1(&input.source_sha256, seq_num).map_err(|_| {
            CarltonPresentationError::IdentityDerivation {
                page_seq_num: seq_num,
            }
        })
    };

    let mut pages = Vec::with_capacity(input.pages.len());
    for page in &input.pages {
        let role = if page.contents_seq_num == master_seq {
            CarltonPageRoleV1::Master
        } else if carrier_set.contains(&page.contents_seq_num) {
            CarltonPageRoleV1::Carrier
        } else if customer_set.contains(&page.contents_seq_num) {
            CarltonPageRoleV1::Customer
        } else {
            CarltonPageRoleV1::InternalService
        };
        let reason_codes = match role {
            CarltonPageRoleV1::Customer => vec![
                "exact_family_admitted".to_owned(),
                "family_oid_dword0_2".to_owned(),
                "expected_master_applied".to_owned(),
            ],
            CarltonPageRoleV1::Master => vec![
                "exact_family_admitted".to_owned(),
                "referenced_as_master_target".to_owned(),
            ],
            CarltonPageRoleV1::Carrier => vec![
                "exact_family_admitted".to_owned(),
                "plccmob_carrier_parent".to_owned(),
            ],
            CarltonPageRoleV1::InternalService => vec![
                "exact_family_admitted".to_owned(),
                "residual_non_customer_page".to_owned(),
            ],
        };
        pages.push(CarltonPageRoleReceiptV1 {
            document_ordinal: page.document_ordinal,
            contents_seq_num: page.contents_seq_num,
            page_id: page_id(page.contents_seq_num)?,
            oid_dword0: page.oid_dword0,
            oid_dword1: page.oid_dword1,
            applied_master_seq_num: page.applied_master_seq_num,
            shape_child_count: page.shape_child_count,
            role,
            reason_codes,
        });
    }

    let customer_page_ids = customer_seq_nums
        .iter()
        .map(|seq_num| page_id(*seq_num))
        .collect::<Result<Vec<_>, _>>()?;
    let master_page_id = page_id(master_seq)?;
    let customer_master_relations = customer_seq_nums
        .iter()
        .map(|seq_num| {
            Ok(CarltonMasterPresentationRelationV1 {
                source_page_seq_num: *seq_num,
                source_page_id: page_id(*seq_num)?,
                master_page_seq_num: master_seq,
                master_page_id: master_page_id.clone(),
            })
        })
        .collect::<Result<Vec<_>, CarltonPresentationError>>()?;

    Ok(CarltonPresentationManifestV1 {
        schema_version: CARLTON_PRESENTATION_MANIFEST_SCHEMA_V1.to_owned(),
        profile_id: profile.profile_id.to_owned(),
        source_sha256: input.source_sha256,
        raw_page_count: input.pages.len(),
        customer_page_count: customer_seq_nums.len(),
        customer_page_seq_nums: customer_seq_nums,
        customer_page_ids,
        master_page_seq_nums: vec![master_seq],
        carrier_page_seq_nums: carrier_set.into_iter().collect(),
        pages,
        customer_master_relations,
        invariants: CarltonPresentationInvariantsV1 {
            exact_source_admission: true,
            family_scoped_oid_rule: true,
            canonical_source_graph_mutated: false,
            canonical_page_ids_preserved: true,
            carrier_pages_exposed_as_customer_pages: false,
            master_pages_exposed_as_customer_pages: false,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn march_input() -> CarltonPresentationProfileInputV1 {
        let seqs = [263, 266, 361, 406, 269, 272, 275, 279];
        let customer = BTreeMap::from([(266, (2, 0)), (361, (2, 5)), (406, (2, 3))]);
        CarltonPresentationProfileInputV1 {
            schema_version: CARLTON_PRESENTATION_INPUT_SCHEMA_V1.to_owned(),
            source_sha256: MARCH_2026_SHA256.to_owned(),
            pages: seqs
                .into_iter()
                .enumerate()
                .map(
                    |(document_ordinal, contents_seq_num)| CarltonPageEvidenceV1 {
                        document_ordinal,
                        contents_seq_num,
                        oid_dword0: customer
                            .get(&contents_seq_num)
                            .map(|value| value.0)
                            .or(Some(0)),
                        oid_dword1: customer
                            .get(&contents_seq_num)
                            .map(|value| value.1)
                            .or(Some(0)),
                        applied_master_seq_num: (contents_seq_num != 263).then_some(263),
                        shape_child_count: match contents_seq_num {
                            263 => 2,
                            266 => 23,
                            361 => 34,
                            406 => 14,
                            279 => 9,
                            _ => 0,
                        },
                    },
                )
                .collect(),
            carrier_page_seq_nums: vec![279],
        }
    }

    fn standard_print_input(customer_seq_nums: &[u32]) -> StandardPrintServiceTailProfileInputV1 {
        let mut pages = vec![StandardPrintServiceTailPageEvidenceV1 {
            document_ordinal: 0,
            contents_seq_num: 263,
            oid_dword0: Some(0),
            oid_dword1: Some(0),
            applied_master_seq_num: None,
        }];
        pages.extend(
            customer_seq_nums
                .iter()
                .enumerate()
                .map(|(index, seq_num)| StandardPrintServiceTailPageEvidenceV1 {
                    document_ordinal: index + 1,
                    contents_seq_num: *seq_num,
                    oid_dword0: Some(if index == 0 { 1 } else { 2 }),
                    oid_dword1: Some(index as u32),
                    applied_master_seq_num: Some(263),
                }),
        );
        for (offset, seq_num) in [269_u32, 272, 275, 279].into_iter().enumerate() {
            pages.push(StandardPrintServiceTailPageEvidenceV1 {
                document_ordinal: customer_seq_nums.len() + 1 + offset,
                contents_seq_num: seq_num,
                oid_dword0: Some(0),
                oid_dword1: Some(0),
                applied_master_seq_num: Some(263),
            });
        }
        StandardPrintServiceTailProfileInputV1 {
            schema_version: STANDARD_PRINT_SERVICE_TAIL_INPUT_SCHEMA_V1.to_owned(),
            document_page_list_entry_count: pages.len(),
            confirmed_page_count: pages.len(),
            special_entry_count: 0,
            scenario_evidence_list_count: 0,
            observed_scenario_page_count: 0,
            pages,
        }
    }

    fn legacy22_input(
        dialect: Legacy22PageListDialectV1,
        physical_page_count: usize,
        raw_types: &[u16],
    ) -> Legacy22PageListProfileInputV1 {
        Legacy22PageListProfileInputV1 {
            schema_version: LEGACY22_PAGE_LIST_PROFILE_INPUT_SCHEMA_V1.to_owned(),
            dialect,
            document_page_list_entry_count: raw_types.len(),
            physical_page_count,
            entries: raw_types
                .iter()
                .copied()
                .enumerate()
                .map(
                    |(document_ordinal, raw_type)| Legacy22PageListEntryEvidenceV1 {
                        document_ordinal,
                        raw_type,
                    },
                )
                .collect(),
        }
    }

    #[test]
    fn legacy22_noquill_profile_selects_only_middle_pages() {
        let selection = select_legacy22_customer_page_indices_v1(legacy22_input(
            Legacy22PageListDialectV1::NoQuill,
            5,
            &[LEGACY22_PAGE_RAW_TYPE_V1; 4],
        ))
        .unwrap();
        assert_eq!(selection.profile_id, LEGACY22_NOQUILL_PAGE_PROFILE_ID_V1);
        assert_eq!(selection.materialized_page_count, 4);
        assert_eq!(selection.customer_page_indices, vec![2]);
    }

    #[test]
    fn legacy22_noquill_profile_fails_open_on_physical_page_drift() {
        assert!(
            select_legacy22_customer_page_indices_v1(legacy22_input(
                Legacy22PageListDialectV1::NoQuill,
                4,
                &[LEGACY22_PAGE_RAW_TYPE_V1; 4],
            ))
            .is_none()
        );
    }

    #[test]
    fn legacy22_quill_profile_accepts_current_special_tail() {
        let selection = select_legacy22_customer_page_indices_v1(legacy22_input(
            Legacy22PageListDialectV1::Quill,
            6,
            &[
                LEGACY22_PAGE_RAW_TYPE_V1,
                LEGACY22_PAGE_RAW_TYPE_V1,
                LEGACY22_PAGE_RAW_TYPE_V1,
                LEGACY22_PAGE_RAW_TYPE_V1,
                LEGACY22_PAGE_LIST_SPECIAL_RAW_TYPE_V1,
                LEGACY22_PAGE_RAW_TYPE_V1,
            ],
        ))
        .unwrap();
        assert_eq!(selection.profile_id, LEGACY22_QUILL_PAGE_PROFILE_ID_V1);
        assert_eq!(selection.materialized_page_count, 5);
        assert_eq!(selection.customer_page_indices, vec![2]);
    }

    #[test]
    fn legacy22_quill_profile_accepts_historical_all_page_tail() {
        let selection = select_legacy22_customer_page_indices_v1(legacy22_input(
            Legacy22PageListDialectV1::Quill,
            8,
            &[
                LEGACY22_PAGE_RAW_TYPE_V1,
                LEGACY22_PAGE_RAW_TYPE_V1,
                LEGACY22_PAGE_RAW_TYPE_V1,
                LEGACY22_PAGE_RAW_TYPE_V1,
                LEGACY22_PAGE_RAW_TYPE_V1,
                LEGACY22_PAGE_RAW_TYPE_V1,
                LEGACY22_PAGE_RAW_TYPE_V1,
            ],
        ))
        .unwrap();
        assert_eq!(selection.customer_page_indices, vec![2, 3]);
    }

    #[test]
    fn legacy22_quill_profile_rejects_unproven_tail_or_ordinal_drift() {
        let bad_tail = legacy22_input(
            Legacy22PageListDialectV1::Quill,
            6,
            &[
                LEGACY22_PAGE_RAW_TYPE_V1,
                LEGACY22_PAGE_RAW_TYPE_V1,
                LEGACY22_PAGE_RAW_TYPE_V1,
                LEGACY22_PAGE_LIST_SPECIAL_RAW_TYPE_V1,
                LEGACY22_PAGE_RAW_TYPE_V1,
                LEGACY22_PAGE_RAW_TYPE_V1,
            ],
        );
        assert!(select_legacy22_customer_page_indices_v1(bad_tail).is_none());

        let mut bad_ordinal = legacy22_input(
            Legacy22PageListDialectV1::NoQuill,
            5,
            &[LEGACY22_PAGE_RAW_TYPE_V1; 4],
        );
        bad_ordinal.entries[2].document_ordinal = 7;
        assert!(select_legacy22_customer_page_indices_v1(bad_ordinal).is_none());
    }

    fn mature_zero_leader_detached_input(
        customer_count: usize,
    ) -> StandardPrintServiceTailProfileInputV1 {
        let leader_seq = 263_u32;
        let mut pages = vec![StandardPrintServiceTailPageEvidenceV1 {
            document_ordinal: 0,
            contents_seq_num: leader_seq,
            oid_dword0: Some(0),
            oid_dword1: Some(0),
            applied_master_seq_num: None,
        }];
        for index in 0..customer_count {
            pages.push(StandardPrintServiceTailPageEvidenceV1 {
                document_ordinal: index + 1,
                contents_seq_num: 300 + index as u32,
                oid_dword0: Some(1),
                oid_dword1: Some(index as u32),
                applied_master_seq_num: Some(leader_seq),
            });
        }
        let service_ordinal = customer_count + 1;
        pages.push(StandardPrintServiceTailPageEvidenceV1 {
            document_ordinal: service_ordinal,
            contents_seq_num: 400,
            oid_dword0: Some(0),
            oid_dword1: Some(0),
            applied_master_seq_num: Some(leader_seq),
        });
        pages.push(StandardPrintServiceTailPageEvidenceV1 {
            document_ordinal: service_ordinal + 2,
            contents_seq_num: 401,
            oid_dword0: Some(0),
            oid_dword1: Some(0),
            applied_master_seq_num: None,
        });
        pages.push(StandardPrintServiceTailPageEvidenceV1 {
            document_ordinal: service_ordinal + 3,
            contents_seq_num: 402,
            oid_dword0: Some(0),
            oid_dword1: Some(0),
            applied_master_seq_num: None,
        });

        StandardPrintServiceTailProfileInputV1 {
            schema_version: STANDARD_PRINT_SERVICE_TAIL_INPUT_SCHEMA_V1.to_owned(),
            document_page_list_entry_count: pages.len() + 1,
            confirmed_page_count: pages.len(),
            special_entry_count: 1,
            scenario_evidence_list_count: 0,
            observed_scenario_page_count: 0,
            pages,
        }
    }

    fn mature_terminal_service_input(
        customer_count: usize,
        special: bool,
        scenario_count: usize,
    ) -> StandardPrintServiceTailProfileInputV1 {
        let leader_seq = 500_u32;
        let mut pages = vec![StandardPrintServiceTailPageEvidenceV1 {
            document_ordinal: 0,
            contents_seq_num: leader_seq,
            oid_dword0: Some(2),
            oid_dword1: Some(9),
            applied_master_seq_num: None,
        }];
        for index in 0..customer_count {
            pages.push(StandardPrintServiceTailPageEvidenceV1 {
                document_ordinal: index + 1,
                contents_seq_num: 600 + index as u32,
                oid_dword0: Some(if index < 2 { 1 } else { 2 }),
                oid_dword1: Some(index as u32),
                applied_master_seq_num: Some(leader_seq),
            });
        }
        let service_ordinal = customer_count + 1;
        pages.push(StandardPrintServiceTailPageEvidenceV1 {
            document_ordinal: service_ordinal,
            contents_seq_num: 700,
            oid_dword0: Some(2),
            oid_dword1: Some(1),
            applied_master_seq_num: Some(leader_seq),
        });
        if special {
            pages.push(StandardPrintServiceTailPageEvidenceV1 {
                document_ordinal: service_ordinal + 2,
                contents_seq_num: 701,
                oid_dword0: Some(0),
                oid_dword1: Some(0),
                applied_master_seq_num: Some(leader_seq),
            });
            pages.push(StandardPrintServiceTailPageEvidenceV1 {
                document_ordinal: service_ordinal + 3,
                contents_seq_num: 702,
                oid_dword0: Some(0),
                oid_dword1: Some(0),
                applied_master_seq_num: Some(leader_seq),
            });
        } else {
            for offset in 1..=3 {
                pages.push(StandardPrintServiceTailPageEvidenceV1 {
                    document_ordinal: service_ordinal + offset,
                    contents_seq_num: 700 + offset as u32,
                    oid_dword0: Some(0),
                    oid_dword1: Some(0),
                    applied_master_seq_num: Some(leader_seq),
                });
            }
        }

        StandardPrintServiceTailProfileInputV1 {
            schema_version: STANDARD_PRINT_SERVICE_TAIL_INPUT_SCHEMA_V1.to_owned(),
            document_page_list_entry_count: pages.len() + if special { 1 } else { 0 },
            confirmed_page_count: pages.len(),
            special_entry_count: if special { 1 } else { 0 },
            scenario_evidence_list_count: scenario_count,
            observed_scenario_page_count: scenario_count,
            pages,
        }
    }

    #[test]
    fn mature_zero_leader_detached_profile_selects_customer_block() {
        let input = mature_zero_leader_detached_input(1);
        let selection =
            select_mature_zero_leader_detached_tail_customer_page_seq_nums_v1(input).unwrap();
        assert_eq!(
            selection.profile_id,
            MATURE_ZERO_LEADER_DETACHED_TAIL_PROFILE_ID_V1
        );
        assert_eq!(selection.customer_page_seq_nums, vec![300]);
        assert_eq!(selection.service_page_seq_nums, vec![400, 401, 402]);
    }

    #[test]
    fn mature_zero_leader_detached_profile_fails_open_when_tail_is_applied() {
        let mut input = mature_zero_leader_detached_input(1);
        let len = input.pages.len();
        input.pages[len - 1].applied_master_seq_num = Some(263);
        assert!(select_mature_zero_leader_detached_tail_customer_page_seq_nums_v1(input).is_none());
    }

    #[test]
    fn mature_detached_post_special_profile_selects_customer_block() {
        let mut input = mature_terminal_service_input(2, true, 0);
        let len = input.pages.len();
        input.pages[len - 2].applied_master_seq_num = None;
        input.pages[len - 1].applied_master_seq_num = None;

        let selection =
            select_mature_detached_post_special_tail_customer_page_seq_nums_v1(input).unwrap();
        assert_eq!(
            selection.profile_id,
            MATURE_DETACHED_POST_SPECIAL_TAIL_PROFILE_ID_V1
        );
        assert_eq!(selection.customer_page_seq_nums, vec![600, 601]);
        assert_eq!(selection.service_page_seq_nums, vec![700, 701, 702]);
    }

    #[test]
    fn mature_detached_post_special_profile_fails_open_when_tail_is_still_applied() {
        let mut input = mature_terminal_service_input(2, true, 0);
        let len = input.pages.len();
        input.pages[len - 1].applied_master_seq_num = None;
        assert!(
            select_mature_detached_post_special_tail_customer_page_seq_nums_v1(input).is_none()
        );
    }

    #[test]
    fn mature_terminal_service_profile_accepts_all_page_suffix() {
        let selection = select_mature_terminal_service_tail_customer_page_seq_nums_v1(
            mature_terminal_service_input(2, false, 0),
        )
        .unwrap();
        assert_eq!(
            selection.profile_id,
            MATURE_TERMINAL_SERVICE_TAIL_PROFILE_ID_V1
        );
        assert_eq!(selection.customer_page_seq_nums, vec![600, 601]);
        assert_eq!(selection.service_page_seq_nums, vec![700, 701, 702, 703]);
    }

    #[test]
    fn mature_terminal_service_profile_accepts_special_suffix_and_scenario_evidence() {
        let selection = select_mature_terminal_service_tail_customer_page_seq_nums_v1(
            mature_terminal_service_input(3, true, 13),
        )
        .unwrap();
        assert_eq!(selection.customer_page_seq_nums, vec![600, 601, 602]);
        assert_eq!(selection.service_page_seq_nums, vec![700, 701, 702]);
    }

    #[test]
    fn mature_terminal_service_profile_fails_open_on_relation_or_suffix_drift() {
        let mut bad_relation = mature_terminal_service_input(2, false, 0);
        bad_relation.pages[2].applied_master_seq_num = Some(999);
        assert!(
            select_mature_terminal_service_tail_customer_page_seq_nums_v1(bad_relation).is_none()
        );

        let mut bad_suffix = mature_terminal_service_input(2, true, 0);
        bad_suffix.pages.last_mut().unwrap().document_ordinal += 1;
        assert!(
            select_mature_terminal_service_tail_customer_page_seq_nums_v1(bad_suffix).is_none()
        );
    }

    #[test]
    fn standard_print_profile_selects_virginia_style_customer_middle() {
        let input = standard_print_input(&[266, 301, 312, 323, 336, 339]);
        let selection =
            select_standard_print_service_tail_customer_page_seq_nums_v1(input).unwrap();
        assert_eq!(
            selection.customer_page_seq_nums,
            vec![266, 301, 312, 323, 336, 339]
        );
        assert_eq!(selection.master_page_seq_num, 263);
        assert_eq!(selection.service_page_seq_nums, vec![269, 272, 275, 279]);
    }

    #[test]
    fn standard_print_profile_accepts_kroy_style_five_page_control() {
        let selection =
            select_standard_print_service_tail_customer_page_seq_nums_v1(standard_print_input(&[
                341, 266, 297, 301, 311,
            ]))
            .unwrap();
        assert_eq!(selection.customer_page_seq_nums.len(), 5);
        assert_eq!(selection.raw_page_count, 10);
    }

    #[test]
    fn standard_print_profile_does_not_require_customer_page_content() {
        let selection =
            select_standard_print_service_tail_customer_page_seq_nums_v1(standard_print_input(&[
                266,
            ]))
            .unwrap();
        assert_eq!(selection.customer_page_seq_nums, vec![266]);
    }

    #[test]
    fn industrial_style_nonzero_master_and_three_service_tail_is_not_admitted() {
        let pages = vec![
            StandardPrintServiceTailPageEvidenceV1 {
                document_ordinal: 0,
                contents_seq_num: 263,
                oid_dword0: Some(2),
                oid_dword1: Some(2),
                applied_master_seq_num: None,
            },
            StandardPrintServiceTailPageEvidenceV1 {
                document_ordinal: 1,
                contents_seq_num: 266,
                oid_dword0: Some(1),
                oid_dword1: Some(0),
                applied_master_seq_num: Some(263),
            },
            StandardPrintServiceTailPageEvidenceV1 {
                document_ordinal: 2,
                contents_seq_num: 343,
                oid_dword0: Some(1),
                oid_dword1: Some(1),
                applied_master_seq_num: Some(263),
            },
            StandardPrintServiceTailPageEvidenceV1 {
                document_ordinal: 3,
                contents_seq_num: 269,
                oid_dword0: Some(2),
                oid_dword1: Some(1),
                applied_master_seq_num: Some(263),
            },
            StandardPrintServiceTailPageEvidenceV1 {
                document_ordinal: 4,
                contents_seq_num: 272,
                oid_dword0: Some(0),
                oid_dword1: Some(0),
                applied_master_seq_num: Some(263),
            },
            StandardPrintServiceTailPageEvidenceV1 {
                document_ordinal: 5,
                contents_seq_num: 275,
                oid_dword0: Some(0),
                oid_dword1: Some(0),
                applied_master_seq_num: Some(263),
            },
            StandardPrintServiceTailPageEvidenceV1 {
                document_ordinal: 6,
                contents_seq_num: 279,
                oid_dword0: Some(0),
                oid_dword1: Some(0),
                applied_master_seq_num: Some(263),
            },
        ];
        let input = StandardPrintServiceTailProfileInputV1 {
            schema_version: STANDARD_PRINT_SERVICE_TAIL_INPUT_SCHEMA_V1.to_owned(),
            document_page_list_entry_count: pages.len(),
            confirmed_page_count: pages.len(),
            special_entry_count: 0,
            scenario_evidence_list_count: 0,
            observed_scenario_page_count: 0,
            pages,
        };
        assert!(select_standard_print_service_tail_customer_page_seq_nums_v1(input).is_none());
    }

    #[test]
    fn standard_print_profile_accepts_one_page_special_interleaved_service_tail() {
        let input = StandardPrintServiceTailProfileInputV1 {
            schema_version: STANDARD_PRINT_SERVICE_TAIL_INPUT_SCHEMA_V1.to_owned(),
            document_page_list_entry_count: 6,
            confirmed_page_count: 5,
            special_entry_count: 1,
            scenario_evidence_list_count: 0,
            observed_scenario_page_count: 0,
            pages: vec![
                StandardPrintServiceTailPageEvidenceV1 {
                    document_ordinal: 0,
                    contents_seq_num: 263,
                    oid_dword0: Some(0),
                    oid_dword1: Some(0),
                    applied_master_seq_num: None,
                },
                StandardPrintServiceTailPageEvidenceV1 {
                    document_ordinal: 1,
                    contents_seq_num: 301,
                    oid_dword0: Some(2),
                    oid_dword1: Some(2),
                    applied_master_seq_num: Some(263),
                },
                StandardPrintServiceTailPageEvidenceV1 {
                    document_ordinal: 2,
                    contents_seq_num: 269,
                    oid_dword0: Some(0),
                    oid_dword1: Some(0),
                    applied_master_seq_num: Some(263),
                },
                StandardPrintServiceTailPageEvidenceV1 {
                    document_ordinal: 4,
                    contents_seq_num: 273,
                    oid_dword0: Some(0),
                    oid_dword1: Some(0),
                    applied_master_seq_num: Some(263),
                },
                StandardPrintServiceTailPageEvidenceV1 {
                    document_ordinal: 5,
                    contents_seq_num: 277,
                    oid_dword0: Some(0),
                    oid_dword1: Some(0),
                    applied_master_seq_num: Some(263),
                },
            ],
        };

        let selection =
            select_standard_print_service_tail_customer_page_seq_nums_v1(input).unwrap();
        assert_eq!(
            selection.profile_id,
            STANDARD_PRINT_SERVICE_TAIL_SPECIAL_PROFILE_ID_V1
        );
        assert_eq!(selection.raw_page_count, 5);
        assert_eq!(selection.customer_page_seq_nums, vec![301]);
        assert_eq!(selection.master_page_seq_num, 263);
        assert_eq!(selection.service_page_seq_nums, vec![269, 273, 277]);
    }

    #[test]
    fn standard_print_profile_accepts_three_page_special_interleaved_service_tail() {
        let input = StandardPrintServiceTailProfileInputV1 {
            schema_version: STANDARD_PRINT_SERVICE_TAIL_INPUT_SCHEMA_V1.to_owned(),
            document_page_list_entry_count: 8,
            confirmed_page_count: 7,
            special_entry_count: 1,
            scenario_evidence_list_count: 0,
            observed_scenario_page_count: 0,
            pages: vec![
                StandardPrintServiceTailPageEvidenceV1 {
                    document_ordinal: 0,
                    contents_seq_num: 263,
                    oid_dword0: Some(0),
                    oid_dword1: Some(0),
                    applied_master_seq_num: None,
                },
                StandardPrintServiceTailPageEvidenceV1 {
                    document_ordinal: 1,
                    contents_seq_num: 301,
                    oid_dword0: Some(2),
                    oid_dword1: Some(0),
                    applied_master_seq_num: Some(263),
                },
                StandardPrintServiceTailPageEvidenceV1 {
                    document_ordinal: 2,
                    contents_seq_num: 302,
                    oid_dword0: Some(2),
                    oid_dword1: Some(1),
                    applied_master_seq_num: Some(263),
                },
                StandardPrintServiceTailPageEvidenceV1 {
                    document_ordinal: 3,
                    contents_seq_num: 303,
                    oid_dword0: Some(2),
                    oid_dword1: Some(2),
                    applied_master_seq_num: Some(263),
                },
                StandardPrintServiceTailPageEvidenceV1 {
                    document_ordinal: 4,
                    contents_seq_num: 269,
                    oid_dword0: Some(0),
                    oid_dword1: Some(0),
                    applied_master_seq_num: Some(263),
                },
                StandardPrintServiceTailPageEvidenceV1 {
                    document_ordinal: 6,
                    contents_seq_num: 273,
                    oid_dword0: Some(0),
                    oid_dword1: Some(0),
                    applied_master_seq_num: Some(263),
                },
                StandardPrintServiceTailPageEvidenceV1 {
                    document_ordinal: 7,
                    contents_seq_num: 277,
                    oid_dword0: Some(0),
                    oid_dword1: Some(0),
                    applied_master_seq_num: Some(263),
                },
            ],
        };

        let selection =
            select_standard_print_service_tail_customer_page_seq_nums_v1(input).unwrap();
        assert_eq!(
            selection.profile_id,
            STANDARD_PRINT_SERVICE_TAIL_SPECIAL_PROFILE_ID_V1
        );
        assert_eq!(selection.customer_page_seq_nums, vec![301, 302, 303]);
        assert_eq!(selection.service_page_seq_nums, vec![269, 273, 277]);
    }

    #[test]
    fn standard_print_special_profile_rejects_special_before_customer_page() {
        let mut input = StandardPrintServiceTailProfileInputV1 {
            schema_version: STANDARD_PRINT_SERVICE_TAIL_INPUT_SCHEMA_V1.to_owned(),
            document_page_list_entry_count: 6,
            confirmed_page_count: 5,
            special_entry_count: 1,
            scenario_evidence_list_count: 0,
            observed_scenario_page_count: 0,
            pages: vec![
                StandardPrintServiceTailPageEvidenceV1 {
                    document_ordinal: 0,
                    contents_seq_num: 263,
                    oid_dword0: Some(0),
                    oid_dword1: Some(0),
                    applied_master_seq_num: None,
                },
                StandardPrintServiceTailPageEvidenceV1 {
                    document_ordinal: 2,
                    contents_seq_num: 301,
                    oid_dword0: Some(2),
                    oid_dword1: Some(2),
                    applied_master_seq_num: Some(263),
                },
                StandardPrintServiceTailPageEvidenceV1 {
                    document_ordinal: 3,
                    contents_seq_num: 269,
                    oid_dword0: Some(0),
                    oid_dword1: Some(0),
                    applied_master_seq_num: Some(263),
                },
                StandardPrintServiceTailPageEvidenceV1 {
                    document_ordinal: 4,
                    contents_seq_num: 273,
                    oid_dword0: Some(0),
                    oid_dword1: Some(0),
                    applied_master_seq_num: Some(263),
                },
                StandardPrintServiceTailPageEvidenceV1 {
                    document_ordinal: 5,
                    contents_seq_num: 277,
                    oid_dword0: Some(0),
                    oid_dword1: Some(0),
                    applied_master_seq_num: Some(263),
                },
            ],
        };
        assert!(
            select_standard_print_service_tail_customer_page_seq_nums_v1(input.clone()).is_none()
        );

        input.pages[1].document_ordinal = 1;
        input.pages[2].document_ordinal = 2;
        input.pages[3].document_ordinal = 3;
        input.pages[4].document_ordinal = 5;
        assert!(select_standard_print_service_tail_customer_page_seq_nums_v1(input).is_none());
    }

    #[test]
    fn standard_print_profile_fails_open_when_scenario_evidence_exists() {
        let mut input = standard_print_input(&[266]);
        input.scenario_evidence_list_count = 1;
        input.observed_scenario_page_count = 1;
        assert!(select_standard_print_service_tail_customer_page_seq_nums_v1(input).is_none());
    }

    #[test]
    fn standard_print_profile_fails_open_on_unresolved_nonempty_scenario_evidence() {
        let mut input = standard_print_input(&[266]);
        input.scenario_evidence_list_count = 1;
        input.observed_scenario_page_count = 0;
        assert!(select_standard_print_service_tail_customer_page_seq_nums_v1(input).is_none());
    }

    #[test]
    fn standard_print_profile_fails_open_on_non_page_list_entry() {
        let mut input = standard_print_input(&[266]);
        input.document_page_list_entry_count += 1;
        input.special_entry_count = 1;
        assert!(select_standard_print_service_tail_customer_page_seq_nums_v1(input).is_none());
    }

    #[test]
    fn newsletter_reference_profile_selects_four_publisher_pages() {
        let selection = select_reference_fixture_customer_page_seq_nums_v1(
            SAMPLE_NEWSLETTER_SHA256,
            &[263, 266, 323, 352, 381, 269, 273, 277],
        )
        .unwrap()
        .unwrap();
        assert_eq!(selection.customer_page_seq_nums, vec![266, 323, 352, 381]);
        assert_eq!(selection.raw_page_count, 8);
    }

    #[test]
    fn brochure_reference_profile_selects_two_publisher_pages() {
        let selection = select_reference_fixture_customer_page_seq_nums_v1(
            SAMPLE_BROCHURE_SHA256,
            &[263, 266, 334, 269, 273, 277],
        )
        .unwrap()
        .unwrap();
        assert_eq!(selection.customer_page_seq_nums, vec![266, 334]);
        assert_eq!(selection.raw_page_count, 6);
    }

    #[test]
    fn reference_profile_drift_fails_closed_and_unknown_hash_is_ignored() {
        assert!(
            select_reference_fixture_customer_page_seq_nums_v1(&"11".repeat(32), &[263, 266],)
                .unwrap()
                .is_none()
        );

        assert!(matches!(
            select_reference_fixture_customer_page_seq_nums_v1(
                SAMPLE_BROCHURE_SHA256,
                &[263, 266, 334, 269, 277],
            ),
            Err(ReferenceFixturePresentationError::RawPageSequenceMismatch { .. })
        ));
    }

    #[test]
    fn march_profile_classifies_three_customer_pages_without_mutating_source_truth() {
        let manifest = build_carlton_presentation_manifest_v1(march_input()).unwrap();
        assert_eq!(manifest.customer_page_seq_nums, vec![266, 361, 406]);
        assert_eq!(manifest.master_page_seq_nums, vec![263]);
        assert_eq!(manifest.carrier_page_seq_nums, vec![279]);
        assert_eq!(
            manifest
                .pages
                .iter()
                .map(|page| (page.contents_seq_num, page.role))
                .collect::<Vec<_>>(),
            vec![
                (263, CarltonPageRoleV1::Master),
                (266, CarltonPageRoleV1::Customer),
                (361, CarltonPageRoleV1::Customer),
                (406, CarltonPageRoleV1::Customer),
                (269, CarltonPageRoleV1::InternalService),
                (272, CarltonPageRoleV1::InternalService),
                (275, CarltonPageRoleV1::InternalService),
                (279, CarltonPageRoleV1::Carrier),
            ]
        );
        assert!(!manifest.invariants.canonical_source_graph_mutated);
        assert!(manifest.invariants.canonical_page_ids_preserved);
    }

    #[test]
    fn applied_master_presence_is_not_a_customer_classifier() {
        let manifest = build_carlton_presentation_manifest_v1(march_input()).unwrap();
        for seq_num in [269, 272, 275, 279] {
            let page = manifest
                .pages
                .iter()
                .find(|page| page.contents_seq_num == seq_num)
                .unwrap();
            assert_eq!(page.applied_master_seq_num, Some(263));
            assert_ne!(page.role, CarltonPageRoleV1::Customer);
        }
    }

    #[test]
    fn unknown_source_hash_fails_closed_before_oid_rule_is_applied() {
        let mut input = march_input();
        input.source_sha256 = "11".repeat(32);
        assert_eq!(
            build_carlton_presentation_manifest_v1(input),
            Err(CarltonPresentationError::UnsupportedSourceHash)
        );
    }

    #[test]
    fn family_drift_in_customer_count_fails_closed() {
        let mut input = march_input();
        input
            .pages
            .iter_mut()
            .find(|page| page.contents_seq_num == 269)
            .unwrap()
            .oid_dword0 = Some(2);
        assert_eq!(
            build_carlton_presentation_manifest_v1(input),
            Err(CarltonPresentationError::CustomerCountMismatch {
                expected: 3,
                observed: 4,
            })
        );
    }

    #[test]
    fn source_neutral_selection_preserves_exact_family_customer_order() {
        let selection = select_carlton_customer_page_seq_nums_v1(march_input()).unwrap();
        assert_eq!(selection.profile_id, "carlton-school-jotter/march-2026/v1");
        assert_eq!(selection.raw_page_count, 8);
        assert_eq!(selection.customer_page_seq_nums, vec![266, 361, 406]);
        assert_eq!(selection.master_page_seq_nums, vec![263]);
        assert_eq!(selection.carrier_page_seq_nums, vec![279]);
    }

    #[test]
    fn exact_profile_rejects_carrier_evidence_drift() {
        let mut input = march_input();
        input.carrier_page_seq_nums.clear();
        assert_eq!(
            select_carlton_customer_page_seq_nums_v1(input),
            Err(CarltonPresentationError::CarrierEvidenceMismatch {
                expected: vec![279],
                observed: vec![],
            })
        );
    }

    #[test]
    fn carrier_evidence_cannot_overlap_customer_or_master_pages() {
        let mut input = march_input();
        input.carrier_page_seq_nums = vec![266];
        assert_eq!(
            build_carlton_presentation_manifest_v1(input),
            Err(CarltonPresentationError::CarrierCustomerOverlap(266))
        );

        let mut input = march_input();
        input.carrier_page_seq_nums = vec![263];
        assert_eq!(
            build_carlton_presentation_manifest_v1(input),
            Err(CarltonPresentationError::CarrierMasterOverlap(263))
        );
    }
}
