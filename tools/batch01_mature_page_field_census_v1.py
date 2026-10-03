#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
from collections import Counter
from pathlib import Path
from typing import Any

SCHEMA = "chaptera.batch01-mature-page-field-census.v1"
PAGE_ROLE_SCHEMA = "chaptera.pub-page-role-observation.v1"
VISUAL_SCHEMA = "chaptera.publisher-visual-fingerprint-compare.v1"

RAW_SHAPE = 0x01
RAW_TABLE = 0x10
RAW_GROUP = 0x30


def load_json(path: Path) -> dict[str, Any]:
    return json.loads(path.read_text(encoding="utf-8"))


def payload_count(page: dict[str, Any]) -> int:
    counts = page.get("child_raw_type_counts", {})
    def c(raw: int) -> int:
        return int(counts.get(str(raw), counts.get(raw, 0)))
    return c(RAW_SHAPE) + c(RAW_TABLE) + c(RAW_GROUP)


def oid_class(page: dict[str, Any]) -> str:
    a = page.get("oid_dword0")
    b = page.get("oid_dword1")
    if a is None or b is None:
        return "absent"
    if a == 0 and b == 0:
        return "zero"
    return f"nonzero:{a}"


def fields_with_id(page: dict[str, Any], field_id: int) -> list[dict[str, Any]]:
    rows = []
    for field in page.get("fields", []):
        if int(field.get("id", -1)) != field_id:
            continue
        row = {
            "id": field_id,
            "block_type": int(field["block_type"]),
        }
        for key in ("u16_value", "u32_value", "declared_length"):
            if key in field:
                row[key] = int(field[key])
        rows.append(row)
    rows.sort(key=lambda r: (r["block_type"], json.dumps(r, sort_keys=True)))
    return rows


def pgid_count(receipt: dict[str, Any]) -> int:
    total = 0
    for controlling in receipt.get("controlling", []):
        for field in controlling.get("fields", []):
            total += len(field.get("pgids", []))
    return total


def source_shape_tag(receipt: dict[str, Any]) -> str:
    pages = sorted(receipt.get("pages", []), key=lambda p: int(p["document_ordinal"]))
    if not pages:
        return "empty"
    leader_seq = pages[0].get("contents_seq_num")

    tags: list[str] = []
    for page in pages:
        applied = page.get("applied_master_seq_num")
        if applied is None:
            rel = "N"
        elif applied == leader_seq:
            rel = "L"
        else:
            rel = "O"
        tags.append(
            f"{oid_class(page)}:{rel}:{'P' if payload_count(page) else 'E'}"
        )
    return "|".join(tags)


def generalized_897_family(receipt: dict[str, Any]) -> dict[str, Any] | None:
    """Source-only recurrence tag from #897; never product authority."""
    if receipt.get("schema") != PAGE_ROLE_SCHEMA or pgid_count(receipt) != 0:
        return None
    pages = sorted(receipt.get("pages", []), key=lambda p: int(p["document_ordinal"]))
    if len(pages) < 5:
        return None

    leader = pages[0]
    leader_seq = leader.get("contents_seq_num")
    if oid_class(leader) == "zero" or leader.get("applied_master_seq_num") is not None:
        return None
    if payload_count(leader) != 0:
        return None

    i = 1
    customer = []
    while i < len(pages):
        page = pages[i]
        if (
            oid_class(page) != "zero"
            and page.get("applied_master_seq_num") == leader_seq
            and payload_count(page) > 0
        ):
            customer.append(page)
            i += 1
            continue
        break
    if not customer or i >= len(pages):
        return None

    service = pages[i]
    if not (
        oid_class(service) != "zero"
        and service.get("applied_master_seq_num") == leader_seq
        and payload_count(service) == 0
    ):
        return None
    i += 1

    tail = pages[i:]
    if not tail:
        return None
    if not all(
        oid_class(page) == "zero"
        and page.get("applied_master_seq_num") == leader_seq
        and payload_count(page) == 0
        for page in tail
    ):
        return None

    return {
        "customer_payload_page_count": len(customer),
        "customer_ordinals": [int(p["document_ordinal"]) for p in customer],
        "service_ordinal": int(service["document_ordinal"]),
        "tail_ordinals": [int(p["document_ordinal"]) for p in tail],
        "research_only": True,
    }


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("visual_summary", type=Path)
    ap.add_argument("receipt_dir", type=Path)
    ap.add_argument("output", type=Path)
    args = ap.parse_args()

    visual = load_json(args.visual_summary)
    if visual.get("schema") != VISUAL_SCHEMA:
        raise ValueError(f"unexpected visual schema: {visual.get('schema')!r}")

    mismatch_rows = [
        row for row in visual.get("pairs", [])
        if row.get("rendered") is True
        and row.get("candidate_pages") != row.get("reference_pages")
    ]
    if len(mismatch_rows) != 23:
        raise ValueError(f"expected 23 current mismatches, got {len(mismatch_rows)}")

    fixtures = []
    recurrence_count = 0
    field_pair_presence = Counter()
    source_shape_counts = Counter()

    for visual_row in sorted(mismatch_rows, key=lambda r: r["fixture"]):
        fixture = visual_row["fixture"]
        path = args.receipt_dir / f"{fixture}.json"
        if not path.exists():
            raise ValueError(f"missing receipt: {path}")
        receipt = load_json(path)
        if receipt.get("schema") != PAGE_ROLE_SCHEMA:
            raise ValueError(f"unexpected page-role schema for {fixture}")

        pages = sorted(receipt.get("pages", []), key=lambda p: int(p["document_ordinal"]))
        if not pages:
            raise ValueError(f"no PAGE observations for {fixture}")

        leader_seq = pages[0].get("contents_seq_num")
        page_rows = []
        for page in pages:
            f1 = fields_with_id(page, 0x01)
            f2 = fields_with_id(page, 0x02)
            field_pair_presence[
                (
                    bool(f1),
                    bool(f2),
                    oid_class(page),
                    page.get("applied_master_seq_num") == leader_seq,
                    payload_count(page) > 0,
                )
            ] += 1
            page_rows.append(
                {
                    "document_ordinal": int(page["document_ordinal"]),
                    "oid_class": oid_class(page),
                    "applied_master_relation": (
                        "none"
                        if page.get("applied_master_seq_num") is None
                        else "leader"
                        if page.get("applied_master_seq_num") == leader_seq
                        else "other"
                    ),
                    "payload_count": payload_count(page),
                    "field_0x01": f1,
                    "field_0x02": f2,
                    "previous_document_entry_raw_type": page.get(
                        "previous_document_entry_raw_type"
                    ),
                    "next_document_entry_raw_type": page.get(
                        "next_document_entry_raw_type"
                    ),
                }
            )

        recurrence = generalized_897_family(receipt)
        if recurrence:
            recurrence_count += 1
        source_shape = source_shape_tag(receipt)
        source_shape_counts[source_shape] += 1

        fixtures.append(
            {
                "fixture": fixture,
                "reader_candidate_pages": int(visual_row["candidate_pages"]),
                "publisher_reference_pages_validation_only": int(
                    visual_row["reference_pages"]
                ),
                "reader_minus_reference_pages_validation_only": int(
                    visual_row["candidate_pages"] - visual_row["reference_pages"]
                ),
                "document_page_list_entry_count": int(
                    receipt.get("document_page_list_entry_count", 0)
                ),
                "confirmed_page_count": int(receipt.get("confirmed_page_count", 0)),
                "special_entry_count": int(receipt.get("special_entry_count", 0)),
                "scenario_pgid_count": pgid_count(receipt),
                "source_shape_tag": source_shape,
                "generalized_897_recurrence": recurrence,
                "pages": page_rows,
            }
        )

    summary_rows = []
    for key, count in sorted(
        field_pair_presence.items(),
        key=lambda item: (-item[1], item[0]),
    ):
        has_f1, has_f2, oid, applies_leader, has_payload = key
        summary_rows.append(
            {
                "page_count": count,
                "field_0x01_present": has_f1,
                "field_0x02_present": has_f2,
                "oid_class": oid,
                "applies_leader": applies_leader,
                "has_visual_payload": has_payload,
            }
        )

    output = {
        "schema": SCHEMA,
        "source_visual_run_id": 37129422737,
        "source_visual_artifact_id": 11276453525,
        "mismatch_fixture_count": len(fixtures),
        "generalized_897_recurrence_count": recurrence_count,
        "source_shape_cluster_count": len(source_shape_counts),
        "source_shape_clusters": [
            {"source_shape_tag": tag, "fixture_count": count}
            for tag, count in source_shape_counts.most_common()
        ],
        "field_presence_summary": summary_rows,
        "fixtures": fixtures,
        "claims": {
            "measurement_only": True,
            "publisher_reference_page_count_used_as_product_authority": False,
            "publisher_reference_page_count_validation_only": True,
            "page_suppression_changed": False,
            "product_semantics_changed": False,
            "raw_story_text_emitted": False,
            "raw_contents_seq_nums_emitted": False,
            "cpo_rgohpo_promoted_to_authority": False,
        },
    }

    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(
        json.dumps(output, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    print(json.dumps({
        "mismatch_fixture_count": len(fixtures),
        "generalized_897_recurrence_count": recurrence_count,
        "source_shape_cluster_count": len(source_shape_counts),
        "field_presence_summary": summary_rows,
    }, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
