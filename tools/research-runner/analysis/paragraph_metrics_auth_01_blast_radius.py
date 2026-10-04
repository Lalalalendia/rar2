#!/usr/bin/env python3
from __future__ import annotations

import argparse
import hashlib
import json
import sys
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[3]
TOOLS = ROOT / "tools"
if str(TOOLS) not in sys.path:
    sys.path.insert(0, str(TOOLS))

from operation_blast_radius_v1 import BlastRadiusError, build_receipt  # noqa: E402

SCHEMA = "chaptera.paragraph-metrics-auth-01.blast-radius.v1"
EXPERIMENT_ID = "PARAGRAPH-METRICS-AUTH-01"


class AnalysisError(RuntimeError):
    pass


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def sha256_file(path: Path) -> str:
    return sha256_bytes(path.read_bytes())


def load_json(path: Path) -> dict[str, Any]:
    value = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(value, dict):
        raise AnalysisError(f"{path}: expected JSON object")
    return value


def without_phase(value: Any) -> Any:
    if isinstance(value, dict):
        return {key: without_phase(item) for key, item in value.items() if key != "phase"}
    if isinstance(value, list):
        return [without_phase(item) for item in value]
    return value


def same_snapshot(arms: list[dict[str, Any]], field: str) -> bool:
    values = [without_phase(arm.get(field)) for arm in arms]
    return bool(values) and all(value == values[0] for value in values[1:])


def verify_artifact(path: Path, expected_sha256: str, label: str) -> None:
    if not path.is_file():
        raise AnalysisError(f"{label}: missing {path}")
    actual = sha256_file(path)
    if actual != expected_sha256:
        raise AnalysisError(f"{label}: SHA mismatch expected={expected_sha256} actual={actual}")


def blast_evidence(arm: dict[str, Any]) -> dict[str, Any]:
    mutation = arm.get("mutation", {})
    if not isinstance(mutation, dict):
        raise AnalysisError(f"{arm.get('arm')}: mutation must be an object")
    return {
        "operation": {
            "kind": "publisher-paragraph-metric",
            "experiment_id": EXPERIMENT_ID,
            "arm": arm.get("arm"),
            "mutation_kind": mutation.get("kind"),
            "requested_value": mutation.get("requested_value"),
        },
        "producer": {
            "publisher_environment": "publisher-2019",
            "experiment_id": EXPERIMENT_ID,
        },
        "arms": {
            "source": {},
            "control": {},
            "mutation": {},
        },
    }


def summarize_blast(receipt: dict[str, Any], arm: dict[str, Any]) -> dict[str, Any]:
    cfb = receipt["cfb"]
    changed_streams = []
    for item in cfb["control_mutation_stream_delta"]:
        changed_streams.append(
            {
                "stream_id": item["stream_id"],
                "before_size": item["before_size"],
                "after_size": item["after_size"],
                "classification": item["classification"],
            }
        )
    changed_streams.sort(key=lambda item: item["stream_id"])
    mutation_kind = str(arm.get("mutation", {}).get("kind", ""))
    is_line_spacing = mutation_kind.startswith("line-")
    return {
        "arm": arm["arm"],
        "mutation": arm["mutation"],
        "output_sha256": arm["output"]["sha256"],
        "changed_stream_count": len(changed_streams),
        "changed_streams": changed_streams,
        "topology_delta_count": len(cfb["control_mutation_topology_delta"]),
        "physical_byte_range_count": len(cfb["control_mutation_byte_ranges"]),
        "classification_counts": receipt["classification_counts"],
        "second_save_convergence": receipt["second_save_convergence"]["status"],
        "raw_fdpp_0x34_status": "not_decoded" if is_line_spacing else "not_applicable",
        "quill_text_invariance_status": "not_structurally_proven",
    }


def analyze(output_root: Path) -> dict[str, Any]:
    analysis_dir = output_root / "analysis"
    private_root = output_root / "private" / "paragraph-metrics-auth-01"
    native_path = analysis_dir / "paragraph-metrics-auth-01.json"
    native = load_json(native_path)

    if native.get("experiment_id") != EXPERIMENT_ID:
        raise AnalysisError(f"unexpected experiment id {native.get('experiment_id')!r}")
    seed = native.get("seed")
    if not isinstance(seed, dict) or not isinstance(seed.get("sha256"), str):
        raise AnalysisError("native receipt has no common seed; rerun the causal-seed revision before blast-radius analysis")

    arms = native.get("arms")
    if not isinstance(arms, list) or not arms:
        raise AnalysisError("native receipt arms missing")
    by_name = {arm.get("arm"): arm for arm in arms if isinstance(arm, dict)}
    if len(by_name) != len(arms):
        raise AnalysisError("native receipt arm names are missing or duplicated")
    if "control" not in by_name:
        raise AnalysisError("matched control arm missing")

    seed_path = private_root / "seed" / "seed.pub"
    control_path = private_root / "control" / "output.pub"
    verify_artifact(seed_path, seed["sha256"], "seed")
    verify_artifact(control_path, by_name["control"]["output"]["sha256"], "control")

    seed_bytes = seed_path.read_bytes()
    control_bytes = control_path.read_bytes()
    detail_dir = private_root / "blast-radius"
    detail_dir.mkdir(parents=True, exist_ok=True)

    summaries: list[dict[str, Any]] = []
    for name in sorted(key for key in by_name if key != "control"):
        arm = by_name[name]
        output_path = private_root / name / "output.pub"
        verify_artifact(output_path, arm["output"]["sha256"], name)
        try:
            receipt = build_receipt(
                seed_bytes,
                control_bytes,
                output_path.read_bytes(),
                evidence=blast_evidence(arm),
            )
        except (BlastRadiusError, OSError, ValueError) as error:
            raise AnalysisError(f"{name}: OperationBlastRadiusV1 failed: {error}") from error
        (detail_dir / f"{name}.json").write_text(
            json.dumps(receipt, indent=2, sort_keys=True) + "\n",
            encoding="utf-8",
        )
        summaries.append(summarize_blast(receipt, arm))

    reopened_lengths = [
        arm.get("frame_fresh_reopen", {}).get("text_length")
        for arm in arms
        if isinstance(arm.get("frame_fresh_reopen"), dict)
    ]
    text_length_invariant = (
        len(reopened_lengths) == len(arms)
        and len(set(json.dumps(value, sort_keys=True) for value in reopened_lengths)) == 1
    )

    result = {
        "schema": SCHEMA,
        "experiment_id": EXPERIMENT_ID,
        "native_receipt_sha256": sha256_file(native_path),
        "seed": {
            "sha256": seed["sha256"],
            "size": seed.get("size"),
        },
        "control": {
            "sha256": by_name["control"]["output"]["sha256"],
            "size": by_name["control"]["output"].get("size"),
        },
        "causal_baseline": {
            "common_seed_used": True,
            "matched_noop_control_used": True,
            "paragraph_before_mutation_identical_across_arms": same_snapshot(arms, "before_mutation"),
            "frame_before_mutation_identical_across_arms": same_snapshot(arms, "frame_before_mutation"),
            "fresh_reopen_text_length_invariant_across_arms": text_length_invariant,
        },
        "arms": summaries,
        "remaining_structural_gap": {
            "raw_fdpp_property_decode": "required",
            "line_spacing_field_candidate": "0x34",
            "line_spacing_tag_hypothesis": "bit0_absolute_points_bit1_proportional_sp_candidate_only",
            "quill_text_byte_invariance": "required",
            "stsh_localization": "required_if_blast_radius_or_raw_fdpp_does_not_localize_property",
        },
        "boundary": (
            "OperationBlastRadiusV1 localizes causal CFB/stream/byte deltas after matched no-op subtraction. "
            "It does not assign paragraph-property semantics. Raw FDPP 0x34 values and Quill TEXT byte invariance "
            "must be established by a structured Quill snapshot before promotion."
        ),
    }
    out = analysis_dir / "paragraph-metrics-auth-01-blast-radius.json"
    out.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    return result


def self_test() -> None:
    arms = [
        {"arm": "control", "before_mutation": {"phase": "a", "x": 1}, "frame_before_mutation": {"phase": "a", "x": 2}},
        {"arm": "mut", "before_mutation": {"phase": "b", "x": 1}, "frame_before_mutation": {"phase": "b", "x": 2}},
    ]
    assert same_snapshot(arms, "before_mutation")
    assert same_snapshot(arms, "frame_before_mutation")
    fake = {
        "cfb": {
            "control_mutation_stream_delta": [
                {
                    "stream_id": "dir:7:CONTENTS",
                    "before_size": 10,
                    "after_size": 11,
                    "classification": "unexplained_collateral",
                }
            ],
            "control_mutation_topology_delta": [],
            "control_mutation_byte_ranges": [{}, {}],
        },
        "classification_counts": {
            "expected_derived": 0,
            "requested_semantic": 0,
            "save_normalization": 0,
            "unavailable": 0,
            "unexplained_collateral": 3,
        },
        "second_save_convergence": {"status": "unavailable"},
    }
    arm = {
        "arm": "line-exact-24pt",
        "mutation": {"kind": "line-exact", "requested_value": 24.0},
        "output": {"sha256": "0" * 64},
    }
    summary = summarize_blast(fake, arm)
    assert summary["changed_stream_count"] == 1
    assert summary["raw_fdpp_0x34_status"] == "not_decoded"
    assert summary["physical_byte_range_count"] == 2
    print("paragraph-metrics blast-radius self-test ok")


def main() -> int:
    parser = argparse.ArgumentParser(description="Normalize PUB-T-823 control/mutation outputs with OperationBlastRadiusV1")
    parser.add_argument("--output-root", type=Path)
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        self_test()
        return 0
    if args.output_root is None:
        parser.error("--output-root is required unless --self-test is used")
    try:
        result = analyze(args.output_root)
    except (OSError, json.JSONDecodeError, AnalysisError) as error:
        print(f"paragraph-metrics-analysis: {error}", file=sys.stderr)
        return 2
    print(
        json.dumps(
            {
                "schema": result["schema"],
                "arms": len(result["arms"]),
                "causal_baseline": result["causal_baseline"],
                "remaining_structural_gap": result["remaining_structural_gap"],
            },
            sort_keys=True,
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
