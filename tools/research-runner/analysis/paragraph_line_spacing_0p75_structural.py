#!/usr/bin/env python3
"""Focused structural join for PARAGRAPH-LINE-SPACING-0P75-NATIVE-01."""

from __future__ import annotations

import argparse
import importlib.util
import json
import math
import sys
from pathlib import Path
from typing import Any

BASE_PATH = Path(__file__).with_name("paragraph_metrics_auth_01_structural.py")
_spec = importlib.util.spec_from_file_location("paragraph_metrics_structural_base", BASE_PATH)
if _spec is None or _spec.loader is None:
    raise RuntimeError("unable to load paragraph metrics structural base")
base = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(base)

EXPERIMENT = "PARAGRAPH-LINE-SPACING-0P75-NATIVE-01"
NATIVE_SCHEMA = "chaptera.paragraph-line-spacing-0p75-native-01.v1"
OUT_SCHEMA = "chaptera.paragraph-line-spacing-0p75-structural.v1"
EXPECTED_ARMS = ("control", "single", "direct-0p75")
EXPECTED_SINGLE_PACKED = 152_400 * 8 + 2
EXPECTED_0P75_PACKED = 114_300 * 8 + 2


def require(condition: bool, label: str) -> None:
    if not condition:
        raise base.StructuralError(label)


def strip_phase(value: Any) -> Any:
    if isinstance(value, dict):
        return {k: strip_phase(v) for k, v in value.items() if k != "phase"}
    if isinstance(value, list):
        return [strip_phase(v) for v in value]
    return value


def safe_number(item: Any, label: str) -> float:
    require(isinstance(item, dict) and item.get("state") == "value", f"{label}: unavailable")
    value = item.get("value")
    require(type(value) in (int, float) and math.isfinite(value), f"{label}: invalid numeric value")
    return float(value)


def geometry(value: Any, label: str) -> dict[str, Any]:
    require(isinstance(value, dict), f"{label}: missing geometry")
    rows = value.get("lines")
    require(isinstance(rows, list) and rows, f"{label}: empty line geometry")
    require(value.get("line_count") == len(rows), f"{label}: line count mismatch")
    normalized = []
    previous = None
    for expected_index, row in enumerate(rows, start=1):
        require(isinstance(row, dict), f"{label}: invalid line row")
        require(row.get("index") == expected_index, f"{label}: line index mismatch")
        start, end = row.get("start"), row.get("end")
        require(type(start) is int and type(end) is int and 0 <= start <= end, f"{label}: invalid line range")
        top = safe_number(row.get("bound_top"), f"{label}: line {expected_index} top")
        height = safe_number(row.get("bound_height"), f"{label}: line {expected_index} height")
        if previous is not None:
            require((start, end) != previous, f"{label}: repeated terminal line")
        previous = (start, end)
        normalized.append(
            {
                "index": expected_index,
                "start": start,
                "end": end,
                "bound_top_points": top,
                "bound_height_points": height,
            }
        )
    tops = [row["bound_top_points"] for row in normalized]
    deltas = [round(tops[i] - tops[i - 1], 9) for i in range(1, len(tops))]
    return {
        "line_count": len(normalized),
        "lines": normalized,
        "bound_top_deltas_points": deltas,
    }


def stsh_identity(snapshot: dict[str, Any]) -> list[tuple[int, int, str]]:
    return [
        (chunk["descriptor_ordinal"], chunk["byte_len"], chunk["sha256"])
        for chunk in snapshot["chunks"]
        if chunk["name"] == "STSH"
    ]


def verify_private(path: Path, expected: dict[str, Any], label: str) -> bytes:
    require(isinstance(expected, dict), f"{label}: missing expected identity")
    require(base.valid_sha(expected.get("sha256")), f"{label}: invalid expected SHA")
    require(base.uint(expected.get("size")), f"{label}: invalid expected size")
    data = path.read_bytes()
    require(base.digest(data) == expected["sha256"], f"{label}: private SHA mismatch")
    require(len(data) == expected["size"], f"{label}: private size mismatch")
    return data


def arm_spacing(arm: dict[str, Any], field: str, label: str) -> float:
    snapshot = arm.get(field)
    require(isinstance(snapshot, dict), f"{label}: missing {field}")
    return safe_number(snapshot.get("line_spacing"), f"{label}: {field}.line_spacing")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output-root", type=Path, required=True)
    parser.add_argument("--snapshot-tool", type=Path, required=True)
    args = parser.parse_args()

    out = args.output_root / "analysis" / "paragraph-line-spacing-0p75-structural.json"
    try:
        out.unlink(missing_ok=True)
        native_path = args.output_root / "analysis" / "paragraph-line-spacing-0p75-native-01.json"
        native = json.loads(native_path.read_text(encoding="utf-8-sig"))
        require(isinstance(native, dict), "native receipt must be an object")
        require(native.get("schema") == NATIVE_SCHEMA, "native schema mismatch")
        require(native.get("experiment_id") == EXPERIMENT, "native experiment mismatch")
        require(
            native.get("verdict") == "native-roundtrip-and-line-geometry-captured-not-yet-carrier-authority",
            "native geometry revision required",
        )

        arms = native.get("arms")
        require(isinstance(arms, list) and len(arms) == len(EXPECTED_ARMS), "three-arm matrix required")
        by_name = {}
        for arm in arms:
            require(isinstance(arm, dict), "invalid native arm")
            name = arm.get("arm")
            require(name in EXPECTED_ARMS and name not in by_name, "unexpected or duplicate arm")
            by_name[name] = arm
        require(tuple(sorted(by_name)) == tuple(sorted(EXPECTED_ARMS)), "missing native arm")

        before_paragraph = [strip_phase(by_name[name].get("before_mutation")) for name in EXPECTED_ARMS]
        before_geometry = [strip_phase(by_name[name].get("line_geometry_before_mutation")) for name in EXPECTED_ARMS]
        require(all(value == before_paragraph[0] for value in before_paragraph[1:]), "pre-mutation paragraph snapshots differ")
        require(all(value == before_geometry[0] for value in before_geometry[1:]), "pre-mutation line geometry differs")

        private = args.output_root / "private" / "paragraph-line-spacing-0p75-native-01"
        paths = {
            "seed": private / "seed.pub",
            "control": private / "control" / "output.pub",
            "single": private / "single" / "output.pub",
            "direct-0p75": private / "direct-0p75" / "output.pub",
        }
        expected = {
            "seed": native.get("seed"),
            **{name: by_name[name].get("output") for name in EXPECTED_ARMS},
        }

        snapshots: dict[str, dict[str, Any]] = {}
        for name, path in paths.items():
            data = verify_private(path, expected[name], name)
            observation = base.run_probe(args.snapshot_tool, path)
            base.validate_snapshot(observation, name)
            require(observation["source"]["sha256"] == base.digest(data), f"{name}: snapshot source SHA mismatch")
            require(observation["source"]["byte_len"] == len(data), f"{name}: snapshot source size mismatch")
            require(path.read_bytes() == data, f"{name}: input changed during snapshot")
            snapshots[name] = observation

        seed = snapshots["seed"]
        for name, observation in snapshots.items():
            require(observation["text"] == seed["text"], f"{name}: Quill TEXT changed")
            require(observation["stories"] == seed["stories"], f"{name}: Story partition changed")

        control = snapshots["control"]
        control_stsh = stsh_identity(control)
        rows = {}
        for name in EXPECTED_ARMS:
            arm = by_name[name]
            observation = snapshots[name]
            candidates = base.candidates(observation)
            rows[name] = {
                "after_mutation_line_spacing": arm_spacing(arm, "after_mutation", name),
                "fresh_reopen_line_spacing": arm_spacing(arm, "fresh_reopen", name),
                "roundtrip_line_spacing_stable": abs(
                    arm_spacing(arm, "after_mutation", name) - arm_spacing(arm, "fresh_reopen", name)
                ) < 1e-6,
                "fresh_reopen_line_spacing_rule": safe_number(
                    arm["fresh_reopen"].get("line_spacing_rule"),
                    f"{name}: fresh_reopen.line_spacing_rule",
                ),
                "geometry_after_mutation": geometry(arm.get("line_geometry_after_mutation"), f"{name}: after"),
                "geometry_fresh_reopen": geometry(arm.get("line_geometry_fresh_reopen"), f"{name}: reopen"),
                "raw_fdpp_0x34": candidates,
                "stsh_unchanged_vs_control": stsh_identity(observation) == control_stsh,
                "control_to_arm_chunk_changes": base.chunk_changes(control, observation),
            }

        control_values = {c["raw_value"] for c in rows["control"]["raw_fdpp_0x34"] if c["raw_value"] is not None}
        single_values = {c["raw_value"] for c in rows["single"]["raw_fdpp_0x34"] if c["raw_value"] is not None}
        p075_values = {c["raw_value"] for c in rows["direct-0p75"]["raw_fdpp_0x34"] if c["raw_value"] is not None}

        p075_spacing = rows["direct-0p75"]["fresh_reopen_line_spacing"]
        carrier_checks = {
            "single_expected_packed_1219202_present": EXPECTED_SINGLE_PACKED in single_values,
            "p075_expected_packed_914402_present": EXPECTED_0P75_PACKED in p075_values,
            "p075_expected_packed_absent_from_control": EXPECTED_0P75_PACKED not in control_values,
            "stsh_unchanged_single": rows["single"]["stsh_unchanged_vs_control"],
            "stsh_unchanged_p075": rows["direct-0p75"]["stsh_unchanged_vs_control"],
            "p075_fresh_reopen_reports_0p75": abs(p075_spacing - 0.75) < 1e-6,
            "p075_roundtrip_stable": rows["direct-0p75"]["roundtrip_line_spacing_stable"],
            "p075_geometry_has_progression": bool(
                rows["direct-0p75"]["geometry_fresh_reopen"]["bound_top_deltas_points"]
            ),
        }
        candidate = all(carrier_checks.values())

        receipt = {
            "schema": OUT_SCHEMA,
            "experiment_id": EXPERIMENT,
            "source_native_receipt_sha256": base.digest(native_path.read_bytes()),
            "expected_packed_values": {
                "single_152400": EXPECTED_SINGLE_PACKED,
                "p075_114300": EXPECTED_0P75_PACKED,
            },
            "invariants": {
                "common_pre_mutation_paragraph_snapshot": True,
                "common_pre_mutation_line_geometry": True,
                "quill_text_byte_invariance_all_arms": True,
                "story_partition_invariance_all_arms": True,
            },
            "arms": rows,
            "carrier_checks": carrier_checks,
            "native_114300_authority_candidate": candidate,
            "product_authority_granted": False,
            "boundary": (
                "A true native_114300_authority_candidate proves the focused Publisher2019 "
                "0.75 roundtrip/carrier/geometry witness only. Product execution still requires "
                "explicit consumer review against the exact082 line-level authority and negative controls."
            ),
        }
        out.write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n", encoding="utf-8")
        print(json.dumps({"native_114300_authority_candidate": candidate, "carrier_checks": carrier_checks}, sort_keys=True))
        return 0
    except (base.StructuralError, OSError, ValueError, TypeError, KeyError, json.JSONDecodeError) as error:
        print(f"0.75 spacing structural analysis failed: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
