#!/usr/bin/env python3
"""File-level feature prevalence over current source-free Reader receipts.

This intentionally distinguishes three states per feature:
- present: the current detector ran and found the capability;
- absent: the current detector ran and did not find it;
- unknown: the file did not open or the required detector field is unavailable.

Provenance strata are joined by exact SHA-256 only. A SHA may legitimately
belong to multiple source/category strata; it is counted at most once within
each stratum.
"""
from __future__ import annotations

import argparse
import json
import re
from collections import defaultdict
from pathlib import Path
from typing import Callable

SHA_RE = re.compile(r"^[0-9a-f]{64}$")
SCHEMA = "chaptera.reader-feature-prevalence.v2"

FeatureDetector = Callable[[dict], bool | None]


def _int_presence(field: str) -> FeatureDetector:
    def detect(row: dict) -> bool | None:
        if row.get("opened") is not True:
            return None
        value = row.get(field)
        if isinstance(value, bool) or not isinstance(value, int) or value < 0:
            return None
        return value > 0

    return detect


def _list_presence(field: str) -> FeatureDetector:
    def detect(row: dict) -> bool | None:
        if row.get("opened") is not True:
            return None
        value = row.get(field)
        if not isinstance(value, list):
            return None
        return bool(value)

    return detect


FEATURES: dict[str, tuple[str, FeatureDetector]] = {
    "has_story": (
        "current Reader story_count > 0",
        _int_presence("story_count"),
    ),
    "has_text_fragment": (
        "current Reader text_fragment_count > 0",
        _int_presence("text_fragment_count"),
    ),
    "has_image_placement": (
        "current Reader image_placement_count > 0",
        _int_presence("image_placement_count"),
    ),
    "has_solid_fill": (
        "current Reader solid_fill_count > 0",
        _int_presence("solid_fill_count"),
    ),
    "has_solid_line": (
        "current Reader solid_line_count > 0",
        _int_presence("solid_line_count"),
    ),
    "has_inherited_typography": (
        "current Reader inherited_typography_run_count > 0",
        _int_presence("inherited_typography_run_count"),
    ),
    "has_reader_diagnostic": (
        "current Reader diagnostic_codes is non-empty",
        _list_presence("diagnostic_codes"),
    ),
    "has_legacy_object_residual": (
        "current Reader legacy_object_residuals is non-empty",
        _list_presence("legacy_object_residuals"),
    ),
}


def _norm_sha(value: object, label: str) -> str:
    sha = str(value or "").strip().casefold()
    if not SHA_RE.fullmatch(sha):
        raise ValueError(f"invalid {label} sha256: {sha!r}")
    return sha


def load_reader_records(path: Path) -> dict[str, dict]:
    if path.is_dir():
        payloads = []
        for file in sorted(path.glob("*.json")):
            payloads.append(json.loads(file.read_text(encoding="utf-8")))
    else:
        payload = json.loads(path.read_text(encoding="utf-8"))
        if isinstance(payload, list):
            payloads = payload
        elif isinstance(payload, dict) and isinstance(payload.get("rows"), list):
            payloads = payload["rows"]
        elif isinstance(payload, dict):
            payloads = [payload]
        else:
            raise ValueError("reader records must be a JSON object/list or directory of JSON files")

    by_sha: dict[str, dict] = {}
    for row in payloads:
        if not isinstance(row, dict):
            raise ValueError("reader record must be a JSON object")
        sha = _norm_sha(row.get("source_sha256"), "reader")
        if sha in by_sha:
            raise ValueError(f"duplicate reader sha256: {sha}")
        by_sha[sha] = row
    return by_sha


def load_provenance(path: Path | None) -> dict[str, list[dict]]:
    if path is None:
        return {}
    payload = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(payload, list):
        raise ValueError("provenance must be a JSON list")
    by_sha: dict[str, list[dict]] = defaultdict(list)
    for row in payload:
        if not isinstance(row, dict):
            raise ValueError("provenance row must be a JSON object")
        sha = _norm_sha(row.get("sha256"), "provenance")
        by_sha[sha].append(row)
    return dict(by_sha)


def feature_state(row: dict, detector: FeatureDetector) -> str:
    value = detector(row)
    if value is True:
        return "present"
    if value is False:
        return "absent"
    return "unknown"


def _summarize(shas: set[str], records: dict[str, dict]) -> dict:
    feature_counts: dict[str, dict] = {}
    for feature, (_, detector) in FEATURES.items():
        counts = {"present": 0, "absent": 0, "unknown": 0}
        for sha in shas:
            row = records.get(sha)
            state = "unknown" if row is None else feature_state(row, detector)
            counts[state] += 1
        known = counts["present"] + counts["absent"]
        feature_counts[feature] = {
            "denominator": len(shas),
            "known_file_count": known,
            "present_file_count": counts["present"],
            "absent_file_count": counts["absent"],
            "unknown_file_count": counts["unknown"],
            "present_ratio_known": counts["present"] / known if known else None,
        }
    return feature_counts


def _strata(
    records: dict[str, dict],
    provenance: dict[str, list[dict]],
    field: str,
) -> dict[str, set[str]]:
    out: dict[str, set[str]] = defaultdict(set)
    for sha in records:
        for row in provenance.get(sha, []):
            value = str(row.get(field) or "").strip()
            if value:
                out[value].add(sha)
    return dict(out)


def build_report(records: dict[str, dict], provenance: dict[str, list[dict]]) -> dict:
    all_shas = set(records)

    formats: dict[str, set[str]] = defaultdict(set)
    for sha, row in records.items():
        value = str(row.get("format_version") or "unknown").strip() or "unknown"
        formats[value].add(sha)

    sources = _strata(records, provenance, "source")
    categories = _strata(records, provenance, "category")

    return {
        "schema": SCHEMA,
        "corpus_sha_count": len(all_shas),
        "opened_file_count": sum(row.get("opened") is True for row in records.values()),
        "failed_or_unopened_file_count": sum(row.get("opened") is not True for row in records.values()),
        "feature_definitions": {
            feature: description for feature, (description, _) in FEATURES.items()
        },
        "overall": _summarize(all_shas, records),
        "by_format_version": {
            key: _summarize(shas, records) for key, shas in sorted(formats.items())
        },
        "by_source": {
            key: _summarize(shas, records) for key, shas in sorted(sources.items())
        },
        "by_category": {
            key: _summarize(shas, records) for key, shas in sorted(categories.items())
        },
        "boundary": (
            "Presence is reported only when a current Reader detector field exists. "
            "Reader-open failure or missing detector data is unknown, never absent. "
            "Source/category membership joins only by exact SHA-256 and is deduplicated "
            "within each stratum. These corpus strata are not market-share claims."
        ),
    }


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--reader-records", required=True, type=Path)
    ap.add_argument("--provenance", type=Path)
    ap.add_argument("--out", required=True, type=Path)
    args = ap.parse_args()

    records = load_reader_records(args.reader_records)
    provenance = load_provenance(args.provenance)
    report = build_report(records, provenance)
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(
        json.dumps(report, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    print(json.dumps(report, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
