#!/usr/bin/env python3
"""Strict, source-free structural join for PUB-T-823; never grants metric semantics."""
from __future__ import annotations

import argparse
import hashlib
import json
import math
import re
import subprocess
import sys
from pathlib import Path
from typing import Any, Callable

SCHEMA = "chaptera.paragraph-metrics-auth-01.structural.v1"
SNAPSHOT_SCHEMA = "chaptera.paragraph-metrics.quill-snapshot.v1"
EXPERIMENT = "PARAGRAPH-METRICS-AUTH-01"
SPECS = {
    "control": ("control", 0.0),
    "line-single": ("line-single", 1.0),
    "line-0p75": ("line-proportional", 0.75),
    "line-1p5": ("line-1p5", 1.5),
    "line-exact-18pt": ("line-exact", 18.0),
    "line-exact-24pt": ("line-exact", 24.0),
    "left-indent-18pt": ("left-indent", 18.0),
    "right-indent-18pt": ("right-indent", 18.0),
    "first-line-18pt": ("first-line-indent", 18.0),
    "hanging-18pt": ("first-line-indent", -18.0),
    "space-before-12pt": ("space-before", 12.0),
    "space-after-12pt": ("space-after", 12.0),
}
METRICS = (
    "alignment", "first_line_indent", "left_indent", "right_indent",
    "space_before", "space_after", "line_spacing", "line_spacing_rule",
)
FRAME = ("left", "top", "width", "height", "overflowing", "text_length")


class StructuralError(RuntimeError):
    pass


def require(condition: bool, label: str) -> None:
    if not condition:
        raise StructuralError(label)


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def valid_sha(value: Any) -> bool:
    return isinstance(value, str) and re.fullmatch(r"[0-9a-f]{64}", value) is not None


def uint(value: Any, maximum: int = 2**64 - 1) -> bool:
    return type(value) is int and 0 <= value <= maximum


def identity(value: Any, label: str) -> None:
    require(isinstance(value, dict), f"{label}: missing identity")
    require(valid_sha(value.get("sha256")), f"{label}: invalid SHA")
    require(uint(value.get("byte_len")), f"{label}: invalid byte length")


def safe_values(value: Any, fields: tuple[str, ...], label: str) -> dict[str, Any]:
    require(isinstance(value, dict), f"{label}: missing COM snapshot")
    out = {}
    for field in fields:
        item = value.get(field)
        require(isinstance(item, dict) and item.get("state") == "value", f"{label}: {field} unavailable")
        v = item.get("value")
        if field == "overflowing":
            require(type(v) is bool, f"{label}: invalid {field}")
        else:
            require(type(v) in (int, float) and math.isfinite(v), f"{label}: invalid {field}")
        out[field] = v
    return out


def validate_snapshot(snapshot: Any, label: str) -> None:
    require(isinstance(snapshot, dict), f"{label}: invalid snapshot")
    require(snapshot.get("schema") == SNAPSHOT_SCHEMA, f"{label}: snapshot schema mismatch")
    require(snapshot.get("authority") == "raw_observation_only", f"{label}: wrong snapshot authority")
    identity(snapshot.get("source"), f"{label}: source")
    identity(snapshot.get("text"), f"{label}: TEXT")
    for field in ("source", "text"):
        require(set(snapshot[field]) == {"sha256", "byte_len"}, f"{label}: unexpected identity payload")
    size = snapshot["text"]["byte_len"]
    require(size > 0 and size % 2 == 0, f"{label}: invalid TEXT extent")
    stories = snapshot.get("stories")
    require(isinstance(stories, list) and bool(stories), f"{label}: no confirmed Stories")
    ids = set()
    total = 0
    for index, story in enumerate(stories):
        require(isinstance(story, dict) and story.get("index") == index, f"{label}: invalid Story order")
        require(set(story) == {"index", "syid", "utf16_len"}, f"{label}: unexpected Story payload")
        require(uint(story.get("syid"), 2**32 - 1) and story["syid"] not in ids, f"{label}: ambiguous Story identity")
        require(uint(story.get("utf16_len"), 2**32 - 1), f"{label}: invalid Story extent")
        ids.add(story["syid"])
        total += story["utf16_len"]
    require(total * 2 == size, f"{label}: Stories do not partition TEXT")
    chunks = snapshot.get("chunks")
    require(isinstance(chunks, list), f"{label}: chunks missing")
    ordinals = set()
    fdpp_ordinals = set()
    fdpp_count = 0
    for chunk in chunks:
        identity(chunk, f"{label}: chunk")
        require(set(chunk) == {"name", "descriptor_ordinal", "byte_len", "sha256"}, f"{label}: unexpected chunk payload")
        require(chunk.get("name") in ("FDPP", "STSH"), f"{label}: unexpected chunk")
        ordinal = chunk.get("descriptor_ordinal")
        require(uint(ordinal) and ordinal not in ordinals, f"{label}: duplicate chunk ordinal")
        ordinals.add(ordinal)
        fdpp_count += chunk["name"] == "FDPP"
        if chunk["name"] == "FDPP":
            fdpp_ordinals.add(ordinal)
    require(fdpp_count > 0, f"{label}: FDPP absent")
    styles = snapshot.get("fdpp_styles")
    require(isinstance(styles, list) and bool(styles), f"{label}: FDPP styles missing")
    previous = 0
    keys = set()
    for style in styles:
        require(isinstance(style, dict) and valid_sha(style.get("sha256")), f"{label}: invalid style identity")
        start, end = style.get("start_utf16"), style.get("end_utf16")
        require(uint(start) and uint(end) and start == previous and start <= end <= total, f"{label}: invalid FDPP coverage")
        descriptor, ordinal = style.get("descriptor_ordinal"), style.get("style_ordinal")
        require(uint(descriptor) and descriptor in fdpp_ordinals and uint(ordinal), f"{label}: invalid style coordinate")
        key = (descriptor, ordinal)
        require(key not in keys, f"{label}: duplicate style coordinate")
        keys.add(key)
        previous = end
        properties = style.get("properties")
        require(isinstance(properties, list), f"{label}: properties missing")
        for prop in properties:
            identity(prop, f"{label}: property")
            require(uint(prop.get("field_id"), 2**16 - 1) and uint(prop.get("wire_type"), 255), f"{label}: invalid property tag")
            tag = prop.get("raw_tag")
            require(isinstance(tag, list) and len(tag) == 2 and all(uint(v, 255) for v in tag), f"{label}: raw tag missing")
            field = tag[0] | ((tag[1] & 7) << 8) if tag[1] & 7 == 2 else tag[0]
            wire = tag[1] & 0xF8 if tag[1] & 7 == 2 else tag[1]
            require((field, wire) == (prop["field_id"], prop["wire_type"]), f"{label}: raw tag mismatch")
            if field in (0x34, 0x234):
                require("raw_value" in prop, f"{label}: candidate observation missing")
                require(prop["raw_value"] is None or uint(prop["raw_value"], 2**32 - 1), f"{label}: invalid candidate scalar")
    require(previous == total, f"{label}: incomplete FDPP coverage")


def candidates(snapshot: dict[str, Any]) -> list[dict[str, Any]]:
    out = []
    for style in snapshot["fdpp_styles"]:
        for ordinal, prop in enumerate(style["properties"]):
            if prop["field_id"] not in (0x34, 0x234):
                continue
            raw = prop["raw_value"]
            out.append({
                "start_utf16": style["start_utf16"], "end_utf16": style["end_utf16"],
                "property_ordinal": ordinal, "field_id": prop["field_id"],
                "raw_tag": prop["raw_tag"], "wire_type": prop["wire_type"],
                "raw_value": raw, "bit0": None if raw is None else raw & 1,
                "bit1": None if raw is None else (raw >> 1) & 1,
                "sha256": prop["sha256"],
            })
    return out


def chunk_changes(before: dict[str, Any], after: dict[str, Any]) -> list[dict[str, Any]]:
    def index(snapshot: dict[str, Any]) -> dict:
        return {(c["name"], c["descriptor_ordinal"]): c for c in snapshot["chunks"]}
    a, b = index(before), index(after)
    return [
        {"name": key[0], "descriptor_ordinal": key[1], "before": a.get(key), "after": b.get(key)}
        for key in sorted(set(a) | set(b)) if a.get(key) != b.get(key)
    ]


def run_probe(tool: Path, path: Path) -> dict[str, Any]:
    try:
        result = subprocess.run([str(tool.resolve()), str(path)], capture_output=True, text=True, timeout=60, check=False)
        require(result.returncode == 0, "strict Quill probe rejected input")
        return json.loads(result.stdout)
    except (OSError, subprocess.TimeoutExpired, json.JSONDecodeError) as error:
        raise StructuralError("Quill snapshot producer unavailable or invalid") from error


def analyze(output_root: Path, snapshot_reader: Callable[[Path], dict[str, Any]]) -> dict[str, Any]:
    native_path = output_root / "analysis" / "paragraph-metrics-auth-01.json"
    native_bytes = native_path.read_bytes()
    native = json.loads(native_bytes)
    require(isinstance(native, dict), "native receipt must be an object")
    require(native.get("schema") == "chaptera.paragraph-metrics-auth-01.native.v1", "native schema mismatch")
    require(native.get("experiment_id") == EXPERIMENT, "native experiment mismatch")
    require(native.get("verdict") == "native-semantic-arms-captured-with-common-seed", "causal-seed native revision required")
    arms = native.get("arms")
    require(isinstance(arms, list) and len(arms) == len(SPECS), "complete declared-arm matrix required")
    by_name = {}
    for arm in arms:
        require(isinstance(arm, dict), "invalid native arm")
        name = arm.get("arm")
        require(isinstance(name, str) and name in SPECS and name not in by_name, "unexpected or duplicate native arm")
        mutation = arm.get("mutation")
        kind, requested = SPECS[name]
        require(isinstance(mutation, dict) and mutation.get("kind") == kind and type(mutation.get("requested_value")) in (int, float) and mutation["requested_value"] == requested, f"{name}: wrong mutation")
        by_name[name] = arm
    require(set(by_name) == set(SPECS), "missing native arm")
    baselines, frames = [], []
    for name, arm in by_name.items():
        baselines.append(safe_values(arm.get("before_mutation"), METRICS, name))
        frames.append(safe_values(arm.get("frame_before_mutation"), FRAME, name))
        require(frames[-1]["width"] > 0 and frames[-1]["height"] > 0 and uint(frames[-1]["text_length"]) and frames[-1]["text_length"] > 0, f"{name}: invalid frame extent")
        safe_values(arm.get("after_mutation"), METRICS, name)
        safe_values(arm.get("frame_after_mutation"), FRAME, name)
        safe_values(arm.get("fresh_reopen"), METRICS, name)
        safe_values(arm.get("frame_fresh_reopen"), FRAME, name)
    require(all(v == baselines[0] for v in baselines), "pre-mutation paragraph baselines differ")
    require(all(v == frames[0] for v in frames), "pre-mutation frame baselines differ")
    private = output_root / "private" / "paragraph-metrics-auth-01"
    snapshots = {}
    for name in ["seed", *SPECS]:
        path = private / name / ("seed.pub" if name == "seed" else "output.pub")
        expected = native.get("seed") if name == "seed" else by_name[name].get("output")
        require(isinstance(expected, dict) and valid_sha(expected.get("sha256")) and uint(expected.get("size")), f"{name}: missing native file identity")
        data = path.read_bytes()
        actual = {"sha256": digest(data), "byte_len": len(data)}
        require((actual["sha256"], actual["byte_len"]) == (expected["sha256"], expected["size"]), f"{name}: private artifact identity mismatch")
        observation = snapshot_reader(path)
        validate_snapshot(observation, name)
        require(observation["source"] == actual, f"{name}: snapshot source identity mismatch")
        # Recheck after probe execution, before accepting the observation.
        require(path.read_bytes() == data, f"{name}: input changed during snapshot")
        snapshots[name] = observation
    seed, control = snapshots["seed"], snapshots["control"]
    for name, observation in snapshots.items():
        require(observation["text"] == seed["text"], f"{name}: Quill TEXT bytes changed")
        require(observation["stories"] == seed["stories"], f"{name}: confirmed Story partition changed")
    rows = []
    for name, arm in by_name.items():
        observation = snapshots[name]
        rows.append({
            "arm": name, "source": observation["source"],
            "mutation": {"kind": SPECS[name][0], "requested_value": SPECS[name][1]},
            "after_mutation_metrics": safe_values(arm["after_mutation"], METRICS, name),
            "reopened_metrics": safe_values(arm["fresh_reopen"], METRICS, name),
            "metrics_unchanged_across_save_reopen": safe_values(arm["after_mutation"], METRICS, name) == safe_values(arm["fresh_reopen"], METRICS, name),
            "reopened_frame": safe_values(arm["frame_fresh_reopen"], FRAME, name),
            "quill_text_byte_invariance": True,
            "raw_fdpp_0x34": candidates(observation),
            "fdpp_range_partition_unchanged": [
                (s["start_utf16"], s["end_utf16"]) for s in control["fdpp_styles"]
            ] == [(s["start_utf16"], s["end_utf16"]) for s in observation["fdpp_styles"]],
            "control_to_arm_chunk_changes": chunk_changes(control, observation),
        })
    return {
        "schema": SCHEMA, "experiment_id": EXPERIMENT,
        "native_receipt_sha256": digest(native_bytes),
        "seed": seed["source"], "control": control["source"], "text": seed["text"],
        "seed_to_control_chunk_changes": chunk_changes(seed, control),
        "arms": rows,
        "invariants": {
            "complete_eleven_arm_matrix": True, "complete_twelve_arm_matrix": True,
            "exact_artifact_identity_join": True,
            "common_pre_mutation_snapshots": True,
            "quill_text_byte_invariance_all_arms": True, "confirmed_story_partition_invariant": True,
            "raw_fdpp_framing_known": True, "paragraph_metric_semantics_granted": False,
        },
        "remaining_authority": {
            "native_rule_to_persisted_carrier_law": "requires_semantic_review_of_raw_matrix",
            "effective_line_origins_and_heights": "not_proven_by_frame_bounds_or_raw_values",
            "product_consumer_1139": "blocked_until_parent_authority_closes",
        },
        "boundary": "Raw framing, scalar bits and TEXT hashes only. No inferred units, omitted/default law, fallback-font authority or product layout change.",
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output-root", type=Path, required=True)
    parser.add_argument("--snapshot-tool", type=Path, required=True)
    args = parser.parse_args()
    out = args.output_root / "analysis" / "paragraph-metrics-auth-01-structural.json"
    try:
        # An earlier successful receipt must not survive a failed fresh analysis.
        out.unlink(missing_ok=True)
        receipt = analyze(args.output_root, lambda path: run_probe(args.snapshot_tool, path))
        out.write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    except (StructuralError, OSError, ValueError, TypeError, KeyError) as error:
        print(f"paragraph structural analysis failed: {error}", file=sys.stderr)
        return 1
    print(json.dumps(receipt["invariants"], sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
