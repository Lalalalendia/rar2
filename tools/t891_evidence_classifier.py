#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import sys
from collections import Counter
from pathlib import Path
from typing import Any

BATCH_SCHEMA = "chaptera.pub.tlb-native-batch-result.v1"
BLAST_SCHEMA = "chaptera.operation-blast-radius.v1"
SUMMARY_SCHEMA = "chaptera.pub.t891-evidence-summary.v1"
EXPERIMENT_ID = "PUB-TLB-SHAPE-EFFECTS-BATCH01"
TASK_ID = "PUB-T-891"
BATCH_ID = "TLB-BATCH-01-SHAPE-EFFECTS"

EXPECTED_CANDIDATES = (
    "GlowFormat.Radius",
    "GlowFormat.Transparency",
    "GlowFormat.Visible",
    "ReflectionFormat.Type",
    "ReflectionFormat.Transparency",
    "ReflectionFormat.Size",
    "ReflectionFormat.Offset",
    "ReflectionFormat.Blur",
    "ReflectionFormat.Visible",
)

ALLOWED_CLASSIFICATIONS = {
    "inconclusive-same-value-arm",
    "inconclusive-changed-value-arm",
    "setter-no-runtime-change",
    "runtime-only-or-normalized-away",
    "persisted-semantic-change",
    "same-value-materialization-plus-persisted-change",
    "persisted-change-with-same-value-side-effect",
    "persistence-only-or-normalization",
    "semantic-change-without-logical-stream-delta",
    "mixed-or-render-derived-divergence",
}


class EvidenceError(RuntimeError):
    pass


def load_json(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8-sig"))
    except (OSError, json.JSONDecodeError) as exc:
        raise EvidenceError(f"{path}: cannot read JSON: {exc}") from exc
    if not isinstance(value, dict):
        raise EvidenceError(f"{path}: expected JSON object")
    return value


def require(condition: bool, message: str) -> None:
    if not condition:
        raise EvidenceError(message)


def slug(candidate_id: str) -> str:
    out = []
    previous_dash = False
    for char in candidate_id:
        if char.isalnum():
            out.append(char.lower())
            previous_dash = False
        elif not previous_dash:
            out.append("-")
            previous_dash = True
    return "".join(out).strip("-")


def normalized_counts(value: Any) -> dict[str, int]:
    require(isinstance(value, dict), "classification_counts must be an object")
    out: dict[str, int] = {}
    for key, raw in value.items():
        require(isinstance(key, str), "classification_counts key must be string")
        require(isinstance(raw, int) and raw >= 0, f"classification_counts[{key}] must be non-negative integer")
        out[key] = raw
    return out


def load_blast(path: Path, candidate_id: str, mode: str) -> dict[str, Any]:
    receipt = load_json(path)
    require(receipt.get("schema_version") == BLAST_SCHEMA, f"{path}: unexpected schema_version")
    operation = receipt.get("operation")
    require(isinstance(operation, dict), f"{path}: operation missing")
    require(operation.get("candidate_id") == candidate_id, f"{path}: candidate_id mismatch")
    require(operation.get("kind") == f"publisher-tlb-property-{mode}", f"{path}: operation.kind mismatch")

    cfb = receipt.get("cfb")
    require(isinstance(cfb, dict), f"{path}: cfb missing")
    for key in (
        "control_mutation_stream_delta",
        "control_mutation_topology_delta",
        "control_mutation_byte_ranges",
    ):
        require(isinstance(cfb.get(key), list), f"{path}: cfb.{key} must be array")

    invariants = receipt.get("invariants")
    require(isinstance(invariants, dict), f"{path}: invariants missing")
    for key in (
        "raw_byte_inequality_is_not_semantic_evidence",
        "matched_noop_control_used",
        "unexplained_collateral_preserved",
        "public_receipt_contains_raw_document_bytes",
        "native_pub_writer_capability_granted",
    ):
        require(key in invariants, f"{path}: invariant {key} missing")
    require(invariants["raw_byte_inequality_is_not_semantic_evidence"] is True, f"{path}: raw-byte invariant false")
    require(invariants["matched_noop_control_used"] is True, f"{path}: matched-noop invariant false")
    require(invariants["unexplained_collateral_preserved"] is True, f"{path}: collateral invariant false")
    require(invariants["public_receipt_contains_raw_document_bytes"] is False, f"{path}: raw bytes unexpectedly public")
    require(invariants["native_pub_writer_capability_granted"] is False, f"{path}: writer capability unexpectedly granted")

    return {
        "path": str(path),
        "changed_stream_count": len(cfb["control_mutation_stream_delta"]),
        "topology_delta_count": len(cfb["control_mutation_topology_delta"]),
        "changed_range_count": len(cfb["control_mutation_byte_ranges"]),
        "classification_counts": normalized_counts(receipt.get("classification_counts")),
        "second_save_convergence": (
            receipt.get("second_save_convergence", {}).get("status")
            if isinstance(receipt.get("second_save_convergence"), dict)
            else None
        ),
    }


def arm_ok(candidate: dict[str, Any], arm: str) -> bool:
    value = candidate.get(arm)
    return isinstance(value, dict) and value.get("status") == "ok"


def comparison(candidate: dict[str, Any], key: str) -> dict[str, Any]:
    root = candidate.get("comparison")
    require(isinstance(root, dict), "candidate.comparison missing")
    value = root.get(key)
    require(isinstance(value, dict), f"candidate.comparison.{key} missing")
    for field in ("semantic_equal", "persistence_equal", "render_equal"):
        require(isinstance(value.get(field), bool), f"candidate.comparison.{key}.{field} must be bool")
    require(isinstance(value.get("changed_streams"), list), f"candidate.comparison.{key}.changed_streams must be array")
    return value


def next_gate(
    classification: str,
    same_cmp: dict[str, Any],
    changed_cmp: dict[str, Any],
    same_blast: dict[str, Any],
    changed_blast: dict[str, Any],
) -> str:
    if classification.startswith("inconclusive-"):
        return "fix-or-explain-arm-failure"
    if classification == "setter-no-runtime-change":
        return "record-negative-runtime-observation"
    if classification == "runtime-only-or-normalized-away":
        return "record-runtime-only-or-normalization-observation"
    if classification in {
        "persisted-semantic-change",
        "same-value-materialization-plus-persisted-change",
        "persisted-change-with-same-value-side-effect",
        "persistence-only-or-normalization",
        "semantic-change-without-logical-stream-delta",
        "mixed-or-render-derived-divergence",
    }:
        if (
            same_blast["changed_range_count"] > 0
            or not same_cmp["persistence_equal"]
            or not same_cmp["semantic_equal"]
            or not same_cmp["render_equal"]
        ):
            return "separate-same-value-materialization-from-changed-value-carrier"
        if (
            changed_blast["classification_counts"].get("unexplained_collateral", 0) > 0
            or changed_blast["changed_range_count"] > 0
            or changed_cmp["changed_streams"]
        ):
            return "candidate-specific-carrier-attribution"
        return "review-semantic-and-render-delta"
    return "manual-review"


def summarize_candidate(
    candidate: dict[str, Any],
    blast_dir: Path,
) -> dict[str, Any]:
    spec = candidate.get("candidate")
    require(isinstance(spec, dict), "candidate metadata missing")
    candidate_id = spec.get("id")
    require(isinstance(candidate_id, str), "candidate.id missing")
    require(candidate_id in EXPECTED_CANDIDATES, f"unexpected candidate {candidate_id}")

    classification = candidate.get("classification")
    require(classification in ALLOWED_CLASSIFICATIONS, f"{candidate_id}: unexpected classification {classification}")

    same_cmp = comparison(candidate, "same_vs_control")
    changed_cmp = comparison(candidate, "changed_vs_control")

    candidate_slug = slug(candidate_id)
    same_path = blast_dir / f"{candidate_slug}-same_value.json"
    changed_path = blast_dir / f"{candidate_slug}-changed_value.json"
    require(same_path.is_file(), f"{candidate_id}: missing blast receipt {same_path}")
    require(changed_path.is_file(), f"{candidate_id}: missing blast receipt {changed_path}")

    same_blast = load_blast(same_path, candidate_id, "same_value")
    changed_blast = load_blast(changed_path, candidate_id, "changed_value")

    embedded = candidate.get("blast_radius")
    require(isinstance(embedded, dict), f"{candidate_id}: embedded blast_radius missing")
    for mode, loaded in (("same_vs_control", same_blast), ("changed_vs_control", changed_blast)):
        inner = embedded.get(mode)
        require(isinstance(inner, dict), f"{candidate_id}: blast_radius.{mode} missing")
        require(inner.get("changed_stream_count") == loaded["changed_stream_count"], f"{candidate_id}: {mode} changed_stream_count mismatch")
        require(inner.get("topology_delta_count") == loaded["topology_delta_count"], f"{candidate_id}: {mode} topology_delta_count mismatch")
        require(inner.get("changed_range_count") == loaded["changed_range_count"], f"{candidate_id}: {mode} changed_range_count mismatch")
        require(
            normalized_counts(inner.get("classification_counts")) == loaded["classification_counts"],
            f"{candidate_id}: {mode} classification_counts mismatch",
        )

    same_value_side_effect = (
        not arm_ok(candidate, "same_value")
        or not same_cmp["semantic_equal"]
        or not same_cmp["persistence_equal"]
        or not same_cmp["render_equal"]
        or bool(same_cmp["changed_streams"])
        or same_blast["changed_range_count"] > 0
        or same_blast["topology_delta_count"] > 0
    )

    changed_arm = candidate.get("changed_value")
    runtime_changed = bool(changed_arm.get("runtime_changed")) if isinstance(changed_arm, dict) else False

    gate = next_gate(classification, same_cmp, changed_cmp, same_blast, changed_blast)
    evidence_state = "complete"
    if not arm_ok(candidate, "control") or not arm_ok(candidate, "same_value") or not arm_ok(candidate, "changed_value"):
        evidence_state = "arm-error"

    return {
        "candidate_id": candidate_id,
        "classification": classification,
        "evidence_state": evidence_state,
        "runtime_changed": runtime_changed,
        "same_value_side_effect": same_value_side_effect,
        "same_vs_control": {
            "semantic_equal": same_cmp["semantic_equal"],
            "persistence_equal": same_cmp["persistence_equal"],
            "render_equal": same_cmp["render_equal"],
            "changed_streams": same_cmp["changed_streams"],
            "blast": same_blast,
        },
        "changed_vs_control": {
            "semantic_equal": changed_cmp["semantic_equal"],
            "persistence_equal": changed_cmp["persistence_equal"],
            "render_equal": changed_cmp["render_equal"],
            "changed_streams": changed_cmp["changed_streams"],
            "blast": changed_blast,
        },
        "next_gate": gate,
        "pub_law_promoted": False,
    }


def build_summary(analysis: dict[str, Any], blast_dir: Path) -> dict[str, Any]:
    require(analysis.get("schema") == BATCH_SCHEMA, "unexpected batch schema")
    require(analysis.get("experiment_id") == EXPERIMENT_ID, "unexpected experiment_id")
    require(analysis.get("task_id") == TASK_ID, "unexpected task_id")
    require(analysis.get("batch_id") == BATCH_ID, "unexpected batch_id")

    candidates = analysis.get("candidates")
    require(isinstance(candidates, list), "candidates must be an array")
    require(len(candidates) == len(EXPECTED_CANDIDATES), f"expected {len(EXPECTED_CANDIDATES)} candidates, got {len(candidates)}")

    rows = [summarize_candidate(item, blast_dir) for item in candidates]
    ids = [row["candidate_id"] for row in rows]
    require(len(set(ids)) == len(ids), "duplicate candidate ids")
    require(set(ids) == set(EXPECTED_CANDIDATES), "candidate set mismatch")

    classification_counts = Counter(row["classification"] for row in rows)
    gate_counts = Counter(row["next_gate"] for row in rows)
    same_effect_count = sum(1 for row in rows if row["same_value_side_effect"])
    arm_error_count = sum(1 for row in rows if row["evidence_state"] != "complete")
    attribution_count = sum(
        1 for row in rows
        if row["next_gate"] in {
            "candidate-specific-carrier-attribution",
            "separate-same-value-materialization-from-changed-value-carrier",
        }
    )

    if arm_error_count:
        batch_state = "inconclusive-arm-errors"
    elif attribution_count:
        batch_state = "evidence-complete-carrier-attribution-required"
    else:
        batch_state = "evidence-complete-no-auto-promotion"

    return {
        "schema": SUMMARY_SCHEMA,
        "experiment_id": EXPERIMENT_ID,
        "task_id": TASK_ID,
        "batch_id": BATCH_ID,
        "batch_state": batch_state,
        "candidate_count": len(rows),
        "blast_receipt_count": len(rows) * 2,
        "arm_count": len(rows) * 3,
        "classification_counts": dict(sorted(classification_counts.items())),
        "next_gate_counts": dict(sorted(gate_counts.items())),
        "same_value_side_effect_count": same_effect_count,
        "arm_error_count": arm_error_count,
        "candidate_summaries": rows,
        "authority_boundary": (
            "This summary validates and classifies T891 evidence only. "
            "It never promotes a setter, stream, byte range, or inferred carrier to a PUB law. "
            "Any persisted-effect candidate still requires candidate-specific carrier attribution, "
            "review, and ordinary law/constraint promotion gates."
        ),
    }


def main() -> int:
    parser = argparse.ArgumentParser(description="Validate and summarize PUB-T-891 native batch evidence")
    parser.add_argument("--analysis", type=Path, required=True)
    parser.add_argument("--blast-dir", type=Path)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()

    blast_dir = args.blast_dir or (args.analysis.parent / "blast-radius")
    try:
        summary = build_summary(load_json(args.analysis), blast_dir)
    except EvidenceError as exc:
        print(f"t891-evidence-classifier: {exc}", file=sys.stderr)
        return 2

    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps({
        "schema": summary["schema"],
        "batch_state": summary["batch_state"],
        "candidate_count": summary["candidate_count"],
        "blast_receipt_count": summary["blast_receipt_count"],
        "same_value_side_effect_count": summary["same_value_side_effect_count"],
        "classification_counts": summary["classification_counts"],
        "next_gate_counts": summary["next_gate_counts"],
    }, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
