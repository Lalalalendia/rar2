#!/usr/bin/env python3
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path

SCHEMA = "chaptera.mature-029-native-page-spread-oracle.v1"
OUT_SCHEMA = "chaptera.mature-029-reference-surface-classification.v1"
EXPECTED_SOURCE_SHA256 = "c0688f73b9bf8fc7677a1eecaadd30fa00fc1813f6b12dc39b8aa5eac973f81e"
EXPECTED_SOURCE_BYTES = 496640
EXPECTED_PUBLISHER_VERSION = "16.0"
EXPECTED_PUBLISHER_BUILD = "12527"
REFERENCE_PAGE_COUNT = 2
BOOKLET_PRINT_STYLES = {5: "booklet_side_fold", 6: "booklet_top_fold"}
KNOWN_029_PAGE_SEQNUMS = {264, 265, 266, 267, 269, 279, 296, 297, 298}


def state_value(record: object) -> object | None:
    if not isinstance(record, dict) or record.get("state") != "value":
        return None
    return record.get("value")


def require_read_only_contract(payload: dict) -> None:
    source = payload.get("source")
    if not isinstance(source, dict):
        raise ValueError("missing source record")
    if source.get("sha256") != EXPECTED_SOURCE_SHA256:
        raise ValueError("unexpected source SHA-256")
    if source.get("size") != EXPECTED_SOURCE_BYTES:
        raise ValueError("unexpected source byte length")
    if source.get("unchanged_after_probe") is not True:
        raise ValueError("source immutability not proven")

    claims = payload.get("claims")
    if not isinstance(claims, dict):
        raise ValueError("missing claims record")
    forbidden = (
        "document_mutation_invoked",
        "save_invoked",
        "print_invoked",
        "export_invoked",
        "macro_execution_invoked",
        "external_link_update_invoked",
    )
    for key in forbidden:
        if claims.get(key) is not False:
            raise ValueError(f"read-only contract violated: {key}")
    if claims.get("opened_read_only") is not True:
        raise ValueError("read-only open was not proven")

    publisher = payload.get("publisher")
    if not isinstance(publisher, dict):
        raise ValueError("missing publisher identity")
    if state_value(publisher.get("version")) != EXPECTED_PUBLISHER_VERSION:
        raise ValueError("unexpected Publisher version")
    if str(state_value(publisher.get("build"))) != EXPECTED_PUBLISHER_BUILD:
        raise ValueError("unexpected Publisher build")


def classify(payload: dict) -> dict:
    if payload.get("schema") != SCHEMA:
        raise ValueError(f"unsupported native receipt schema: {payload.get('schema')!r}")
    require_read_only_contract(payload)

    document = payload.get("document")
    if not isinstance(document, dict):
        raise ValueError("missing document record")

    pages_count = state_value(document.get("pages_count"))
    print_style = state_value(document.get("print_style"))
    view_two_page_spread = state_value(document.get("view_two_page_spread"))

    logical_page_count = int(pages_count) if isinstance(pages_count, int) else None
    print_style_value = int(print_style) if isinstance(print_style, int) else None
    booklet_kind = BOOKLET_PRINT_STYLES.get(print_style_value)

    pages = payload.get("pages")
    if not isinstance(pages, list):
        raise ValueError("pages must be a list")
    if logical_page_count is not None and len(pages) != logical_page_count:
        raise ValueError("native page-row count does not match Document.Pages.Count")

    two_page_spread_rows = 0
    spread_access_ok_rows = 0
    page_id_bitpack_match_count = 0
    x_offsets = []
    spread_signatures = set()

    for row in pages:
        if not isinstance(row, dict):
            raise ValueError("page row must be an object")

        page_id = state_value(row.get("page_id"))
        if isinstance(page_id, int):
            seq = page_id & 0x00FFFFFF
            if (page_id & 0xFF000000) == 0x02000000 and seq in KNOWN_029_PAGE_SEQNUMS:
                page_id_bitpack_match_count += 1

        xoff = state_value(row.get("x_offset_within_reader_spread"))
        if isinstance(xoff, (int, float)):
            x_offsets.append(float(xoff))

        spread = row.get("reader_spread")
        if isinstance(spread, dict) and spread.get("access") == "ok":
            spread_access_ok_rows += 1
            spread_count = state_value(spread.get("page_count"))
            if spread_count == 2:
                two_page_spread_rows += 1
            members = spread.get("page_ids")
            if isinstance(members, list):
                ids = []
                for member in members:
                    if isinstance(member, dict) and isinstance(member.get("page_id"), int):
                        ids.append(int(member["page_id"]))
                if ids:
                    raw = ",".join(str(value) for value in ids).encode("ascii")
                    spread_signatures.add(hashlib.sha256(raw).hexdigest())

    stage = "unknown"
    reason = "native evidence does not yet prove the Publisher PDF output surface stage"

    if (
        booklet_kind is not None
        and logical_page_count is not None
        and logical_page_count > 0
        and logical_page_count % 2 == 0
        and REFERENCE_PAGE_COUNT * 2 == logical_page_count
    ):
        stage = "production_sheet"
        reason = (
            "native Publisher reports booklet PrintStyle and twice as many logical Pages "
            "as the exact 029 Publisher PDF output pages"
        )

    result = {
        "schema": OUT_SCHEMA,
        "source_sha256": EXPECTED_SOURCE_SHA256,
        "reference_page_count": REFERENCE_PAGE_COUNT,
        "logical_page_count": logical_page_count,
        "print_style": print_style_value,
        "booklet_intent": {
            "confirmed": booklet_kind is not None,
            "kind": booklet_kind,
        },
        "view_two_page_spread": view_two_page_spread,
        "reader_spread": {
            "page_row_count": len(pages),
            "spread_access_ok_row_count": spread_access_ok_rows,
            "two_page_spread_row_count": two_page_spread_rows,
            "unique_spread_signature_count": len(spread_signatures),
            "distinct_x_offsets": sorted(set(x_offsets)),
        },
        "page_id_validation": {
            "same_build_bitpack_candidate_match_count": page_id_bitpack_match_count,
            "same_build_bitpack_candidate_full_match": (
                logical_page_count is not None
                and logical_page_count > 0
                and page_id_bitpack_match_count == logical_page_count
            ),
            "product_authority": False,
        },
        "reference_surface_stage": stage,
        "classification_reason": reason,
        "claims": {
            "classification_uses_native_publisher_state": True,
            "raster_similarity_used": False,
            "pdf_page_count_used_as_pub_semantics": False,
            "reference_page_count_used_only_to_classify_output_surface_stage": True,
            "page_id_bitpack_candidate_is_validation_only": True,
            "unknown_is_fail_closed": True,
        },
    }
    return result


def self_test() -> None:
    def safe(value):
        return {"state": "value", "value": value}

    def receipt(pages_count: int, print_style: int | None) -> dict:
        rows = []
        for index in range(pages_count):
            seq = 296 + index
            rows.append({
                "page_id": safe(0x02000000 | seq),
                "x_offset_within_reader_spread": safe(float(index % 2) * 420.0),
                "reader_spread": {
                    "access": "ok",
                    "page_count": safe(2),
                    "page_ids": [
                        {"page_id": 0x02000128},
                        {"page_id": 0x02000129},
                    ],
                },
            })
        return {
            "schema": SCHEMA,
            "source": {
                "sha256": EXPECTED_SOURCE_SHA256,
                "size": EXPECTED_SOURCE_BYTES,
                "unchanged_after_probe": True,
            },
            "publisher": {
                "version": safe(EXPECTED_PUBLISHER_VERSION),
                "build": safe(EXPECTED_PUBLISHER_BUILD),
            },
            "document": {
                "pages_count": safe(pages_count),
                "print_style": safe(print_style) if print_style is not None else {"state": "error"},
                "view_two_page_spread": safe(True),
            },
            "pages": rows,
            "claims": {
                "opened_read_only": True,
                "document_mutation_invoked": False,
                "save_invoked": False,
                "print_invoked": False,
                "export_invoked": False,
                "macro_execution_invoked": False,
                "external_link_update_invoked": False,
            },
        }

    side = classify(receipt(4, 5))
    assert side["reference_surface_stage"] == "production_sheet"
    assert side["booklet_intent"]["kind"] == "booklet_side_fold"

    top = classify(receipt(4, 6))
    assert top["reference_surface_stage"] == "production_sheet"
    assert top["booklet_intent"]["kind"] == "booklet_top_fold"

    normal = classify(receipt(2, 1))
    assert normal["reference_surface_stage"] == "unknown"
    assert normal["booklet_intent"]["confirmed"] is False

    missing = classify(receipt(4, None))
    assert missing["reference_surface_stage"] == "unknown"

    bad = receipt(4, 5)
    bad["claims"]["save_invoked"] = True
    try:
        classify(bad)
    except ValueError:
        pass
    else:
        raise AssertionError("mutation contract violation must fail")

    bad_source = receipt(4, 5)
    bad_source["source"]["sha256"] = "0" * 64
    try:
        classify(bad_source)
    except ValueError:
        pass
    else:
        raise AssertionError("source mismatch must fail")

    print("mature 029 native receipt classifier self-test: ok")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("receipt", nargs="?", type=Path)
    parser.add_argument("--out", type=Path)
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()

    if args.self_test:
        self_test()
        return

    if args.receipt is None:
        parser.error("receipt is required unless --self-test is used")

    result = classify(json.loads(args.receipt.read_text(encoding="utf-8")))
    text = json.dumps(result, indent=2, sort_keys=True) + "\n"
    if args.out is not None:
        args.out.parent.mkdir(parents=True, exist_ok=True)
        args.out.write_text(text, encoding="utf-8")
    print(text, end="")


if __name__ == "__main__":
    main()
