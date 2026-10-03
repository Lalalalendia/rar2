#!/usr/bin/env python3
"""Source-safe corpus discriminator for the trophy_traditional Quill descriptor tail.

The discriminator tests only the fixed Quill descriptor-list framing already
consumed by pub-quill. It retains SHA identities, structural counts, booleans,
marker ordinals and hashes; it never retains document text or raw stream bytes.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import struct
import sys
from pathlib import Path
from typing import Any

REPO_ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(REPO_ROOT / "tools" / "corpus"))

from cfb_physical import CFB, END, NO  # noqa: E402

SCHEMA = "chaptera.reader1050-trophy-quill-census.v1"
TARGET_SHA = "32b857475ae5ca8207942a40dc708d63153c9140bb06ee740d7e235944c0a027"
CONTROL_SHA = "7a5393159155ad6b7e1e47fa2769e05ffdb7124298470bae15faa556b3c06cd6"
ROOT_OFFSET = 0x18
DESCRIPTOR_OFFSET = 0x20
DESCRIPTOR_SIZE = 24
PRESENCE_MARKER = 0x0018
EXPECTED_SERVICE = 0x01F8
EXPECTED_COUNT = 8
EXPECTED_NEXT = 0xFFFFFFFF
PREFIX_END = 0x80


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def ordered_children(cfb: CFB, parent_sid: int) -> list[int]:
    parent = cfb.directory_entry_by_sid(parent_sid)
    root = int(parent["child"])
    out: list[int] = []
    seen: set[int] = set()

    def walk(sid: int) -> None:
        if sid in (NO, END) or sid >= len(cfb.dirs) or sid in seen:
            return
        seen.add(sid)
        entry = cfb.directory_entry_by_sid(sid)
        walk(int(entry["left"]))
        out.append(sid)
        walk(int(entry["right"]))

    walk(root)
    return out


def logical_stream_sid(cfb: CFB, parts: list[str]) -> int | None:
    current_sid = int(cfb.root["i"])
    for index, part in enumerate(parts):
        matches = [
            sid
            for sid in ordered_children(cfb, current_sid)
            if cfb.directory_entry_by_sid(sid)["name"] == part
        ]
        if len(matches) != 1:
            return None
        current_sid = matches[0]
        entry = cfb.directory_entry_by_sid(current_sid)
        expected_type = 2 if index == len(parts) - 1 else 1
        if int(entry["type"]) != expected_type:
            return None
    return current_sid


def descriptor_summary(quill: bytes) -> dict[str, Any] | None:
    if len(quill) < DESCRIPTOR_OFFSET:
        return None
    service, count, next_value = struct.unpack_from("<HHI", quill, ROOT_OFFSET)
    descriptor_end = DESCRIPTOR_OFFSET + int(count) * DESCRIPTOR_SIZE
    area_fits = descriptor_end <= len(quill)
    markers: list[int | None] = []
    valid: list[bool] = []
    if area_fits:
        for ordinal in range(int(count)):
            offset = DESCRIPTOR_OFFSET + ordinal * DESCRIPTOR_SIZE
            marker = struct.unpack_from("<H", quill, offset)[0]
            markers.append(marker)
            valid.append(marker == PRESENCE_MARKER)
    valid_prefix = 0
    for item in valid:
        if not item:
            break
        valid_prefix += 1
    first_invalid = next((i for i, item in enumerate(valid) if not item), None)
    return {
        "stream_byte_len": len(quill),
        "root_service": service,
        "root_count": count,
        "root_next": next_value,
        "descriptor_area_fits": area_fits,
        "valid_marker_count": sum(valid),
        "valid_marker_prefix_count": valid_prefix,
        "first_invalid_descriptor_ordinal": first_invalid,
        "invalid_marker_count": len(valid) - sum(valid),
        "all_declared_markers_valid": bool(area_fits and valid and all(valid)),
        "marker_vector_sha256": sha256_bytes(
            b"".join(
                struct.pack("<H", marker if marker is not None else 0xFFFF)
                for marker in markers
            )
        ),
        "prefix_0x80_sha256": sha256_bytes(quill[:PREFIX_END])
        if len(quill) >= PREFIX_END
        else None,
    }


def find_unique(root: Path, name: str) -> Path:
    matches = sorted(root.rglob(name))
    if len(matches) != 1:
        raise ValueError(f"expected exactly one {name} below {root}, got {len(matches)}")
    return matches[0]


def read_baseline(baseline_root: Path) -> tuple[dict[str, Any], dict[str, Any]]:
    records = json.loads(find_unique(baseline_root, "reader-records.json").read_text())
    salvage = json.loads(find_unique(baseline_root, "salvage-acceptance.json").read_text())
    record_index = {
        str(row.get("source_sha256") or "").lower(): row
        for row in records
        if row.get("source_sha256")
    }
    salvage_index = {
        str(row.get("source_sha256") or "").lower(): row
        for row in salvage.get("rows") or []
        if row.get("source_sha256")
    }
    return record_index, salvage_index


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--corpus-root", type=Path, required=True)
    parser.add_argument("--baseline-root", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()

    reader_records, salvage_rows = read_baseline(args.baseline_root)
    quill_present = 0
    same_envelope: list[dict[str, Any]] = []
    target_prefix_hash: str | None = None
    all_quill: list[tuple[str, dict[str, Any]]] = []

    pubs = sorted(args.corpus_root.rglob("*.pub"))
    for path in pubs:
        data = path.read_bytes()
        sha = sha256_bytes(data)
        if path.stem.lower() != sha:
            raise ValueError(f"corpus identity drift: {path.name} hashes to {sha}")
        cfb = CFB(data)
        sid = logical_stream_sid(cfb, ["Quill", "QuillSub", "CONTENTS"])
        if sid is None:
            continue
        quill = cfb.read_stream_by_sid(sid)
        quill_present += 1
        summary = descriptor_summary(quill)
        if summary is None:
            continue
        all_quill.append((sha, summary))
        if sha == TARGET_SHA:
            target_prefix_hash = summary["prefix_0x80_sha256"]
        if (
            summary["root_service"] == EXPECTED_SERVICE
            and summary["root_count"] == EXPECTED_COUNT
            and summary["root_next"] == EXPECTED_NEXT
        ):
            row = {
                "source_sha256": sha,
                **summary,
            }
            record = reader_records.get(sha) or {}
            salvage = salvage_rows.get(sha) or {}
            row["reader_opened"] = record.get("opened")
            row["reader_outcome"] = salvage.get("outcome")
            row["salvage_eligibility"] = salvage.get("salvage_eligibility")
            same_envelope.append(row)

    if target_prefix_hash is None:
        raise ValueError("target Quill prefix was not found")

    prefix_matches = [
        sha
        for sha, summary in all_quill
        if summary.get("prefix_0x80_sha256") == target_prefix_hash
    ]
    same_envelope.sort(key=lambda row: row["source_sha256"])
    target = next(
        (row for row in same_envelope if row["source_sha256"] == TARGET_SHA),
        None,
    )
    control = next(
        (row for row in same_envelope if row["source_sha256"] == CONTROL_SHA),
        None,
    )
    if target is None or control is None:
        raise ValueError("exact target/control are not both in the expected envelope")

    valid_controls = [
        row
        for row in same_envelope
        if row["source_sha256"] != TARGET_SHA
        and row["all_declared_markers_valid"] is True
    ]
    invalid_rows = [
        row for row in same_envelope if row["all_declared_markers_valid"] is not True
    ]
    normal_open_controls = [
        row
        for row in same_envelope
        if row["source_sha256"] != TARGET_SHA
        and row["reader_opened"] is True
        and row["reader_outcome"] == "normal_open"
    ]

    authority_gates = {
        "target_descriptor_area_fits": target["descriptor_area_fits"] is True,
        "target_first_four_markers_valid": target["valid_marker_prefix_count"] == 4,
        "target_remaining_four_markers_invalid": (
            target["root_count"] == 8
            and target["valid_marker_count"] == 4
            and target["invalid_marker_count"] == 4
        ),
        "control_all_markers_valid": control["all_declared_markers_valid"] is True,
        "same_envelope_count_is_eight": len(same_envelope) == 8,
        "target_is_unique_invalid_same_envelope": (
            len(invalid_rows) == 1
            and invalid_rows[0]["source_sha256"] == TARGET_SHA
        ),
        "seven_same_envelope_controls_normal_open": len(normal_open_controls) == 7,
        "target_currently_unsupported_awaiting_typed_evidence": (
            target["reader_opened"] is False
            and target["reader_outcome"] == "unsupported"
            and target["salvage_eligibility"] == "awaiting_typed_corruption_evidence"
        ),
        "exact_prefix_match_is_target_plus_control": (
            sorted(prefix_matches) == sorted([TARGET_SHA, CONTROL_SHA])
        ),
    }
    authorized = all(authority_gates.values())

    payload = {
        "schema": SCHEMA,
        "corpus_pub_count": len(pubs),
        "quill_stream_present_count": quill_present,
        "expected_descriptor_presence_marker": PRESENCE_MARKER,
        "same_root_envelope": {
            "service": EXPECTED_SERVICE,
            "count": EXPECTED_COUNT,
            "next": EXPECTED_NEXT,
            "witness_count": len(same_envelope),
            "all_markers_valid_count": len(valid_controls),
            "invalid_marker_witness_count": len(invalid_rows),
            "normal_open_control_count": len(normal_open_controls),
            "witnesses": same_envelope,
        },
        "target_prefix_0x80_sha256": target_prefix_hash,
        "target_prefix_match_count": len(prefix_matches),
        "target_prefix_match_shas": sorted(prefix_matches),
        "authority_gates": authority_gates,
        "decision": {
            "status": (
                "typed_structural_corruption_evidence"
                if authorized
                else "descriptor_corruption_not_yet_authorized"
            ),
            "typed_corruption_authorized": authorized,
            "classification": (
                "quill_descriptor_tail_overwritten_or_malformed"
                if authorized
                else None
            ),
            "next_operation": (
                "Bind this exact-SHA evidence digest into Reader salvage authority and replay the exact 1050 acceptance."
                if authorized
                else "Keep the target unsupported and localize the failed authority gate."
            ),
        },
        "evidence_boundary": (
            "source-safe structural census only: SHA identities, descriptor framing scalars, "
            "marker validity, hashes and Reader outcome classes; no document text, raw streams, "
            "filenames/paths or repaired PUB materialization retained"
        ),
    }

    args.out.mkdir(parents=True, exist_ok=True)
    (args.out / "quill-census.json").write_text(
        json.dumps(payload, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    (args.out / "quill-census.md").write_text(
        "\n".join([
            "# Reader-1050 trophy Quill descriptor census",
            "",
            f"- Corpus PUBs: **{len(pubs)}**",
            f"- Quill streams: **{quill_present}**",
            f"- Same root envelope witnesses: **{len(same_envelope)}**",
            f"- Normal-open controls: **{len(normal_open_controls)}**",
            f"- Invalid marker witnesses: **{len(invalid_rows)}**",
            f"- Target valid marker prefix: **{target['valid_marker_prefix_count']} / 8**",
            f"- Exact 0x80-prefix matches: **{len(prefix_matches)}**",
            f"- Decision: **{payload['decision']['status']}**",
            f"- Typed corruption authorized: **{authorized}**",
            "",
            payload["decision"]["next_operation"],
            "",
        ]) + "\n",
        encoding="utf-8",
    )
    print(json.dumps(payload["decision"], indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
