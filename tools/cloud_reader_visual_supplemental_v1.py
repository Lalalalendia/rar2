#!/usr/bin/env python3
from __future__ import annotations

import argparse
import csv
import hashlib
import json
from pathlib import Path

SCHEMA = "chaptera.publisher-visual-supplemental-hosted.v1"
EXTERNAL_FAMILY = "manual-reduction-family"


def sha256(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def hosted_rows(pairs_csv: Path) -> list[dict[str, str]]:
    rows = list(csv.DictReader(pairs_csv.open(newline="", encoding="utf-8-sig")))
    hosted = [row for row in rows if row["family"] != EXTERNAL_FAMILY]
    if len(rows) != 31:
        raise ValueError(f"expected 31 registered supplemental rows, got {len(rows)}")
    if len(hosted) != 24:
        raise ValueError(f"expected 24 hosted-materializable rows, got {len(hosted)}")
    if sum(int(row["pdf_pages"]) for row in hosted) != 52:
        raise ValueError("hosted supplemental reference-page count drift")
    return hosted


def prepare(pairs_csv: Path, source_root: Path, out_dir: Path, manifest: Path) -> dict:
    rows = hosted_rows(pairs_csv)
    candidates: dict[str, list[Path]] = {}
    for path in source_root.rglob("*.pub"):
        candidates.setdefault(sha256(path), []).append(path)

    out_dir.mkdir(parents=True, exist_ok=True)
    fixtures = []
    for row in rows:
        digest = row["pub_sha256"].strip().lower()
        matches = candidates.get(digest, [])
        if len(matches) != 1:
            raise ValueError(f"expected one source for {row['basename']} {digest}, got {len(matches)}")
        source = matches[0]
        expected_bytes = int(row["pub_bytes"])
        if source.stat().st_size != expected_bytes:
            raise ValueError(f"source byte length mismatch for {row['basename']}")
        target = out_dir / row["pub_filename"]
        target.write_bytes(source.read_bytes())
        if sha256(target) != digest:
            raise ValueError(f"copied source SHA mismatch for {row['basename']}")
        fixtures.append({
            "name": row["basename"],
            "sha256": digest,
            "bytes": expected_bytes,
            "require_render": False,
            "require_shared_text": False,
            "source_path": str(target.resolve()),
        })

    payload = {
        "schema": "chaptera.cloud-reader-real-fixtures.v1",
        "fixtures": fixtures,
    }
    manifest.parent.mkdir(parents=True, exist_ok=True)
    manifest.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    return payload


def summarize(pairs_csv: Path, browser_receipt: Path, out: Path) -> dict:
    rows = hosted_rows(pairs_csv)
    browser = json.loads(browser_receipt.read_text(encoding="utf-8"))
    if browser.get("protocol") != "chaptera.cloud-reader-real-scene-browser.v1":
        raise ValueError(f"unsupported browser receipt: {browser.get('protocol')!r}")

    by_name = {}
    for row in browser.get("results", []):
        name = row.get("fixture")
        if name in by_name:
            raise ValueError(f"duplicate browser fixture {name!r}")
        by_name[name] = row

    results = []
    unsupported = []
    page_mismatches = []
    for row in rows:
        name = row["basename"]
        actual = by_name.get(name)
        if actual is None:
            raise ValueError(f"missing browser result for {name}")
        if actual.get("source_sha256") != row["pub_sha256"]:
            raise ValueError(f"source SHA drift for {name}")
        rendered = actual.get("rendered") is True
        candidate_pages = actual.get("pages") if rendered else None
        expected_pages = int(row["pdf_pages"])
        page_match = rendered and candidate_pages == expected_pages
        result = {
            "oracle_id": row["oracle_id"],
            "basename": name,
            "family": row["family"],
            "warning_state": row["warning_state"],
            "pub_sha256": row["pub_sha256"],
            "reference_pdf_sha256": row["pdf_sha256"],
            "reference_pages": expected_pages,
            "rendered": rendered,
            "classification": actual.get("classification"),
            "terminal_code": actual.get("terminal_code"),
            "candidate_pages": candidate_pages,
            "page_count_match": page_match,
            "fidelity": actual.get("fidelity"),
            "fidelity_reasons": actual.get("fidelity_reasons", []),
            "diagnostic_codes": actual.get("diagnostic_codes", []),
        }
        results.append(result)
        if not rendered:
            unsupported.append(result)
        elif not page_match:
            page_mismatches.append(result)

    payload = {
        "schema": SCHEMA,
        "repository_commit_sha": browser.get("repository_commit_sha"),
        "registered_supplemental_pair_count": 31,
        "registered_supplemental_reference_page_count": 59,
        "hosted_pair_count": len(results),
        "hosted_reference_page_count": sum(x["reference_pages"] for x in results),
        "rendered_pair_count": sum(x["rendered"] for x in results),
        "unsupported_pair_count": len(unsupported),
        "page_count_mismatch_pair_count": len(page_mismatches),
        "font_warning_pair_count": sum(bool(x["warning_state"]) for x in results),
        "external_manual_reduction_pair_count": 7,
        "external_manual_reduction_reference_page_count": 7,
        "visual_reference_fingerprint_state": "MISSING_FOR_24_HOSTED_PAIRS",
        "claims": {
            "this_is_not_visual_parity": True,
            "publisher_pdf_sha_is_identity_only_until_reference_fingerprint_bytes_are_available": True,
            "open_render_and_page_count_are_current_reader_execution_evidence": True,
        },
        "pairs": results,
        "unsupported_pairs": unsupported,
        "page_count_mismatches": page_mismatches,
    }
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    return payload


def main() -> None:
    parser = argparse.ArgumentParser()
    sub = parser.add_subparsers(dest="cmd", required=True)

    p = sub.add_parser("prepare")
    p.add_argument("pairs_csv", type=Path)
    p.add_argument("source_root", type=Path)
    p.add_argument("out_dir", type=Path)
    p.add_argument("manifest", type=Path)

    s = sub.add_parser("summarize")
    s.add_argument("pairs_csv", type=Path)
    s.add_argument("browser_receipt", type=Path)
    s.add_argument("out", type=Path)

    args = parser.parse_args()
    if args.cmd == "prepare":
        payload = prepare(args.pairs_csv, args.source_root, args.out_dir, args.manifest)
        print(json.dumps({"fixtures": len(payload["fixtures"])}, sort_keys=True))
    else:
        payload = summarize(args.pairs_csv, args.browser_receipt, args.out)
        print(json.dumps({
            "hosted_pairs": payload["hosted_pair_count"],
            "hosted_reference_pages": payload["hosted_reference_page_count"],
            "rendered_pairs": payload["rendered_pair_count"],
            "unsupported_pairs": payload["unsupported_pair_count"],
            "page_count_mismatches": payload["page_count_mismatch_pair_count"],
            "visual_reference_fingerprint_state": payload["visual_reference_fingerprint_state"],
        }, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
