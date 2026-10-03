#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Any

SCHEMA = "chaptera.mature-multimaster-page-role-029.v1"
PAGE_ROLE_SCHEMA = "chaptera.pub-page-role-observation.v1"

SOURCES = [
    "273d283c93fafb0f14b772ba8475a6b48db0a4f9c1a0789605ce26ea811996d3",
    "5eb5055bc75918ca8dbc7093fa267a3365f25129fd2c3dc88c7be3540f44e4cd",
    "7c630704ce369f775fe7a24ec180f5c7997eb779f28bd57ab8c7dda66297a17c",
    "9915c5612d7d8ce49fc733bbbe748c80325fb0094af0ba0d42a2b15fc648e5bd",
    "a8c8a3c36a925fdc9de7c4f5e43e014a606751b2d09635cbc1b04f50ccb7fcea",
    "c0688f73b9bf8fc7677a1eecaadd30fa00fc1813f6b12dc39b8aa5eac973f81e",
]

RAW_SHAPE = 0x01
RAW_TABLE = 0x10
RAW_GROUP = 0x30


def load(path: Path) -> dict[str, Any]:
    return json.loads(path.read_text(encoding="utf-8"))


def payload_count(page: dict[str, Any]) -> int:
    counts = page.get("child_raw_type_counts", {})
    return sum(int(counts.get(str(t), counts.get(t, 0))) for t in (RAW_SHAPE, RAW_TABLE, RAW_GROUP))


def sanitize_fields(page: dict[str, Any]) -> list[dict[str, Any]]:
    out = []
    for field in page.get("fields", []):
        row = {"id": int(field["id"]), "block_type": int(field["block_type"])}
        for key in ("u16_value", "u32_value", "declared_length"):
            if key in field:
                row[key] = int(field[key])
        out.append(row)
    return sorted(out, key=lambda r: (r["id"], r["block_type"], json.dumps(r, sort_keys=True)))


def features(page: dict[str, Any]) -> set[tuple]:
    out = set()
    for field in sanitize_fields(page):
        out.add(("presence", field["id"], field["block_type"]))
        for key in ("u16_value", "u32_value", "declared_length"):
            if key in field:
                out.add((key, field["id"], field["block_type"], field[key]))
    return out


def feature_json(feature: tuple) -> dict[str, Any]:
    if feature[0] == "presence":
        return {"kind": "presence", "id": feature[1], "block_type": feature[2]}
    return {
        "kind": feature[0],
        "id": feature[1],
        "block_type": feature[2],
        "value": feature[3],
    }


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("receipt_dir", type=Path)
    ap.add_argument("output", type=Path)
    args = ap.parse_args()

    docs = []
    first_applied_feature_sets = []
    second_applied_feature_sets = []

    for sha in SOURCES:
        receipt = load(args.receipt_dir / f"{sha}.json")
        if receipt.get("schema") != PAGE_ROLE_SCHEMA:
            raise ValueError(f"unexpected schema for {sha}")

        pages = sorted(receipt["pages"], key=lambda p: int(p["document_ordinal"]))
        by_seq = {int(p["contents_seq_num"]): int(p["document_ordinal"]) for p in pages}
        roots = [p for p in pages if p.get("applied_master_seq_num") is None]
        if len(roots) < 2:
            raise ValueError(f"{sha}: expected at least two roots")

        applied_by_master: dict[int, list[dict[str, Any]]] = {}
        for page in pages:
            master_seq = page.get("applied_master_seq_num")
            if master_seq is None:
                continue
            master_ordinal = by_seq.get(int(master_seq))
            if master_ordinal is None:
                continue
            applied_by_master.setdefault(master_ordinal, []).append(page)

        normalized_master_rows = []
        for master_ordinal, children in sorted(applied_by_master.items()):
            children = sorted(children, key=lambda p: int(p["document_ordinal"]))
            positive = [p for p in children if payload_count(p) > 0]
            if len(positive) >= 1:
                first_applied_feature_sets.append(features(positive[0]))
            if len(positive) >= 2:
                second_applied_feature_sets.append(features(positive[1]))
            normalized_master_rows.append({
                "master_ordinal": master_ordinal,
                "applied_page_ordinals": [int(p["document_ordinal"]) for p in children],
                "positive_payload_ordinals": [int(p["document_ordinal"]) for p in positive],
            })

        page_rows = []
        for page in pages:
            master_seq = page.get("applied_master_seq_num")
            page_rows.append({
                "document_ordinal": int(page["document_ordinal"]),
                "oid_dword0": page.get("oid_dword0"),
                "oid_dword1": page.get("oid_dword1"),
                "applied_master_ordinal": (
                    by_seq.get(int(master_seq)) if master_seq is not None else None
                ),
                "payload_count": payload_count(page),
                "fields": sanitize_fields(page),
                "previous_document_entry_raw_type": page.get("previous_document_entry_raw_type"),
                "next_document_entry_raw_type": page.get("next_document_entry_raw_type"),
            })

        docs.append({
            "source_sha256": sha,
            "document_page_list_entry_count": int(receipt["document_page_list_entry_count"]),
            "confirmed_page_count": int(receipt["confirmed_page_count"]),
            "special_entry_count": int(receipt["special_entry_count"]),
            "master_relations": normalized_master_rows,
            "pages": page_rows,
        })

    def common(items: list[set[tuple]]) -> set[tuple]:
        return set.intersection(*items) if items else set()

    first_common = common(first_applied_feature_sets)
    second_common = common(second_applied_feature_sets)

    out = {
        "schema": SCHEMA,
        "source_count": len(docs),
        "documents": docs,
        "candidate_comparison": {
            "first_positive_applied_page_per_master_common_features": [
                feature_json(f) for f in sorted(first_common)
            ],
            "second_positive_applied_page_per_master_common_features": [
                feature_json(f) for f in sorted(second_common)
            ],
            "features_common_to_first_not_second": [
                feature_json(f) for f in sorted(first_common - second_common)
            ],
            "features_common_to_second_not_first": [
                feature_json(f) for f in sorted(second_common - first_common)
            ],
            "discovery_only": True,
            "first_applied_promoted_to_customer_authority": False,
        },
        "claims": {
            "measurement_only": True,
            "publisher_or_pdf_reference_used": False,
            "product_selection_changed": False,
            "raw_story_text_emitted": False,
            "raw_contents_seq_nums_emitted": False,
        },
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(out, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps(out["candidate_comparison"], indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
