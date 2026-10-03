#!/usr/bin/env python3
from __future__ import annotations

import importlib.util
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
MODULE_PATH = ROOT / "tools" / "pub_vba_migration_plan.py"
spec = importlib.util.spec_from_file_location("pub_vba_migration_plan", MODULE_PATH)
module = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = module
assert spec.loader is not None
spec.loader.exec_module(module)


def receipt(files):
    return {
        "schema": "chaptera.pub-vba-estate-scan.v1",
        "call_classifier_version": "v2.2",
        "claims": {
            "vba_executed": False,
            "ole_com_activated": False,
            "source_text_emitted": False,
        },
        "totals": {"file_count": len(files)},
        "files": files,
    }


def main() -> int:
    scan = receipt(
        [
            {
                "path": "/private/do-not-emit/sample.pub",
                "sha256": "a" * 64,
                "byte_len": 12345,
                "vba_state": "source_extracted",
                "vba_project_count": 1,
                "call_families": {
                    "documents": 2,
                    "pages": 4,
                    "page_identity": 3,
                    "hyperlinks": 5,
                    "selection": 1,
                    "output": 2,
                    "ole_links": 1,
                    "future_unknown_family": 7,
                },
                "symbols": {"ThisDocument": 2},
            },
            {
                "sha256": "b" * 64,
                "byte_len": 45678,
                "vba_state": "source_extracted",
                "vba_project_count": 1,
                "call_families": {
                    "mail_merge": 6,
                    "output": 3,
                    "tables": 4,
                    "shapes": 2,
                    "picture": 1,
                },
            },
            {
                "sha256": "c" * 64,
                "byte_len": 999,
                "vba_state": "non_project_vba_storage",
                "vba_project_count": 0,
                "call_families": {},
            },
        ]
    )
    plan = module.build_plan(scan)
    assert plan["schema"] == "chaptera.pub-vba-migration-plan.v1"
    assert plan["mapping_version"] == "v1"
    assert plan["claims"]["vba_executed"] is False
    assert plan["claims"]["migration_plan_executes_code"] is False
    assert plan["summary"]["file_count"] == 3
    assert plan["summary"]["vba_project_files"] == 2
    assert plan["summary"]["call_bearing_files"] == 2

    first = plan["files"][0]
    assert "path" not in first
    assert "symbols" not in first
    auto = {x["family"] for x in first["plan"]["automatic"]}
    choice = {x["family"] for x in first["plan"]["user_choice"]}
    unsupported = {x["family"] for x in first["plan"]["unsupported"]}
    assert {"documents", "pages", "page_identity", "hyperlinks", "output"} <= auto
    assert {"selection", "ole_links"} <= choice
    assert unsupported == {"future_unknown_family"}
    recipes = {x["id"] for x in first["recipe_candidates"]}
    assert "computed_page_reference_refresh" in recipes
    assert "batch_file_output" in recipes

    second = plan["files"][1]
    second_recipes = {x["id"] for x in second["recipe_candidates"]}
    assert {
        "data_recipe_batch_output",
        "data_bound_table",
        "bulk_picture_replace",
    } <= second_recipes
    assert {x["family"] for x in second["plan"]["automatic"]} == {
        "mail_merge",
        "output",
        "tables",
        "shapes",
        "picture",
    }
    assert second["plan"]["user_choice"] == []
    assert second["plan"]["unsupported"] == []

    empty = module.build_plan(receipt([]))
    assert empty["summary"] == {
        "file_count": 0,
        "vba_project_files": 0,
        "call_bearing_files": 0,
        "family_hit_dispositions": {},
        "files_by_disposition": {},
    }

    bad = receipt([])
    bad["claims"]["vba_executed"] = True
    try:
        module.build_plan(bad)
    except ValueError as exc:
        assert "vba_executed=false" in str(exc)
    else:
        raise AssertionError("planner accepted an execution-unsafe receipt")

    print({"tests": "ok", "mapping_version": plan["mapping_version"]})
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
