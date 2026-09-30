#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import subprocess
from pathlib import Path

import fitz

PAIRS = [
    {
        "id": "virginia-devinettes-2021",
        "pub": "virginia-devinettes.pub",
        "pdf": "virginia-devinettes-reference.pdf",
    },
    {
        "id": "virginia-remplacante-zone-a-2015",
        "pub": "virginia-remplacante-modifiable.pub",
        "pdf": "virginia-remplacante-zone-a-reference.pdf",
    },
]


def run_page_role(source: Path, output: Path) -> None:
    subprocess.run(
        [
            "cargo", "run", "--quiet",
            "--manifest-path", "vendor/producer-a/crates/pub-reader/Cargo.toml",
            "--bin", "page-role-receipt", "--",
            str(source), str(output),
        ],
        check=True,
    )


def project_current_scenario(receipt: dict) -> dict:
    pages = sorted(receipt["pages"], key=lambda page: page["document_ordinal"])
    document_order = [page["contents_seq_num"] for page in pages]

    pages_by_oid: dict[tuple[int, int], list[int]] = {}
    for page in pages:
        d0 = page.get("oid_dword0")
        d1 = page.get("oid_dword1")
        if d0 is None or d1 is None:
            continue
        pages_by_oid.setdefault((int(d0), int(d1)), []).append(
            int(page["contents_seq_num"])
        )

    pgid_lists = [
        [tuple(int(value) for value in pgid) for pgid in field.get("pgids", [])]
        for controlling in receipt.get("controlling", [])
        for field in controlling.get("fields", [])
        if field.get("id") == 6
    ]
    pgid_lists = [items for items in pgid_lists if items]

    base = {
        "raw_document_page_count": len(document_order),
        "raw_document_page_seq_nums": document_order,
        "scenario_evidence_list_count": len(pgid_lists),
    }
    if not pgid_lists:
        return {
            **base,
            "authority_state": "unavailable",
            "reason": "no_nonempty_controlling_page_list",
            "projected_page_seq_nums": [],
        }

    consensus = pgid_lists[0]
    if any(candidate != consensus for candidate in pgid_lists[1:]):
        return {
            **base,
            "authority_state": "unavailable",
            "reason": "controlling_page_lists_disagree",
            "projected_page_seq_nums": [],
        }

    resolved = []
    resolution = []
    for pgid in consensus:
        matches = pages_by_oid.get(pgid, [])
        resolution.append(
            {
                "pgid": list(pgid),
                "matching_page_seq_nums": matches,
            }
        )
        if len(matches) != 1:
            return {
                **base,
                "authority_state": "unavailable",
                "reason": "pgid_resolution_not_unique",
                "pgid_resolution": resolution,
                "projected_page_seq_nums": [],
            }
        resolved.append(matches[0])

    if len(set(resolved)) != len(resolved):
        return {
            **base,
            "authority_state": "unavailable",
            "reason": "scenario_repeats_physical_page",
            "pgid_resolution": resolution,
            "projected_page_seq_nums": [],
        }

    membership = set(resolved)
    projected = [seq for seq in document_order if seq in membership]
    if len(projected) != len(resolved):
        return {
            **base,
            "authority_state": "unavailable",
            "reason": "scenario_page_not_in_document_page_list",
            "pgid_resolution": resolution,
            "projected_page_seq_nums": projected,
        }

    return {
        **base,
        "authority_state": "resolved_current_scenario",
        "reason": None,
        "pgid_resolution": resolution,
        "pgid_list_order_seq_nums": resolved,
        "projected_page_seq_nums": projected,
        "projected_page_count": len(projected),
        "projection_law": "DOCUMENT order filtered by current OplControlling PageList Pgid -> Page.Oid membership",
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--pair-dir", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()

    args.out.mkdir(parents=True, exist_ok=True)
    rows = []
    for spec in PAIRS:
        pub = args.pair_dir / spec["pub"]
        pdf = args.pair_dir / spec["pdf"]
        if not pub.is_file() or not pdf.is_file():
            raise SystemExit(f"required exact pair is unavailable: {spec['id']}")

        raw_receipt = args.out / f"{spec['id']}-page-role.json"
        run_page_role(pub, raw_receipt)
        observation = json.loads(raw_receipt.read_text(encoding="utf-8"))
        scenario = project_current_scenario(observation)
        with fitz.open(pdf) as document:
            reference_pages = document.page_count

        projected_count = scenario.get("projected_page_count")
        pages = sorted(
            observation["pages"], key=lambda page: page["document_ordinal"]
        )
        positive_shape_nonzero_oid = [
            page["contents_seq_num"]
            for page in pages
            if (page.get("shape_child_count", 0) + page.get("group_child_count", 0) > 0)
            and (page.get("oid_dword0", 0) != 0 or page.get("oid_dword1", 0) != 0)
        ]
        content_oid_or_table = [
            page["contents_seq_num"]
            for page in pages
            if (
                (
                    page.get("shape_child_count", 0)
                    + page.get("group_child_count", 0)
                    > 0
                )
                and (
                    page.get("oid_dword0", 0) != 0
                    or page.get("oid_dword1", 0) != 0
                )
            )
            or page.get("child_raw_type_counts", {}).get("16", 0) > 0
        ]
        zero_oid_table_pages = [
            {
                "contents_seq_num": page["contents_seq_num"],
                "table_child_count": page.get("child_raw_type_counts", {}).get("16", 0),
            }
            for page in pages
            if page.get("oid_dword0", 0) == 0
            and page.get("oid_dword1", 0) == 0
            and page.get("child_raw_type_counts", {}).get("16", 0) > 0
        ]

        discovery_selected = set(positive_shape_nonzero_oid)
        auxiliary_profiles = []
        customer_candidate_profiles = []
        for page in pages:
            profile = {
                "document_ordinal": page["document_ordinal"],
                "contents_seq_num": page["contents_seq_num"],
                "oid": [
                    page.get("oid_dword0", 0),
                    page.get("oid_dword1", 0),
                ],
                "pgt_type": page.get("pgt_type"),
                "applied_master_seq_num": page.get("applied_master_seq_num"),
                "applied_master_raw_type": page.get("applied_master_raw_type"),
                "previous_document_entry_raw_type": page.get(
                    "previous_document_entry_raw_type"
                ),
                "next_document_entry_raw_type": page.get(
                    "next_document_entry_raw_type"
                ),
                "adjacent_to_page_list_special_0x59": (
                    page.get("previous_document_entry_raw_type") == 0x59
                    or page.get("next_document_entry_raw_type") == 0x59
                ),
                "shape_child_count": page.get("shape_child_count", 0),
                "group_child_count": page.get("group_child_count", 0),
                "child_raw_type_counts": page.get("child_raw_type_counts", {}),
            }
            if page["contents_seq_num"] in discovery_selected:
                customer_candidate_profiles.append(profile)
            else:
                auxiliary_profiles.append(profile)

        document_entry_raw_types = [
            entry.get("raw_type")
            for entry in observation.get("document_entries", [])
        ]
        rows.append(
            {
                "pair_id": spec["id"],
                "raw_page_count": scenario["raw_document_page_count"],
                "reference_pdf_page_count": reference_pages,
                "roles_unresolved_at_baseline": True,
                "scenario_authority_state": scenario["authority_state"],
                "scenario_reason": scenario["reason"],
                "scenario_evidence_list_count": scenario["scenario_evidence_list_count"],
                "scenario_projected_page_count": projected_count,
                "scenario_projected_page_seq_nums": scenario["projected_page_seq_nums"],
                "scenario_count_matches_reference": (
                    projected_count == reference_pages
                    if projected_count is not None
                    else None
                ),
                "raw_count_delta_vs_reference": (
                    scenario["raw_document_page_count"] - reference_pages
                ),
                "authority": (
                    scenario.get("projection_law")
                    if scenario["authority_state"] == "resolved_current_scenario"
                    else None
                ),
                "discovery_only": {
                    "positive_shape_nonzero_oid_seq_nums": positive_shape_nonzero_oid,
                    "positive_shape_nonzero_oid_count": len(
                        positive_shape_nonzero_oid
                    ),
                    "positive_shape_nonzero_oid_count_matches_reference": (
                        len(positive_shape_nonzero_oid) == reference_pages
                    ),
                    "content_oid_or_table_seq_nums": content_oid_or_table,
                    "content_oid_or_table_count": len(content_oid_or_table),
                    "content_oid_or_table_count_matches_reference": (
                        len(content_oid_or_table) == reference_pages
                    ),
                    "zero_oid_table_pages": zero_oid_table_pages,
                    "document_page_list_raw_types": document_entry_raw_types,
                    "customer_candidate_profiles": customer_candidate_profiles,
                    "auxiliary_profiles": auxiliary_profiles,
                    "auxiliary_page_count": len(auxiliary_profiles),
                    "auxiliary_all_zero_oid": all(
                        profile["oid"] == [0, 0]
                        for profile in auxiliary_profiles
                    ),
                    "auxiliary_adjacent_0x59_count": sum(
                        profile["adjacent_to_page_list_special_0x59"]
                        for profile in auxiliary_profiles
                    ),
                    "customer_candidate_adjacent_0x59_count": sum(
                        profile["adjacent_to_page_list_special_0x59"]
                        for profile in customer_candidate_profiles
                    ),
                    "promoted_to_authority": False,
                },
            }
        )

    summary = {
        "schema": "chaptera.viewer-page-role-virginia-validation.v1",
        "purpose": "validate existing generic source-semantic page-role authority against exact author PUB-PDF pairs",
        "pairs": rows,
        "claims": {
            "pdf_page_count_is_validation_only": True,
            "pdf_page_count_is_not_page_role_authority": True,
            "libmspub_magic_constants_used": False,
            "cloud_specific_page_filter_used": False,
            "service_role_profiles_are_discovery_only": True,
            "raw_pub_bytes_emitted": False,
            "raw_story_text_emitted": False,
        },
    }
    target = args.out / "summary.json"
    target.write_text(
        json.dumps(summary, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    print(json.dumps(summary, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
