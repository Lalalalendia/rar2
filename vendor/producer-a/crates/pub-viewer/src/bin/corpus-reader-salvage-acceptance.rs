use anyhow::{Context, Result};
use pub_viewer::{
    ReaderPartialSourceFact, ReaderSalvageCorruptionEvidence, ReaderSalvageEligibility,
    ViewerProductOpenOutcome, classify_pub_family, open_pub_or_salvage,
    probe_reader_salvage_candidate, viewer_geometry_environment_v0_1,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
};

const SCHEMA: &str = "chaptera.reader-salvage-1050-acceptance.v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
enum AcceptanceOutcome {
    NormalOpen,
    SalvageOpen,
    Unsupported,
    Unsafe,
}

#[derive(Debug, Serialize)]
struct AcceptanceRow {
    source_sha256: String,
    byte_len: usize,
    outcome: AcceptanceOutcome,
    reader_route: String,
    pub_profile: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    salvage_eligibility: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    corruption_evidence: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    has_surviving_evidence: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cfb_inventory_available: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    contents_family: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    salvage_fact_counts: Option<BTreeMap<&'static str, usize>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    salvage_gap_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    open_error_signature_sha256: Option<String>,
    source_modified: bool,
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn eligibility_name(value: ReaderSalvageEligibility) -> &'static str {
    match value {
        ReaderSalvageEligibility::EligibleDamagedPublisher => "eligible_damaged_publisher",
        ReaderSalvageEligibility::EligibleKnownPublisherCorruption => {
            "eligible_known_publisher_corruption"
        }
        ReaderSalvageEligibility::AwaitingTypedCorruptionEvidence => {
            "awaiting_typed_corruption_evidence"
        }
        ReaderSalvageEligibility::IneligibleUnproven => "ineligible_unproven",
        ReaderSalvageEligibility::IneligibleArchive => "ineligible_archive",
        ReaderSalvageEligibility::IneligibleForeign => "ineligible_foreign",
        ReaderSalvageEligibility::IneligibleSuspicious => "ineligible_suspicious",
        ReaderSalvageEligibility::IneligibleResourceLimit => "ineligible_resource_limit",
    }
}

fn corruption_name(value: ReaderSalvageCorruptionEvidence) -> &'static str {
    match value {
        ReaderSalvageCorruptionEvidence::QuillDescriptorNodeTruncated => {
            "quill_descriptor_node_truncated"
        }
        ReaderSalvageCorruptionEvidence::QuillStrsServiceSpanOutOfBounds => {
            "quill_strs_service_span_out_of_bounds"
        }
    }
}

fn failed_outcome(eligibility: ReaderSalvageEligibility) -> AcceptanceOutcome {
    match eligibility {
        ReaderSalvageEligibility::IneligibleArchive
        | ReaderSalvageEligibility::IneligibleForeign
        | ReaderSalvageEligibility::IneligibleSuspicious
        | ReaderSalvageEligibility::IneligibleResourceLimit => AcceptanceOutcome::Unsafe,
        ReaderSalvageEligibility::EligibleDamagedPublisher
        | ReaderSalvageEligibility::EligibleKnownPublisherCorruption
        | ReaderSalvageEligibility::AwaitingTypedCorruptionEvidence
        | ReaderSalvageEligibility::IneligibleUnproven => AcceptanceOutcome::Unsupported,
    }
}

fn fact_counts(
    facts: &[ReaderPartialSourceFact],
) -> BTreeMap<&'static str, usize> {
    let mut counts = BTreeMap::new();
    for fact in facts {
        let key = match fact {
            ReaderPartialSourceFact::TextRange { .. } => "text_range",
            ReaderPartialSourceFact::VerifiedImage { .. } => "verified_image",
            ReaderPartialSourceFact::GroundedGeometry { .. } => "grounded_geometry",
        };
        *counts.entry(key).or_insert(0) += 1;
    }
    counts
}

fn classify(bytes: &[u8]) -> AcceptanceRow {
    let source_sha256 = sha256_hex(bytes);
    let family = classify_pub_family(bytes);

    match open_pub_or_salvage(bytes, viewer_geometry_environment_v0_1()) {
        Ok(ViewerProductOpenOutcome::Normal(_)) => AcceptanceRow {
            source_sha256,
            byte_len: bytes.len(),
            outcome: AcceptanceOutcome::NormalOpen,
            reader_route: family.route.as_str().to_owned(),
            pub_profile: family.profile.as_str().to_owned(),
            salvage_eligibility: None,
            corruption_evidence: None,
            has_surviving_evidence: None,
            cfb_inventory_available: None,
            contents_family: None,
            salvage_fact_counts: None,
            salvage_gap_count: None,
            open_error_signature_sha256: None,
            source_modified: false,
        },
        Ok(ViewerProductOpenOutcome::Salvage(graph)) => {
            let probe = probe_reader_salvage_candidate(bytes);
            debug_assert_eq!(probe.source_sha256, source_sha256);
            debug_assert_eq!(graph.source_sha256, source_sha256);
            AcceptanceRow {
                source_sha256,
                byte_len: bytes.len(),
                outcome: AcceptanceOutcome::SalvageOpen,
                reader_route: family.route.as_str().to_owned(),
                pub_profile: family.profile.as_str().to_owned(),
                salvage_eligibility: Some(eligibility_name(probe.eligibility)),
                corruption_evidence: probe.corruption_evidence.map(corruption_name),
                has_surviving_evidence: Some(probe.has_surviving_evidence()),
                cfb_inventory_available: Some(probe.cfb_inventory_available),
                contents_family: graph.contents_family.clone(),
                salvage_fact_counts: Some(fact_counts(&graph.facts)),
                salvage_gap_count: Some(graph.gaps.len()),
                open_error_signature_sha256: None,
                source_modified: probe.source_modified,
            }
        }
        Err(error) => {
            let probe = probe_reader_salvage_candidate(bytes);
            AcceptanceRow {
                source_sha256,
                byte_len: bytes.len(),
                outcome: failed_outcome(probe.eligibility),
                reader_route: family.route.as_str().to_owned(),
                pub_profile: family.profile.as_str().to_owned(),
                salvage_eligibility: Some(eligibility_name(probe.eligibility)),
                corruption_evidence: probe.corruption_evidence.map(corruption_name),
                has_surviving_evidence: Some(probe.has_surviving_evidence()),
                cfb_inventory_available: Some(probe.cfb_inventory_available),
                contents_family: probe.contents_family.clone(),
                salvage_fact_counts: None,
                salvage_gap_count: None,
                open_error_signature_sha256: Some(sha256_hex(format!("{error:#}").as_bytes())),
                source_modified: probe.source_modified,
            }
        }
    }
}

fn pub_paths(root: &Path) -> Result<Vec<PathBuf>> {
    let mut paths = fs::read_dir(root)
        .with_context(|| format!("read corpus dir {}", root.display()))?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            path.extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("pub"))
        })
        .collect::<Vec<_>>();
    paths.sort();
    Ok(paths)
}

fn main() -> Result<()> {
    let mut args = env::args_os().skip(1);
    let root = PathBuf::from(
        args.next()
            .context("usage: corpus-reader-salvage-acceptance CORPUS_DIR OUTPUT.json")?,
    );
    let output = PathBuf::from(
        args.next()
            .context("usage: corpus-reader-salvage-acceptance CORPUS_DIR OUTPUT.json")?,
    );
    if args.next().is_some() {
        anyhow::bail!(
            "corpus-reader-salvage-acceptance accepts exactly CORPUS_DIR OUTPUT.json"
        );
    }

    let paths = pub_paths(&root)?;
    let mut rows = Vec::with_capacity(paths.len());
    let mut outcome_counts = BTreeMap::<AcceptanceOutcome, usize>::new();
    let mut eligibility_counts = BTreeMap::<String, usize>::new();
    let mut corruption_evidence_counts = BTreeMap::<String, usize>::new();

    for path in paths {
        let bytes = fs::read(&path).with_context(|| format!("read {}", path.display()))?;
        let row = classify(&bytes);
        *outcome_counts.entry(row.outcome).or_default() += 1;
        if let Some(eligibility) = row.salvage_eligibility {
            *eligibility_counts.entry(eligibility.to_owned()).or_default() += 1;
        }
        if let Some(evidence) = row.corruption_evidence {
            *corruption_evidence_counts.entry(evidence.to_owned()).or_default() += 1;
        }
        rows.push(row);
    }

    let report = serde_json::json!({
        "schema": SCHEMA,
        "corpus_file_count": rows.len(),
        "outcome_counts": outcome_counts,
        "salvage_eligibility_counts": eligibility_counts,
        "corruption_evidence_counts": corruption_evidence_counts,
        "rows": rows,
        "evidence_boundary": "source-safe acceptance only; no filenames, paths, document text, raw streams, source bytes, repaired PUB materialization, or guessed geometry are retained",
    });

    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&output, serde_json::to_vec_pretty(&report)?)?;
    println!("{}", serde_json::to_string_pretty(&report["outcome_counts"])?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsafe_is_reserved_for_policy_or_resource_fail_closed_classes() {
        for eligibility in [
            ReaderSalvageEligibility::IneligibleArchive,
            ReaderSalvageEligibility::IneligibleForeign,
            ReaderSalvageEligibility::IneligibleSuspicious,
            ReaderSalvageEligibility::IneligibleResourceLimit,
        ] {
            assert_eq!(failed_outcome(eligibility), AcceptanceOutcome::Unsafe);
        }
    }

    #[test]
    fn known_pub_without_admitted_salvage_remains_unsupported() {
        for eligibility in [
            ReaderSalvageEligibility::EligibleDamagedPublisher,
            ReaderSalvageEligibility::EligibleKnownPublisherCorruption,
            ReaderSalvageEligibility::AwaitingTypedCorruptionEvidence,
            ReaderSalvageEligibility::IneligibleUnproven,
        ] {
            assert_eq!(
                failed_outcome(eligibility),
                AcceptanceOutcome::Unsupported
            );
        }
    }
}
