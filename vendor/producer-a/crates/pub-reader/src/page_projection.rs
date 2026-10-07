//! Mature Publisher page-role evidence and effective page projection.
//!
//! This module owns PAGE-role observation plus source-neutral effective-page
//! projection. It deliberately does not own graph construction, typography,
//! OfficeArt paint, StoryFrame correlation, or Viewer rendering.

use super::*;

pub const PUB_PAGE_ROLE_OBSERVATION_SCHEMA_V1: &str = "chaptera.pub-page-role-observation.v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PubPageRoleObservationReceipt {
    pub schema: String,
    pub document_page_list_entry_count: usize,
    pub confirmed_page_count: usize,
    pub special_entry_count: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub document_entries: Vec<PubDocumentPageListEntryObservation>,
    pub pages: Vec<PubPageRoleObservation>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub controlling: Vec<PubControllingObservation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PubDocumentPageListEntryObservation {
    pub document_ordinal: usize,
    pub contents_seq_num: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_type: Option<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PubPageRoleObservation {
    pub document_ordinal: usize,
    pub contents_seq_num: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oid_dword0: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oid_dword1: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub applied_master_seq_num: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub applied_master_field_id: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub applied_master_block_type: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub applied_master_raw_type: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pgt_type: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_document_entry_seq_num: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_document_entry_raw_type: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_document_entry_seq_num: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_document_entry_raw_type: Option<u16>,
    pub child_raw_type_counts: BTreeMap<u16, usize>,
    pub shape_child_count: usize,
    pub group_child_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PubControllingObservation {
    pub contents_seq_num: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_seq_num: Option<u32>,
    pub fully_decoded: bool,
    pub fields: Vec<PubControllingFieldObservation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PubControllingFieldObservation {
    pub id: u16,
    pub block_type: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub declared_length: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_container_hex: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pgids: Vec<[u32; 2]>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PubEffectivePageProjectionAuthority {
    RawDocumentPageList,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PubEffectivePageProjection {
    pub authority: PubEffectivePageProjectionAuthority,
    pub page_ids: Vec<PageId>,
    pub raw_page_count: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub observed_scenario_page_ids: Vec<PageId>,
    pub scenario_evidence_list_count: usize,
}

/// Emits source-free PAGE-role evidence without classifying customer-visible pages.
///
/// Oid, applied-master state, PgtType and child inventory remain independent
/// axes here. No single axis is promoted to a universal visible-page rule.
pub fn analyze_mature_0x2c_page_roles<R: Read + Seek>(
    mut reader: R,
) -> Result<PubPageRoleObservationReceipt> {
    reader.seek(SeekFrom::Start(0))?;
    let mut pub_bytes = Vec::new();
    reader.read_to_end(&mut pub_bytes)?;

    let contents =
        pub_cfb::read_stream_reader(Cursor::new(pub_bytes.as_slice()), CONTENTS_STREAM_PATH)
            .with_context(|| format!("read {CONTENTS_STREAM_PATH}"))?;
    let contents_stream = StreamPath(CONTENTS_STREAM_PATH.into());
    let header = parse_0x2c_header(contents_stream.clone(), &contents)
        .context("parse mature-0x2C Contents header for PAGE-role observation")?;
    let trailer = parse_confirmed_0x2c_trailer_root(&contents, &header)
        .context("parse mature-0x2C Contents trailer for PAGE-role observation")?;
    let references = build_reference_index(&contents, &trailer.directory)?;

    let document_reference =
        unique_reference_by_raw_type(&references, RAW_TYPE_DOCUMENT, "DOCUMENT")?;
    let document_chunk =
        chunk_for_reference(contents_stream.clone(), &contents, document_reference)?;
    let page_list_block = unique_block(&document_chunk, DOCUMENT_PAGE_LIST_ID)?.clone();
    let page_list = parse_confirmed_document_page_list(&contents, page_list_block)
        .context("parse DOCUMENT PageList for PAGE-role observation")?;

    let document_entries = page_list
        .entries
        .iter()
        .enumerate()
        .map(
            |(document_ordinal, entry)| PubDocumentPageListEntryObservation {
                document_ordinal,
                contents_seq_num: entry.handle,
                raw_type: references.get(&entry.handle).and_then(single_raw_type),
            },
        )
        .collect::<Vec<_>>();

    let mut pages = Vec::new();
    let mut special_entry_count = 0_usize;

    for (document_ordinal, entry) in page_list.entries.iter().enumerate() {
        let Some(reference) = references.get(&entry.handle) else {
            continue;
        };
        match single_raw_type(reference) {
            Some(RAW_TYPE_PAGE_LIST_SPECIAL) => {
                special_entry_count += 1;
                continue;
            }
            Some(RAW_TYPE_PAGE) => {}
            _ => continue,
        }

        let chunk = chunk_for_reference(contents_stream.clone(), &contents, reference)?;
        let mut oid = None;
        let mut applied_master_seq_num = None;
        let mut applied_master_field_id = None;
        let mut applied_master_block_type = None;
        let mut pgt_type = None;

        for field in &chunk.fields {
            match (field.id, field.block_type) {
                (0x06, BLOCK_TYPE_FIXED_8) => {
                    if oid.is_some() {
                        bail!("PAGE {} repeats OplPd.Oid field0x06", entry.handle);
                    }
                    let parsed = parse_confirmed_oid_identity_payload(field.clone())
                        .context("parse exact OplPd.Oid field0x06")?;
                    oid = Some((parsed.dword0, parsed.dword1));
                }
                (0x0d, BLOCK_TYPE_REFERENCE_U32) => {
                    if applied_master_seq_num.is_some() {
                        bail!("PAGE {} repeats OplPd.OhpdMaster field0x0D", entry.handle);
                    }
                    let RawContentsBlockBody::U32 { value, .. } = &field.body else {
                        bail!("PAGE {} field0x0D has inconsistent body", entry.handle);
                    };
                    applied_master_seq_num = Some(*value);
                    applied_master_field_id = Some(field.id);
                    applied_master_block_type = Some(field.block_type);
                }
                (0x0d, actual_wire) => {
                    bail!(
                        "PAGE {} OplPd.OhpdMaster field0x0D uses wire 0x{actual_wire:02X}, expected 0x{BLOCK_TYPE_REFERENCE_U32:02X}",
                        entry.handle
                    );
                }
                (0x10, BLOCK_TYPE_U32) => {
                    if pgt_type.is_some() {
                        bail!("PAGE {} repeats OplPd.PgtType field0x10", entry.handle);
                    }
                    let RawContentsBlockBody::U32 { value, .. } = &field.body else {
                        bail!("PAGE {} field0x10 has inconsistent body", entry.handle);
                    };
                    pgt_type = Some(*value);
                }
                _ => {}
            }
        }

        let mut child_raw_type_counts = BTreeMap::<u16, usize>::new();
        for child in references.values() {
            if single_parent_seq(child) != Some(entry.handle) {
                continue;
            }
            if let Some(raw_type) = single_raw_type(child) {
                *child_raw_type_counts.entry(raw_type).or_insert(0) += 1;
            }
        }

        let applied_master_raw_type = applied_master_seq_num
            .and_then(|seq_num| references.get(&seq_num))
            .and_then(single_raw_type);
        let previous_document_entry = document_ordinal
            .checked_sub(1)
            .and_then(|ordinal| document_entries.get(ordinal));
        let next_document_entry = document_entries.get(document_ordinal + 1);

        pages.push(PubPageRoleObservation {
            document_ordinal,
            contents_seq_num: entry.handle,
            oid_dword0: oid.map(|value| value.0),
            oid_dword1: oid.map(|value| value.1),
            applied_master_seq_num,
            applied_master_field_id,
            applied_master_block_type,
            applied_master_raw_type,
            pgt_type,
            previous_document_entry_seq_num: previous_document_entry
                .map(|item| item.contents_seq_num),
            previous_document_entry_raw_type: previous_document_entry
                .and_then(|item| item.raw_type),
            next_document_entry_seq_num: next_document_entry.map(|item| item.contents_seq_num),
            next_document_entry_raw_type: next_document_entry.and_then(|item| item.raw_type),
            shape_child_count: child_raw_type_counts
                .get(&RAW_TYPE_SHAPE)
                .copied()
                .unwrap_or(0),
            group_child_count: child_raw_type_counts
                .get(&RAW_TYPE_GROUP)
                .copied()
                .unwrap_or(0),
            child_raw_type_counts,
        });
    }

    let mut controlling = references
        .values()
        .filter(|reference| single_raw_type(reference) == Some(RAW_TYPE_CONTROLLING))
        .map(|reference| {
            let chunk = chunk_for_reference(contents_stream.clone(), &contents, reference)?;
            let fields = chunk
                .fields
                .iter()
                .map(|field| {
                    let (declared_length, observed_container_hex) = match &field.body {
                        RawContentsBlockBody::Container {
                            declared_length,
                            content_source,
                            ..
                        } => {
                            let observed = (field.id == 0x06)
                                .then(|| raw_span_hex(&contents, content_source))
                                .transpose()?;
                            (Some(*declared_length), observed)
                        }
                        _ => (None, None),
                    };
                    let pgids = if field.id == 0x06 {
                        parse_confirmed_controlling_page_list(&contents, field.clone())
                            .context("parse OplControlling PageList/Pgid observation")?
                            .entries
                            .into_iter()
                            .map(|entry| [entry.pgid.dword0, entry.pgid.dword1])
                            .collect()
                    } else {
                        Vec::new()
                    };
                    Ok(PubControllingFieldObservation {
                        id: field.id,
                        block_type: field.block_type,
                        declared_length,
                        observed_container_hex,
                        pgids,
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            Ok(PubControllingObservation {
                contents_seq_num: seq_u32(reference.seq_num)?,
                parent_seq_num: single_parent_seq(reference),
                fully_decoded: chunk.is_fully_decoded(),
                fields,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    controlling.sort_by_key(|item| item.contents_seq_num);

    Ok(PubPageRoleObservationReceipt {
        schema: PUB_PAGE_ROLE_OBSERVATION_SCHEMA_V1.to_owned(),
        document_page_list_entry_count: page_list.entries.len(),
        confirmed_page_count: pages.len(),
        special_entry_count,
        document_entries,
        pages,
        controlling,
    })
}

pub(super) fn derive_effective_page_projection(
    contents_stream: StreamPath,
    contents: &[u8],
    references: &BTreeMap<u32, Contents0x2cChunkReference>,
    page_seq_to_id: &BTreeMap<u32, PageId>,
    raw_page_ids: &[PageId],
) -> (PubEffectivePageProjection, Vec<PubBridgeDiagnostic>) {
    let mut diagnostics = vec![PubBridgeDiagnostic::PageRoleClassificationUnresolved {
        raw_page_count: raw_page_ids.len(),
    }];

    let mut pgid_lists = Vec::<Vec<(u32, u32)>>::new();
    let mut observed_page_list = false;
    for reference in references
        .values()
        .filter(|reference| single_raw_type(reference) == Some(RAW_TYPE_CONTROLLING))
    {
        let chunk = match chunk_for_reference(contents_stream.clone(), contents, reference) {
            Ok(chunk) => chunk,
            Err(error) => {
                diagnostics.push(PubBridgeDiagnostic::ScenarioPageOrderUnavailable {
                    reason: format!("controlling_chunk_unavailable:{error}"),
                    raw_page_count: raw_page_ids.len(),
                });
                return (
                    build_effective_page_projection(raw_page_ids, Vec::new(), 0),
                    diagnostics,
                );
            }
        };
        let page_list_fields = chunk
            .fields
            .iter()
            .filter(|field| field.id == pub_contents::CONTROLLING_PAGE_LIST_ID)
            .cloned()
            .collect::<Vec<_>>();
        if page_list_fields.len() > 1 {
            diagnostics.push(PubBridgeDiagnostic::ScenarioPageOrderUnavailable {
                reason: format!("duplicate_controlling_page_list:seq={}", reference.seq_num),
                raw_page_count: raw_page_ids.len(),
            });
            return (
                build_effective_page_projection(raw_page_ids, Vec::new(), 0),
                diagnostics,
            );
        }
        let Some(field) = page_list_fields.into_iter().next() else {
            continue;
        };
        observed_page_list = true;
        let parsed = match parse_confirmed_controlling_page_list(contents, field) {
            Ok(parsed) => parsed,
            Err(error) => {
                diagnostics.push(PubBridgeDiagnostic::ScenarioPageOrderUnavailable {
                    reason: format!(
                        "controlling_page_list_invalid:seq={}:{}",
                        reference.seq_num, error
                    ),
                    raw_page_count: raw_page_ids.len(),
                });
                return (
                    build_effective_page_projection(raw_page_ids, Vec::new(), 0),
                    diagnostics,
                );
            }
        };
        let pgids = parsed
            .entries
            .into_iter()
            .map(|entry| (entry.pgid.dword0, entry.pgid.dword1))
            .collect::<Vec<_>>();
        if pgids.is_empty() {
            diagnostics.push(PubBridgeDiagnostic::ScenarioPageOrderUnavailable {
                reason: format!("controlling_page_list_empty:seq={}", reference.seq_num),
                raw_page_count: raw_page_ids.len(),
            });
            return (
                build_effective_page_projection(raw_page_ids, Vec::new(), 0),
                diagnostics,
            );
        }
        pgid_lists.push(pgids);
    }

    let mut pages_by_oid = BTreeMap::<(u32, u32), Vec<PageId>>::new();
    for (seq_num, page_id) in page_seq_to_id {
        let Some(reference) = references.get(seq_num) else {
            continue;
        };
        let chunk = match chunk_for_reference(contents_stream.clone(), contents, reference) {
            Ok(chunk) => chunk,
            Err(_) => continue,
        };
        let oid_fields = chunk
            .fields
            .iter()
            .filter(|field| field.id == 0x06 && field.block_type == BLOCK_TYPE_FIXED_8)
            .collect::<Vec<_>>();
        if oid_fields.len() != 1 {
            continue;
        }
        let Ok(oid) = parse_confirmed_oid_identity_payload(oid_fields[0].clone()) else {
            continue;
        };
        pages_by_oid
            .entry((oid.dword0, oid.dword1))
            .or_default()
            .push(*page_id);
    }

    let observed_scenario_page_ids =
        match resolve_scenario_page_ids_from_evidence(&pgid_lists, &pages_by_oid) {
            Ok(projected) => {
                diagnostics.push(PubBridgeDiagnostic::ScenarioPageOrderObserved {
                    raw_page_count: raw_page_ids.len(),
                    scenario_page_count: projected.len(),
                    evidence_list_count: pgid_lists.len(),
                });
                projected
            }
            Err(reason) if observed_page_list => {
                diagnostics.push(PubBridgeDiagnostic::ScenarioPageOrderUnavailable {
                    reason,
                    raw_page_count: raw_page_ids.len(),
                });
                Vec::new()
            }
            Err(_) => Vec::new(),
        };

    (
        build_effective_page_projection(raw_page_ids, observed_scenario_page_ids, pgid_lists.len()),
        diagnostics,
    )
}

pub(super) fn build_effective_page_projection(
    raw_page_ids: &[PageId],
    observed_scenario_page_ids: Vec<PageId>,
    scenario_evidence_list_count: usize,
) -> PubEffectivePageProjection {
    // Pgid is scenario/design identity evidence, not generic physical visible-page authority.
    // Native Page.Duplicate/Pages.Add can create persisted visible pages without OplControlling/Pgid,
    // so generic Reader must not suppress any raw PAGE from this observation alone.
    // observed_scenario_page_ids is evidence only; any product-specific filtering needs a separate,
    // explicitly authorized profile law and must not mutate this generic page_ids projection.
    PubEffectivePageProjection {
        authority: PubEffectivePageProjectionAuthority::RawDocumentPageList,
        page_ids: raw_page_ids.to_vec(),
        raw_page_count: raw_page_ids.len(),
        observed_scenario_page_ids,
        scenario_evidence_list_count,
    }
}

pub(super) fn resolve_scenario_page_ids_from_evidence(
    pgid_lists: &[Vec<(u32, u32)>],
    pages_by_oid: &BTreeMap<(u32, u32), Vec<PageId>>,
) -> std::result::Result<Vec<PageId>, String> {
    let Some(consensus) = pgid_lists.first() else {
        return Err("no_controlling_page_list_observation".to_owned());
    };
    if consensus.is_empty() {
        return Err("controlling_scenario_page_projection_empty".to_owned());
    }
    if pgid_lists
        .iter()
        .skip(1)
        .any(|candidate| candidate != consensus)
    {
        return Err("controlling_page_lists_disagree".to_owned());
    }

    let mut projected = Vec::with_capacity(consensus.len());
    let mut seen = BTreeSet::new();
    for pgid in consensus {
        let Some(matches) = pages_by_oid.get(pgid) else {
            return Err(format!("pgid_has_no_page:{:08x}:{:08x}", pgid.0, pgid.1));
        };
        if matches.len() != 1 {
            return Err(format!(
                "pgid_is_ambiguous:{:08x}:{:08x}:matches={}",
                pgid.0,
                pgid.1,
                matches.len()
            ));
        }
        let page_id = matches[0];
        if !seen.insert(page_id) {
            return Err("controlling_page_list_repeats_page".to_owned());
        }
        projected.push(page_id);
    }

    Ok(projected)
}
