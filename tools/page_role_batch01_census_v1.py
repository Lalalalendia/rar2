#!/usr/bin/env python3
from __future__ import annotations

import argparse
import csv
import hashlib
import json
from collections import Counter, defaultdict
from pathlib import Path
from typing import Any

SCHEMA = "chaptera.batch01-page-role-census.v1"
BROWSER_SCHEMA = "chaptera.publisher-visual-fingerprint-compare.v1"
PAGE_ROLE_SCHEMA = "chaptera.pub-page-role-observation.v1"
STANDARD_PROFILE = "publisher-mature-0x2c/standard-print-service-tail/v1"
INTERLEAVED_PROFILE = "publisher-mature-0x2c/standard-print-service-tail-interleaved/v1"
CANDIDATE_PROFILE = "research/nonzero-leader-single-visual-page-service-tail/v1"

RAW_SHAPE = 0x01
RAW_TABLE = 0x10
RAW_GROUP = 0x30


def load_json(path: Path) -> dict[str, Any]:
    return json.loads(path.read_text(encoding="utf-8"))


def oid_class(page: dict[str, Any]) -> str:
    a = page.get("oid_dword0")
    b = page.get("oid_dword1")
    if a is None or b is None:
        return "absent"
    return "zero" if a == 0 and b == 0 else "nonzero"


def child_count(page: dict[str, Any], raw_type: int) -> int:
    counts = page.get("child_raw_type_counts", {})
    return int(counts.get(str(raw_type), counts.get(raw_type, 0)))


def payload_count(page: dict[str, Any]) -> int:
    return (
        child_count(page, RAW_SHAPE)
        + child_count(page, RAW_TABLE)
        + child_count(page, RAW_GROUP)
    )


def pgid_count(receipt: dict[str, Any]) -> int:
    return sum(
        len(field.get("pgids", []))
        for controlling in receipt.get("controlling", [])
        for field in controlling.get("fields", [])
    )


def standard_profile_structural_hit(
    receipt: dict[str, Any],
) -> dict[str, Any] | None:
    """Mirror the source-shape part of the current bounded #451 selector.

    The product selector additionally requires effective-page scenario counts
    to be zero. This census conservatively requires zero decoded Pgid evidence
    and labels the result as a structural hit, not a product admission decision.
    """
    if receipt.get("schema") != PAGE_ROLE_SCHEMA:
        return None
    pages = sorted(
        receipt.get("pages", []), key=lambda p: int(p["document_ordinal"])
    )
    confirmed = int(receipt.get("confirmed_page_count", -1))
    entries = int(receipt.get("document_page_list_entry_count", -1))
    special = int(receipt.get("special_entry_count", -1))
    if (
        confirmed != len(pages)
        or entries != confirmed + special
        or special not in (0, 1)
    ):
        return None
    if pgid_count(receipt) != 0 or not pages:
        return None

    ordinals = [int(p["document_ordinal"]) for p in pages]
    if (
        len(set(ordinals)) != len(ordinals)
        or any(o < 0 or o >= entries for o in ordinals)
    ):
        return None
    seqs = [int(p["contents_seq_num"]) for p in pages]
    if len(set(seqs)) != len(seqs):
        return None

    master = pages[0]
    master_seq = master.get("contents_seq_num")
    if (
        int(master["document_ordinal"]) != 0
        or oid_class(master) != "zero"
        or master.get("applied_master_seq_num") is not None
    ):
        return None

    def applied_to_master(page: dict[str, Any]) -> bool:
        return page.get("applied_master_seq_num") == master_seq

    if special == 0:
        if len(pages) < 6 or ordinals != list(range(len(pages))):
            return None
        split = len(pages) - 4
        customers = pages[1:split]
        service = pages[split:]
        if not customers:
            return None
        if not all(
            oid_class(p) == "nonzero" and applied_to_master(p)
            for p in customers
        ):
            return None
        if not all(
            oid_class(p) == "zero" and applied_to_master(p)
            for p in service
        ):
            return None
        return {
            "profile_id": STANDARD_PROFILE,
            "predicted_customer_page_count": len(customers),
            "predicted_service_page_count": len(service),
        }

    if entries != 6 or confirmed != 5 or ordinals != [0, 1, 2, 4, 5]:
        return None
    customer = pages[1]
    service = pages[2:]
    if oid_class(customer) != "nonzero" or not applied_to_master(customer):
        return None
    if not all(
        oid_class(p) == "zero" and applied_to_master(p) for p in service
    ):
        return None
    return {
        "profile_id": INTERLEAVED_PROFILE,
        "predicted_customer_page_count": 1,
        "predicted_service_page_count": len(service),
    }


def candidate_service_scaffold_hit(
    receipt: dict[str, Any],
) -> dict[str, Any] | None:
    """Reproduce the #649/#652 research scaffold without promoting it."""
    if receipt.get("schema") != PAGE_ROLE_SCHEMA or pgid_count(receipt) != 0:
        return None
    pages = sorted(
        receipt.get("pages", []), key=lambda p: int(p["document_ordinal"])
    )
    if len(pages) < 5:
        return None
    special = int(receipt.get("special_entry_count", -1))
    entries = int(receipt.get("document_page_list_entry_count", -1))
    confirmed = int(receipt.get("confirmed_page_count", -1))
    if (
        special not in (0, 1)
        or confirmed != len(pages)
        or entries != confirmed + special
    ):
        return None

    leader, visible, service2 = pages[0], pages[1], pages[2]
    leader_seq = leader.get("contents_seq_num")
    ordinals = [int(p["document_ordinal"]) for p in pages]
    if not (
        leader["document_ordinal"] == 0
        and visible["document_ordinal"] == 1
        and service2["document_ordinal"] == 2
        and all(b > a for a, b in zip(ordinals, ordinals[1:]))
    ):
        return None
    if not (
        oid_class(leader) == "nonzero"
        and leader.get("applied_master_seq_num") is None
        and payload_count(leader) == 0
    ):
        return None
    if not (
        oid_class(visible) == "nonzero"
        and visible.get("applied_master_seq_num") == leader_seq
        and payload_count(visible) > 0
    ):
        return None
    if not (
        oid_class(service2) == "nonzero"
        and service2.get("applied_master_seq_num") == leader_seq
        and payload_count(service2) == 0
    ):
        return None
    tail = pages[3:]
    if not all(
        oid_class(p) == "zero"
        and p.get("applied_master_seq_num") == leader_seq
        and payload_count(p) == 0
        for p in tail
    ):
        return None
    return {
        "profile_id": CANDIDATE_PROFILE,
        "hypothetical_customer_page_count": 1,
        "hypothetical_service_page_count": len(pages) - 1,
        "research_only": True,
    }


def source_signature(receipt: dict[str, Any]) -> dict[str, Any]:
    pages = sorted(
        receipt.get("pages", []), key=lambda p: int(p["document_ordinal"])
    )
    leader_seq = pages[0].get("contents_seq_num") if pages else None

    def master_relation(page: dict[str, Any]) -> str:
        applied = page.get("applied_master_seq_num")
        if applied is None:
            return "none"
        return "leader" if applied == leader_seq else "other"

    page_classes = []
    for p in pages:
        shape = child_count(p, RAW_SHAPE)
        table = child_count(p, RAW_TABLE)
        group = child_count(p, RAW_GROUP)
        page_classes.append(
            {
                "ordinal": int(p["document_ordinal"]),
                "oid": oid_class(p),
                "master": master_relation(p),
                "pgt_type": p.get("pgt_type"),
                "payload": "positive" if shape + table + group > 0 else "zero",
                "shape": "positive" if shape > 0 else "zero",
                "group": "positive" if group > 0 else "zero",
                "table": "positive" if table > 0 else "zero",
                "previous_raw_type": p.get("previous_document_entry_raw_type"),
                "next_raw_type": p.get("next_document_entry_raw_type"),
            }
        )
    signature = {
        "entry_count": int(receipt.get("document_page_list_entry_count", 0)),
        "page_count": int(receipt.get("confirmed_page_count", 0)),
        "special_entry_count": int(receipt.get("special_entry_count", 0)),
        "pgid_count": pgid_count(receipt),
        "pages": page_classes,
    }
    packed = json.dumps(
        signature, sort_keys=True, separators=(",", ":")
    ).encode("utf-8")
    return {
        "id": hashlib.sha256(packed).hexdigest()[:16],
        "shape": signature,
    }


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("pairs_csv", type=Path)
    ap.add_argument("browser_summary", type=Path)
    ap.add_argument("receipt_dir", type=Path)
    ap.add_argument("output", type=Path)
    args = ap.parse_args()

    pairs = list(
        csv.DictReader(
            args.pairs_csv.open(newline="", encoding="utf-8-sig")
        )
    )
    if len(pairs) != 55:
        raise ValueError(f"expected 55 Batch01 pairs, got {len(pairs)}")

    browser = load_json(args.browser_summary)
    if browser.get("schema") != BROWSER_SCHEMA:
        raise ValueError(
            f"unexpected browser summary schema: {browser.get('schema')!r}"
        )
    browser_pairs = {
        row["fixture"]: row for row in browser.get("pairs", [])
    }
    if len(browser_pairs) != 55:
        raise ValueError(
            f"expected 55 browser pair rows, got {len(browser_pairs)}"
        )

    rows: list[dict[str, Any]] = []
    clusters: dict[str, list[str]] = defaultdict(list)
    standard_hits = Counter()
    mismatch_standard_hits = Counter()
    mismatch_candidate_hits: list[str] = []
    mismatch_residual: list[str] = []

    for pair in pairs:
        name = pair["basename"]
        b = browser_pairs.get(name)
        if b is None:
            raise ValueError(f"browser summary missing {name}")
        receipt_path = args.receipt_dir / f"{name}.json"
        if not receipt_path.exists():
            raise ValueError(f"page-role receipt missing {receipt_path}")
        receipt = load_json(receipt_path)
        if receipt.get("schema") != PAGE_ROLE_SCHEMA:
            raise ValueError(
                f"unexpected page-role schema for {name}: "
                f"{receipt.get('schema')!r}"
            )

        rendered = b.get("rendered") is True
        candidate_pages = b.get("candidate_pages") if rendered else None
        reference_pages = int(pair["pdf_pages"])
        if b.get("reference_pages") != reference_pages:
            raise ValueError(f"reference page count drift for {name}")
        mismatch = rendered and candidate_pages != reference_pages
        if mismatch and candidate_pages < reference_pages:
            raise ValueError(
                f"unexpected Reader under-admission in pinned baseline: {name}"
            )

        standard = standard_profile_structural_hit(receipt)
        candidate = candidate_service_scaffold_hit(receipt)
        signature = source_signature(receipt)
        clusters[signature["id"]].append(name)
        if standard:
            standard_hits[standard["profile_id"]] += 1
            if mismatch:
                mismatch_standard_hits[standard["profile_id"]] += 1
        if mismatch and candidate:
            mismatch_candidate_hits.append(name)
        if mismatch and not standard and not candidate:
            mismatch_residual.append(name)

        rows.append(
            {
                "fixture": name,
                "source_sha256": pair["pub_sha256"],
                "rendered_in_pinned_visual_baseline": rendered,
                "reader_candidate_pages": candidate_pages,
                "publisher_reference_pages": reference_pages,
                "reader_minus_reference_pages": (
                    candidate_pages - reference_pages if rendered else None
                ),
                "pinned_page_count_mismatch": mismatch,
                "raw_confirmed_page_count": receipt.get(
                    "confirmed_page_count"
                ),
                "document_page_list_entry_count": receipt.get(
                    "document_page_list_entry_count"
                ),
                "special_entry_count": receipt.get("special_entry_count"),
                "pgid_count": pgid_count(receipt),
                "standard_profile_structural_hit": standard,
                "research_candidate_scaffold_hit": candidate,
                "source_signature_id": signature["id"],
                "source_signature": signature["shape"],
            }
        )

    mismatch_rows = [
        r for r in rows if r["pinned_page_count_mismatch"]
    ]
    if len(mismatch_rows) != 31:
        raise ValueError(
            f"expected 31 pinned mismatches, got {len(mismatch_rows)}"
        )

    delta_hist = Counter(
        str(r["reader_minus_reference_pages"]) for r in mismatch_rows
    )
    cluster_rows = []
    row_by_fixture = {r["fixture"]: r for r in rows}
    row_by_signature = {r["source_signature_id"]: r for r in rows}
    for signature_id, fixtures in clusters.items():
        mismatch_fixtures = [
            name
            for name in fixtures
            if row_by_fixture[name]["pinned_page_count_mismatch"]
        ]
        representative = row_by_signature[signature_id]
        cluster_rows.append(
            {
                "source_signature_id": signature_id,
                "fixture_count": len(fixtures),
                "mismatch_fixture_count": len(mismatch_fixtures),
                "fixtures": sorted(fixtures),
                "mismatch_fixtures": sorted(mismatch_fixtures),
                "signature": representative["source_signature"],
            }
        )
    cluster_rows.sort(
        key=lambda r: (
            -r["mismatch_fixture_count"],
            -r["fixture_count"],
            r["source_signature_id"],
        )
    )

    out = {
        "schema": SCHEMA,
        "pinned_visual_baseline": {
            "run_id": 37110491071,
            "artifact_id": 11269199072,
            "summary_sha256": (
                "5a8110736dd5707402bc9f572f9603cb"
                "837bbb629d7e310cd957a4ae542a283f"
            ),
            "mismatch_pair_count": len(mismatch_rows),
            "over_admission_pair_count": sum(
                1
                for r in mismatch_rows
                if r["reader_minus_reference_pages"] > 0
            ),
            "under_admission_pair_count": sum(
                1
                for r in mismatch_rows
                if r["reader_minus_reference_pages"] < 0
            ),
            "reader_minus_reference_histogram": dict(
                sorted(delta_hist.items(), key=lambda kv: int(kv[0]))
            ),
        },
        "existing_profile_structural_hits": dict(sorted(standard_hits.items())),
        "mismatch_existing_profile_structural_hits": dict(
            sorted(mismatch_standard_hits.items())
        ),
        "mismatch_research_candidate_scaffold_count": len(
            mismatch_candidate_hits
        ),
        "mismatch_research_candidate_scaffold_fixtures": sorted(
            mismatch_candidate_hits
        ),
        "mismatch_residual_count": len(mismatch_residual),
        "mismatch_residual_fixtures": sorted(mismatch_residual),
        "cluster_count": len(cluster_rows),
        "clusters": cluster_rows,
        "fixtures": rows,
        "claims": {
            "measurement_only": True,
            "publisher_reference_page_count_used_as_product_authority": False,
            "publisher_reference_page_count_used_as_observational_falsifier": True,
            "research_candidate_promoted_to_product_rule": False,
            "raw_story_text_emitted": False,
            "raw_contents_seq_nums_emitted": False,
            "source_graph_mutated": False,
        },
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(
        json.dumps(out, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    print(
        json.dumps(
            {
                "mismatches": len(mismatch_rows),
                "histogram": out["pinned_visual_baseline"][
                    "reader_minus_reference_histogram"
                ],
                "mismatch_existing_profile_structural_hits": out[
                    "mismatch_existing_profile_structural_hits"
                ],
                "mismatch_research_candidate_scaffold_count": out[
                    "mismatch_research_candidate_scaffold_count"
                ],
                "mismatch_residual_count": out["mismatch_residual_count"],
                "cluster_count": out["cluster_count"],
            },
            indent=2,
            sort_keys=True,
        )
    )


if __name__ == "__main__":
    main()
