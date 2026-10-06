use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::{env, fs, path::PathBuf};

const INPUT_SCHEMA: &str = "chaptera.recovery-damage-signatures.v1";
const OUTPUT_SCHEMA: &str = "chaptera.recovery-damage-predictions.v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ReaderAdmission {
    OrdinaryOpen,
    TypedSalvage,
    UntypedDamaged,
    ExistingFormatGap,
    Rejected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum FactState {
    Proven,
    Ambiguous,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum RepairAuthority {
    None,
    SalvageOnly,
    BoundedRepairCandidate,
    ConstraintRepairProven,
    BoundedNativeAccepted,
    DiagnosticOnly,
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum PredictedOutcome {
    Open,
    OpenPartial,
    Rescue,
    NoSafeRecovery,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum PredictionConfidence {
    High,
    Medium,
}

#[derive(Debug, Clone, Deserialize)]
struct DamageSignatureFile {
    schema: String,
    rows: Vec<DamageSignatureV1>,
}

#[derive(Debug, Clone, Deserialize)]
struct DamageSignatureV1 {
    id: String,
    reader_admission: ReaderAdmission,
    source_immutable: bool,
    fabricated_bytes: u64,
    silent_drops: u64,
    identity_ambiguous: bool,
    text: FactState,
    images: FactState,
    geometry: FactState,
    repair_authority: RepairAuthority,
    #[serde(default)]
    damage_hints: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
struct DamagePredictionV1 {
    id: String,
    predicted_outcome: PredictedOutcome,
    confidence: PredictionConfidence,
    reason: &'static str,
    useful_fact_count: usize,
    repair_authority: RepairAuthority,
    damage_hints: Vec<String>,
}

#[derive(Debug, Serialize)]
struct DamagePredictionFile {
    schema: &'static str,
    rows: Vec<DamagePredictionV1>,
}

fn useful_fact_count(signature: &DamageSignatureV1) -> usize {
    [signature.text, signature.images, signature.geometry]
        .into_iter()
        .filter(|state| *state == FactState::Proven)
        .count()
}

fn repair_can_route_to_rescue(authority: RepairAuthority) -> bool {
    matches!(
        authority,
        RepairAuthority::BoundedRepairCandidate
            | RepairAuthority::ConstraintRepairProven
            | RepairAuthority::BoundedNativeAccepted
    )
}

fn predict(signature: DamageSignatureV1) -> DamagePredictionV1 {
    let facts = useful_fact_count(&signature);

    let (predicted_outcome, confidence, reason) = if !signature.source_immutable
        || signature.fabricated_bytes != 0
        || signature.silent_drops != 0
    {
        (
            PredictedOutcome::NoSafeRecovery,
            PredictionConfidence::High,
            "recovery_evidence_integrity_failed",
        )
    } else if signature.identity_ambiguous {
        (
            PredictedOutcome::NoSafeRecovery,
            PredictionConfidence::High,
            "carrier_or_object_identity_ambiguous",
        )
    } else {
        match signature.reader_admission {
            ReaderAdmission::OrdinaryOpen => (
                PredictedOutcome::Open,
                PredictionConfidence::High,
                "ordinary_reader_authority",
            ),
            ReaderAdmission::ExistingFormatGap => (
                PredictedOutcome::NoSafeRecovery,
                PredictionConfidence::High,
                "existing_format_gap_is_not_damage_authority",
            ),
            ReaderAdmission::Rejected => (
                PredictedOutcome::NoSafeRecovery,
                PredictionConfidence::High,
                "intake_or_safety_reject",
            ),
            ReaderAdmission::TypedSalvage if facts > 0 => (
                PredictedOutcome::OpenPartial,
                PredictionConfidence::High,
                "typed_damage_authority_plus_useful_source_neutral_fact",
            ),
            ReaderAdmission::TypedSalvage
                if repair_can_route_to_rescue(signature.repair_authority) =>
            {
                (
                    PredictedOutcome::Rescue,
                    PredictionConfidence::High,
                    "typed_damage_without_reader_fact_but_bounded_repair_authority_exists",
                )
            }
            ReaderAdmission::TypedSalvage => (
                PredictedOutcome::NoSafeRecovery,
                PredictionConfidence::High,
                "typed_damage_without_useful_reader_fact",
            ),
            ReaderAdmission::UntypedDamaged
                if repair_can_route_to_rescue(signature.repair_authority) =>
            {
                (
                    PredictedOutcome::Rescue,
                    PredictionConfidence::High,
                    "bounded_repair_authority_exists",
                )
            }
            ReaderAdmission::UntypedDamaged
                if facts > 0 && signature.repair_authority == RepairAuthority::SalvageOnly =>
            {
                (
                    PredictedOutcome::Rescue,
                    PredictionConfidence::Medium,
                    "useful_recovery_fact_exists_but_production_typed_damage_authority_is_missing",
                )
            }
            ReaderAdmission::UntypedDamaged if facts > 0 => (
                PredictedOutcome::Rescue,
                PredictionConfidence::Medium,
                "useful_recovery_fact_exists_but_reader_promotion_authority_is_missing",
            ),
            ReaderAdmission::UntypedDamaged => (
                PredictedOutcome::NoSafeRecovery,
                PredictionConfidence::High,
                "no_useful_fact_or_bounded_repair_authority",
            ),
        }
    };

    DamagePredictionV1 {
        id: signature.id,
        predicted_outcome,
        confidence,
        reason,
        useful_fact_count: facts,
        repair_authority: signature.repair_authority,
        damage_hints: signature.damage_hints,
    }
}

fn main() -> Result<()> {
    let mut args = env::args_os().skip(1);
    let input = PathBuf::from(args.next().context(
        "usage: recovery-damage-signature-predictor INPUT.json OUTPUT.json",
    )?);
    let output = PathBuf::from(args.next().context(
        "usage: recovery-damage-signature-predictor INPUT.json OUTPUT.json",
    )?);
    if args.next().is_some() {
        bail!("recovery-damage-signature-predictor accepts INPUT.json OUTPUT.json");
    }

    let payload: DamageSignatureFile = serde_json::from_slice(
        &fs::read(&input).with_context(|| format!("read {}", input.display()))?,
    )
    .with_context(|| format!("parse {}", input.display()))?;
    if payload.schema != INPUT_SCHEMA {
        bail!(
            "unexpected input schema {:?}; expected {:?}",
            payload.schema,
            INPUT_SCHEMA
        );
    }

    let rows = payload.rows.into_iter().map(predict).collect();
    let result = DamagePredictionFile {
        schema: OUTPUT_SCHEMA,
        rows,
    };
    let encoded = serde_json::to_vec_pretty(&result).context("serialize predictions")?;
    fs::write(&output, encoded).with_context(|| format!("write {}", output.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn signature(admission: ReaderAdmission) -> DamageSignatureV1 {
        DamageSignatureV1 {
            id: "witness".to_owned(),
            reader_admission: admission,
            source_immutable: true,
            fabricated_bytes: 0,
            silent_drops: 0,
            identity_ambiguous: false,
            text: FactState::Unavailable,
            images: FactState::Unavailable,
            geometry: FactState::Unavailable,
            repair_authority: RepairAuthority::None,
            damage_hints: Vec::new(),
        }
    }

    #[test]
    fn ordinary_reader_authority_is_open() {
        let result = predict(signature(ReaderAdmission::OrdinaryOpen));
        assert_eq!(result.predicted_outcome, PredictedOutcome::Open);
    }

    #[test]
    fn typed_damage_needs_a_useful_fact_for_open_partial() {
        let mut input = signature(ReaderAdmission::TypedSalvage);
        input.text = FactState::Proven;
        let result = predict(input);
        assert_eq!(result.predicted_outcome, PredictedOutcome::OpenPartial);
        assert_eq!(result.useful_fact_count, 1);

        let result = predict(signature(ReaderAdmission::TypedSalvage));
        assert_eq!(result.predicted_outcome, PredictedOutcome::NoSafeRecovery);
    }

    #[test]
    fn untyped_useful_recovery_stays_rescue_not_reader_open() {
        let mut input = signature(ReaderAdmission::UntypedDamaged);
        input.images = FactState::Proven;
        input.repair_authority = RepairAuthority::SalvageOnly;
        let result = predict(input);
        assert_eq!(result.predicted_outcome, PredictedOutcome::Rescue);
        assert_eq!(result.confidence, PredictionConfidence::Medium);
    }

    #[test]
    fn bounded_repair_authority_routes_to_rescue() {
        for authority in [
            RepairAuthority::BoundedRepairCandidate,
            RepairAuthority::ConstraintRepairProven,
            RepairAuthority::BoundedNativeAccepted,
        ] {
            let mut input = signature(ReaderAdmission::UntypedDamaged);
            input.repair_authority = authority;
            let result = predict(input);
            assert_eq!(result.predicted_outcome, PredictedOutcome::Rescue);
            assert_eq!(result.confidence, PredictionConfidence::High);
        }
    }

    #[test]
    fn format_gap_never_becomes_damage_from_forced_facts() {
        let mut input = signature(ReaderAdmission::ExistingFormatGap);
        input.text = FactState::Proven;
        input.repair_authority = RepairAuthority::SalvageOnly;
        let result = predict(input);
        assert_eq!(
            result.predicted_outcome,
            PredictedOutcome::NoSafeRecovery
        );
    }

    #[test]
    fn ambiguous_identity_fails_closed_even_with_surviving_payload() {
        let mut input = signature(ReaderAdmission::TypedSalvage);
        input.images = FactState::Proven;
        input.identity_ambiguous = true;
        let result = predict(input);
        assert_eq!(
            result.predicted_outcome,
            PredictedOutcome::NoSafeRecovery
        );
        assert_eq!(result.reason, "carrier_or_object_identity_ambiguous");
    }

    #[test]
    fn fabricated_or_silently_dropped_bytes_invalidate_recovery_evidence() {
        let mut fabricated = signature(ReaderAdmission::TypedSalvage);
        fabricated.text = FactState::Proven;
        fabricated.fabricated_bytes = 1;
        assert_eq!(
            predict(fabricated).predicted_outcome,
            PredictedOutcome::NoSafeRecovery
        );

        let mut dropped = signature(ReaderAdmission::TypedSalvage);
        dropped.text = FactState::Proven;
        dropped.silent_drops = 1;
        assert_eq!(
            predict(dropped).predicted_outcome,
            PredictedOutcome::NoSafeRecovery
        );
    }

    #[test]
    fn diagnostic_only_without_facts_is_not_rescue() {
        let mut input = signature(ReaderAdmission::UntypedDamaged);
        input.repair_authority = RepairAuthority::DiagnosticOnly;
        assert_eq!(predict(input).predicted_outcome, PredictedOutcome::NoSafeRecovery);
    }
}
