#!/usr/bin/env python3
from __future__ import annotations

import importlib.util
import json
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location(
    "reader_feature_prevalence", HERE / "reader_feature_prevalence.py"
)
assert SPEC and SPEC.loader
MOD = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MOD)


def sha(ch: str) -> str:
    return ch * 64


def records() -> dict[str, dict]:
    return {
        sha("a"): {
            "source_sha256": sha("a"),
            "opened": True,
            "format_version": "0x2c",
            "story_count": 2,
            "text_fragment_count": 3,
            "image_placement_count": 1,
            "solid_fill_count": 0,
            "solid_line_count": 1,
            "inherited_typography_run_count": 0,
            "diagnostic_codes": [],
            "legacy_object_residuals": [],
        },
        sha("b"): {
            "source_sha256": sha("b"),
            "opened": True,
            "format_version": "0x22",
            "story_count": 0,
            "text_fragment_count": 0,
            "image_placement_count": 0,
            "solid_fill_count": 2,
            "solid_line_count": 0,
            "inherited_typography_run_count": 4,
            "diagnostic_codes": ["partial"],
            "legacy_object_residuals": [{"raw_type_hex": "0x1234"}],
        },
        sha("c"): {
            "source_sha256": sha("c"),
            "opened": False,
            "open_error_kind": "viewer_open_failed",
        },
    }


def main() -> None:
    provenance = {
        sha("a"): [
            {"sha256": sha("a"), "source": "Source A", "category": "Newsletters"},
            {"sha256": sha("a"), "source": "Source A", "category": "Newsletters"},
        ],
        sha("b"): [
            {"sha256": sha("b"), "source": "Source A", "category": "Brochures"},
            {"sha256": sha("b"), "source": "Source B", "category": "Brochures"},
        ],
        sha("c"): [{"sha256": sha("c"), "source": "Source B", "category": ""}],
    }
    report = MOD.build_report(records(), provenance)

    assert report["schema"] == "chaptera.reader-feature-prevalence.v2"
    assert report["corpus_sha_count"] == 3
    assert report["opened_file_count"] == 2
    assert report["failed_or_unopened_file_count"] == 1

    story = report["overall"]["has_story"]
    assert story == {
        "denominator": 3,
        "known_file_count": 2,
        "present_file_count": 1,
        "absent_file_count": 1,
        "unknown_file_count": 1,
        "present_ratio_known": 0.5,
    }

    source_a = report["by_source"]["Source A"]["has_story"]
    assert source_a["denominator"] == 2
    assert source_a["known_file_count"] == 2
    assert source_a["present_file_count"] == 1

    source_b = report["by_source"]["Source B"]["has_story"]
    assert source_b["denominator"] == 2
    assert source_b["known_file_count"] == 1
    assert source_b["unknown_file_count"] == 1

    assert report["by_format_version"]["0x2c"]["has_image_placement"]["present_file_count"] == 1
    assert report["by_format_version"]["0x22"]["has_legacy_object_residual"]["present_file_count"] == 1
    assert report["by_format_version"]["unknown"]["has_story"]["unknown_file_count"] == 1

    missing = records()
    del missing[sha("a")]["solid_fill_count"]
    report_missing = MOD.build_report(missing, {})
    fill = report_missing["overall"]["has_solid_fill"]
    assert fill["unknown_file_count"] == 2
    assert fill["known_file_count"] == 1

    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        (root / "one.json").write_text(json.dumps(records()[sha("a")]), encoding="utf-8")
        loaded = MOD.load_reader_records(root)
        assert list(loaded) == [sha("a")]

    duplicate_path = None
    with tempfile.TemporaryDirectory() as tmp:
        duplicate_path = Path(tmp) / "records.json"
        duplicate_path.write_text(
            json.dumps([records()[sha("a")], records()[sha("a")]]),
            encoding="utf-8",
        )
        try:
            MOD.load_reader_records(duplicate_path)
        except ValueError as error:
            assert "duplicate reader sha256" in str(error)
        else:
            raise AssertionError("duplicate reader SHA must fail closed")

    print("reader_feature_prevalence: ok")


if __name__ == "__main__":
    main()
