#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
from collections import Counter
from pathlib import Path

SCHEMA = "chaptera.pub-vba-migration-plan.v1"
MAPPING_VERSION = "v1"

# The planner is deliberately family-level. It never consumes or emits recovered
# VBA source text. The scanner remains the sole MS-OVBA/source extraction layer.
FAMILY_MAP: dict[str, dict[str, str]] = {
    "application_lifecycle": {
        "disposition": "automatic",
        "target": "cli_api_lifecycle",
        "recipe": "file_lifecycle",
        "reason": "Replace CreateObject/Open/Close/Quit orchestration with Chaptera CLI/API lifecycle.",
    },
    "documents": {
        "disposition": "automatic",
        "target": "document_scope",
        "recipe": "file_lifecycle",
        "reason": "Document access maps to explicit Chaptera document/file scope.",
    },
    "pages": {
        "disposition": "automatic",
        "target": "page_query",
        "recipe": "page_query",
        "reason": "Page traversal maps to stable page queries/ranges.",
    },
    "page_identity": {
        "disposition": "automatic",
        "target": "stable_page_identity",
        "recipe": "computed_page_reference",
        "reason": "PageID/PageNumber maps to stable page identity plus computed display references.",
    },
    "page_lifecycle": {
        "disposition": "automatic",
        "target": "page_actions",
        "recipe": "page_lifecycle",
        "reason": "Add/duplicate/delete/move maps to typed page lifecycle actions.",
    },
    "scratch_area": {
        "disposition": "user_choice",
        "target": "off_page_object_scope",
        "recipe": "shape_scope",
        "reason": "ScratchArea semantics require an explicit choice about off-page object scope.",
    },
    "shapes": {
        "disposition": "automatic",
        "target": "shape_query_action",
        "recipe": "shape_transform",
        "reason": "Shape traversal maps to object queries plus typed mutations.",
    },
    "selection": {
        "disposition": "user_choice",
        "target": "explicit_selector",
        "recipe": "shape_or_text_selector",
        "reason": "UI Selection is ambient state; migration must replace it with an explicit selector.",
    },
    "text": {
        "disposition": "automatic",
        "target": "text_actions",
        "recipe": "text_transform",
        "reason": "TextFrame/TextRange/Find maps to text query, replace and formatting actions.",
    },
    "picture": {
        "disposition": "automatic",
        "target": "replace_picture",
        "recipe": "asset_replace",
        "reason": "Picture replacement maps to explicit asset replacement with frame/crop policy.",
    },
    "tables": {
        "disposition": "automatic",
        "target": "table_actions",
        "recipe": "table_transform",
        "reason": "Table row/column/cell operations map to typed table actions.",
    },
    "mail_merge": {
        "disposition": "automatic",
        "target": "data_recipe",
        "recipe": "data_recipe",
        "reason": "MailMerge/DataSource semantics map to record iteration, filters and fields.",
    },
    "layout": {
        "disposition": "automatic",
        "target": "layout_actions",
        "recipe": "layout_transform",
        "reason": "Guides/align/distribute maps to explicit deterministic layout actions.",
    },
    "output": {
        "disposition": "automatic",
        "target": "deterministic_output",
        "recipe": "batch_output",
        "reason": "Save/export/print maps to output recipes with explicit geometry and policy.",
    },
    "hyperlinks": {
        "disposition": "automatic",
        "target": "link_actions",
        "recipe": "link_refresh",
        "reason": "Hyperlink target/display updates map to typed link and computed-reference actions.",
    },
    "linked_text": {
        "disposition": "automatic",
        "target": "story_frame_link_actions",
        "recipe": "story_flow",
        "reason": "Linked text frame/story traversal maps to story/frame-link operations.",
    },
    "metadata_selectors": {
        "disposition": "automatic",
        "target": "automation_tags",
        "recipe": "semantic_selector",
        "reason": "Tags/AlternativeText map to stable semantic selectors and automation tags.",
    },
    "ole_links": {
        "disposition": "user_choice",
        "target": "external_resource_policy",
        "recipe": "external_resource_refresh",
        "reason": "OLE/link refresh must become explicit inspect/relink/update policy; never implicit COM activation.",
    },
}

COMPOSITE_RECIPES: tuple[dict[str, object], ...] = (
    {
        "id": "data_recipe_batch_output",
        "requires": {"mail_merge", "output"},
        "target": "Data Recipe + Batch Output",
        "reason": "Record-driven Publisher output can become deterministic one-record/batch materialization.",
    },
    {
        "id": "computed_page_reference_refresh",
        "requires": {"pages", "page_identity", "hyperlinks"},
        "target": "Stable Page Identity + Computed References",
        "reason": "Page-number/bookmark maintenance can become generated references with explicit refresh.",
    },
    {
        "id": "bulk_picture_replace",
        "requires": {"shapes", "picture"},
        "target": "Bulk Asset Replace",
        "reason": "Shape traversal plus picture replacement maps to a selector-driven asset recipe.",
    },
    {
        "id": "data_bound_table",
        "requires": {"mail_merge", "tables"},
        "target": "Data-bound Table Recipe",
        "reason": "Datasource plus table operations map to repeated/populated table rows.",
    },
    {
        "id": "batch_file_output",
        "requires": {"documents", "output"},
        "target": "Batch File Output",
        "reason": "Document lifecycle plus output calls map to file-set iteration and deterministic export.",
    },
)


def _family_plan(family: str, count: int) -> dict:
    mapping = FAMILY_MAP.get(family)
    if mapping is None:
        return {
            "family": family,
            "count": count,
            "disposition": "unsupported",
            "target": None,
            "recipe": None,
            "reason": "No bounded Publisher-to-Chaptera family mapping exists in mapping v1.",
        }
    return {
        "family": family,
        "count": count,
        **mapping,
    }


def _composite_candidates(families: set[str]) -> list[dict]:
    rows = []
    for candidate in COMPOSITE_RECIPES:
        required = set(candidate["requires"])
        if required.issubset(families):
            rows.append(
                {
                    "id": candidate["id"],
                    "requires": sorted(required),
                    "target": candidate["target"],
                    "reason": candidate["reason"],
                }
            )
    return rows


def build_plan(scan: dict) -> dict:
    if scan.get("schema") != "chaptera.pub-vba-estate-scan.v1":
        raise ValueError("unsupported scanner schema")
    claims = scan.get("claims") or {}
    if claims.get("vba_executed") is not False:
        raise ValueError("scanner receipt must prove vba_executed=false")
    if claims.get("ole_com_activated") is not False:
        raise ValueError("scanner receipt must prove ole_com_activated=false")
    if claims.get("source_text_emitted") is not False:
        raise ValueError("scanner receipt must prove source_text_emitted=false")

    files = []
    disposition_hits: Counter[str] = Counter()
    disposition_files: Counter[str] = Counter()
    call_bearing_files = 0
    vba_project_files = 0

    for row in scan.get("files") or []:
        family_counts = {
            str(k): int(v)
            for k, v in sorted((row.get("call_families") or {}).items())
            if int(v) > 0
        }
        plans = [_family_plan(family, count) for family, count in family_counts.items()]
        buckets = {"automatic": [], "user_choice": [], "unsupported": []}
        for plan in plans:
            buckets[plan["disposition"]].append(plan)
            disposition_hits[plan["disposition"]] += int(plan["count"])
        for name, items in buckets.items():
            if items:
                disposition_files[name] += 1

        if family_counts:
            call_bearing_files += 1
        if row.get("vba_state") in {"structural_only", "source_partial", "source_extracted"}:
            vba_project_files += 1

        files.append(
            {
                "sha256": row.get("sha256"),
                "byte_len": row.get("byte_len"),
                "vba_state": row.get("vba_state"),
                "vba_project_count": int(row.get("vba_project_count") or 0),
                "call_families": family_counts,
                "plan": buckets,
                "recipe_candidates": _composite_candidates(set(family_counts)),
            }
        )

    return {
        "schema": SCHEMA,
        "mapping_version": MAPPING_VERSION,
        "source": {
            "scanner_schema": scan.get("schema"),
            "call_classifier_version": scan.get("call_classifier_version"),
        },
        "claims": {
            "vba_executed": False,
            "ole_com_activated": False,
            "source_text_emitted": False,
            "source_paths_emitted": False,
            "migration_plan_executes_code": False,
            "automatic_means_semantic_family_mapping_not_full_program_translation": True,
        },
        "summary": {
            "file_count": len(files),
            "vba_project_files": vba_project_files,
            "call_bearing_files": call_bearing_files,
            "family_hit_dispositions": dict(sorted(disposition_hits.items())),
            "files_by_disposition": dict(sorted(disposition_files.items())),
        },
        "files": files,
    }


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Build a source-free Chaptera migration plan from an inert Publisher VBA scanner receipt."
    )
    parser.add_argument("--scan", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()

    scan = json.loads(args.scan.read_text(encoding="utf-8"))
    plan = build_plan(scan)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(plan, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps(plan["summary"], indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
