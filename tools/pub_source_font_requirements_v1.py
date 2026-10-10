#!/usr/bin/env python3
"""Source-declared font requirements from a verified Publisher Reader receipt.

This private *planning* output contains source family labels, styles and Quill
index candidates. It neither discovers physical font bytes nor grants authoring,
native-Publisher fidelity, fixed PDF or font redistribution rights.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import re
import sys
from collections import defaultdict
from pathlib import Path

PROTOCOL = "chaptera.pub-source-font-requirements.v1"
SOURCE_SHA_RE = re.compile(r"^[0-9a-f]{64}$")
UUID_RE = re.compile(r"^[0-9a-f]{8}-(?:[0-9a-f]{4}-){3}[0-9a-f]{12}$")
STYLE_NAMES = {
    (False, False): "regular",
    (True, False): "bold",
    (False, True): "italic",
    (True, True): "bold_italic",
}


class SourceFontRequirementsError(ValueError):
    pass


def _object(value: object, label: str) -> dict:
    if not isinstance(value, dict):
        raise SourceFontRequirementsError(f"{label} must be an object")
    return value


def _list(value: object, label: str) -> list:
    if not isinstance(value, list):
        raise SourceFontRequirementsError(f"{label} must be a list")
    return value


def _family(value: object) -> str:
    if not isinstance(value, str):
        return ""
    name = value.strip()
    if not name or len(name) > 128 or any(ord(ch) < 32 for ch in name):
        return ""
    return name


def _range(entry: dict, story_by_id: dict[str, str], name: str) -> tuple[str, int, int]:
    story_id = entry.get("story_id")
    if not isinstance(story_id, str) or not UUID_RE.fullmatch(story_id):
        raise SourceFontRequirementsError(f"{name} lacks canonical Story identity")
    if story_id not in story_by_id:
        raise SourceFontRequirementsError(f"{name} refers to missing source Story")
    start, end = entry.get("scalar_start"), entry.get("scalar_end")
    if type(start) is not int or type(end) is not int or not (0 <= start < end <= len(story_by_id[story_id])):
        raise SourceFontRequirementsError(f"{name} has invalid Unicode scalar interval")
    digest = entry.get("source_story_text_sha256")
    expected = hashlib.sha256(story_by_id[story_id].encode("utf-8")).hexdigest()
    if digest != expected:
        raise SourceFontRequirementsError(f"{name} has stale or fabricated source Story text hash")
    return story_id, start, end


def _style(run: dict) -> str:
    flags = []
    for property_name in ("bold", "italic"):
        item = run.get(property_name)
        effective = item.get("effective_value") if isinstance(item, dict) else None
        if type(effective) is not bool:
            return "unknown"
        flags.append(effective)
    return STYLE_NAMES[tuple(flags)]


def source_font_requirements_v1(viewer: dict) -> dict:
    viewer = _object(viewer, "Publisher Viewer receipt")
    if viewer.get("schema_version") != "0.1":
        raise SourceFontRequirementsError("unsupported Publisher Viewer receipt schema")
    doc = _object(viewer.get("document"), "Publisher Viewer document")
    source = _object(doc.get("source"), "Publisher Viewer source")
    source_hash = source.get("source_hash")
    if source.get("format") != "pub" or not isinstance(source_hash, str) or not SOURCE_SHA_RE.fullmatch(source_hash):
        raise SourceFontRequirementsError("exact source-backed PUB SHA-256 required")

    stories = _list(doc.get("stories"), "Publisher Viewer stories")
    story_by_id: dict[str, str] = {}
    for raw in stories:
        story = _object(raw, "Reader Story")
        story_id, content = story.get("id"), story.get("text")
        if (not isinstance(story_id, str) or not UUID_RE.fullmatch(story_id)
                or not isinstance(content, str) or story_id in story_by_id):
            raise SourceFontRequirementsError("duplicate or invalid source Story identity/text")
        story_by_id[story_id] = content
    if not story_by_id:
        raise SourceFontRequirementsError("source has no provable Stories")

    visible = set()
    for raw in _list(viewer.get("story_frames"), "Publisher Viewer Story frames"):
        frame = _object(raw, "Story frame")
        story_id = frame.get("story_id")
        if story_id not in story_by_id:
            raise SourceFontRequirementsError("source Story frame has an unknown Story ID")
        visible.add(story_id)

    # Never combine different Publisher spellings or font indices into a
    # guessed physical face. The index is source-side Quill metadata, NOT a
    # SHA-256, font file, or a license to render/author.
    family_usage: dict[str, dict] = {}
    def add_family(name: str) -> dict:
        if name not in family_usage:
            family_usage[name] = {
                "source_family": name,
                "source_typography_run_count": 0,
                "visible_typography_run_count": 0,
                "script_map_reference_count": 0,
                "source_font_index_candidates": set(),
                "script_slots": set(),
                "visible_story_ids": set(),
                "effective_styles": defaultdict(int),
            }
        return family_usage[name]

    missing_labels = 0
    unknown_effective_styles = 0
    for raw in _list(viewer.get("typography_runs"), "Publisher Viewer typography runs"):
        run = _object(raw, "source typography run")
        story_id, _start, _end = _range(run, story_by_id, "source typography run")
        family = _family(run.get("source_font_name"))
        if not family:
            missing_labels += 1
            continue
        use = add_family(family)
        use["source_typography_run_count"] += 1
        if story_id in visible:
            use["visible_typography_run_count"] += 1
            use["visible_story_ids"].add(story_id)
        style = _style(run)
        use["effective_styles"][style] += 1
        if style == "unknown":
            unknown_effective_styles += 1

    unresolved_script_entries = 0
    for raw in _list(viewer.get("script_font_maps"), "Publisher Viewer script-font maps"):
        script_map = _object(raw, "source script-font map")
        story_id, _start, _end = _range(script_map, story_by_id, "source script-font map")
        for item in _list(script_map.get("entries"), "source script-font map entries"):
            entry = _object(item, "source script-font entry")
            family = _family(entry.get("source_font_name"))
            idx = entry.get("source_font_index")
            slot = entry.get("script_slot")
            if (entry.get("disposition") != "resolved" or not family
                    or type(idx) is not int or idx < 0 or idx > 65535
                    or type(slot) is not int or slot < 0 or slot > 65535):
                unresolved_script_entries += 1
                continue
            use = add_family(family)
            use["script_map_reference_count"] += 1
            use["source_font_index_candidates"].add(idx)
            use["script_slots"].add(slot)
            if story_id in visible:
                use["visible_story_ids"].add(story_id)

    families = []
    for name in sorted(family_usage, key=lambda s: (s.casefold(), s)):
        item = family_usage[name]
        styles = dict(sorted(item["effective_styles"].items()))
        families.append({
            "source_family": name,
            "source_typography_run_count": item["source_typography_run_count"],
            "visible_typography_run_count": item["visible_typography_run_count"],
            "script_map_reference_count": item["script_map_reference_count"],
            "source_font_index_candidates": sorted(item["source_font_index_candidates"]),
            "source_script_slots": sorted(item["script_slots"]),
            "visible_story_count": len(item["visible_story_ids"]),
            "effective_style_run_counts": styles,
            "needs_non_regular_style": any(styles.get(style, 0) for style in ("bold", "italic", "bold_italic")),
            "unknown_effective_styles": styles.get("unknown", 0),
            "physical_face_authorized": False,
        })

    if not families:
        raise SourceFontRequirementsError("no source family labels proved by Publisher Reader")
    return {
        "protocol_version": PROTOCOL,
        "document_source_sha256": source_hash,
        "reader_receipt_schema": viewer["schema_version"],
        "source_declared_families_only": True,
        "requires_private_licensed_physical_bytes": True,
        "physical_font_bytes_included": False,
        "original_publisher_layout_authoritative": False,
        "fixed_pdf_allowed": False,
        "source_story_count": len(story_by_id),
        "visible_story_count": len(visible),
        "unresolved_source_family_run_count": missing_labels,
        "unknown_effective_style_run_count": unknown_effective_styles,
        "unresolved_script_font_entry_count": unresolved_script_entries,
        "families": families,
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="Generate a source-safe, non-authorizing private PUB font provisioning plan"
    )
    parser.add_argument("--viewer", required=True, type=Path, help="source-backed Reader Viewer JSON")
    parser.add_argument("--output", required=True, type=Path, help="private requirements JSON file")
    args = parser.parse_args(argv)
    try:
        if args.viewer.resolve() == args.output.resolve():
            raise SourceFontRequirementsError("cannot overwrite original Publisher Viewer receipt")
        plan = source_font_requirements_v1(
            json.loads(args.viewer.read_text(encoding="utf-8"))
        )
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(plan, ensure_ascii=False, sort_keys=True, indent=2) + "\n", encoding="utf-8")
    except (OSError, ValueError, json.JSONDecodeError) as exc:
        print(f"source font requirements blocked: {exc}", file=sys.stderr)
        return 2
    print("PUB_SOURCE_FONT_REQUIREMENTS_OK " + json.dumps({
        "source_sha256": plan["document_source_sha256"],
        "families": len(plan["families"]),
        "visible_stories": plan["visible_story_count"],
        "unresolved_source_labels": plan["unresolved_source_family_run_count"],
        "non_regular_families": sum(bool(f["needs_non_regular_style"]) for f in plan["families"]),
        "native_publisher_layout_authoritative": False,
    }, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
