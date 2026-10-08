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

def require(condition: bool, message: str) -> None:
    if not condition:
        raise EvidenceError(message)

def load_json(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8-sig"))
    except (OSError, json.JSONDecodeError) as exc:
        raise EvidenceError(f"{path}: cannot read JSON: {exc}") from exc
    require(isinstance(value, dict), f"{path}: expected JSON object")
    return value

def slug(value: str) -> str:
    chars: list[str] = []
    dash = False
    for char in value:
        if char.isalnum():
            chars.append(char.lower())
            dash = False
        elif not dash:
            chars.append("-")
            dash = True
    return "".join(chars).strip("-")

def normalized_counts(value: Any) -> dict[str, int]:
    require(isinstance(value, dict), "classification_counts must be an object")
    out: dict[str, int] = {}
    for key, raw in value.items():
        require(isinstance(key, str), "classification_counts key must be a string")
        require(isinstance(raw, int) and raw >= 0, f"classification_counts[{key}] must be a non-negative integer")
        out[key] = raw
    return out

def load_blast(path: Path, candidate_id: str, mode: str) -> dict[str, Any]:
    receipt = load_json(path)
    require(receipt.get("schema_version") == BLAST_SCHEMA, f"{path}: unexpected schema")
    operation = receipt.get("operation")
    require(isinstance(operation, dict), f"{path}: operation missing")
    require(operation.get("candidate_id") == candidate_id, f"{path}: candidate_id mismatch")
    require(operation.get("kind") == f"publisher-tlb-property-{mode}", f"{path}: operation.kind mismatch")

    cfb = receipt.get("cfb")
    require(isinstance(cfb, dict), f"{path}: cfb missing")
    for key in ("control_mutation_stream_delta", "control_mutation_topology_delta", "control_mutation_byte_ranges"):
        require(isinstance(cfb.get(key), list), f"{path}: cfb.{key} must be an array")

    invariants = receipt.get("invariants")
    require(isinstance(invariants, dict), f"{path}: invariants missing")
    expected_invariants = {
        "raw_byte_inequality_is_not_semantic_evidence": True,
        "matched_noop_control_used": True,
        "unexplained_collateral_preserved": True,
        "public_receipt_contains_raw_document_bytes": False,
        "native_pub_writer_capability_granted": False,
    }
    for key, expected in expected_invariants.items():
        require(invariants.get(key) is expected, f"{path}: invariant {key} mismatch")

    return {
        "receipt_path": f"analysis/blast-radius/{path.name}",
        "changed_stream_count": len(cfb["control_mutation_stream_delta"]),
        "topology_delta_count": len(cfb["control_mutation_topology_delta"]),
        "changed_range_count": len(cfb["control_mutation_byte_ranges"]),
        "classification_counts": normalized_counts(receipt.get("classification_counts")),
    }

def comparison(candidate: dict[str, Any], key: str) -> dict[str, Any]:
    root = candidate.get("comparison")
    require(isinstance(root, dict), "candidate.comparison missing")
    value = root.get(key)
    require(isinstance(value, dict), f"candidate.comparison.{key} missing")
    for field in ("semantic_equal", "persistence_equal", "render_equal"):
        require(isinstance(value.get(field), bool), f"candidate.comparison.{key}.{field} must be bool")
    require(isinstance(value.get("changed_streams"), list), f"candidate.comparison.{key}.changed_streams must be an array")
    return value

def arm_ok(candidate: dict[str, Any], key: str) -> bool:
    arm = candidate.get(key)
    return isinstance(arm, dict) and arm.get("status") == "ok"

def verify_embedded(candidate_id: str, embedded: dict[str, Any], mode: str, loaded: dict[str, Any]) -> None:
    value = embedded.get(mode)
    require(isinstance(value, dict), f"{candidate_id}: blast_radius.{mode} missing")
    for key in ("changed_stream_count", "topology_delta_count", "changed_range_count"):
        require(value.get(key) == loaded[key], f"{candidate_id}: {mode}.{key} mismatch")
    require(
        normalized_counts(value.get("classification_counts")) == loaded["classification_counts"],
        f"{candidate_id}: {mode}.classification_counts mismatch",
    )

def choose_next_gate(
    classification: str,
    same_cmp: dict[str, Any],
    same_blast: dict[str, Any],
    changed_blast: dict[str, Any],
) -> str:
    if classification.startswith("inconclusive-"):
        return "fix-or-explain-arm-failure"
    if classification == "setter-no-runtime-change":
        return "record-negative-runtime-observation"
    if classification == "runtime-only-or-normalized-away":
        return "record-runtime-only-or-normalization-observation"

    same_side_effect = (
        not same_cmp["semantic_equal"]
        or not same_cmp["persistence_equal"]
        or not same_cmp["render_equal"]
        or bool(same_cmp["changed_streams"])
        or same_blast["changed_range_count"] > 0
        or same_blast["topology_delta_count"] > 0
    )
    if same_side_effect:
        return "separate-same-value-materialization-from-changed-value-carrier"

    if (
        changed_blast["changed_stream_count"] > 0
        or changed_blast["changed_range_count"] > 0
        or changed_blast["topology_delta_count"] > 0
    ):
        return "candidate-specific-carrier-attribution"
    return "review-semantic-and-render-delta"

def summarize_candidate(candidate: dict[str, Any], blast_dir: Path) -> dict[str, Any]:
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
    verify_embedded(candidate_id, embedded, "same_vs_control", same_blast)
    verify_embedded(candidate_id, embedded, "changed_vs_control", changed_blast)

    same_side_effect = (
        not arm_ok(candidate, "same_value")
        or not same_cmp["semantic_equal"]
        or not same_cmp["persistence_equal"]
        or not same_cmp["render_equal"]
        or bool(same_cmp["changed_streams"])
        or same_blast["changed_range_count"] > 0
        or same_blast["topology_delta_count"] > 0
    )
    arm_error = not all(arm_ok(candidate, key) for key in ("control", "same_value", "changed_value"))
    changed_arm = candidate.get("changed_value")
    runtime_changed = bool(changed_arm.get("runtime_changed")) if isinstance(changed_arm, dict) else False
    next_gate = choose_next_gate(classification, same_cmp, same_blast, changed_blast)

    return {
        "candidate_id": candidate_id,
        "classification": classification,
        "evidence_state": "arm-error" if arm_error else "complete",
        "runtime_changed": runtime_changed,
        "same_value_side_effect": same_side_effect,
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
        "next_gate": next_gate,
        "pub_law_promoted": False,
    }

def build_summary(analysis: dict[str, Any], blast_dir: Path) -> dict[str, Any]:
    require(analysis.get("schema") == BATCH_SCHEMA, "unexpected batch schema")
    require(analysis.get("experiment_id") == EXPERIMENT_ID, "unexpected experiment_id")
    require(analysis.get("task_id") == TASK_ID, "unexpected task_id")
    require(analysis.get("batch_id") == BATCH_ID, "unexpected batch_id")

    candidates = analysis.get("candidates")
    require(isinstance(candidates, list), "candidates must be an array")
    require(len(candidates) == len(EXPECTED_CANDIDATES), f"expected 9 candidates, got {len(candidates)}")
    rows = [summarize_candidate(item, blast_dir) for item in candidates]
    ids = [row["candidate_id"] for row in rows]
    require(len(set(ids)) == len(ids), "duplicate candidate ids")
    require(set(ids) == set(EXPECTED_CANDIDATES), "candidate set mismatch")

    classification_counts = Counter(row["classification"] for row in rows)
    next_gate_counts = Counter(row["next_gate"] for row in rows)
    arm_error_count = sum(row["evidence_state"] != "complete" for row in rows)
    same_value_side_effect_count = sum(row["same_value_side_effect"] for row in rows)
    attribution_count = sum(
        row["next_gate"] in {
            "candidate-specific-carrier-attribution",
            "separate-same-value-materialization-from-changed-value-carrier",
        }
        for row in rows
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
        "arm_count": len(rows) * 3,
        "blast_receipt_count": len(rows) * 2,
        "arm_error_count": arm_error_count,
        "same_value_side_effect_count": same_value_side_effect_count,
        "classification_counts": dict(sorted(classification_counts.items())),
        "next_gate_counts": dict(sorted(next_gate_counts.items())),
        "candidate_summaries": rows,
        "authority_boundary": (
            "This summary validates and classifies T891 evidence only. "
            "It never promotes a setter, stream, byte range, or inferred carrier to a PUB law. "
            "Persisted-effect candidates still require candidate-specific carrier attribution "
            "and ordinary law/constraint promotion gates."
        ),
    }

def main() -> int:
    parser = argparse.ArgumentParser(description="Validate and summarize PUB-T-891 native evidence")
    parser.add_argument("--analysis", required=True, type=Path)
    parser.add_argument("--blast-dir", type=Path)
    parser.add_argument("--out", required=True, type=Path)
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
        "arm_count": summary["arm_count"],
        "blast_receipt_count": summary["blast_receipt_count"],
        "arm_error_count": summary["arm_error_count"],
        "same_value_side_effect_count": summary["same_value_side_effect_count"],
        "classification_counts": summary["classification_counts"],
        "next_gate_counts": summary["next_gate_counts"],
    }, sort_keys=True))
    return 0

if __name__ == "__main__":
    raise SystemExit(main())
