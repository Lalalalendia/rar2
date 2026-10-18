#!/usr/bin/env python3
"""Independent byte-integrity gate for *private* Chaptera Rescue Story exports.

This gate does not decode Quill and is not an alternative semantic parser.
The Rust pub-reader producer owns source-backed admission and typed provenance.
Never publish stories.json/stories.txt as source-free CI artifacts.
"""
from __future__ import annotations

import hashlib
import json
from pathlib import Path

SCHEMA = "chaptera.rescue-story-evidence.v1"
MAX_SOURCE_BYTES = 128 * 1024 * 1024
MAX_MANIFEST_BYTES = 24 * 1024 * 1024
MAX_TEXT_BYTES = 20 * 1024 * 1024
MAX_STORIES = 16_384
LEGACY_EXACT = {
    "legacy_0x22_quill_authoritative_story":
        "legacy_0x22_quill_authoritative_exact",
    "legacy_0x22_noquill_authoritative_story":
        "legacy_0x22_noquill_authoritative_exact",
}
EVIDENCE_CLASSES = frozenset({
    "mature_authoritative_exact",
    "salvage_confirmed_quill_text_range",
    *LEGACY_EXACT.values(),
})
EXTRACTION_CLASSES = frozenset({
    "mature_0x2c_authoritative_story",
    "eligible_corrupt_source_confirmed_quill",
    *LEGACY_EXACT.keys(),
})


class StoryEvidenceError(ValueError):
    pass


def sha256(content: bytes) -> str:
    return hashlib.sha256(content).hexdigest()


def _safe_regular(path: Path, limit: int) -> bytes:
    if any(segment.is_symlink() for segment in (path, *path.parents)):
        raise StoryEvidenceError("symlink_path")
    try:
        if not path.is_file() or not 0 <= path.stat().st_size <= limit:
            raise StoryEvidenceError("missing_or_oversize_evidence")
        return path.read_bytes()
    except OSError as error:
        raise StoryEvidenceError("cannot_read_evidence") from error


def _story_key(value: object) -> bool:
    return (isinstance(value, str) and 0 < len(value) <= 96
            and all(char.isascii() and (char.isalnum() or char in "-:_") for char in value))


def verify(source: Path, output: Path) -> dict:
    if source.suffix.casefold() != ".pub":
        raise StoryEvidenceError("source_not_pub")
    source_data = _safe_regular(source, MAX_SOURCE_BYTES)
    if not source_data:
        raise StoryEvidenceError("empty_source")
    if output.is_symlink() or not output.is_dir():
        raise StoryEvidenceError("unsafe_output_dir")
    names = {path.name for path in output.iterdir()}
    if names != {"stories.json", "stories.txt"}:
        raise StoryEvidenceError("unexpected_evidence_entry")
    raw_json = _safe_regular(output / "stories.json", MAX_MANIFEST_BYTES)
    raw_txt = _safe_regular(output / "stories.txt", MAX_TEXT_BYTES)
    try:
        record = json.loads(raw_json)
    except (ValueError, UnicodeDecodeError) as error:
        raise StoryEvidenceError("unreadable_story_manifest") from error
    if not isinstance(record, dict):
        raise StoryEvidenceError("manifest_not_object")
    if (record.get("schema_version") != SCHEMA
            or record.get("source_sha256") != sha256(source_data)
            or record.get("source_immutable") is not True
            or record.get("native_pub_delivery_allowed") is not False
            or record.get("native_pub_repair_attempted") is not False
            or record.get("page_geometry_reconstructed") is not False
            or record.get("extraction_class") not in EXTRACTION_CLASSES
            or record.get("stories_txt_sha256") != sha256(raw_txt)):
        raise StoryEvidenceError("source_or_manifest_authority_mismatch")

    legacy_class = record["extraction_class"] in LEGACY_EXACT
    if legacy_class:
        omitted = record.get("unadmitted_story_count")
        if type(omitted) is not int or not 0 <= omitted <= MAX_STORIES:
            raise StoryEvidenceError("invalid_legacy_omitted_count")
    elif "unadmitted_story_count" in record:
        raise StoryEvidenceError("nonlegacy_unadmitted_count")

    stories = record.get("stories")
    if (not isinstance(stories, list) or not stories
            or len(stories) > MAX_STORIES
            or record.get("story_count") != len(stories)):
        raise StoryEvidenceError("invalid_story_count")
    extracted = bytearray()
    previously = None
    total = 0
    for entry in stories:
        if not isinstance(entry, dict) or not _story_key(entry.get("story_key")):
            raise StoryEvidenceError("invalid_story_key")
        key = entry["story_key"]
        start, end = entry.get("utf16_start"), entry.get("utf16_end")
        if (type(start) is not int or type(end) is not int
                or not 0 <= start < end <= 0xFFFFFFFF):
            raise StoryEvidenceError("invalid_story_range")
        text = entry.get("text")
        if not isinstance(text, str) or not text:
            raise StoryEvidenceError("missing_story_text")
        raw = text.encode("utf-8")
        utf16_units = len(text.encode("utf-16-le")) // 2
        if (end - start != utf16_units or entry.get("utf8_byte_len") != len(raw)
                or entry.get("text_sha256") != sha256(raw)):
            raise StoryEvidenceError("story_content_mismatch")
        total += len(raw)
        if total > 16 * 1024 * 1024:
            raise StoryEvidenceError("story_content_limit")

        evidence = entry.get("evidence_class")
        if evidence not in EVIDENCE_CLASSES:
            raise StoryEvidenceError("unknown_evidence_class")
        if evidence == "mature_authoritative_exact" or evidence in LEGACY_EXACT.values():
            expected_class = ("mature_authoritative_exact" if
                              record["extraction_class"] == "mature_0x2c_authoritative_story"
                              else LEGACY_EXACT.get(record["extraction_class"]))
            if (evidence != expected_class
                    or not isinstance(entry.get("carrier"), str)
                    or not entry["carrier"]
                    or type(entry.get("carrier_byte_offset")) is not int
                    or type(entry.get("carrier_byte_len")) is not int
                    or entry["carrier_byte_offset"] < 0
                    or entry["carrier_byte_len"] <= 0
                    or entry["carrier_byte_offset"] + entry["carrier_byte_len"] > 2**64 - 1):
                raise StoryEvidenceError(
                    "unverified_mature_story_provenance" if
                    evidence == "mature_authoritative_exact" else
                    "unverified_legacy_story_provenance")
        elif record["extraction_class"] != "eligible_corrupt_source_confirmed_quill":
            raise StoryEvidenceError("salvage_story_class_mismatch")
        marker = (key, start, end)
        if previously is not None:
            if marker <= previously:
                raise StoryEvidenceError("unordered_or_duplicate_story")
            if key == previously[0] and start < previously[2]:
                raise StoryEvidenceError("overlapping_story_ranges")
        previously = marker
        extracted.extend(
            f"=== Story {key} [utf16 {start}..{end}] ===\n".encode("ascii")
        )
        extracted.extend(raw)
        extracted.extend(b"\n\n")
    if bytes(extracted) != raw_txt:
        raise StoryEvidenceError("story_text_projection_mismatch")
    return {
        "schema_version": SCHEMA,
        "source_sha256": record["source_sha256"],
        "story_count": len(stories),
        "byte_verified": True,
        "source_immutable_claim": True,
        "native_pub_repair_claim": False,
        "semantic_source_independently_decoded": False,
    }


def main() -> int:
    import argparse
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    try:
        result = verify(args.source, args.output)
    except StoryEvidenceError as error:
        parser.error(str(error))
    # No names, source paths, text or raw bytes in stdout.
    print(json.dumps(result, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
