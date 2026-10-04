#!/usr/bin/env python3
"""Build auditable rich-provenance rows for artifact-family replay.

The current exact corpus materializer intentionally keeps only SHA identity and
coarse source labels/paths. This bridge enriches those SHA identities from
retained provenance without guessing:

1. materialized manifest SHA-256 is the corpus membership authority;
2. lalamu manifest rows join ONLY by exact SHA-256;
3. curated legacy-seed metadata joins ONLY by exact normalized source URL;
4. filenames are copied as labels after an authoritative join, never used to
   create the join itself.

Missing provenance stays explicit and unlabeled.
"""
from __future__ import annotations

import argparse
import csv
import json
from collections import defaultdict
from pathlib import Path
from urllib.parse import unquote, urlsplit, urlunsplit


def _norm_url(value: object) -> str:
    text = str(value or "").strip()
    if not text:
        return ""
    p = urlsplit(text)
    scheme = p.scheme.casefold()
    host = p.netloc.casefold()
    path = unquote(p.path)
    # Query is retained: for some archive/download endpoints it is identity.
    return urlunsplit((scheme, host, path, p.query, ""))


def load_materialized(path: Path) -> list[dict]:
    payload = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(payload, list):
        raise ValueError("materialized manifest must be a JSON list")
    rows = []
    seen = set()
    for row in payload:
        sha = str(row.get("sha256") or "").casefold()
        if len(sha) != 64:
            raise ValueError(f"invalid materialized sha256: {sha!r}")
        if sha in seen:
            raise ValueError(f"duplicate materialized sha256: {sha}")
        seen.add(sha)
        rows.append(row)
    return rows


def load_lalamu_jsonl(path: Path) -> dict[str, list[dict]]:
    by_sha: dict[str, list[dict]] = defaultdict(list)
    for line_no, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        if not line.strip():
            continue
        row = json.loads(line)
        sha = str(row.get("sha256") or "").casefold()
        if len(sha) != 64:
            raise ValueError(f"invalid lalamu sha256 at line {line_no}: {sha!r}")
        by_sha[sha].append(row)
    return by_sha


def load_legacy_seed(path: Path | None) -> dict[str, list[dict]]:
    if path is None:
        return {}
    by_url: dict[str, list[dict]] = defaultdict(list)
    with path.open("r", encoding="utf-8-sig", newline="") as handle:
        for row in csv.DictReader(handle):
            url = _norm_url(row.get("direct_url"))
            if url:
                by_url[url].append(row)
    return by_url


def build_join(
    materialized_rows: list[dict],
    lalamu_by_sha: dict[str, list[dict]],
    legacy_by_url: dict[str, list[dict]],
) -> list[dict]:
    out: list[dict] = []
    for materialized in sorted(materialized_rows, key=lambda row: row["sha256"]):
        sha = str(materialized["sha256"]).casefold()
        provenance = lalamu_by_sha.get(sha, [])
        if not provenance:
            out.append(
                {
                    "sha256": sha,
                    "source": "",
                    "source_page": "",
                    "category": "",
                    "candidate_filename": "",
                    "parent_archive_filename": "",
                    "source_url": "",
                    "provenance_join": "materialized-sha-only",
                    "materialized_sources": materialized.get("sources", []),
                    "materialized_source_paths": materialized.get("source_paths", []),
                }
            )
            continue

        for source_row in sorted(
            provenance,
            key=lambda row: (
                str(row.get("source_url") or ""),
                str(row.get("source_filename") or ""),
                str(row.get("candidate_id") or ""),
            ),
        ):
            source_url = str(source_row.get("source_url") or "")
            legacy_matches = legacy_by_url.get(_norm_url(source_url), [])
            if len(legacy_matches) > 1:
                # Multiple curated rows on one exact URL are ambiguous. Keep the
                # SHA/source provenance, but refuse category/source-page promotion.
                legacy = None
                join = "sha:lalamu+url:legacy-ambiguous"
            elif len(legacy_matches) == 1:
                legacy = legacy_matches[0]
                join = "sha:lalamu+url:legacy-exact"
            else:
                legacy = None
                join = "sha:lalamu"

            out.append(
                {
                    "sha256": sha,
                    "source": (
                        str(legacy.get("source") or "")
                        if legacy
                        else str(source_row.get("source_repo") or source_row.get("source_type") or "")
                    ),
                    "source_page": str(legacy.get("source_page") or "") if legacy else "",
                    "category": str(legacy.get("category") or "") if legacy else "",
                    "candidate_filename": (
                        str(legacy.get("candidate_filename") or "")
                        if legacy
                        else str(source_row.get("source_filename") or "")
                    ),
                    "parent_archive_filename": "",
                    "source_url": source_url,
                    "source_type": str(source_row.get("source_type") or ""),
                    "source_repo": str(source_row.get("source_repo") or ""),
                    "candidate_id": str(source_row.get("candidate_id") or ""),
                    "provenance_join": join,
                    "materialized_sources": materialized.get("sources", []),
                    "materialized_source_paths": materialized.get("source_paths", []),
                }
            )
    return out


def build_summary(materialized_rows: list[dict], joined: list[dict]) -> dict:
    shas = {str(row["sha256"]).casefold() for row in materialized_rows}
    lalamu = {row["sha256"] for row in joined if row["provenance_join"] != "materialized-sha-only"}
    legacy = {row["sha256"] for row in joined if row["provenance_join"] == "sha:lalamu+url:legacy-exact"}
    ambiguous = {row["sha256"] for row in joined if row["provenance_join"] == "sha:lalamu+url:legacy-ambiguous"}
    return {
        "schema": "chaptera.artifact-family-rich-provenance-join.v1",
        "materialized_sha_count": len(shas),
        "sha_with_lalamu_provenance": len(lalamu),
        "sha_with_exact_legacy_url_enrichment": len(legacy),
        "sha_with_ambiguous_legacy_url": len(ambiguous),
        "sha_without_lalamu_provenance": len(shas - lalamu),
        "joined_row_count": len(joined),
        "boundary": (
            "Corpus membership joins by exact SHA-256. Curated category/source-page "
            "metadata joins only by exact normalized URL. Filename similarity is "
            "never a join authority; missing provenance remains explicit."
        ),
    }


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--materialized-manifest", required=True, type=Path)
    ap.add_argument("--lalamu-manifest-jsonl", required=True, type=Path)
    ap.add_argument("--legacy-seed", type=Path)
    ap.add_argument("--out", required=True, type=Path)
    ap.add_argument("--summary-out", type=Path)
    args = ap.parse_args()

    materialized = load_materialized(args.materialized_manifest)
    lalamu = load_lalamu_jsonl(args.lalamu_manifest_jsonl)
    legacy = load_legacy_seed(args.legacy_seed)
    joined = build_join(materialized, lalamu, legacy)
    args.out.write_text(json.dumps(joined, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    summary = build_summary(materialized, joined)
    if args.summary_out:
        args.summary_out.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps(summary, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
