use anyhow::{Context, Result};
use pub_viewer::{
    build_reader_partial_source_graph, classify_pub_family, open_pub_or_salvage,
    probe_reader_salvage_candidate, probe_reader_salvage_candidate_with_trigger,
    viewer_geometry_environment_v0_1, ReaderPartialSourceFact, ReaderPartialSourceGraphError,
    ReaderSalvageCorruptionEvidence, ReaderSalvageEligibility, ReaderSalvageProbe,
    ReaderSalvageSubsystemProbe, ReaderSalvageTrigger, ViewerProductOpenOutcome,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    forced_trigger_probe: Option<ForcedTriggerProbeReceipt>,
    #[serde(skip_serializing_if = "Option::is_none")]
    forced_partial_graph: Option<ForcedPartialGraphReceipt>,
    source_modified: bool,
}

#[derive(Debug, Serialize)]
struct ForcedTriggerProbeReceipt {
    eligibility: &'static str,
    cfb_inventory_available: bool,
    contents_family: Option<String>,
    has_surviving_evidence: bool,
    subsystems: ReaderSalvageSubsystemProbe,
    source_modified: bool,
}

#[derive(Debug, Serialize)]
struct ForcedPartialGraphReceipt {
    status: &'static str,
    fact_counts: BTreeMap<&'static str, usize>,
    gap_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<&'static str>,
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
        ReaderSalvageCorruptionEvidence::Publisher97MalformedOrStaleMediaVariant => {
            "publisher97_malformed_or_stale_media_variant"
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

fn partial_graph_error_name(value: ReaderPartialSourceGraphError) -> &'static str {
    match value {
        ReaderPartialSourceGraphError::SourceIdentityMismatch => "source_identity_mismatch",
        ReaderPartialSourceGraphError::SourceModified => "source_modified",
        ReaderPartialSourceGraphError::ProbeMismatch => "probe_mismatch",
        ReaderPartialSourceGraphError::Ineligible => "ineligible",
    }
}

fn forced_trigger_diagnostic(
    bytes: &[u8],
    intake_probe: &ReaderSalvageProbe,
) -> (
    Option<ForcedTriggerProbeReceipt>,
    Option<ForcedPartialGraphReceipt>,
) {
    if intake_probe.eligibility != ReaderSalvageEligibility::AwaitingTypedCorruptionEvidence {
        return (None, None);
    }

    let forced = probe_reader_salvage_candidate_with_trigger(
        bytes,
        ReaderSalvageTrigger::ProvenStructuralCorruption,
    );
    let probe_receipt = ForcedTriggerProbeReceipt {
        eligibility: eligibility_name(forced.eligibility),
        cfb_inventory_available: forced.cfb_inventory_available,
        contents_family: forced.contents_family.clone(),
        has_surviving_evidence: forced.has_surviving_evidence(),
        subsystems: forced.subsystems,
        source_modified: forced.source_modified,
    };

    let graph_receipt = match build_reader_partial_source_graph(bytes, &forced) {
        Ok(graph) => ForcedPartialGraphReceipt {
            status: "constructed",
            fact_counts: fact_counts(&graph.facts),
            gap_count: graph.gaps.len(),
            error: None,
        },
        Err(error) => ForcedPartialGraphReceipt {
            status: "error",
            fact_counts: BTreeMap::new(),
            gap_count: 0,
            error: Some(partial_graph_error_name(error)),
        },
    };

    (Some(probe_receipt), Some(graph_receipt))
}

fn fact_counts(facts: &[ReaderPartialSourceFact]) -> BTreeMap<&'static str, usize> {
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
            forced_trigger_probe: None,
            forced_partial_graph: None,
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
                forced_trigger_probe: None,
                forced_partial_graph: None,
                source_modified: probe.source_modified,
            }
        }
        Err(error) => {
            let probe = probe_reader_salvage_candidate(bytes);
            let (forced_trigger_probe, forced_partial_graph) =
                forced_trigger_diagnostic(bytes, &probe);
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
                forced_trigger_probe,
                forced_partial_graph,
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
        anyhow::bail!("corpus-reader-salvage-acceptance accepts exactly CORPUS_DIR OUTPUT.json");
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
            *eligibility_counts
                .entry(eligibility.to_owned())
                .or_default() += 1;
        }
        if let Some(evidence) = row.corruption_evidence {
            *corruption_evidence_counts
                .entry(evidence.to_owned())
                .or_default() += 1;
        }
        rows.push(row);
    }

    let forced_trigger_attempted_count = rows
        .iter()
        .filter(|row| row.forced_trigger_probe.is_some())
        .count();
    let forced_cfb_inventory_available_count = rows
        .iter()
        .filter_map(|row| row.forced_trigger_probe.as_ref())
        .filter(|probe| probe.cfb_inventory_available)
        .count();
    let forced_surviving_evidence_count = rows
        .iter()
        .filter_map(|row| row.forced_trigger_probe.as_ref())
        .filter(|probe| probe.has_surviving_evidence)
        .count();
    let forced_partial_graph_constructed_count = rows
        .iter()
        .filter_map(|row| row.forced_partial_graph.as_ref())
        .filter(|graph| graph.status == "constructed")
        .count();
    let mut forced_contents_family_counts = BTreeMap::<String, usize>::new();
    for family in rows
        .iter()
        .filter_map(|row| row.forced_trigger_probe.as_ref())
        .filter_map(|probe| probe.contents_family.as_deref())
    {
        *forced_contents_family_counts
            .entry(family.to_owned())
            .or_default() += 1;
    }

    let report = serde_json::json!({
        "schema": SCHEMA,
        "corpus_file_count": rows.len(),
        "outcome_counts": outcome_counts,
        "salvage_eligibility_counts": eligibility_counts,
        "corruption_evidence_counts": corruption_evidence_counts,
        "forced_trigger_summary": {
            "attempted_count": forced_trigger_attempted_count,
            "cfb_inventory_available_count": forced_cfb_inventory_available_count,
            "surviving_evidence_count": forced_surviving_evidence_count,
            "partial_graph_constructed_count": forced_partial_graph_constructed_count,
            "contents_family_counts": forced_contents_family_counts,
        },
        "rows": rows,
        "evidence_boundary": "source-safe acceptance only; no filenames, paths, document text, raw streams, source bytes, repaired PUB materialization, or guessed geometry are retained",
    });

    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&output, serde_json::to_vec_pretty(&report)?)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&report["outcome_counts"])?
    );
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
    fn forced_trigger_diagnostic_error_names_are_stable() {
        assert_eq!(
            partial_graph_error_name(ReaderPartialSourceGraphError::Ineligible),
            "ineligible"
        );
        assert_eq!(
            partial_graph_error_name(ReaderPartialSourceGraphError::ProbeMismatch),
            "probe_mismatch"
        );
    }

    #[test]
    fn known_pub_without_admitted_salvage_remains_unsupported() {
        for eligibility in [
            ReaderSalvageEligibility::EligibleDamagedPublisher,
            ReaderSalvageEligibility::EligibleKnownPublisherCorruption,
            ReaderSalvageEligibility::AwaitingTypedCorruptionEvidence,
            ReaderSalvageEligibility::IneligibleUnproven,
        ] {
            assert_eq!(failed_outcome(eligibility), AcceptanceOutcome::Unsupported);
        }
    }
}
