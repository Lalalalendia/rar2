#!/usr/bin/env python3
"""Build a bounded, source-free frontier from Reader-1050 unsupported cases.

This tool is intentionally narrow. It does not choose work from the global PUB
backlog and it does not mutate parser/product code. It ranks only the current
Reader-1050 rows whose salvage outcome is `unsupported`, joins them with a
bounded static CFB shape, and emits one next discriminator plus the full queue.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import sys
from collections import Counter
from pathlib import Path
from typing import Any

REPO_ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(REPO_ROOT / "tools" / "corpus"))

from cfb_physical import CFB  # noqa: E402
from reader1050_knowledge import (  # noqa: E402
    evidence_for_sha,
    ledger_for_sha,
    load_discriminator_ledger,
    load_evidence_registry,
)

SCHEMA = "chaptera.reader1050-hosted-frontier.v1"
CASE_SCHEMA = "chaptera.reader1050-hosted-frontier-case.v1"

KNOWN_STREAMS = {
    "contents": "Contents",
    "quill": "Quill",
    "summary_information": "\x05SummaryInformation",
    "document_summary_information": "\x05DocumentSummaryInformation",
}


def read_json(path: Path) -> Any:
    return json.loads(path.read_text(encoding="utf-8"))


def find_unique(root: Path, name: str) -> Path:
    matches = sorted(root.rglob(name))
    if len(matches) != 1:
        raise ValueError(f"expected exactly one {name} below {root}, got {len(matches)}")
    return matches[0]


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def load_reader_inputs(reader_root: Path) -> tuple[dict[str, Any], dict[str, dict[str, Any]]]:
    salvage = read_json(find_unique(reader_root, "salvage-acceptance.json"))
    if salvage.get("schema") != "chaptera.reader-salvage-1050-acceptance.v1":
        raise ValueError(f"unexpected salvage schema: {salvage.get('schema')!r}")

    records_raw = read_json(find_unique(reader_root, "reader-records.json"))
    if not isinstance(records_raw, list):
        raise ValueError("reader-records.json must be a list")

    records = {
        str(row.get("source_sha256", "")).lower(): row
        for row in records_raw
        if row.get("source_sha256")
    }
    return salvage, records


def locate_pub(corpus_root: Path, source_sha256: str) -> Path:
    matches = sorted(corpus_root.rglob(f"{source_sha256}.pub"))
    if len(matches) != 1:
        raise ValueError(
            f"expected exactly one corpus PUB for {source_sha256}, got {len(matches)}"
        )
    return matches[0]


def size_bucket(size: int) -> str:
    if size < 256:
        return "<256"
    if size < 1024:
        return "256-1023"
    if size < 4096:
        return "1K-4K"
    if size < 16384:
        return "4K-16K"
    if size < 65536:
        return "16K-64K"
    if size < 262144:
        return "64K-256K"
    return ">=256K"


def source_free_cfb_shape(data: bytes) -> dict[str, Any]:
    cfb = CFB(data)
    streams = [entry for entry in cfb.dirs if entry.get("type") == 2 and entry.get("size")]
    stream_names = {str(entry.get("name", "")) for entry in streams}
    storage_counts = Counter(
        "minifat" if int(entry["size"]) < cfb.cut else "fat" for entry in streams
    )
    size_buckets = Counter(size_bucket(int(entry["size"])) for entry in streams)
    type_counts = Counter(str(entry.get("type")) for entry in cfb.dirs if entry.get("type"))

    known_stream_presence = {
        key: value in stream_names for key, value in KNOWN_STREAMS.items()
    }
    return {
        "cfb_major": cfb.major,
        "sector_size": cfb.ss,
        "mini_sector_size": cfb.ms,
        "byte_len": len(data),
        "stream_count": len(streams),
        "stream_storage_counts": dict(sorted(storage_counts.items())),
        "stream_size_buckets": dict(sorted(size_buckets.items())),
        "directory_entry_type_counts": dict(sorted(type_counts.items())),
        "known_stream_presence": known_stream_presence,
        "fat_sector_count": len(cfb.fatsecs),
        "difat_sector_count": len(cfb.difsecs),
        "directory_sector_count": len(cfb.dirsecs),
        "minifat_sector_count": len(cfb.minisecs),
    }


def suggested_discriminator(
    row: dict[str, Any],
    registry: dict[str, dict[str, Any]] | None = None,
) -> tuple[str, str]:
    source_sha256 = str(row.get("source_sha256") or "").lower()
    known_evidence = evidence_for_sha(source_sha256, registry)
    if known_evidence is not None:
        if known_evidence.get("kind") == "format_owner":
            return (
                "existing_format_owner",
                "Hand off to the already-grounded format-research owner; do not reinterpret normal-open failure as corruption evidence.",
            )
        if known_evidence.get("kind") == "typed_corruption_evidence":
            return (
                "existing_typed_corruption_evidence",
                "Bind the existing exact-SHA corruption authority into the Reader salvage path and validate bounded admission; do not rediscover corruption.",
            )
    if row.get("cfb_inventory_available") is False:
        return (
            "container_integrity_gap",
            "Localize why bounded CFB inventory is unavailable before widening salvage admission.",
        )
    if not row.get("contents_family"):
        return (
            "family_classification_gap",
            "Classify the exact Contents family or prove typed structural corruption; do not guess a marketing version.",
        )
    if row.get("salvage_eligibility") == "awaiting_typed_corruption_evidence":
        return (
            "typed_corruption_evidence_gap",
            "Build one bounded corruption discriminator for this exact failing family and replay only this witness before changing salvage policy.",
        )
    return (
        "unsupported_reader_gap",
        "Localize the exact Reader-open failure on this witness before changing parser or salvage behavior.",
    )


def priority(
    row: dict[str, Any],
    registry: dict[str, dict[str, Any]] | None = None,
    ledger_entries: list[dict[str, Any]] | None = None,
) -> tuple[int, list[str], bool, str | None]:
    score = 0
    reasons: list[str] = []
    suppressed = False
    suppression_reason = None

    source_sha256 = str(row.get("source_sha256") or "").lower()
    known_evidence = evidence_for_sha(source_sha256, registry)
    if known_evidence is not None:
        if known_evidence.get("kind") == "format_owner":
            score -= 500
            reasons.append(
                f"already owned by {known_evidence['owner']} ({known_evidence['kind']})"
            )
        elif known_evidence.get("kind") == "typed_corruption_evidence":
            score += 500
            reasons.append(
                f"existing typed corruption evidence from {known_evidence['owner']}"
            )

    history = ledger_for_sha(source_sha256, ledger_entries)
    terminal = [
        item for item in history
        if item.get("status") in {"executed", "closed", "handoff"}
    ]
    if terminal:
        latest = terminal[-1]
        suppressed = True
        suppression_reason = (
            f"ledger status {latest.get('status')} from discriminator run "
            f"{latest.get('discriminator_run_id')}"
        )
        reasons.append(suppression_reason)

    if row.get("salvage_eligibility") == "awaiting_typed_corruption_evidence":
        score += 100
        reasons.append("awaiting typed corruption evidence")

    forced_probe = row.get("forced_trigger_probe")
    forced_graph = row.get("forced_partial_graph")
    if isinstance(forced_probe, dict) and isinstance(forced_graph, dict):
        if forced_probe.get("cfb_inventory_available") is False:
            score += 260
            reasons.append("forced trigger still cannot build Reader CFB inventory")
        elif forced_graph.get("status") == "error":
            score += 250
            reasons.append("forced trigger reaches CFB but partial graph fails")
        elif (
            forced_graph.get("status") == "constructed"
            and forced_probe.get("has_surviving_evidence") is True
        ):
            score += 240
            reasons.append("forced trigger constructs partial salvage graph")
    if row.get("has_surviving_evidence") is True:
        score += 80
        reasons.append("surviving evidence exists")
    if row.get("cfb_inventory_available") is True:
        score += 40
        reasons.append("bounded CFB inventory available")

    family = row.get("contents_family")
    if family == "0x2c":
        score += 30
        reasons.append("mature 0x2c family")
    elif family == "0x22":
        score += 20
        reasons.append("legacy 0x22 family")
    elif family:
        score += 15
        reasons.append("known nonstandard family")
    else:
        score += 5
        reasons.append("family unresolved")

    if row.get("open_error_signature_sha256"):
        score += 10
        reasons.append("stable open-error signature available")

    return score, reasons, suppressed, suppression_reason


def build_case(
    row: dict[str, Any],
    reader_record: dict[str, Any] | None,
    corpus_root: Path,
    registry: dict[str, dict[str, Any]],
    ledger_entries: list[dict[str, Any]],
) -> dict[str, Any]:
    source_sha256 = str(row["source_sha256"]).lower()
    pub_path = locate_pub(corpus_root, source_sha256)
    data = pub_path.read_bytes()
    actual_sha256 = sha256_bytes(data)
    if actual_sha256 != source_sha256:
        raise ValueError(
            f"corpus identity drift for {source_sha256}: materialized bytes hash to {actual_sha256}"
        )

    cfb_shape = source_free_cfb_shape(data)
    score, score_reasons, suppressed, suppression_reason = priority(
        row,
        registry,
        ledger_entries,
    )
    gap_class, next_operation = suggested_discriminator(row, registry)
    known_evidence = evidence_for_sha(source_sha256, registry)
    discriminator_history = ledger_for_sha(source_sha256, ledger_entries)

    return {
        "schema": CASE_SCHEMA,
        "source_sha256": source_sha256,
        "known_evidence": known_evidence,
        "discriminator_history": discriminator_history,
        "byte_len": len(data),
        "outcome": row.get("outcome"),
        "reader_route": row.get("reader_route"),
        "pub_profile": row.get("pub_profile"),
        "contents_family": row.get("contents_family"),
        "salvage_eligibility": row.get("salvage_eligibility"),
        "corruption_evidence": row.get("corruption_evidence"),
        "has_surviving_evidence": row.get("has_surviving_evidence"),
        "cfb_inventory_available": row.get("cfb_inventory_available"),
        "open_error_signature_sha256": row.get("open_error_signature_sha256"),
        "forced_trigger_probe": row.get("forced_trigger_probe"),
        "forced_partial_graph": row.get("forced_partial_graph"),
        "normal_reader_opened": (
            reader_record.get("opened") if isinstance(reader_record, dict) else None
        ),
        "normal_reader_error_signature_sha256": (
            reader_record.get("open_error_signature_sha256")
            if isinstance(reader_record, dict)
            else None
        ),
        "cfb_shape": cfb_shape,
        "frontier_score": score,
        "frontier_score_reasons": score_reasons,
        "frontier_suppressed": suppressed,
        "frontier_suppression_reason": suppression_reason,
        "gap_class": gap_class,
        "suggested_next_operation": next_operation,
        "evidence_boundary": (
            "source-free/static triage only: no filenames, paths, document text, raw streams, "
            "source bytes, repaired PUB materialization, or semantic claim promotion"
        ),
    }


def build_frontier(
    reader_root: Path,
    corpus_root: Path,
    out_root: Path,
    source_run_id: str,
    source_sha: str,
) -> dict[str, Any]:
    salvage, records = load_reader_inputs(reader_root)
    registry = load_evidence_registry()
    ledger_entries = load_discriminator_ledger(registry=registry)
    unsupported = [
        row for row in salvage.get("rows", []) if row.get("outcome") == "unsupported"
    ]

    cases = [
        build_case(
            row,
            records.get(str(row.get("source_sha256", "")).lower()),
            corpus_root,
            registry,
            ledger_entries,
        )
        for row in unsupported
    ]
    cases.sort(key=lambda row: (-int(row["frontier_score"]), row["source_sha256"]))

    selectable = [row for row in cases if not row.get("frontier_suppressed")]
    selected = selectable[0] if selectable else None
    if not cases:
        status = "exhausted"
    elif selected is None:
        status = "awaiting_ledger_or_new_evidence"
    else:
        status = "frontier_selected"
    payload = {
        "schema": SCHEMA,
        "source_reader_run_id": str(source_run_id),
        "source_main_sha": source_sha,
        "unsupported_count": len(cases),
        "status": status,
        "selected": selected,
        "queue": [
            {
                "source_sha256": row["source_sha256"],
                "frontier_score": row["frontier_score"],
                "frontier_suppressed": row.get("frontier_suppressed"),
                "frontier_suppression_reason": row.get("frontier_suppression_reason"),
                "known_evidence": row.get("known_evidence"),
                "gap_class": row["gap_class"],
                "contents_family": row["contents_family"],
                "salvage_eligibility": row["salvage_eligibility"],
                "has_surviving_evidence": row["has_surviving_evidence"],
                "cfb_inventory_available": row["cfb_inventory_available"],
            }
            for row in cases
        ],
        "evidence_boundary": (
            "bounded Reader-1050 unsupported campaign only; this is not global backlog "
            "reprioritization and cannot promote canonical format claims by itself"
        ),
    }

    out_root.mkdir(parents=True, exist_ok=True)
    cases_root = out_root / "cases"
    cases_root.mkdir(parents=True, exist_ok=True)
    for row in cases:
        (cases_root / f"{row['source_sha256']}.json").write_text(
            json.dumps(row, indent=2, sort_keys=True) + "\n",
            encoding="utf-8",
        )
    (out_root / "frontier.json").write_text(
        json.dumps(payload, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    (out_root / "frontier.md").write_text(
        render_markdown(payload),
        encoding="utf-8",
    )
    return payload


def render_markdown(payload: dict[str, Any]) -> str:
    lines = [
        "# Reader-1050 autonomous hosted frontier",
        "",
        f"- Reader source run: `{payload['source_reader_run_id']}`",
        f"- Main SHA: `{payload['source_main_sha']}`",
        f"- Unsupported cases: **{payload['unsupported_count']}**",
        f"- Status: **{payload['status']}**",
        "",
    ]

    selected = payload.get("selected")
    if selected:
        shape = selected["cfb_shape"]
        lines.extend(
            [
                "## Selected bounded discriminator",
                "",
                f"- Source SHA-256: `{selected['source_sha256']}`",
                f"- Gap class: `{selected['gap_class']}`",
                f"- Contents family: `{selected.get('contents_family') or 'unresolved'}`",
                f"- PUB profile: `{selected.get('pub_profile') or 'unknown'}`",
                f"- Salvage eligibility: `{selected.get('salvage_eligibility') or 'none'}`",
                f"- Surviving evidence: `{selected.get('has_surviving_evidence')}`",
                f"- CFB inventory available: `{selected.get('cfb_inventory_available')}`",
                f"- Static stream count: **{shape['stream_count']}**",
                f"- Known Contents stream: **{shape['known_stream_presence']['contents']}**",
                f"- Known Quill stream: **{shape['known_stream_presence']['quill']}**",
                "",
                "**Next bounded operation:** "
                + selected["suggested_next_operation"],
                "",
                "Selection basis: "
                + "; ".join(selected["frontier_score_reasons"])
                + ".",
                "",
            ]
        )
    else:
        if payload["status"] == "exhausted":
            message = "No Reader-1050 rows currently have salvage outcome `unsupported`."
        else:
            message = (
                "Unsupported rows remain, but every current candidate is suppressed by the "
                "durable discriminator ledger. A reviewed ledger/evidence change is required "
                "before repeating a completed discriminator."
            )
        lines.extend(
            [
                "## No selectable frontier",
                "",
                message,
                "",
            ]
        )

    lines.extend(
        [
            "## Queue",
            "",
            "| SHA | score | gap | family | surviving evidence | CFB inventory |",
            "| --- | ---: | --- | --- | --- | --- |",
        ]
    )
    for row in payload["queue"]:
        lines.append(
            "| "
            + " | ".join(
                [
                    f"`{row['source_sha256'][:12]}`",
                    str(row["frontier_score"]),
                    f"`{row['gap_class']}`",
                    f"`{row.get('contents_family') or 'unresolved'}`",
                    str(row.get("has_surviving_evidence")),
                    str(row.get("cfb_inventory_available")),
                ]
            )
            + " |"
        )

    lines.extend(
        [
            "",
            "## Boundary",
            "",
            payload["evidence_boundary"] + ".",
            "",
            "The generated artifacts contain source-free structural metadata only. "
            "They do not contain PUB bytes, document text, filenames, or raw stream payloads.",
            "",
        ]
    )
    return "\n".join(lines)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--reader-root", type=Path, required=True)
    parser.add_argument("--corpus-root", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--source-run-id", required=True)
    parser.add_argument("--source-sha", required=True)
    args = parser.parse_args()

    payload = build_frontier(
        args.reader_root,
        args.corpus_root,
        args.out,
        args.source_run_id,
        args.source_sha,
    )
    print(
        json.dumps(
            {
                "status": payload["status"],
                "unsupported_count": payload["unsupported_count"],
                "selected_sha256": (
                    payload["selected"]["source_sha256"]
                    if payload.get("selected")
                    else None
                ),
                "selected_gap_class": (
                    payload["selected"]["gap_class"]
                    if payload.get("selected")
                    else None
                ),
            },
            indent=2,
            sort_keys=True,
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
