#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
from collections import Counter, defaultdict
from pathlib import Path
from typing import Any

SCHEMA = "chaptera.mature-page-role-outlier-recurrence.v1"
PAGE_ROLE_SCHEMA = "chaptera.pub-page-role-observation.v1"

RAW_SHAPE = 0x01
RAW_TABLE = 0x10
RAW_GROUP = 0x30

TARGETS = {
    "024": "nonzero:2:N:E|nonzero:1:L:P|nonzero:1:L:P|nonzero:2:L:E|zero:L:E|zero:L:E|zero:L:P",
    "025": "nonzero:2:N:P|nonzero:1:L:P|nonzero:1:L:P|nonzero:1:L:P|nonzero:2:L:P|nonzero:2:L:P|nonzero:2:L:P|nonzero:2:L:P|nonzero:2:L:P|nonzero:2:L:P|nonzero:2:L:P|nonzero:2:L:P|zero:L:E|zero:L:E",
    "029": "nonzero:2:N:E|nonzero:2:N:E|nonzero:1:O:P|nonzero:2:L:P|nonzero:2:O:P|nonzero:2:L:P|nonzero:2:O:E|nonzero:2:N:E|zero:N:E",
    "033": "zero:N:P|zero:N:E|zero:N:E|zero:N:P|nonzero:2:L:P|nonzero:2:O:P|nonzero:2:O:P|nonzero:2:O:P|zero:L:E|zero:L:E|zero:L:E|zero:L:E",
    "041": "nonzero:2:N:E|nonzero:1:L:P|nonzero:1:L:P|nonzero:2:L:P|zero:L:E|zero:L:E|zero:L:E",
    "060": "zero:N:P|nonzero:1:L:P|zero:L:E|zero:N:E|zero:N:E",
    "061": "nonzero:2:N:E|nonzero:1:L:P|nonzero:1:L:P|nonzero:2:L:E|zero:N:E|zero:N:E",
    "062": "nonzero:2:N:E|nonzero:1:L:P|nonzero:1:L:E|nonzero:2:L:E|zero:L:E|zero:L:E",
    "064": "nonzero:2:N:E|nonzero:1:L:P|nonzero:1:L:P|nonzero:1:L:P|nonzero:2:L:P|nonzero:2:L:P|nonzero:2:L:P|nonzero:2:L:E|zero:L:E|zero:L:P",
    "065": "nonzero:2:N:E|nonzero:1:L:P|nonzero:2:L:P|zero:L:E|zero:L:E|zero:L:E",
    "073": "zero:N:E|nonzero:2:L:P|nonzero:2:L:P|nonzero:2:L:P|zero:L:E|zero:L:E|zero:L:E",
}


def load(path: Path) -> dict[str, Any]:
    return json.loads(path.read_text(encoding="utf-8"))


def oid_class(page: dict[str, Any]) -> str:
    a, b = page.get("oid_dword0"), page.get("oid_dword1")
    if a is None or b is None:
        return "absent"
    if a == 0 and b == 0:
        return "zero"
    return f"nonzero:{a}"


def payload_count(page: dict[str, Any]) -> int:
    counts = page.get("child_raw_type_counts", {})
    return sum(int(counts.get(str(t), counts.get(t, 0))) for t in (RAW_SHAPE, RAW_TABLE, RAW_GROUP))


def field_present(page: dict[str, Any], field_id: int) -> bool:
    return any(int(f.get("id", -1)) == field_id for f in page.get("fields", []))


def pgid_count(receipt: dict[str, Any]) -> int:
    return sum(
        len(field.get("pgids", []))
        for controlling in receipt.get("controlling", [])
        for field in controlling.get("fields", [])
    )


def shape_tag(receipt: dict[str, Any]) -> str:
    pages = sorted(receipt.get("pages", []), key=lambda p: int(p["document_ordinal"]))
    if not pages:
        return "empty"
    leader = pages[0].get("contents_seq_num")
    tags = []
    for page in pages:
        applied = page.get("applied_master_seq_num")
        relation = "N" if applied is None else "L" if applied == leader else "O"
        tags.append(f"{oid_class(page)}:{relation}:{'P' if payload_count(page) else 'E'}")
    return "|".join(tags)


def field_signature(receipt: dict[str, Any]) -> str:
    pages = sorted(receipt.get("pages", []), key=lambda p: int(p["document_ordinal"]))
    return "|".join(
        f"{int(field_present(p,1))}{int(field_present(p,2))}"
        for p in pages
    )


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("receipt_dir", type=Path)
    ap.add_argument("output", type=Path)
    args = ap.parse_args()

    receipts = []
    for path in sorted(args.receipt_dir.glob("*.json")):
        d = load(path)
        if d.get("schema") != PAGE_ROLE_SCHEMA:
            continue
        source_sha256 = path.stem
        receipts.append((source_sha256, d))

    by_tag: dict[str, list[tuple[str, dict[str, Any]]]] = defaultdict(list)
    for sha, receipt in receipts:
        by_tag[shape_tag(receipt)].append((sha, receipt))

    target_rows = []
    matched_shas = set()
    for target_id, tag in TARGETS.items():
        matches = by_tag.get(tag, [])
        rows = []
        for sha, receipt in matches:
            matched_shas.add(sha)
            rows.append({
                "source_sha256": sha,
                "special_entry_count": int(receipt.get("special_entry_count", 0)),
                "scenario_pgid_count": pgid_count(receipt),
                "field_presence_signature": field_signature(receipt),
            })
        target_rows.append({
            "target_batch01_id": target_id,
            "source_shape_tag": tag,
            "exact_shape_match_count": len(matches),
            "matches": rows,
        })

    shape_hist = Counter(shape_tag(r) for _, r in receipts)
    field_hist = Counter(field_signature(r) for _, r in receipts)

    out = {
        "schema": SCHEMA,
        "parseable_mature_receipt_count": len(receipts),
        "target_count": len(TARGETS),
        "target_exact_shape_match_total_unique_sources": len(matched_shas),
        "targets": target_rows,
        "corpus_shape_cluster_count": len(shape_hist),
        "largest_shape_clusters": [
            {"source_shape_tag": tag, "source_count": count}
            for tag, count in shape_hist.most_common(30)
        ],
        "field_presence_signature_cluster_count": len(field_hist),
        "largest_field_presence_clusters": [
            {"field_presence_signature": sig, "source_count": count}
            for sig, count in field_hist.most_common(30)
        ],
        "claims": {
            "measurement_only": True,
            "publisher_or_pdf_reference_used": False,
            "product_selection_changed": False,
            "raw_story_text_emitted": False,
            "raw_contents_seq_nums_emitted": False,
            "field_presence_promoted_to_authority": False,
        },
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(out, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps({
        "parseable_mature_receipt_count": len(receipts),
        "target_results": [
            {"target": r["target_batch01_id"], "matches": r["exact_shape_match_count"]}
            for r in target_rows
        ],
    }, indent=2))


if __name__ == "__main__":
    main()
