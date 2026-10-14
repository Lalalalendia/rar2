#!/usr/bin/env python3
from __future__ import annotations

import argparse
from collections import Counter
import hashlib
import json
import sys
import zlib
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[3]
TOOLS = ROOT / "tools"
if str(TOOLS) not in sys.path:
    sys.path.insert(0, str(TOOLS))

from operation_blast_radius_v1 import (  # noqa: E402
    BlastRadiusError,
    CFB,
    build_receipt,
    stream_bytes,
)

SCHEMA = "chaptera.ignore-master-auth-01.analysis.v1"
EXPERIMENT_ID = "IGNORE-MASTER-AUTH-01"
MASTER1_ID = 33554695
MASTER2_ID = 33554741


class AnalysisError(RuntimeError):
    pass


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def load_json(path: Path) -> dict[str, Any]:
    value = json.loads(path.read_text(encoding="utf-8-sig"))
    if not isinstance(value, dict):
        raise AnalysisError(f"{path}: expected JSON object")
    return value


def resolve_artifact(output_root: Path, summary: dict[str, Any]) -> Path:
    relative = summary.get("relative_path")
    expected = summary.get("sha256")
    if not isinstance(relative, str) or not relative:
        raise AnalysisError("artifact summary missing relative_path")
    if not isinstance(expected, str) or len(expected) != 64:
        raise AnalysisError(f"{relative}: artifact summary missing sha256")
    path = output_root / Path(relative)
    if not path.is_file():
        raise AnalysisError(f"artifact missing: {path}")
    actual = sha256_bytes(path.read_bytes())
    if actual != expected:
        raise AnalysisError(f"{relative}: SHA mismatch expected={expected} actual={actual}")
    return path


def get_contents(data: bytes) -> bytes:
    cfb = CFB(data)
    matches = [entry for entry in cfb.dirs if entry["type"] == 2 and entry["name"] == "Contents"]
    if len(matches) != 1:
        raise AnalysisError(f"expected exactly one root Contents stream, found {len(matches)}")
    return stream_bytes(cfb, matches[0])


def logical_ranges(left: bytes, right: bytes) -> list[dict[str, Any]]:
    limit = min(len(left), len(right))
    offsets = [i for i in range(limit) if left[i] != right[i]]
    if len(left) != len(right):
        offsets.extend(range(limit, max(len(left), len(right))))
    if not offsets:
        return []

    out: list[dict[str, Any]] = []
    start = prev = offsets[0]
    for offset in offsets[1:]:
        if offset == prev + 1:
            prev = offset
            continue
        out.append(range_row(left, right, start, prev + 1))
        start = prev = offset
    out.append(range_row(left, right, start, prev + 1))
    return out


def range_row(left: bytes, right: bytes, start: int, end: int) -> dict[str, Any]:
    clip_end_left = min(end, len(left))
    clip_end_right = min(end, len(right))
    return {
        "offset": start,
        "length": end - start,
        "before_hex": left[start:clip_end_left].hex(),
        "after_hex": right[start:clip_end_right].hex(),
    }


def scan_oplpd_master_refs(contents: bytes) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    marker = b"\x0d\x68"
    pos = 0
    while True:
        pos = contents.find(marker, pos)
        if pos < 0:
            break
        if pos + 6 <= len(contents):
            raw = contents[pos : pos + 6]
            rows.append(
                {
                    "offset": pos,
                    "raw_hex": raw.hex(),
                    "value_u32": int.from_bytes(raw[2:6], "little"),
                }
            )
        pos += 2
    return rows


def ref_counts(contents: bytes) -> dict[str, int]:
    counts = Counter(row["value_u32"] for row in scan_oplpd_master_refs(contents))
    return {str(key): counts[key] for key in sorted(counts)}


def png_visual_fingerprint(path: Path) -> dict[str, Any]:
    data = path.read_bytes()
    signature = b"\x89PNG\r\n\x1a\n"
    if not data.startswith(signature):
        raise AnalysisError(f"{path}: expected PNG")
    pos = len(signature)
    ihdr = None
    idat_parts: list[bytes] = []
    while pos + 12 <= len(data):
        length = int.from_bytes(data[pos : pos + 4], "big")
        kind = data[pos + 4 : pos + 8]
        payload_start = pos + 8
        payload_end = payload_start + length
        if payload_end + 4 > len(data):
            raise AnalysisError(f"{path}: truncated PNG chunk")
        payload = data[payload_start:payload_end]
        if kind == b"IHDR":
            ihdr = payload
        elif kind == b"IDAT":
            idat_parts.append(payload)
        elif kind == b"IEND":
            break
        pos = payload_end + 4
    if ihdr is None or not idat_parts:
        raise AnalysisError(f"{path}: missing IHDR/IDAT")
    raw_scanlines = zlib.decompress(b"".join(idat_parts))
    return {
        "file_sha256": sha256_bytes(data),
        "visual_payload_sha256": sha256_bytes(ihdr + raw_scanlines),
        "ihdr_hex": ihdr.hex(),
        "decompressed_scanline_bytes": len(raw_scanlines),
    }


def blast_evidence(control_bytes: bytes, arm: str) -> dict[str, Any]:
    cfb = CFB(control_bytes)
    requested_streams = [
        f"dir:{entry['i']}:{entry['name']}"
        for entry in cfb.dirs
        if entry["type"] == 2 and entry["name"] == "Contents"
    ]
    return {
        "operation": {
            "kind": "publisher-ignore-master",
            "experiment_id": EXPERIMENT_ID,
            "arm": arm,
        },
        "producer": {
            "publisher_environment": "publisher-2019",
            "experiment_id": EXPERIMENT_ID,
        },
        "requested_streams": requested_streams,
        "arms": {"source": {}, "control": {}, "mutation": {}},
    }


def summarize_receipt(receipt: dict[str, Any]) -> dict[str, Any]:
    changed_streams = [
        {
            "stream_id": item["stream_id"],
            "before_size": item["before_size"],
            "after_size": item["after_size"],
            "classification": item["classification"],
        }
        for item in receipt["cfb"]["control_mutation_stream_delta"]
    ]
    return {
        "changed_stream_count": len(changed_streams),
        "changed_streams": changed_streams,
        "topology_delta_count": len(receipt["cfb"]["control_mutation_topology_delta"]),
        "physical_byte_range_count": len(receipt["cfb"]["control_mutation_byte_ranges"]),
        "classification_counts": receipt["classification_counts"],
    }


def stage(native: dict[str, Any], *keys: str) -> dict[str, Any]:
    value: Any = native
    for key in keys:
        value = value[key]
    if not isinstance(value, dict):
        raise AnalysisError("stage is not an object: " + ".".join(keys))
    return value


def analyze(output_root: Path) -> dict[str, Any]:
    native_path = output_root / "analysis" / "ignore-master-auth-01.json"
    native = load_json(native_path)
    if native.get("experiment_id") != EXPERIMENT_ID:
        raise AnalysisError(f"unexpected experiment id {native.get('experiment_id')!r}")

    seed_summary = stage(native, "seed", "output")
    control_summary = stage(native, "arms", "control", "output")
    seed_path = resolve_artifact(output_root, seed_summary)
    control_path = resolve_artifact(output_root, control_summary)
    seed_bytes = seed_path.read_bytes()
    control_bytes = control_path.read_bytes()
    control_contents = get_contents(control_bytes)

    mutation_stages: dict[str, dict[str, Any]] = {
        "ignore_true": stage(native, "arms", "ignore_true"),
        "true_false_before_save": stage(native, "arms", "true_false_before_save"),
        "reversible_true": stage(native, "arms", "reversible", "after_true"),
        "reversible_false": stage(native, "arms", "reversible", "final_false"),
        "rebind_while_true": stage(native, "arms", "rebind_while_true"),
    }

    detail_dir = output_root / "private" / "ignore-master-auth-01" / "blast-radius"
    detail_dir.mkdir(parents=True, exist_ok=True)

    arms: dict[str, Any] = {}
    contents_counts = {"control": ref_counts(control_contents)}
    for name, value in mutation_stages.items():
        output_summary = stage(value, "output")
        mutation_path = resolve_artifact(output_root, output_summary)
        mutation_bytes = mutation_path.read_bytes()
        mutation_contents = get_contents(mutation_bytes)
        receipt = build_receipt(
            seed_bytes,
            control_bytes,
            mutation_bytes,
            evidence=blast_evidence(control_bytes, name),
        )
        (detail_dir / f"{name}.json").write_text(
            json.dumps(receipt, indent=2, sort_keys=True) + "\n",
            encoding="utf-8",
        )
        counts = ref_counts(mutation_contents)
        contents_counts[name] = counts
        arms[name] = {
            "output_sha256": output_summary["sha256"],
            "blast_radius": summarize_receipt(receipt),
            "contents_logical_changed_ranges": logical_ranges(control_contents, mutation_contents),
            "oplpd_0x0d_master_refs": scan_oplpd_master_refs(mutation_contents),
            "oplpd_0x0d_master_ref_counts": counts,
        }

    render_stages: dict[str, dict[str, Any]] = {
        "seed": stage(native, "seed", "fresh_reopen", "renders", "page_a"),
        "control": stage(native, "arms", "control", "fresh_reopen", "renders", "page_a"),
        "ignore_true": stage(native, "arms", "ignore_true", "fresh_reopen", "renders", "page_a"),
        "true_false_before_save": stage(native, "arms", "true_false_before_save", "fresh_reopen", "renders", "page_a"),
        "reversible_true": stage(native, "arms", "reversible", "after_true", "fresh_reopen", "renders", "page_a"),
        "reversible_false": stage(native, "arms", "reversible", "final_false", "fresh_reopen", "renders", "page_a"),
        "rebind_while_true": stage(native, "arms", "rebind_while_true", "fresh_reopen", "renders", "page_a"),
    }
    renders: dict[str, Any] = {}
    for name, summary in render_stages.items():
        path = resolve_artifact(output_root, summary)
        renders[name] = png_visual_fingerprint(path)

    visual = {name: value["visual_payload_sha256"] for name, value in renders.items()}
    visual_checks = {
        "seed_matches_control": visual["seed"] == visual["control"],
        "ignore_true_differs_from_control": visual["ignore_true"] != visual["control"],
        "true_false_before_save_matches_control": visual["true_false_before_save"] == visual["control"],
        "reversible_true_matches_ignore_true": visual["reversible_true"] == visual["ignore_true"],
        "reversible_false_matches_control": visual["reversible_false"] == visual["control"],
        "rebind_while_true_matches_ignore_true": visual["rebind_while_true"] == visual["ignore_true"],
    }

    control_counts = Counter({int(k): v for k, v in contents_counts["control"].items()})
    ignore_counts = Counter({int(k): v for k, v in contents_counts["ignore_true"].items()})
    rebind_counts = Counter({int(k): v for k, v in contents_counts["rebind_while_true"].items()})
    raw_checks = {
        "ignore_true_preserves_0x0d_master_ref_multiset": ignore_counts == control_counts,
        "true_false_preserves_0x0d_master_ref_multiset": contents_counts["true_false_before_save"] == contents_counts["control"],
        "reversible_true_preserves_0x0d_master_ref_multiset": contents_counts["reversible_true"] == contents_counts["control"],
        "reversible_false_preserves_0x0d_master_ref_multiset": contents_counts["reversible_false"] == contents_counts["control"],
        "rebind_while_true_moves_one_0x0d_ref_m1_to_m2": (
            rebind_counts[MASTER1_ID] == control_counts[MASTER1_ID] - 1
            and rebind_counts[MASTER2_ID] == control_counts[MASTER2_ID] + 1
            and sum(rebind_counts.values()) == sum(control_counts.values())
        ),
        "ignore_true_changes_contents_logically": bool(arms["ignore_true"]["contents_logical_changed_ranges"]),
    }

    native_checks = native.get("checks")
    if not isinstance(native_checks, dict):
        raise AnalysisError("native checks missing")

    projection_checks = {
        "control_master_one_visible": bool(native_checks.get("control_master_one_visible")),
        "control_master_two_visible": bool(native_checks.get("control_master_two_visible")),
        "control_page_local_visible": bool(native_checks.get("control_page_local_visible")),
        "ignore_true_master_one_suppressed": bool(native_checks.get("ignore_true_master_one_suppressed")),
        "ignore_true_master_two_suppressed": bool(native_checks.get("ignore_true_master_two_suppressed")),
        "ignore_true_page_local_visible": bool(native_checks.get("ignore_true_page_local_visible")),
        "true_false_master_projection_restored": bool(native_checks.get("true_false_master_projection_restored")),
        "reversible_true_master_projection_suppressed": bool(native_checks.get("reversible_true_master_projection_suppressed")),
        "reversible_false_master_projection_restored": bool(native_checks.get("reversible_false_master_projection_restored")),
        "rebind_while_true_page_local_visible": bool(native_checks.get("rebind_while_true_page_local_visible")),
    }

    law71_confirmed = all(
        [
            bool(native_checks.get("control_false_and_master_1")),
            bool(native_checks.get("ignore_true_persists")),
            bool(native_checks.get("ignore_true_preserves_master_1_binding")),
            bool(native_checks.get("true_false_before_save_returns_false")),
            bool(native_checks.get("reversible_true_stage_persists")),
            bool(native_checks.get("reversible_final_false_persists")),
            bool(native_checks.get("rebind_while_true_keeps_ignore_true")),
            bool(native_checks.get("rebind_while_true_persists_master_2")),
            bool(native_checks.get("tagged_master_and_page_local_shape_identity_stable")),
            all(projection_checks.values()),
            all(visual_checks.values()),
            raw_checks["ignore_true_preserves_0x0d_master_ref_multiset"],
            raw_checks["rebind_while_true_moves_one_0x0d_ref_m1_to_m2"],
            raw_checks["ignore_true_changes_contents_logically"],
        ]
    )

    classification = (
        "law71-independent-page-local-projection-suppression-confirmed"
        if law71_confirmed
        else "inconclusive-or-candidate-falsified"
    )

    result = {
        "schema": SCHEMA,
        "experiment_id": EXPERIMENT_ID,
        "native_receipt_sha256": sha256_bytes(native_path.read_bytes()),
        "source_fixture_sha256": native["fixture"]["expected_sha256"],
        "matched_control": {
            "seed_sha256": seed_summary["sha256"],
            "control_sha256": control_summary["sha256"],
        },
        "renders": renders,
        "visual_checks": visual_checks,
        "projection_region_checks": projection_checks,
        "raw_persistence_checks": raw_checks,
        "oplpd_0x0d_master_ref_counts": contents_counts,
        "arms": arms,
        "native_checks": native_checks,
        "classification": classification,
        "law71_confirmed": law71_confirmed,
        "carrier_boundary": (
            "OplPd 0x0D is checked only as the already-grounded applied-master relation. "
            "IgnoreMaster exact field identity is not guessed from byte inequality: matched-control "
            "Contents logical ranges localize the causal persistence delta for a later named-field decode."
        ),
        "scope_boundary": native["boundary"],
    }
    out = output_root / "analysis" / "ignore-master-auth-01-blast-radius.json"
    out.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    return result


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Analyze PUB-T-828 IgnoreMaster COM, render and matched-control persistence evidence"
    )
    parser.add_argument("--output-root", required=True, type=Path)
    args = parser.parse_args()
    try:
        result = analyze(args.output_root)
    except (OSError, ValueError, KeyError, json.JSONDecodeError, zlib.error, BlastRadiusError, AnalysisError) as error:
        print(f"ignore-master-analysis: {error}", file=sys.stderr)
        return 2
    print(
        json.dumps(
            {
                "schema": result["schema"],
                "law71_confirmed": result["law71_confirmed"],
                "classification": result["classification"],
                "visual_checks": result["visual_checks"],
                "projection_region_checks": result["projection_region_checks"],
                "raw_persistence_checks": result["raw_persistence_checks"],
            },
            sort_keys=True,
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
