#!/usr/bin/env python3
"""Bounded same-locator Wayback lineage discriminator for Reader-1050 trophy_traditional.

This tool never acquires network data itself. The existing Wayback CDX/harvest
tools perform inert acquisition. This layer consumes the exact current source,
the harvest manifest and harvested CFB candidates, then emits only source-safe
hash/count/openability facts.

A historical normal-opening candidate is a control candidate, not corruption
authority. Promotion to typed corruption requires a separate independent A/B
discriminator.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import Any

REPO_ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(REPO_ROOT / "tools" / "corpus"))

from cfb_physical import CFB, SIG  # noqa: E402

SCHEMA = "chaptera.reader1050-wayback-lineage.v1"
SOURCE_SHA256 = "32b857475ae5ca8207942a40dc708d63153c9140bb06ee740d7e235944c0a027"
SOURCE_URL = "http://helenhudspith.com/resources/product/amran/TROPHY/trophy_traditional.pub"

KNOWN_STREAM_NAMES = (
    "Contents",
    "CONTENTS",
    "EscherStm",
    "EscherDelayStm",
    "\x05SummaryInformation",
    "\x05DocumentSummaryInformation",
)


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def canonical_hash(value: Any) -> str:
    payload = json.dumps(
        value,
        sort_keys=True,
        separators=(",", ":"),
        ensure_ascii=False,
    ).encode("utf-8")
    return sha256_bytes(payload)


def name_hash(value: str) -> str:
    return sha256_bytes(value.encode("utf-8", errors="surrogatepass"))


def cfb_profile(data: bytes) -> dict[str, Any]:
    cfb = CFB(data)
    directory_shape: list[dict[str, Any]] = []
    stream_layout: list[dict[str, Any]] = []
    stream_payloads: list[dict[str, Any]] = []
    known_counts = {name: 0 for name in KNOWN_STREAM_NAMES}

    storage_count = 0
    stream_count = 0
    empty_stream_count = 0

    for entry in cfb.dirs:
        entry_type = int(entry.get("type") or 0)
        if entry_type == 0:
            continue

        raw_name = str(entry.get("name") or "")
        item = {
            "type": entry_type,
            "name_sha256": name_hash(raw_name.casefold()),
        }

        if entry_type == 1:
            storage_count += 1
        elif entry_type == 2:
            stream_count += 1
            size = int(entry.get("size") or 0)
            if size == 0:
                empty_stream_count += 1
            storage = "minifat" if size < cfb.cut else "fat"
            item["size"] = size
            item["storage"] = storage

            payload = cfb.read_stream_by_sid(int(entry["i"]))
            layout_item = {
                "name_sha256": item["name_sha256"],
                "size": size,
                "storage": storage,
            }
            stream_layout.append(layout_item)
            stream_payloads.append(
                {
                    **layout_item,
                    "payload_sha256": sha256_bytes(payload),
                }
            )
            if raw_name in known_counts:
                known_counts[raw_name] += 1

        directory_shape.append(item)

    stream_layout.sort(
        key=lambda row: (
            row["name_sha256"],
            int(row["size"]),
            row["storage"],
        )
    )
    stream_payloads.sort(
        key=lambda row: (
            row["name_sha256"],
            int(row["size"]),
            row["payload_sha256"],
        )
    )

    return {
        "cfb_major": cfb.major,
        "sector_size": cfb.ss,
        "mini_sector_size": cfb.ms,
        "byte_len": len(data),
        "directory_entry_count": len(
            [entry for entry in cfb.dirs if int(entry.get("type") or 0) != 0]
        ),
        "storage_count": storage_count,
        "stream_count": stream_count,
        "empty_stream_count": empty_stream_count,
        "known_stream_name_counts": known_counts,
        "directory_shape_sha256": canonical_hash(directory_shape),
        "stream_layout_multiset_sha256": canonical_hash(stream_layout),
        "stream_payload_multiset_sha256": canonical_hash(stream_payloads),
    }


def sanitized_reader_receipt(payload: dict[str, Any]) -> dict[str, Any]:
    allowed = (
        "schema",
        "opened",
        "source_sha256",
        "byte_len",
        "format",
        "format_version",
        "fidelity_status",
        "viewer_page_count",
        "scene_surface_count",
        "scene_node_count",
        "story_count",
        "story_frame_count",
        "text_fragment_count",
        "typography_run_count",
        "image_resource_count",
        "image_placement_count",
        "paint_node_count",
        "solid_fill_count",
        "solid_line_count",
        "diagnostic_codes",
        "open_error_kind",
        "open_error_signature_sha256",
        "visual_fidelity_proven",
    )
    return {key: payload.get(key) for key in allowed if key in payload}


def run_reader_receipt(exe: Path, source: Path) -> dict[str, Any]:
    with tempfile.TemporaryDirectory() as td:
        output = Path(td) / "receipt.json"
        proc = subprocess.run(
            [str(exe), str(source), str(output)],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
            timeout=120,
        )
        if proc.returncode != 0 or not output.exists():
            signature = sha256_bytes(proc.stderr + b"\0" + proc.stdout)
            return {
                "schema": "chaptera.reader-corpus-structural-receipt.v1",
                "opened": False,
                "source_sha256": sha256_bytes(source.read_bytes()),
                "byte_len": source.stat().st_size,
                "open_error_kind": "receipt_runner_failed",
                "open_error_signature_sha256": signature,
            }
        return sanitized_reader_receipt(
            json.loads(output.read_text(encoding="utf-8"))
        )


def relation(
    source_sha256: str,
    source_profile: dict[str, Any],
    candidate_sha256: str,
    candidate_profile: dict[str, Any],
) -> tuple[int, str]:
    if source_sha256 == candidate_sha256:
        return 6, "byte_identical"

    if (
        source_profile["directory_shape_sha256"]
        == candidate_profile["directory_shape_sha256"]
        and source_profile["stream_layout_multiset_sha256"]
        == candidate_profile["stream_layout_multiset_sha256"]
    ):
        return 5, "same_directory_and_stream_layout"

    if (
        source_profile["stream_layout_multiset_sha256"]
        == candidate_profile["stream_layout_multiset_sha256"]
    ):
        return 4, "same_stream_layout"

    if (
        source_profile["directory_shape_sha256"]
        == candidate_profile["directory_shape_sha256"]
    ):
        return 3, "same_directory_shape"

    coarse_keys = (
        "cfb_major",
        "sector_size",
        "stream_count",
        "storage_count",
        "known_stream_name_counts",
    )
    if all(source_profile[key] == candidate_profile[key] for key in coarse_keys):
        return 2, "same_coarse_cfb_shape"

    if (
        source_profile["cfb_major"] == candidate_profile["cfb_major"]
        and source_profile["known_stream_name_counts"]
        == candidate_profile["known_stream_name_counts"]
    ):
        return 1, "same_major_carrier_set"

    return 0, "structurally_divergent"


def decide(
    source_reader: dict[str, Any],
    candidates: list[dict[str, Any]],
) -> dict[str, Any]:
    if source_reader.get("opened") is True:
        return {
            "status": "obsolete_source_now_opens",
            "typed_corruption_authorized": False,
            "next_operation": (
                "Stop this discriminator: current Reader now opens the exact source."
            ),
        }

    distinct = [
        row for row in candidates
        if row.get("relation") != "byte_identical"
    ]
    normal_open = [
        row for row in distinct
        if (row.get("reader") or {}).get("opened") is True
    ]
    close_normal_open = [
        row for row in normal_open
        if int(row.get("relation_rank") or 0) >= 4
    ]

    if close_normal_open:
        return {
            "status": "normal_open_close_historical_control_found",
            "typed_corruption_authorized": False,
            "control_count": len(close_normal_open),
            "next_operation": (
                "Run an independent parser/openability A/B plus stream-localized "
                "source-safe diff on the closest normal-opening historical control "
                "before promoting typed corruption."
            ),
        }

    if normal_open:
        return {
            "status": "normal_open_historical_variant_not_close_enough",
            "typed_corruption_authorized": False,
            "control_count": len(normal_open),
            "next_operation": (
                "Do not use the distant historical variant as corruption authority; "
                "close same-layout lineage and continue with a structural discriminator."
            ),
        }

    if distinct:
        return {
            "status": "distinct_historical_variants_without_normal_open_control",
            "typed_corruption_authorized": False,
            "distinct_variant_count": len(distinct),
            "next_operation": (
                "Close Wayback lineage as a corruption discriminator and continue "
                "with a bounded structural corruption discriminator on the exact source."
            ),
        }

    return {
        "status": "no_distinct_historical_variant",
        "typed_corruption_authorized": False,
        "next_operation": (
            "Close exact-URL Wayback lineage and continue with a bounded structural "
            "corruption discriminator on the exact source."
        ),
    }


def load_harvest_candidates(
    harvest_root: Path,
    manifest_path: Path,
    source_sha256: str,
    source_profile: dict[str, Any],
    reader_exe: Path,
) -> tuple[list[dict[str, Any]], dict[str, int]]:
    rows = json.loads(manifest_path.read_text(encoding="utf-8"))
    candidates: list[dict[str, Any]] = []
    seen_sha: set[str] = set()

    counters = {
        "manifest_rows": len(rows),
        "exact_url_rows": 0,
        "fetched_rows": 0,
        "cfb_valid_rows": 0,
        "unique_payload_rows": 0,
        "invalid_or_missing_payload_rows": 0,
    }

    for row in rows:
        if str(row.get("wayback_original_url") or "").casefold() != SOURCE_URL.casefold():
            continue
        counters["exact_url_rows"] += 1

        if row.get("fetch_status") != "ok":
            continue
        counters["fetched_rows"] += 1

        stored = row.get("stored_path")
        declared_sha = str(row.get("sha256") or "").lower()
        if not stored or len(declared_sha) != 64:
            counters["invalid_or_missing_payload_rows"] += 1
            continue

        path = harvest_root / str(stored)
        if not path.is_file():
            counters["invalid_or_missing_payload_rows"] += 1
            continue

        data = path.read_bytes()
        actual_sha = sha256_bytes(data)
        if actual_sha != declared_sha:
            raise ValueError(
                f"harvest identity drift for {declared_sha}: got {actual_sha}"
            )
        if actual_sha in seen_sha:
            continue
        seen_sha.add(actual_sha)
        counters["unique_payload_rows"] += 1

        if len(data) < 512 or data[:8] != SIG:
            counters["invalid_or_missing_payload_rows"] += 1
            continue

        try:
            profile = cfb_profile(data)
        except ValueError:
            counters["invalid_or_missing_payload_rows"] += 1
            continue
        counters["cfb_valid_rows"] += 1

        reader = run_reader_receipt(reader_exe, path)
        rank, relation_name = relation(
            source_sha256,
            source_profile,
            actual_sha,
            profile,
        )
        candidates.append(
            {
                "wayback_timestamp": row.get("wayback_timestamp"),
                "wayback_digest": row.get("wayback_digest"),
                "wayback_mimetype": row.get("wayback_mimetype"),
                "wayback_length": row.get("wayback_length"),
                "payload_sha256": actual_sha,
                "byte_len": len(data),
                "harvest_classification": row.get("classification"),
                "cfb": profile,
                "reader": reader,
                "relation_rank": rank,
                "relation": relation_name,
            }
        )

    candidates.sort(
        key=lambda row: (
            -int(row["relation_rank"]),
            str(row.get("wayback_timestamp") or ""),
            row["payload_sha256"],
        )
    )
    return candidates, counters


def render_markdown(payload: dict[str, Any]) -> str:
    decision = payload["decision"]
    lines = [
        "# Reader-1050 trophy_traditional Wayback lineage",
        "",
        f"- Exact source SHA-256: `{payload['source_sha256']}`",
        f"- Exact public URL: `{payload['source_url']}`",
        f"- Source Reader opened: **{payload['source']['reader'].get('opened')}**",
        f"- CDX locator rows: **{payload['discovery'].get('deduplicated_locator_rows', 0)}**",
        f"- Unique CFB payloads inspected: **{payload['harvest_counters']['cfb_valid_rows']}**",
        f"- Decision: **{decision['status']}**",
        f"- Typed corruption authorized: **{decision['typed_corruption_authorized']}**",
        "",
        "## Candidates",
        "",
        "| capture | sha256 | relation | Reader opened | bytes |",
        "| --- | --- | --- | --- | ---: |",
    ]
    for row in payload["candidates"]:
        lines.append(
            "| "
            + " | ".join(
                [
                    str(row.get("wayback_timestamp") or ""),
                    f"`{row['payload_sha256'][:16]}…`",
                    f"`{row['relation']}`",
                    str((row.get("reader") or {}).get("opened")),
                    str(row["byte_len"]),
                ]
            )
            + " |"
        )

    lines.extend(
        [
            "",
            "## Next bounded operation",
            "",
            decision["next_operation"],
            "",
            "Evidence boundary: no document text, raw stream bytes, recovered PUB bytes, "
            "local paths or parser error strings are retained in the public receipt.",
            "",
        ]
    )
    return "\n".join(lines)


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--source", type=Path, required=True)
    ap.add_argument("--harvest-root", type=Path, required=True)
    ap.add_argument("--harvest-manifest", type=Path, required=True)
    ap.add_argument("--discovery-summary", type=Path, required=True)
    ap.add_argument("--reader-receipt-exe", type=Path, required=True)
    ap.add_argument("--out", type=Path, required=True)
    ap.add_argument("--expected-source-sha256", default=SOURCE_SHA256)
    args = ap.parse_args()

    source_bytes = args.source.read_bytes()
    source_sha256 = sha256_bytes(source_bytes)
    if source_sha256 != args.expected_source_sha256:
        raise ValueError(
            f"source identity drift: expected {args.expected_source_sha256}, got {source_sha256}"
        )
    if source_bytes[:8] != SIG:
        raise ValueError("exact source is not CFB")

    source_profile = cfb_profile(source_bytes)
    source_reader = run_reader_receipt(args.reader_receipt_exe, args.source)
    if source_reader.get("source_sha256") != source_sha256:
        raise ValueError("Reader receipt source identity drift")

    discovery = json.loads(args.discovery_summary.read_text(encoding="utf-8"))
    candidates, counters = load_harvest_candidates(
        args.harvest_root,
        args.harvest_manifest,
        source_sha256,
        source_profile,
        args.reader_receipt_exe,
    )

    payload = {
        "schema": SCHEMA,
        "source_sha256": source_sha256,
        "source_url": SOURCE_URL,
        "source": {
            "byte_len": len(source_bytes),
            "cfb": source_profile,
            "reader": source_reader,
        },
        "discovery": {
            "schema": discovery.get("schema"),
            "exact_queries": discovery.get("exact_queries"),
            "raw_capture_rows": discovery.get("raw_capture_rows"),
            "deduplicated_locator_rows": discovery.get(
                "deduplicated_locator_rows"
            ),
            "errors": len(discovery.get("errors") or []),
        },
        "harvest_counters": counters,
        "candidates": candidates,
        "decision": decide(source_reader, candidates),
        "evidence_boundary": (
            "source-safe exact-URL lineage only; no document text, raw stream bytes, "
            "recovered PUB bytes, local paths or parser error strings retained; "
            "historical normal-open controls do not authorize typed corruption"
        ),
    }

    args.out.mkdir(parents=True, exist_ok=True)
    (args.out / "decision.json").write_text(
        json.dumps(payload, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    (args.out / "decision.md").write_text(
        render_markdown(payload),
        encoding="utf-8",
    )
    print(json.dumps(payload["decision"], indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
