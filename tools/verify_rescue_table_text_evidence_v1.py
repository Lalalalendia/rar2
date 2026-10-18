#!/usr/bin/env python3
"""Independent private artifact/text integrity gate for opted-in table cells.

Physical /Contents source-byte verification is delegated to the separate
read-only Rust verifier, not to the text producer. This Python validator checks
the manifest, per-cell SHA formats, exact rendered text, and verifier receipt.
Never print a customer Story, source filename/path or raw Contents bytes.
"""
from __future__ import annotations

import hashlib
import json
from pathlib import Path
import subprocess

from tools import verify_rescue_text_evidence_v1 as old

SCHEMA = "chaptera.rescue-table-cell-evidence.v1"
VERIFY_SCHEMA = "chaptera.rescue-table-cell-verified.v1"
CLASS = "legacy_0x22_noquill_source_exact_cells_v1"
EVIDENCE = "cell_proven_projected_story_v1"
KEYS = {
    "schema_version", "source_sha256", "source_byte_len", "extraction_class",
    "source_immutable", "native_pub_repair_attempted", "native_pub_delivery_allowed",
    "page_geometry_reconstructed", "projected_story_not_semantic", "story_count",
    "unadmitted_projection_count", "stories_txt_sha256", "stories",
}
STORY_KEYS = {
    "story_key", "utf16_start", "utf16_end", "text", "utf8_byte_len",
    "text_sha256", "evidence_class", "projection_carrier",
    "projection_byte_offset", "projection_byte_len", "rows", "columns", "cells",
}
CELL_KEYS = {
    "row", "column", "utf16_start", "utf16_end",
    "carrier_byte_offset", "carrier_byte_len", "cell_text_sha256",
    "boundary_byte_offset", "boundary_byte_len", "boundary_sha256",
    "virtual_cr_before",
}


class TableEvidenceError(ValueError):
    pass


def sha(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def _range(value: object, maximum: int, *, nonzero: bool = False) -> bool:
    return type(value) is int and (int(nonzero) <= value <= maximum)


def _hex_sha(value: object) -> bool:
    return type(value) is str and len(value) == 64 and all(c in "0123456789abcdef" for c in value)


def verify_private_structure(source: Path, output: Path) -> dict:
    source_data = old._safe_regular(source, old.MAX_SOURCE_BYTES)
    if source.suffix.casefold() != ".pub" or not source_data:
        raise TableEvidenceError("invalid_original_pub")
    if output.is_symlink() or not output.is_dir():
        raise TableEvidenceError("unsafe_table_output")
    for parent in output.parents:
        if parent.is_symlink():
            raise TableEvidenceError("table_parent_symlink")
    if {path.name for path in output.iterdir()} != {"stories.json", "stories.txt"}:
        raise TableEvidenceError("unexpected_table_files")
    json_data = old._safe_regular(output / "stories.json", old.MAX_MANIFEST_BYTES)
    txt = old._safe_regular(output / "stories.txt", old.MAX_TEXT_BYTES)
    try:
        evidence = json.loads(json_data)
    except (ValueError, UnicodeDecodeError) as error:
        raise TableEvidenceError("invalid_table_manifest_json") from error
    if type(evidence) is not dict or set(evidence) != KEYS:
        raise TableEvidenceError("invalid_manifest_protocol")
    if (evidence["schema_version"] != SCHEMA
            or evidence["extraction_class"] != CLASS
            or evidence["source_sha256"] != sha(source_data)
            or evidence["source_byte_len"] != len(source_data)
            or evidence["stories_txt_sha256"] != sha(txt)
            or evidence["source_immutable"] is not True
            or evidence["native_pub_repair_attempted"] is not False
            or evidence["native_pub_delivery_allowed"] is not False
            or evidence["page_geometry_reconstructed"] is not False
            or evidence["projected_story_not_semantic"] is not True):
        raise TableEvidenceError("untrusted_table_manifest_authority")
    stories = evidence["stories"]
    if (type(stories) is not list
            or not _range(evidence["story_count"], 16384, nonzero=True)
            or len(stories) != evidence["story_count"]
            or not _range(evidence["unadmitted_projection_count"], 16384)):
        raise TableEvidenceError("bad_table_story_count")
    expected_txt = bytearray()
    previous = None
    total_cells = total_separators = total_payload = 0
    for story in stories:
        if type(story) is not dict or set(story) != STORY_KEYS:
            raise TableEvidenceError("bad_table_story_fields")
        key, content = story["story_key"], story["text"]
        if not old._story_key(key) or (previous is not None and key <= previous):
            raise TableEvidenceError("unsafe_or_unordered_table_story_key")
        previous = key
        if (type(content) is not str or not content
                or story["evidence_class"] != EVIDENCE
                or story["projection_carrier"] != "/Contents"
                or story["utf16_start"] != 0
                or not _range(story["utf16_end"], 2**32 - 1, nonzero=True)
                or story["utf16_end"] != len(content.encode("utf-16-le")) // 2
                or story["utf8_byte_len"] != len(content.encode("utf-8"))
                or story["text_sha256"] != sha(content.encode("utf-8"))):
            raise TableEvidenceError("table_story_text_or_class_mismatch")
        if (not _range(story["projection_byte_offset"], 2**64 - 1)
                or not _range(story["projection_byte_len"], 2**64 - 1)
                or story["projection_byte_offset"] + story["projection_byte_len"] > 2**64 - 1):
            raise TableEvidenceError("invalid_table_story_projection")
        cells = story["cells"]
        rows, columns = story["rows"], story["columns"]
        if (not _range(rows, 4096, nonzero=True)
                or not _range(columns, 4096, nonzero=True)
                or type(cells) is not list
                or not 1 <= len(cells) <= 4096
                or rows * columns != len(cells)):
            raise TableEvidenceError("invalid_table_cell_grid")
        utf16 = 0
        for index, cell in enumerate(cells):
            if type(cell) is not dict or set(cell) != CELL_KEYS:
                raise TableEvidenceError("invalid_cell_manifest_fields")
            if (cell["row"] != index // columns
                    or cell["column"] != index % columns
                    or cell["virtual_cr_before"] is not (index > 0)
                    or cell["utf16_start"] != utf16):
                raise TableEvidenceError("invalid_cell_order_or_separator")
            for key_name in ("carrier_byte_offset", "carrier_byte_len",
                             "boundary_byte_offset", "boundary_byte_len"):
                if not _range(cell[key_name], 2**64 - 1):
                    raise TableEvidenceError("invalid_cell_carrier_range")
            if (cell["carrier_byte_offset"] + cell["carrier_byte_len"] > 2**64 - 1
                    or cell["boundary_byte_offset"] + cell["boundary_byte_len"] > 2**64 - 1
                    or cell["boundary_byte_len"] == 0
                    or not _hex_sha(cell["cell_text_sha256"])
                    or not _hex_sha(cell["boundary_sha256"])):
                raise TableEvidenceError("bad_cell_source_hash_or_boundary")
            if index:
                utf16 += 1
                total_separators += 1
            if (not _range(cell["utf16_end"], 2**32 - 1)
                    or cell["utf16_end"] < utf16):
                raise TableEvidenceError("invalid_cell_utf16_end")
            utf16 = cell["utf16_end"]
            total_cells += 1
        if utf16 != story["utf16_end"]:
            raise TableEvidenceError("table_utf16_span_incomplete")
        data = content.encode("utf-8")
        total_payload += len(data)
        if total_payload > 16 * 1024 * 1024:
            raise TableEvidenceError("table_total_text_limit")
        expected_txt.extend(f"=== Story {story['story_key']} [utf16 0..{story['utf16_end']}] ===\n".encode("ascii"))
        expected_txt.extend(data)
        expected_txt.extend(b"\n\n")
    if bytes(expected_txt) != txt or total_separators != total_cells - len(stories):
        raise TableEvidenceError("table_story_text_projection_mismatch")
    return {
        "schema_version": SCHEMA,
        "source_sha256": evidence["source_sha256"],
        "story_count": len(stories),
        "cell_count": total_cells,
        "virtual_separator_count": total_separators,
    }


def verify(source: Path, output: Path, pub_cli: Path) -> dict:
    receipt = verify_private_structure(source, output)
    if not pub_cli.is_file() or pub_cli.is_symlink():
        raise TableEvidenceError("missing_trusted_source_verifier")
    try:
        worker = subprocess.run(
            [str(pub_cli), "verify-legacy-table-text", str(source), "--output", str(output)],
            stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL, timeout=60, check=False,
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        raise TableEvidenceError("byte_source_verifier_unavailable") from error
    if worker.returncode != 0 or not 0 < len(worker.stdout) < 4096:
        raise TableEvidenceError("byte_source_verification_failed")
    try:
        result = json.loads(worker.stdout)
    except ValueError as error:
        raise TableEvidenceError("invalid_source_verifier_receipt") from error
    if (type(result) is not dict
            or set(result) != {
                "schema_version", "source_sha256", "verified_table_stories",
                "verified_source_cells", "synthetic_cr_separators",
                "source_immutable", "native_pub_repair",
            }
            or result["schema_version"] != VERIFY_SCHEMA
            or result["source_sha256"] != receipt["source_sha256"]
            or result["verified_table_stories"] != receipt["story_count"]
            or result["verified_source_cells"] != receipt["cell_count"]
            or result["synthetic_cr_separators"] != receipt["virtual_separator_count"]
            or result["source_immutable"] is not True
            or result["native_pub_repair"] is not False):
        raise TableEvidenceError("source_byte_receipt_disagrees_with_manifest")
    return {
        "source_sha256": receipt["source_sha256"],
        "verified_table_stories": receipt["story_count"],
        "verified_source_cells": receipt["cell_count"],
        "virtual_separator_count": receipt["virtual_separator_count"],
        "source_byte_verified": True,
        "native_pub_repair": False,
    }


if __name__ == "__main__":
    import argparse
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--pub-cli", type=Path, required=True)
    args = parser.parse_args()
    try:
        report = verify(args.source, args.output, args.pub_cli)
    except (TableEvidenceError, old.StoryEvidenceError, OSError, ValueError):
        parser.error("table_byte_or_artifact_validation_failed")
    print(json.dumps(report, sort_keys=True))
