#!/usr/bin/env python3
"""Private, non-authorizing audit of local face/style coverage for source PUB.

Runs on the operator's machine. Does not copy fonts or license-check them.
A matching local family/style is only a candidate, NEVER Publisher face parity.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path

import export_cloud_reader_windows_fonts as fonts
import private_font_os2_embedding_v1 as os2

MAX_LOCAL_FONT_BYTES = 64 * 1024 * 1024

PROTOCOL = "chaptera.private-pub-font-style-coverage.v1"
STYLES = ("regular", "bold", "italic", "bold_italic")


def style_from_subfamily(value: str) -> str | None:
    normalized = fonts.normalize_name(value.replace("-", " "))
    if fonts.is_regular_style(normalized):
        return "regular"
    if normalized == "bold":
        return "bold"
    if normalized == "italic":
        return "italic"
    if normalized in ("bold italic", "italic bold", "bolditalic"):
        return "bold_italic"
    # SemiBold, Oblique, variable axes and localized nonstandard styles are
    # *not* silently substituted for exact requested Publisher style flags.
    return None


def audit_style_coverage(requirements: Path, font_dir: Path) -> dict:
    # The private requirements parser checks source SHA, candidates, counters
    # and output-authority fences; this audit adds no signing or authoring grant.
    families, source = fonts.read_private_source_requirements(requirements)
    packet = json.loads(requirements.read_text(encoding="utf-8"))
    root = font_dir.resolve(strict=True)
    if not root.is_dir():
        raise fonts.FontPacketError("private fonts directory is not a directory")

    allowed = {fonts.normalize_name(name) for name in families}
    candidates = {name: [] for name in allowed}
    malformed_files = 0
    oversized_files = 0
    ignored_links = 0
    # A single scan is important for large Windows Fonts directories.
    for path in sorted(root.iterdir(), key=lambda file: file.name.casefold()):
        if path.is_symlink():
            ignored_links += 1
            continue
        if not path.is_file() or path.suffix.casefold() not in fonts.SUPPORTED_SUFFIXES:
            continue
        try:
            if path.stat().st_size > MAX_LOCAL_FONT_BYTES:
                oversized_files += 1
                continue
            parsed = fonts.parse_font_faces(path)
        except (OSError, fonts.FontPacketError):
            malformed_files += 1
            continue
        for face in parsed:
            family = fonts.normalize_name(face.family)
            if family in candidates:
                candidates[family].append(face)

    records = []
    statuses = {}
    for family in packet["families"]:
        name = family["source_family"]
        needed = family["effective_style_run_counts"]
        requested = [(style, needed.get(style, 0)) for style in STYLES if needed.get(style, 0)]
        if needed.get("unknown", 0) or not requested:
            requested.append(("unknown", needed.get("unknown", 0)))
        source_index_proven = bool(family.get("direct_run_source_font_indices"))
        for style, count in requested:
            options = []
            if style != "unknown":
                for face in candidates[fonts.normalize_name(name)]:
                    if style_from_subfamily(face.subfamily) != style:
                        continue
                    content = face.path.read_bytes()
                    options.append((
                        hashlib.sha256(content).hexdigest(), len(content), face,
                        os2.inspect_os2_embedding_signal(content, face.face_index)
                    ))
            unique = {(digest, face.face_index): (digest, length, face, policy)
                      for digest, length, face, policy in options}
            supported = [entry for entry in unique.values()
                         if entry[2].container_kind != "collection" or entry[2].face_index == 0]
            if style == "unknown":
                status = "unknown_source_style"
            elif len(supported) > 1:
                status = "ambiguous_local_faces"
            elif len(supported) == 1:
                status = "single_local_candidate_unverified"
            elif options:
                status = "nonzero_collection_face_unsupported"
            else:
                status = "missing_local_face"
            statuses[status] = statuses.get(status, 0) + 1
            record = {
                "source_family": name,
                "requested_style": style,
                "source_run_count": count,
                "family_has_direct_source_index_evidence": source_index_proven,
                "status": status,
                "source_to_physical_face_verified": False,
                "license_verified": False,
                "editor_authoring_admitted": False,
                "publisher_layout_authoritative": False,
                "fixed_pdf_allowed": False,
            }
            if status == "single_local_candidate_unverified":
                digest, byte_len, face, policy = supported[0]
                record["local_candidate"] = {
                    "sha256": digest,
                    "byte_len": byte_len,
                    "face_index": face.face_index,
                    "subfamily": face.subfamily,
                    "postscript_name": face.postscript_name,
                    "collection_browser_face_zero_only": True,
                    "os2_embedding_metadata": policy,
                }
            records.append(record)
    return {
        "protocol_version": PROTOCOL,
        "private_operator_only": True,
        "source_sha256": source["source_sha256"],
        "source_to_physical_face_verified": False,
        "licenses_verified": False,
        "publisher_layout_authoritative": False,
        "fixed_pdf_allowed": False,
        "required_style_pairs": len(records),
        "coverage_status_counts": dict(sorted(statuses.items())),
        "malformed_local_font_files": malformed_files,
        "oversized_local_font_files": oversized_files,
        "ignored_local_symlinks": ignored_links,
        "source_families": len(families),
        "records": records,
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--requirements", required=True, type=Path)
    parser.add_argument("--font-dir", type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args(argv)
    try:
        folder = args.font_dir or fonts.default_windows_font_dir()
        result = audit_style_coverage(args.requirements, folder)
        # Prevent accidental replacement of a more recent private audit.
        args.output.parent.mkdir(parents=True, exist_ok=True)
        fd = os.open(args.output, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        with os.fdopen(fd, "w", encoding="utf-8") as output:
            json.dump(result, output, ensure_ascii=False, sort_keys=True, indent=2)
            output.write("\n")
    except (fonts.FontPacketError, OSError, ValueError, KeyError, TypeError) as exc:
        parser.exit(2, f"private font style coverage refused: {exc}\n")
    # Never print physical font bytes or local paths into GitHub log output.
    print(json.dumps({
        "protocol_version": PROTOCOL,
        "required_style_pairs": result["required_style_pairs"],
        "coverage_status_counts": result["coverage_status_counts"],
        "license_verified": False,
        "fixed_pdf_allowed": False,
    }, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
