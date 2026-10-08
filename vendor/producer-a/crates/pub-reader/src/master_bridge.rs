use anyhow::{Context, Result, bail};
use pub_master_projection::{
    ExactReferenceFieldV1, MASTER_FIELD_ID, MasterProjectionSourceInputV1,
    MasterProjectionSourceOutputV1, PageProjectionCoordinateV1, RAW_TYPE_PAGE, REFERENCE_U32_WIRE,
    build_source_output_v1,
};
use pub_model::{CanonicalId, PageId, Sha256Digest};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Cursor;

use super::{PubSourceGraph, analyze_mature_0x2c_page_roles, derive_pub_page_id};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PubMasterProjectionBridgeV1 {
    pub output: MasterProjectionSourceOutputV1,
    pub active_graph_identity_parity: bool,
}

/// Rebuilds the already-proven PAGE -> master semantic context from the active
/// mature PAGE-role observation without introducing a second Contents parser.
///
/// Authority is deliberately split:
/// - the active vendor Reader owns Contents traversal and exact field/wire
///   observation;
/// - pub-master-projection owns semantic admission for field0x0D/wire0x68,
///   PAGE target type, uniqueness and self-reference rejection;
/// - this bridge proves that root-derived canonical PageIds are identical to
///   the active vendor SourceGraph identities before any consumer may use them.
pub fn build_mature_0x2c_master_projection_bridge_v1(
    bytes: &[u8],
    source_hash: Sha256Digest,
    source_graph: &PubSourceGraph,
) -> Result<PubMasterProjectionBridgeV1> {
    let roles = analyze_mature_0x2c_page_roles(Cursor::new(bytes))
        .context("build active Reader PAGE-role observation for master projection")?;

    let mut document_raw_types = BTreeMap::<u32, u16>::new();
    for entry in &roles.document_entries {
        let Some(raw_type) = entry.raw_type else {
            continue;
        };
        if document_raw_types
            .insert(entry.contents_seq_num, raw_type)
            .is_some()
        {
            bail!(
                "PAGE-role observation repeats DOCUMENT handle {}",
                entry.contents_seq_num
            );
        }
    }

    let mut coordinates = Vec::with_capacity(roles.pages.len());
    let mut expected_relation_sources = BTreeSet::<u32>::new();

    for page in &roles.pages {
        let raw_type = document_raw_types
            .get(&page.contents_seq_num)
            .copied()
            .with_context(|| {
                format!(
                    "PAGE {} is absent from typed DOCUMENT observation",
                    page.contents_seq_num
                )
            })?;
        if raw_type != u16::from(RAW_TYPE_PAGE) {
            bail!(
                "PAGE-role row {} resolves to raw type 0x{raw_type:02X}, expected PAGE",
                page.contents_seq_num
            );
        }

        let master_fields = match page.applied_master_seq_num {
            Some(master_seq_num) => {
                let field_id = page.applied_master_field_id.with_context(|| {
                    format!(
                        "PAGE {} master relation lost exact field identity",
                        page.contents_seq_num
                    )
                })?;
                let block_type = page.applied_master_block_type.with_context(|| {
                    format!(
                        "PAGE {} master relation lost exact wire identity",
                        page.contents_seq_num
                    )
                })?;
                if field_id != u16::from(MASTER_FIELD_ID) {
                    bail!(
                        "PAGE {} master relation reports field 0x{field_id:02X}, expected 0x{MASTER_FIELD_ID:02X}",
                        page.contents_seq_num
                    );
                }
                if block_type != REFERENCE_U32_WIRE {
                    bail!(
                        "PAGE {} master relation reports wire 0x{block_type:02X}, expected 0x{REFERENCE_U32_WIRE:02X}",
                        page.contents_seq_num
                    );
                }
                if page.applied_master_raw_type != Some(u16::from(RAW_TYPE_PAGE)) {
                    bail!(
                        "PAGE {} master target is not an exact PAGE",
                        page.contents_seq_num
                    );
                }
                if !expected_relation_sources.insert(page.contents_seq_num) {
                    bail!(
                        "PAGE-role observation repeats master source {}",
                        page.contents_seq_num
                    );
                }
                vec![ExactReferenceFieldV1 {
                    field_id: u8::try_from(field_id)
                        .context("master field id does not fit root projection input")?,
                    block_type,
                    value: master_seq_num,
                }]
            }
            None => {
                if page.applied_master_field_id.is_some()
                    || page.applied_master_block_type.is_some()
                    || page.applied_master_raw_type.is_some()
                {
                    bail!(
                        "PAGE {} has partial master evidence without a target",
                        page.contents_seq_num
                    );
                }
                Vec::new()
            }
        };

        coordinates.push(PageProjectionCoordinateV1 {
            seq_num: page.contents_seq_num,
            raw_type,
            master_fields,
        });
    }

    let input = MasterProjectionSourceInputV1 {
        source_hash: source_hash.to_string(),
        pages: coordinates,
    };
    let output =
        build_source_output_v1(&input).context("apply canonical root PAGE-master semantic gate")?;

    if output.receipt.relation_count != expected_relation_sources.len() {
        bail!(
            "root/vendor master relation count mismatch: root={} vendor={}",
            output.receipt.relation_count,
            expected_relation_sources.len()
        );
    }

    let actual_relation_sources = output
        .context
        .master_relations
        .iter()
        .map(|relation| relation.source_page_seq_num)
        .collect::<BTreeSet<_>>();
    if actual_relation_sources != expected_relation_sources {
        bail!("root/vendor master relation source set mismatch");
    }

    for relation in &output.context.master_relations {
        verify_page_identity(
            source_graph,
            &source_hash,
            relation.source_page_seq_num,
            &relation.source_page_id,
            "source",
        )?;
        verify_page_identity(
            source_graph,
            &source_hash,
            relation.master_page_seq_num,
            &relation.master_page_id,
            "master",
        )?;
    }

    Ok(PubMasterProjectionBridgeV1 {
        output,
        active_graph_identity_parity: true,
    })
}

fn verify_page_identity(
    graph: &PubSourceGraph,
    source_hash: &Sha256Digest,
    seq_num: u32,
    root_page_id: &str,
    label: &str,
) -> Result<()> {
    let canonical = root_page_id
        .parse::<CanonicalId>()
        .with_context(|| format!("root {label} PAGE identity is not canonical: {root_page_id}"))?;
    let root_as_vendor = PageId::from_canonical(canonical);
    let vendor_derived = derive_pub_page_id(source_hash, seq_num)
        .with_context(|| format!("derive vendor {label} PAGE identity for seq {seq_num}"))?;

    if root_as_vendor != vendor_derived {
        bail!("root/vendor {label} PAGE identity mismatch for source coordinate {seq_num}");
    }
    if !graph.pages.contains_key(&vendor_derived) {
        bail!(
            "root/vendor {label} PAGE identity is absent from active SourceGraph for source coordinate {seq_num}"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_master_constants_match_active_reader_wire_contract() {
        assert_eq!(MASTER_FIELD_ID, 0x0d);
        assert_eq!(REFERENCE_U32_WIRE, 0x68);
        assert_eq!(RAW_TYPE_PAGE, 0x43);
    }
}
