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

SCHEMA = "chaptera.reader1050-hosted-frontier.v1"
CASE_SCHEMA = "chaptera.reader1050-hosted-frontier-case.v1"

DEFAULT_EVIDENCE_REGISTRY = (
    REPO_ROOT
    / "vendor"
    / "producer-a"
    / "crates"
    / "pub-reader"
    / "data"
    / "reader-evidence-registry.json"
)
DEFAULT_RESEARCH_LEDGER = REPO_ROOT / "tools" / "research-runner" / "reader1050-research-ledger.json"

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


def load_evidence_registry(path: Path) -> dict[str, dict[str, Any]]:
    payload = read_json(path)
    if payload.get("schema") != "chaptera.reader-evidence-registry.v1":
        raise ValueError(f"unexpected evidence registry schema: {payload.get('schema')!r}")
    entries: dict[str, dict[str, Any]] = {}
    for row in payload.get("entries") or []:
        sha = str(row.get("source_sha256") or "").lower()
        if len(sha) != 64 or any(ch not in "0123456789abcdef" for ch in sha):
            raise ValueError(f"invalid evidence registry SHA-256: {sha!r}")
        if sha in entries:
            raise ValueError(f"duplicate evidence registry SHA-256: {sha}")
        entries[sha] = row
    return entries


def load_research_ledger(path: Path) -> dict[str, list[dict[str, Any]]]:
    payload = read_json(path)
    if payload.get("schema") != "chaptera.reader1050-research-ledger.v1":
        raise ValueError(f"unexpected research ledger schema: {payload.get('schema')!r}")
    entries: dict[str, list[dict[str, Any]]] = {}
    for row in payload.get("entries") or []:
        sha = str(row.get("source_sha256") or "").lower()
        if len(sha) != 64 or any(ch not in "0123456789abcdef" for ch in sha):
            raise ValueError(f"invalid research ledger SHA-256: {sha!r}")
        entries.setdefault(sha, []).append(row)
    return entries


def effective_reader_state(row: dict[str, Any]) -> dict[str, Any]:
    state = {
        "cfb_inventory_available": row.get("cfb_inventory_available"),
        "contents_family": row.get("contents_family"),
        "has_surviving_evidence": row.get("has_surviving_evidence"),
    }
    probe = row.get("forced_trigger_probe")
    graph = row.get("forced_partial_graph")
    if (
        row.get("salvage_eligibility") == "awaiting_typed_corruption_evidence"
        and isinstance(probe, dict)
    ):
        if probe.get("cfb_inventory_available") is not None:
            state["cfb_inventory_available"] = probe.get("cfb_inventory_available")
        if probe.get("contents_family"):
            state["contents_family"] = probe.get("contents_family")
        if probe.get("has_surviving_evidence") is not None:
            state["has_surviving_evidence"] = probe.get("has_surviving_evidence")
        state["forced_partial_graph_status"] = (
            graph.get("status") if isinstance(graph, dict) else None
        )
    return state


def research_resolution(
    source_sha256: str,
    evidence_entry: dict[str, Any] | None,
    ledger_entries: list[dict[str, Any]],
) -> dict[str, Any] | None:
    if evidence_entry is None:
        return None

    disposition = evidence_entry.get("disposition")
    if disposition == "existing_format_owner":
        return {
            "kind": "existing_format_owner",
            "owner": evidence_entry.get("owner"),
            "evidence_class": evidence_entry.get("evidence_class"),
            "selectable": False,
        }

    if disposition != "existing_typed_corruption_evidence":
        return None

    receipt = evidence_entry.get("authority_receipt") or {}
    authority_digest = receipt.get("evidence_digest")
    if not authority_digest:
        return None

    closed = any(
        row.get("discriminator") == "typed_corruption_evidence_discovery"
        and row.get("state") == "closed"
        and row.get("evidence_digest") == authority_digest
        for row in ledger_entries
    )
    if not closed:
        return None

    handoff = next(
        (
            row
            for row in ledger_entries
            if row.get("discriminator") == "bounded_salvage_admission_validation"
            and row.get("state") in {"handoff", "closed"}
            and row.get("evidence_digest") == authority_digest
        ),
        None,
    )
    return {
        "kind": "existing_typed_corruption_evidence",
        "owner": evidence_entry.get("owner"),
        "evidence_class": evidence_entry.get("evidence_class"),
        "authority_evidence_digest": authority_digest,
        "authority_classification": receipt.get("classification"),
        "typed_discriminator_closed": True,
        "salvage_admission_handoff": handoff is not None,
        "selectable": False,
    }


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
    resolution: dict[str, Any] | None = None,
) -> tuple[str, str]:
    if resolution is not None:
        if resolution.get("kind") == "existing_format_owner":
            return (
                "existing_format_owner",
                "Hand off to the already-grounded format-research owner; do not reinterpret normal-open failure as corruption evidence.",
            )
        if resolution.get("kind") == "existing_typed_corruption_evidence":
            return (
                "existing_typed_corruption_evidence",
                "Bind the exact-SHA authority and reuse the completed typed-corruption discriminator; do not rediscover corruption evidence.",
            )

    effective = effective_reader_state(row)
    if effective.get("cfb_inventory_available") is False:
        return (
            "container_integrity_gap",
            "Localize why bounded CFB inventory is unavailable before widening salvage admission.",
        )
    if not effective.get("contents_family"):
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
    resolution: dict[str, Any] | None = None,
) -> tuple[int, list[str]]:
    score = 0
    reasons: list[str] = []

    if resolution is not None:
        score -= 1000
        reasons.append(f"resolved by {resolution['kind']}")


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
    effective = effective_reader_state(row)
    if effective.get("has_surviving_evidence") is True:
        score += 80
        reasons.append("surviving evidence exists")
    if effective.get("cfb_inventory_available") is True:
        score += 40
        reasons.append("bounded CFB inventory available")

    family = effective.get("contents_family")
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

    return score, reasons


def build_case(
    row: dict[str, Any],
    reader_record: dict[str, Any] | None,
    corpus_root: Path,
    evidence_entry: dict[str, Any] | None = None,
    ledger_entries: list[dict[str, Any]] | None = None,
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
    resolution = research_resolution(
        source_sha256,
        evidence_entry,
        ledger_entries or [],
    )
    effective = effective_reader_state(row)
    score, score_reasons = priority(row, resolution)
    gap_class, next_operation = suggested_discriminator(row, resolution)

    return {
        "schema": CASE_SCHEMA,
        "source_sha256": source_sha256,
        "existing_owner": (
            evidence_entry.get("owner")
            if evidence_entry is not None
            and evidence_entry.get("disposition") == "existing_format_owner"
            else None
        ),
        "evidence_registry_entry": evidence_entry,
        "research_resolution": resolution,
        "selectable": resolution is None,
        "byte_len": len(data),
        "outcome": row.get("outcome"),
        "reader_route": row.get("reader_route"),
        "pub_profile": row.get("pub_profile"),
        "contents_family": effective.get("contents_family"),
        "salvage_eligibility": row.get("salvage_eligibility"),
        "corruption_evidence": row.get("corruption_evidence"),
        "has_surviving_evidence": effective.get("has_surviving_evidence"),
        "cfb_inventory_available": effective.get("cfb_inventory_available"),
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
    evidence_registry_path: Path = DEFAULT_EVIDENCE_REGISTRY,
    research_ledger_path: Path = DEFAULT_RESEARCH_LEDGER,
) -> dict[str, Any]:
    salvage, records = load_reader_inputs(reader_root)
    evidence_registry = load_evidence_registry(evidence_registry_path)
    research_ledger = load_research_ledger(research_ledger_path)
    unsupported = [
        row for row in salvage.get("rows", []) if row.get("outcome") == "unsupported"
    ]

    cases = [
        build_case(
            row,
            records.get(str(row.get("source_sha256", "")).lower()),
            corpus_root,
            evidence_registry.get(str(row.get("source_sha256", "")).lower()),
            research_ledger.get(str(row.get("source_sha256", "")).lower(), []),
        )
        for row in unsupported
    ]
    cases.sort(key=lambda row: (-int(row["frontier_score"]), row["source_sha256"]))
    unresolved = [row for row in cases if row.get("selectable") is True]
    unresolved.sort(key=lambda row: (-int(row["frontier_score"]), row["source_sha256"]))

    selected = unresolved[0] if unresolved else None
    payload = {
        "schema": SCHEMA,
        "source_reader_run_id": str(source_run_id),
        "source_main_sha": source_sha,
        "unsupported_count": len(cases),
        "resolved_count": len(cases) - len(unresolved),
        "unresolved_count": len(unresolved),
        "status": "frontier_selected" if selected else "exhausted",
        "selected": selected,
        "queue": [
            {
                "source_sha256": row["source_sha256"],
                "frontier_score": row["frontier_score"],
                "existing_owner": row.get("existing_owner"),
                "research_resolution": row.get("research_resolution"),
                "selectable": row.get("selectable"),
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
        f"- Resolved by registry/ledger: **{payload['resolved_count']}**",
        f"- Unresolved selectable cases: **{payload['unresolved_count']}**",
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
        lines.extend(
            [
                "## Frontier exhausted",
                "",
                "No Reader-1050 rows currently have salvage outcome `unsupported`.",
                "",
            ]
        )

    lines.extend(
        [
            "## Queue",
            "",
            "| SHA | score | selectable | gap | family | surviving evidence | CFB inventory |",
            "| --- | ---: | --- | --- | --- | --- | --- |",
        ]
    )
    for row in payload["queue"]:
        lines.append(
            "| "
            + " | ".join(
                [
                    f"`{row['source_sha256'][:12]}`",
                    str(row["frontier_score"]),
                    str(row.get("selectable")),
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
    parser.add_argument(
        "--evidence-registry",
        type=Path,
        default=DEFAULT_EVIDENCE_REGISTRY,
    )
    parser.add_argument(
        "--research-ledger",
        type=Path,
        default=DEFAULT_RESEARCH_LEDGER,
    )
    args = parser.parse_args()

    payload = build_frontier(
        args.reader_root,
        args.corpus_root,
        args.out,
        args.source_run_id,
        args.source_sha,
        args.evidence_registry,
        args.research_ledger,
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
