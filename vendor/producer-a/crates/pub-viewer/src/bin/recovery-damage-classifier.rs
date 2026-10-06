use anyhow::{Context, Result, bail};
use pub_viewer::{
    FailureIntakeClass, ReaderPartialSourceFact, ReaderPartialSourceGap,
    ReaderSalvageCorruptionEvidence, ReaderSalvageEligibility, ReaderSalvageTrigger,
    ViewerProductOpenOutcome, build_reader_partial_source_graph, classify_failure_candidate,
    classify_pub_family, open_pub_or_salvage, probe_reader_salvage_candidate,
    probe_reader_salvage_candidate_with_trigger, viewer_geometry_environment_v0_1,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
};

const SCHEMA: &str = "chaptera.recovery-damage-classifier.v1";
const RESCUE_EVIDENCE_SCHEMA: &str = "chaptera.recovery-rescue-evidence.v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
enum CompactVerdict {
    Open,
    OpenPartial,
    Rescue,
    NoSafeRecovery,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum ReaderOutcome {
    NormalOpen,
    SalvageOpen,
    CannotSafelyDisplay,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum RescueOutcome {
    NoneNeeded,
    BoundedRepairCandidate,
    SalvageOnly,
    DiagnosticOnly,
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum EvidenceLevel {
    Production,
    ExactShaAuthority,
    ControlledNative,
    RecoveryResearch,
    Inferred,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Confidence {
    High,
    Medium,
    Low,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum SalvageValue {
    NotApplicable,
    Proven,
    Ambiguous,
    Unavailable,
}

#[derive(Debug, Clone, Deserialize)]
struct RescueEvidenceFile {
    schema: String,
    rows: Vec<RescueEvidenceRow>,
}

#[derive(Debug, Clone, Deserialize)]
struct RescueEvidenceRow {
    source_sha256: String,
    rescue_outcome: RescueOutcome,
    #[serde(default)]
    repair_gate: Option<String>,
    #[serde(default)]
    repair_native_status: Option<String>,
    evidence_level: EvidenceLevel,
    confidence: Confidence,
    #[serde(default)]
    promotion_gap: Option<String>,
}

#[derive(Debug, Serialize)]
struct ClassifierRow {
    source_sha256: String,
    byte_len: usize,
    compact_verdict: CompactVerdict,
    reader_outcome: ReaderOutcome,
    reader_reason: String,
    intake_class: &'static str,
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
    salvage_gap_counts: Option<BTreeMap<&'static str, usize>>,
    salvage_text: SalvageValue,
    salvage_images: SalvageValue,
    salvage_geometry: SalvageValue,
    rescue_outcome: RescueOutcome,
    #[serde(skip_serializing_if = "Option::is_none")]
    repair_gate: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    repair_native_status: Option<String>,
    evidence_level: EvidenceLevel,
    confidence: Confidence,
    #[serde(skip_serializing_if = "Option::is_none")]
    promotion_gap: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    open_error_signature_sha256: Option<String>,
    source_modified: bool,
}

#[derive(Debug, Serialize)]
struct ClassifierSummary {
    schema: &'static str,
    file_count: usize,
    verdict_counts: BTreeMap<CompactVerdict, usize>,
    reader_outcome_counts: BTreeMap<String, usize>,
    rescue_outcome_counts: BTreeMap<String, usize>,
    rows: Vec<ClassifierRow>,
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn intake_name(value: FailureIntakeClass) -> &'static str {
    match value {
        FailureIntakeClass::PubHighValue => "pub_high_value",
        FailureIntakeClass::PubDamaged => "pub_damaged",
        FailureIntakeClass::PubPossible => "pub_possible",
        FailureIntakeClass::ArchiveWithPub => "archive_with_pub",
        FailureIntakeClass::NotPub => "not_pub",
        FailureIntakeClass::SuspiciousPolyglot => "suspicious_polyglot",
    }
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
        ReaderSalvageCorruptionEvidence::ExactShaTypedCorruptionAuthority => {
            "exact_sha_typed_corruption_authority"
        }
    }
}

fn rescue_outcome_name(value: RescueOutcome) -> &'static str {
    match value {
        RescueOutcome::NoneNeeded => "none_needed",
        RescueOutcome::BoundedRepairCandidate => "bounded_repair_candidate",
        RescueOutcome::SalvageOnly => "salvage_only",
        RescueOutcome::DiagnosticOnly => "diagnostic_only",
        RescueOutcome::Unsupported => "unsupported",
    }
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

fn gap_counts(gaps: &[ReaderPartialSourceGap]) -> BTreeMap<&'static str, usize> {
    let mut counts = BTreeMap::new();
    for gap in gaps {
        let key = match gap {
            ReaderPartialSourceGap::TextUnavailable => "text_unavailable",
            ReaderPartialSourceGap::TextSemanticAmbiguity => "text_semantic_ambiguity",
            ReaderPartialSourceGap::ImageFactsUnavailable => "image_facts_unavailable",
            ReaderPartialSourceGap::GeometryFactsUnavailable => "geometry_facts_unavailable",
        };
        *counts.entry(key).or_insert(0) += 1;
    }
    counts
}

fn salvage_values(
    facts: &BTreeMap<&'static str, usize>,
    gaps: &BTreeMap<&'static str, usize>,
) -> (SalvageValue, SalvageValue, SalvageValue) {
    let text = if facts.get("text_range").copied().unwrap_or(0) > 0 {
        SalvageValue::Proven
    } else if gaps
        .get("text_semantic_ambiguity")
        .copied()
        .unwrap_or(0)
        > 0
    {
        SalvageValue::Ambiguous
    } else {
        SalvageValue::Unavailable
    };

    let images = if facts.get("verified_image").copied().unwrap_or(0) > 0 {
        SalvageValue::Proven
    } else {
        SalvageValue::Unavailable
    };

    let geometry = if facts.get("grounded_geometry").copied().unwrap_or(0) > 0 {
        SalvageValue::Proven
    } else if gaps
        .get("geometry_facts_unavailable")
        .copied()
        .unwrap_or(0)
        > 0
    {
        SalvageValue::Unavailable
    } else {
        SalvageValue::Ambiguous
    };

    (text, images, geometry)
}

fn load_rescue_evidence(path: Option<&Path>) -> Result<BTreeMap<String, RescueEvidenceRow>> {
    let Some(path) = path else {
        return Ok(BTreeMap::new());
    };
    let payload: RescueEvidenceFile = serde_json::from_slice(
        &fs::read(path).with_context(|| format!("read rescue evidence {}", path.display()))?,
    )
    .with_context(|| format!("parse rescue evidence {}", path.display()))?;
    if payload.schema != RESCUE_EVIDENCE_SCHEMA {
        bail!(
            "unexpected rescue evidence schema {:?}; expected {:?}",
            payload.schema,
            RESCUE_EVIDENCE_SCHEMA
        );
    }

    let mut rows = BTreeMap::new();
    for row in payload.rows {
        let sha = row.source_sha256.to_ascii_lowercase();
        if sha.len() != 64 || !sha.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            bail!("invalid rescue evidence sha256 {sha:?}");
        }
        if rows.insert(sha.clone(), row).is_some() {
            bail!("duplicate rescue evidence sha256 {sha}");
        }
    }
    Ok(rows)
}

fn forced_recovery_projection(
    bytes: &[u8],
    intake_probe: &pub_viewer::ReaderSalvageProbe,
) -> Option<(
    BTreeMap<&'static str, usize>,
    BTreeMap<&'static str, usize>,
    bool,
    bool,
    Option<String>,
)> {
    if intake_probe.eligibility != ReaderSalvageEligibility::AwaitingTypedCorruptionEvidence {
        return None;
    }

    let forced = probe_reader_salvage_candidate_with_trigger(
        bytes,
        ReaderSalvageTrigger::ProvenStructuralCorruption,
    );
    if !forced.eligibility.is_eligible() || !forced.has_surviving_evidence() {
        return None;
    }

    let graph = build_reader_partial_source_graph(bytes, &forced).ok()?;
    Some((
        fact_counts(&graph.facts),
        gap_counts(&graph.gaps),
        forced.cfb_inventory_available,
        forced.has_surviving_evidence(),
        forced.contents_family,
    ))
}

fn apply_external_rescue(
    row: &mut ClassifierRow,
    evidence: Option<&RescueEvidenceRow>,
) {
    let Some(evidence) = evidence else {
        return;
    };

    row.rescue_outcome = evidence.rescue_outcome;
    row.repair_gate = evidence.repair_gate.clone();
    row.repair_native_status = evidence.repair_native_status.clone();
    row.evidence_level = evidence.evidence_level;
    row.confidence = evidence.confidence;
    row.promotion_gap = evidence.promotion_gap.clone();

    if row.reader_outcome == ReaderOutcome::CannotSafelyDisplay
        && matches!(
            row.rescue_outcome,
            RescueOutcome::BoundedRepairCandidate | RescueOutcome::SalvageOnly
        )
    {
        row.compact_verdict = CompactVerdict::Rescue;
    }
}

fn classify(bytes: &[u8], rescue_evidence: Option<&RescueEvidenceRow>) -> ClassifierRow {
    let source_sha256 = sha256_hex(bytes);
    let intake = classify_failure_candidate(bytes);
    let family = classify_pub_family(bytes);

    match open_pub_or_salvage(bytes, viewer_geometry_environment_v0_1()) {
        Ok(ViewerProductOpenOutcome::Normal(_)) => {
            let mut row = ClassifierRow {
                source_sha256,
                byte_len: bytes.len(),
                compact_verdict: CompactVerdict::Open,
                reader_outcome: ReaderOutcome::NormalOpen,
                reader_reason: "ordinary_reader_succeeded".to_owned(),
                intake_class: intake_name(intake.class),
                reader_route: family.route.as_str().to_owned(),
                pub_profile: family.profile.as_str().to_owned(),
                salvage_eligibility: None,
                corruption_evidence: None,
                has_surviving_evidence: None,
                cfb_inventory_available: None,
                contents_family: None,
                salvage_fact_counts: None,
                salvage_gap_counts: None,
                salvage_text: SalvageValue::NotApplicable,
                salvage_images: SalvageValue::NotApplicable,
                salvage_geometry: SalvageValue::NotApplicable,
                rescue_outcome: RescueOutcome::NoneNeeded,
                repair_gate: None,
                repair_native_status: None,
                evidence_level: EvidenceLevel::Production,
                confidence: Confidence::High,
                promotion_gap: None,
                open_error_signature_sha256: None,
                source_modified: false,
            };
            // A repair receipt never overrides an ordinary successful Reader open.
            if let Some(evidence) = rescue_evidence {
                if evidence.promotion_gap.is_some() {
                    row.promotion_gap = evidence.promotion_gap.clone();
                }
            }
            row
        }
        Ok(ViewerProductOpenOutcome::Salvage(graph)) => {
            let probe = probe_reader_salvage_candidate(bytes);
            let facts = fact_counts(&graph.facts);
            let gaps = gap_counts(&graph.gaps);
            let (salvage_text, salvage_images, salvage_geometry) =
                salvage_values(&facts, &gaps);
            let mut row = ClassifierRow {
                source_sha256,
                byte_len: bytes.len(),
                compact_verdict: CompactVerdict::OpenPartial,
                reader_outcome: ReaderOutcome::SalvageOpen,
                reader_reason: probe
                    .corruption_evidence
                    .map(corruption_name)
                    .unwrap_or_else(|| eligibility_name(probe.eligibility))
                    .to_owned(),
                intake_class: intake_name(intake.class),
                reader_route: family.route.as_str().to_owned(),
                pub_profile: family.profile.as_str().to_owned(),
                salvage_eligibility: Some(eligibility_name(probe.eligibility)),
                corruption_evidence: probe.corruption_evidence.map(corruption_name),
                has_surviving_evidence: Some(probe.has_surviving_evidence()),
                cfb_inventory_available: Some(probe.cfb_inventory_available),
                contents_family: graph.contents_family.clone(),
                salvage_fact_counts: Some(facts),
                salvage_gap_counts: Some(gaps),
                salvage_text,
                salvage_images,
                salvage_geometry,
                rescue_outcome: RescueOutcome::SalvageOnly,
                repair_gate: None,
                repair_native_status: None,
                evidence_level: if probe.authority.is_some() {
                    EvidenceLevel::ExactShaAuthority
                } else {
                    EvidenceLevel::Production
                },
                confidence: Confidence::High,
                promotion_gap: None,
                open_error_signature_sha256: None,
                source_modified: probe.source_modified,
            };
            apply_external_rescue(&mut row, rescue_evidence);
            row
        }
        Err(error) => {
            let probe = probe_reader_salvage_candidate(bytes);
            let forced = forced_recovery_projection(bytes, &probe);

            let (
                fact_counts_value,
                gap_counts_value,
                salvage_text,
                salvage_images,
                salvage_geometry,
                forced_cfb_inventory,
                forced_survives,
                forced_contents_family,
            ) = if let Some((facts, gaps, cfb, survives, contents_family)) = forced {
                let (text, images, geometry) = salvage_values(&facts, &gaps);
                (
                    Some(facts),
                    Some(gaps),
                    text,
                    images,
                    geometry,
                    Some(cfb),
                    Some(survives),
                    contents_family,
                )
            } else {
                (
                    None,
                    None,
                    SalvageValue::Unavailable,
                    SalvageValue::Unavailable,
                    SalvageValue::Unavailable,
                    None,
                    None,
                    None,
                )
            };

            let has_forced_recovery = fact_counts_value.is_some();
            let mut row = ClassifierRow {
                source_sha256,
                byte_len: bytes.len(),
                compact_verdict: if has_forced_recovery {
                    CompactVerdict::Rescue
                } else {
                    CompactVerdict::NoSafeRecovery
                },
                reader_outcome: ReaderOutcome::CannotSafelyDisplay,
                reader_reason: if has_forced_recovery {
                    "recovery_primitives_succeed_but_typed_authority_missing".to_owned()
                } else {
                    eligibility_name(probe.eligibility).to_owned()
                },
                intake_class: intake_name(intake.class),
                reader_route: family.route.as_str().to_owned(),
                pub_profile: family.profile.as_str().to_owned(),
                salvage_eligibility: Some(eligibility_name(probe.eligibility)),
                corruption_evidence: probe.corruption_evidence.map(corruption_name),
                has_surviving_evidence: forced_survives.or(Some(probe.has_surviving_evidence())),
                cfb_inventory_available: forced_cfb_inventory.or(Some(probe.cfb_inventory_available)),
                contents_family: forced_contents_family.or(probe.contents_family.clone()),
                salvage_fact_counts: fact_counts_value,
                salvage_gap_counts: gap_counts_value,
                salvage_text,
                salvage_images,
                salvage_geometry,
                rescue_outcome: if has_forced_recovery {
                    RescueOutcome::SalvageOnly
                } else if matches!(
                    probe.eligibility,
                    ReaderSalvageEligibility::IneligibleArchive
                        | ReaderSalvageEligibility::IneligibleForeign
                        | ReaderSalvageEligibility::IneligibleSuspicious
                        | ReaderSalvageEligibility::IneligibleResourceLimit
                ) {
                    RescueOutcome::Unsupported
                } else {
                    RescueOutcome::DiagnosticOnly
                },
                repair_gate: None,
                repair_native_status: None,
                evidence_level: if has_forced_recovery {
                    EvidenceLevel::Inferred
                } else {
                    EvidenceLevel::Production
                },
                confidence: if has_forced_recovery {
                    Confidence::Medium
                } else {
                    Confidence::High
                },
                promotion_gap: has_forced_recovery
                    .then(|| "missing_production_typed_corruption_authority".to_owned()),
                open_error_signature_sha256: Some(sha256_hex(format!("{error:#}").as_bytes())),
                source_modified: probe.source_modified,
            };
            apply_external_rescue(&mut row, rescue_evidence);
            row
        }
    }
}

fn collect_pub_paths(root: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(root).with_context(|| format!("read {}", root.display()))? {
        let entry = entry.with_context(|| format!("read entry in {}", root.display()))?;
        let path = entry.path();
        if path.is_dir() {
            collect_pub_paths(&path, out)?;
            continue;
        }
        if path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("pub"))
        {
            out.push(path);
        }
    }
    Ok(())
}

fn main() -> Result<()> {
    let mut args = env::args_os().skip(1);
    let root = PathBuf::from(
        args.next().context(
            "usage: recovery-damage-classifier CORPUS_DIR OUTPUT.json [RESCUE_EVIDENCE.json]",
        )?,
    );
    let output = PathBuf::from(
        args.next().context(
            "usage: recovery-damage-classifier CORPUS_DIR OUTPUT.json [RESCUE_EVIDENCE.json]",
        )?,
    );
    let rescue_evidence_path = args.next().map(PathBuf::from);
    if args.next().is_some() {
        bail!(
            "recovery-damage-classifier accepts CORPUS_DIR OUTPUT.json [RESCUE_EVIDENCE.json]"
        );
    }

    let rescue_evidence = load_rescue_evidence(rescue_evidence_path.as_deref())?;

    let mut paths = Vec::new();
    collect_pub_paths(&root, &mut paths)?;
    paths.sort();

    let mut rows = Vec::with_capacity(paths.len());
    let mut verdict_counts = BTreeMap::<CompactVerdict, usize>::new();
    let mut reader_outcome_counts = BTreeMap::<String, usize>::new();
    let mut rescue_outcome_counts = BTreeMap::<String, usize>::new();

    for path in paths {
        let bytes = fs::read(&path).with_context(|| format!("read {}", path.display()))?;
        let sha = sha256_hex(&bytes);
        let row = classify(&bytes, rescue_evidence.get(&sha));
        *verdict_counts.entry(row.compact_verdict).or_default() += 1;
        *reader_outcome_counts
            .entry(format!("{:?}", row.reader_outcome).to_ascii_lowercase())
            .or_default() += 1;
        *rescue_outcome_counts
            .entry(rescue_outcome_name(row.rescue_outcome).to_owned())
            .or_default() += 1;
        rows.push(row);
    }

    let summary = ClassifierSummary {
        schema: SCHEMA,
        file_count: rows.len(),
        verdict_counts,
        reader_outcome_counts,
        rescue_outcome_counts,
        rows,
    };

    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    fs::write(
        &output,
        serde_json::to_vec_pretty(&summary).context("serialize classifier output")?,
    )
    .with_context(|| format!("write {}", output.display()))?;

    Ok(())
}
