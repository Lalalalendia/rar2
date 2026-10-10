#!/usr/bin/env python3
"""Source-free adversarial bounds plus real 0x2C Publisher Reader receipt."""
from __future__ import annotations

import copy
import hashlib
import importlib.util
import json
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
MODULE = ROOT / "tools" / "pub_source_font_requirements_v1.py"
spec = importlib.util.spec_from_file_location("pub_requirements", MODULE)
assert spec and spec.loader
req = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = req
spec.loader.exec_module(req)

REAL = ROOT / "apps/web/acceptance/receipts/viewer-geometry.real.json"
SOURCE_SHA = "6a825ba26ba35d6e885acdc62e859591ed37cb0ff7480b554b9cb362b644dfcf"
TARGET_STORY = "15613e56-726e-5ae7-8c54-ec876c9bcfda"
ALL_FAMILIES = {
    "Rockwell Condensed", "Franklin Gothic Book", "Franklin Gothic Demi Cond",
    "Franklin Gothic Medium Cond", "Sylfaen", "Times New Roman",
    "Elephant", "MV Boli",
}
ID = TARGET_STORY


def synthetic() -> dict:
    text = "AéB\r"
    digest = hashlib.sha256(text.encode("utf-8")).hexdigest()
    def run(start: int, end: int, family: str, bold: bool = False) -> dict:
        return {
            "story_id": ID, "scalar_start": start, "scalar_end": end,
            "source_story_text_sha256": digest, "source_font_name": family,
            "bold": {"effective_value": bold},
            "italic": {"effective_value": False},
        }
    return {
        "schema_version": "0.1",
        "document": {
            "source": {"format": "pub", "source_hash": SOURCE_SHA},
            "stories": [{"id": ID, "text": text}],
        },
        "story_frames": [{"story_id": ID, "frame_id": ID}],
        "typography_runs": [
            run(0, 1, "Example Serif"),
            run(1, 2, "Example Serif", True),
            run(2, 4, ""),
        ],
        "script_font_maps": [{
            "story_id": ID, "scalar_start": 0, "scalar_end": 4,
            "source_story_text_sha256": digest,
            "entries": [
                {"source_font_name": "Example Serif", "source_font_index": 7,
                 "script_slot": 2, "disposition": "resolved"},
                {"source_font_name": "Supplement Sans", "source_font_index": 8,
                 "script_slot": 31, "disposition": "resolved"},
                {"source_font_name": "Substitute", "source_font_index": 9,
                 "script_slot": 37, "disposition": "unknown"},
            ],
        }],
    }


def must_block(viewer: dict, phrase: str) -> None:
    try:
        req.source_font_requirements_v1(viewer)
    except req.SourceFontRequirementsError as exc:
        assert phrase in str(exc), (phrase, str(exc))
    else:
        raise AssertionError("untrusted Publisher font requirement accepted: " + phrase)


def synthetic_controls() -> None:
    viewer = synthetic()
    out = req.source_font_requirements_v1(viewer)
    assert out["protocol_version"] == req.PROTOCOL
    assert out["document_source_sha256"] == SOURCE_SHA
    assert out["source_declared_families_only"] is True
    assert out["requires_private_licensed_physical_bytes"] is True
    assert out["physical_font_bytes_included"] is False
    assert out["fixed_pdf_allowed"] is False
    assert out["original_publisher_layout_authoritative"] is False

    assert req.source_font_binding_id_v1(
        SOURCE_SHA, 18, "Rockwell Condensed"
    ) == "pub-source-font:b402e726-72b4-5d52-bb0d-07a0c13dc05d"
    assert out["direct_source_binding_count"] == 0
    assert out["direct_source_bindings"] == []
    assert out["direct_source_bindings_only_source_identity"] is True
    # A modern Reader receipt carries the index from the actual typography
    # run; script font *alternatives* alone never create this exact binding.
    directly_proven = copy.deepcopy(viewer)
    directly_proven["typography_runs"][0]["source_font_index"] = 7
    directly_proven["typography_runs"][1]["source_font_index"] = 7
    current = req.source_font_requirements_v1(directly_proven)
    assert current["direct_source_binding_count"] == 2
    assert len(current["direct_source_bindings"]) == 2
    expected_id = req.source_font_binding_id_v1(SOURCE_SHA, 7, "Example Serif")
    assert all(record["source_font_binding_id"] == expected_id for record
               in current["direct_source_bindings"])
    assert all(record["source_font_index"] == 7 for record
               in current["direct_source_bindings"])
    assert current["families"][0]["direct_run_source_font_indices"] == [7]
    assert current["physical_font_bytes_included"] is False
    assert current["fixed_pdf_allowed"] is False
    direct_wrong = copy.deepcopy(directly_proven)
    direct_wrong["typography_runs"][0]["source_font_index"] = True
    must_block(direct_wrong, "invalid direct source Quill")
    direct_wrong = copy.deepcopy(directly_proven)
    direct_wrong["typography_runs"][0]["source_font_index"] = -1
    must_block(direct_wrong, "invalid direct source Quill")
    direct_wrong = copy.deepcopy(directly_proven)
    direct_wrong["typography_runs"][0]["source_font_name"] = " Example Serif "
    must_block(direct_wrong, "cannot be silently trimmed")
    assert req.source_font_binding_id_v1(SOURCE_SHA, 18, "Rockwell Condensed") != (
        req.source_font_binding_id_v1("a" * 64, 18, "Rockwell Condensed")
    )
    assert out["unresolved_source_family_run_count"] == 1
    assert out["unresolved_script_font_entry_count"] == 1
    assert {x["source_family"] for x in out["families"]} == {"Example Serif", "Supplement Sans"}
    example = next(x for x in out["families"] if x["source_family"] == "Example Serif")
    assert example["source_font_index_candidates"] == [7]
    assert example["source_quill_index_proven"] is True
    assert out["source_families_without_quill_index_count"] == 0
    assert example["source_script_slots"] == [2]
    assert example["effective_style_run_counts"] == {"bold": 1, "regular": 1}
    assert example["needs_non_regular_style"] is True
    assert example["physical_face_authorized"] is False
    assert out["families"][1]["source_typography_run_count"] == 0
    assert out["families"][1]["script_map_reference_count"] == 1
    assert "AéB" not in json.dumps(out, ensure_ascii=False)
    assert "Substitute" not in json.dumps(out, ensure_ascii=False)

    bad = copy.deepcopy(viewer)
    bad["typography_runs"][0]["source_story_text_sha256"] = "0" * 64
    must_block(bad, "stale or fabricated")
    bad = copy.deepcopy(viewer)
    bad["typography_runs"][1]["scalar_end"] = 5
    must_block(bad, "invalid Unicode scalar")
    bad = copy.deepcopy(viewer)
    bad["script_font_maps"][0]["source_story_text_sha256"] = "f" * 64
    must_block(bad, "stale or fabricated")
    bad = copy.deepcopy(viewer)
    bad["document"]["source"]["format"] = "pdf"
    must_block(bad, "source-backed PUB")
    bad = copy.deepcopy(viewer)
    bad["document"]["stories"].append(bad["document"]["stories"][0])
    must_block(bad, "duplicate or invalid")
    bad = copy.deepcopy(viewer)
    bad["story_frames"][0]["story_id"] = "00000000-0000-4000-8000-000000000000"
    must_block(bad, "unknown Story")


def real_publisher() -> None:
    source = json.loads(REAL.read_text(encoding="utf-8"))
    out = req.source_font_requirements_v1(source)
    assert out["document_source_sha256"] == SOURCE_SHA
    # Historical pinned receipt predates the direct-index Viewer field; no
    # source binding ID may be inferred from script names alone.
    assert out["direct_source_binding_count"] == 0
    assert out["direct_source_bindings"] == []
    assert {item["source_family"] for item in out["families"]} == ALL_FAMILIES
    assert out["visible_story_count"] > 0
    assert out["source_story_count"] >= out["visible_story_count"]
    assert all(item["physical_face_authorized"] is False for item in out["families"])
    assert any(item["source_font_index_candidates"] for item in out["families"])
    assert any(not item["source_font_index_candidates"] for item in out["families"])
    assert out["source_families_without_quill_index_count"] == sum(
        not family["source_quill_index_proven"] for family in out["families"]
    )
    assert out["source_families_without_quill_index_count"] > 0
    rockwell = next(x for x in out["families"] if x["source_family"] == "Rockwell Condensed")
    assert rockwell["source_font_index_candidates"] == [18]
    assert rockwell["source_typography_run_count"] > 0
    assert rockwell["visible_typography_run_count"] > 0
    assert rockwell["needs_non_regular_style"] is True
    assert "Inside Story Headline" not in json.dumps(out, ensure_ascii=False)
    assert out["fixed_pdf_allowed"] is False
    assert out["physical_font_bytes_included"] is False

    with tempfile.TemporaryDirectory() as temp:
        result = Path(temp) / "source-requirements.json"
        assert req.main(["--viewer", str(REAL), "--output", str(result)]) == 0
        serialized = json.loads(result.read_text(encoding="utf-8"))
        assert serialized == out
        assert req.main(["--viewer", str(REAL), "--output", str(REAL)]) == 2

    print("REAL_PUB_SOURCE_FONT_REQUIREMENTS_OK " + json.dumps({
        "source_sha256": SOURCE_SHA,
        "family_names": len(out["families"]),
        "Rockwell_Quill_index": rockwell["source_font_index_candidates"],
        "source_stories": out["source_story_count"],
        "visible_stories": out["visible_story_count"],
        "source_families_without_quill_index": out["source_families_without_quill_index_count"],
        "physical_fonts_admitted": False,
        "fixed_pdf_allowed": False,
    }, sort_keys=True))


if __name__ == "__main__":
    synthetic_controls()
    real_publisher()
    print("PUB source-font requirements provenance/security tests: PASS")
