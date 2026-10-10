#!/usr/bin/env python3
from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
from dataclasses import dataclass
from pathlib import Path, PurePosixPath
import struct
import sys
from typing import Iterable

TOOL_SCHEMA = "chaptera.cloud-reader-private-font-packet.v1"
REGULAR_STYLE_NAMES = {"", "regular", "roman", "normal", "book", "plain"}
SUPPORTED_SUFFIXES = {".ttf", ".otf", ".ttc", ".otc"}


class FontPacketError(RuntimeError):
    pass


@dataclass(frozen=True)
class FontFace:
    path: Path
    face_index: int
    family: str
    subfamily: str
    postscript_name: str | None
    container_kind: str
    mime: str


def _u16(data: bytes, offset: int) -> int:
    if offset < 0 or offset + 2 > len(data):
        raise FontPacketError(f"truncated u16 at {offset}")
    return struct.unpack_from(">H", data, offset)[0]


def _u32(data: bytes, offset: int) -> int:
    if offset < 0 or offset + 4 > len(data):
        raise FontPacketError(f"truncated u32 at {offset}")
    return struct.unpack_from(">I", data, offset)[0]


def _decode_name(platform_id: int, raw: bytes) -> str | None:
    try:
        if platform_id in (0, 3):
            return raw.decode("utf-16-be").strip("\x00").strip()
        if platform_id == 1:
            return raw.decode("mac_roman").strip("\x00").strip()
    except UnicodeDecodeError:
        return None
    return None


def _name_score(platform_id: int, language_id: int) -> tuple[int, int]:
    platform_score = {3: 3, 0: 2, 1: 1}.get(platform_id, 0)
    if platform_id == 3 and language_id == 0x0409:
        language_score = 3
    elif platform_id == 1 and language_id == 0:
        language_score = 3
    elif language_id in (0, 0x0409):
        language_score = 2
    else:
        language_score = 1
    return platform_score, language_score


def _preferred_name(records: list[tuple[int, int, int, str]], ids: Iterable[int]) -> str | None:
    ordered_ids = tuple(ids)
    allowed = set(ordered_ids)
    candidates = [record for record in records if record[2] in allowed and record[3]]
    if not candidates:
        return None
    candidates.sort(
        key=lambda rec: (_name_score(rec[0], rec[1]), -ordered_ids.index(rec[2])),
        reverse=True,
    )
    return candidates[0][3]


def _parse_face(data: bytes, path: Path, face_offset: int, face_index: int, container_kind: str) -> FontFace:
    if face_offset < 0 or face_offset + 12 > len(data):
        raise FontPacketError(f"{path}: truncated sfnt face {face_index}")
    scaler = data[face_offset : face_offset + 4]
    if scaler not in (b"\x00\x01\x00\x00", b"OTTO", b"true", b"typ1"):
        raise FontPacketError(f"{path}: unsupported sfnt scaler {scaler!r} for face {face_index}")
    num_tables = _u16(data, face_offset + 4)
    record_base = face_offset + 12
    if record_base + num_tables * 16 > len(data):
        raise FontPacketError(f"{path}: truncated table directory for face {face_index}")

    name_offset = None
    name_length = None
    for table_index in range(num_tables):
        entry = record_base + table_index * 16
        tag = data[entry : entry + 4]
        table_offset = _u32(data, entry + 8)
        table_length = _u32(data, entry + 12)
        if table_offset + table_length > len(data):
            raise FontPacketError(f"{path}: table {tag!r} escapes file bounds")
        if tag == b"name":
            name_offset = table_offset
            name_length = table_length
            break
    if name_offset is None or name_length is None:
        raise FontPacketError(f"{path}: face {face_index} has no name table")
    if name_length < 6:
        raise FontPacketError(f"{path}: face {face_index} has truncated name table")

    count = _u16(data, name_offset + 2)
    strings_offset = _u16(data, name_offset + 4)
    records_end = name_offset + 6 + count * 12
    strings_base = name_offset + strings_offset
    if records_end > name_offset + name_length or strings_base > name_offset + name_length:
        raise FontPacketError(f"{path}: malformed name table in face {face_index}")

    names: list[tuple[int, int, int, str]] = []
    for record_index in range(count):
        pos = name_offset + 6 + record_index * 12
        platform_id = _u16(data, pos)
        language_id = _u16(data, pos + 4)
        name_id = _u16(data, pos + 6)
        length = _u16(data, pos + 8)
        offset = _u16(data, pos + 10)
        start = strings_base + offset
        end = start + length
        if start < strings_base or end > name_offset + name_length:
            continue
        decoded = _decode_name(platform_id, data[start:end])
        if decoded:
            names.append((platform_id, language_id, name_id, decoded))

    family = _preferred_name(names, (16, 1))
    subfamily = _preferred_name(names, (17, 2)) or ""
    postscript_name = _preferred_name(names, (6,))
    if not family:
        raise FontPacketError(f"{path}: face {face_index} has no usable family name")
    mime = "font/otf" if scaler == b"OTTO" else "font/ttf"
    return FontFace(
        path=path,
        face_index=face_index,
        family=family,
        subfamily=subfamily,
        postscript_name=postscript_name,
        container_kind=container_kind,
        mime=mime,
    )


def parse_font_faces(path: Path) -> list[FontFace]:
    data = path.read_bytes()
    if len(data) < 12:
        raise FontPacketError(f"{path}: file too small to be an sfnt font")
    if data[:4] == b"ttcf":
        count = _u32(data, 8)
        if count == 0 or count > 4096:
            raise FontPacketError(f"{path}: invalid collection face count {count}")
        if 12 + count * 4 > len(data):
            raise FontPacketError(f"{path}: truncated collection header")
        offsets = [_u32(data, 12 + index * 4) for index in range(count)]
        return [
            _parse_face(data, path, offset, index, "collection")
            for index, offset in enumerate(offsets)
        ]
    return [_parse_face(data, path, 0, 0, "sfnt")]


def normalize_name(value: str) -> str:
    return " ".join(value.casefold().split())


def is_regular_style(value: str) -> bool:
    return normalize_name(value) in REGULAR_STYLE_NAMES


def discover_regular_face(font_dir: Path, requested_family: str) -> FontFace:
    if not font_dir.is_dir():
        raise FontPacketError(f"font directory does not exist: {font_dir}")
    target = normalize_name(requested_family)
    candidates: list[FontFace] = []
    parse_failures: list[str] = []
    for path in sorted(font_dir.iterdir(), key=lambda p: p.name.casefold()):
        if not path.is_file() or path.suffix.casefold() not in SUPPORTED_SUFFIXES:
            continue
        try:
            faces = parse_font_faces(path)
        except (OSError, FontPacketError) as exc:
            parse_failures.append(f"{path.name}: {exc}")
            continue
        for face in faces:
            if normalize_name(face.family) == target and is_regular_style(face.subfamily):
                candidates.append(face)
    if not candidates:
        detail = ""
        if parse_failures:
            detail = f"; {len(parse_failures)} font file(s) could not be parsed"
        raise FontPacketError(f"no unique Regular face found for {requested_family!r}{detail}")
    unique = {(face.path.resolve(), face.face_index): face for face in candidates}
    candidates = list(unique.values())
    if len(candidates) != 1:
        rendered = ", ".join(
            f"{face.path.name}#face{face.face_index} ({face.family} {face.subfamily})"
            for face in sorted(candidates, key=lambda f: (f.path.name.casefold(), f.face_index))
        )
        raise FontPacketError(f"ambiguous Regular face for {requested_family!r}: {rendered}")
    face = candidates[0]
    if face.container_kind == "collection" and face.face_index != 0:
        raise FontPacketError(
            f"{requested_family!r} resolves to collection face_index={face.face_index}; "
            "current Cloud browser font transport does not prove non-zero collection-face paint. "
            "Refusing to produce a misleading exact-font packet."
        )
    return face


def _safe_slug(value: str) -> str:
    slug = "".join(ch.lower() if ch.isalnum() else "-" for ch in value)
    while "--" in slug:
        slug = slug.replace("--", "-")
    return slug.strip("-") or "font"


def _toml_string(value: str) -> str:
    return json.dumps(value, ensure_ascii=False)


REQUIREMENTS_PROTOCOL = "chaptera.pub-source-font-requirements.v1"
PUB_SHA = re.compile(r"^[0-9a-f]{64}$")


def read_private_source_requirements(path: Path) -> tuple[list[str], dict]:
    """Read source-declared, *non-authorizing* Writer/Reader requirements."""
    try:
        packet = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise FontPacketError("cannot read verified private source-font requirements") from exc
    if (not isinstance(packet, dict)
            or packet.get("protocol_version") != REQUIREMENTS_PROTOCOL
            or not isinstance(packet.get("document_source_sha256"), str)
            or not PUB_SHA.fullmatch(packet["document_source_sha256"])
            or packet.get("source_declared_families_only") is not True
            or packet.get("requires_private_licensed_physical_bytes") is not True
            or packet.get("physical_font_bytes_included") is not False
            or packet.get("original_publisher_layout_authoritative") is not False
            or packet.get("fixed_pdf_allowed") is not False):
        raise FontPacketError("invalid or falsely authorized Publisher source-font requirements")
    items = packet.get("families")
    if not isinstance(items, list) or not items or len(items) > 512:
        raise FontPacketError("bounded source family requirements are missing")
    families: list[str] = []
    lower_names = set()
    incomplete_styles = False
    missing_quill_indices = 0
    for item in items:
        if not isinstance(item, dict):
            raise FontPacketError("malformed source family requirement")
        family = item.get("source_family")
        if (not isinstance(family, str) or family != family.strip()
                or not family or len(family) > 128
                or any(ord(ch) < 32 for ch in family)
                or item.get("physical_face_authorized") is not False):
            raise FontPacketError("source family is missing or falsely treated as a physical font")
        key = normalize_name(family)
        if key in lower_names:
            raise FontPacketError("ambiguous normalized family names in source requirements")
        lower_names.add(key)
        indices = item.get("source_font_index_candidates")
        if (not isinstance(indices, list) or any(
            type(index) is not int or not 0 <= index <= 65535 for index in indices
        ) or indices != sorted(set(indices))):
            raise FontPacketError("source Quill font index candidates are malformed")
        if item.get("source_quill_index_proven") is not bool(indices):
            raise FontPacketError("source font index proof contradicted by source family requirements")
        missing_quill_indices += not bool(indices)
        styles = item.get("effective_style_run_counts")
        if not isinstance(styles, dict):
            raise FontPacketError("source run style counts are required")
        if any(
            name not in {"regular", "bold", "italic", "bold_italic", "unknown"}
            or type(count) is not int or count < 0
            for name, count in styles.items()
        ):
            raise FontPacketError("unsupported or malformed source font style counts")
        needs_non_regular = bool(
            styles.get("bold", 0) or styles.get("italic", 0) or styles.get("bold_italic", 0)
        )
        if item.get("needs_non_regular_style") is not needs_non_regular:
            raise FontPacketError("source font style requirements contradict Reader evidence")
        incomplete_styles |= needs_non_regular or bool(styles.get("unknown", 0)) or not bool(indices)
        families.append(family)
    if (type(packet.get("source_families_without_quill_index_count")) is not int
            or packet["source_families_without_quill_index_count"] != missing_quill_indices):
        raise FontPacketError("source Quill-index gap count disagrees with family requirements")
    if (type(packet.get("unresolved_source_family_run_count")) is not int
            or packet["unresolved_source_family_run_count"] < 0
            or type(packet.get("unknown_effective_style_run_count")) is not int
            or packet["unknown_effective_style_run_count"] < 0):
        raise FontPacketError("unresolved source font counters are not trustworthy")
    incomplete_styles |= bool(packet["unresolved_source_family_run_count"])
    incomplete_styles |= bool(packet["unknown_effective_style_run_count"])
    return families, {
        "source_sha256": packet["document_source_sha256"],
        "source_family_requirements_only": True,
        "partial_style_or_source_coverage": incomplete_styles,
        "native_publisher_layout_authoritative": False,
        "fixed_pdf_allowed": False,
    }


def build_packet(
    families: list[str],
    font_dir: Path,
    output_dir: Path,
    cloud_font_dir: str,
    *,
    source_requirements: dict | None = None,
    allow_incomplete_styles: bool = False,
) -> dict:
    if not families:
        raise FontPacketError("at least one source family is required")
    if source_requirements:
        if (source_requirements["partial_style_or_source_coverage"]
                and not allow_incomplete_styles):
            raise FontPacketError(
                "source PUB needs styled or unresolved fonts; Regular-only packet cannot "
                "satisfy them. Pass --allow-incomplete-styles to export a clearly "
                "marked partial private packet; do not certify visual parity."
            )
    normalized = [normalize_name(family) for family in families]
    if len(set(normalized)) != len(normalized):
        raise FontPacketError("duplicate --family values are not allowed")
    if output_dir.exists() and any(output_dir.iterdir()):
        raise FontPacketError(f"output directory must be absent or empty: {output_dir}")
    cloud_root = PurePosixPath(cloud_font_dir)
    if not cloud_root.is_absolute() or ".." in cloud_root.parts:
        raise FontPacketError(f"--cloud-font-dir must be an absolute normalized POSIX path")

    output_dir.mkdir(parents=True, exist_ok=True)
    fonts_dir = output_dir / "fonts"
    fonts_dir.mkdir(exist_ok=True)

    resources = []
    toml_parts: list[str] = []
    for requested_family in families:
        face = discover_regular_face(font_dir, requested_family)
        source_bytes = face.path.read_bytes()
        sha256 = hashlib.sha256(source_bytes).hexdigest()
        suffix = face.path.suffix.casefold() or ".font"
        packet_name = f"{_safe_slug(requested_family)}-{sha256[:16]}{suffix}"
        packet_path = fonts_dir / packet_name
        packet_path.write_bytes(source_bytes)
        cloud_path = str(cloud_root / packet_name)
        resources.append(
            {
                "source_family": requested_family,
                "resolved_family": face.family,
                "subfamily": face.subfamily,
                "postscript_name": face.postscript_name,
                "packet_file": f"fonts/{packet_name}",
                "sha256": sha256,
                "byte_len": len(source_bytes),
                "face_index": face.face_index,
                "container_kind": face.container_kind,
                "mime": face.mime,
                "cloud_path": cloud_path,
                "browser_collection_policy": (
                    "first_face_only" if face.container_kind == "collection" else "standalone_face"
                ),
            }
        )
        toml_parts.extend(
            [
                "[[cloud_reader_guest.font_resources]]",
                f"source_family = {_toml_string(requested_family)}",
                f"path = {_toml_string(cloud_path)}",
                f"expected_sha256 = {_toml_string(sha256)}",
                f"face_index = {face.face_index}",
                f"mime = {_toml_string(face.mime)}",
                "",
            ]
        )

    manifest = {
        "schema": TOOL_SCHEMA,
        "private_operator_packet": True,
        "redistribution_rights_not_granted": True,
        "font_bytes_must_not_be_committed_or_uploaded_to_public_ci": True,
        "cloud_font_dir": cloud_font_dir,
        "resources": resources,
        "publisher_source_requirements": (
            {
                **source_requirements,
                "regular_only_packet": True,
                "private_licensed_source_face_mapping_unverified": True,
                "publisher_visual_parity_verified": False,
            }
            if source_requirements else None
        ),
    }
    (output_dir / "manifest.json").write_text(
        json.dumps(manifest, indent=2, ensure_ascii=False) + "\n", encoding="utf-8"
    )
    (output_dir / "cloud-reader-font-resources.toml").write_text(
        "\n".join(toml_parts), encoding="utf-8"
    )
    (output_dir / "README.txt").write_text(
        "PRIVATE OPERATOR FONT PACKET\n"
        "\n"
        "This packet contains font bytes copied from this machine. It does not grant redistribution or server-use rights.\n"
        "Verify your license before deploying. Do not commit these files or upload them to public CI.\n"
        "\n"
        f"1. Copy files under fonts/ to {cloud_font_dir}/ on the private Cloud Reader host.\n"
        "2. Append cloud-reader-font-resources.toml to the private Chaptera production config.\n"
        "3. Keep the generated expected_sha256 and face_index unchanged.\n"
        "4. Run the existing Cloud Reader acceptance against the private host.\n",
        encoding="utf-8",
    )
    return manifest


def default_windows_font_dir() -> Path:
    windir = os.environ.get("WINDIR")
    if not windir:
        raise FontPacketError("WINDIR is not set; pass --font-dir explicitly")
    return Path(windir) / "Fonts"


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Build a private Cloud Reader configured-font packet from exact local Windows font files."
    )
    parser.add_argument("--family", action="append", default=[], help="source family to export; repeatable")
    parser.add_argument("--requirements", type=Path,
                        help="private source-font requirement plan from pub_source_font_requirements_v1.py")
    parser.add_argument("--allow-incomplete-styles", action="store_true",
                        help="explicitly export partial Regular-only packet when PUB needs bold/italic/unknown styles")
    parser.add_argument("--font-dir", type=Path, help="font directory; defaults to %%WINDIR%%\\Fonts")
    parser.add_argument("--output-dir", type=Path, required=True, help="new/empty private packet directory")
    parser.add_argument(
        "--cloud-font-dir",
        default="/etc/chaptera/fonts",
        help="absolute private host directory used in generated TOML",
    )
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(sys.argv[1:] if argv is None else argv)
    try:
        font_dir = args.font_dir if args.font_dir is not None else default_windows_font_dir()
        required, provenance = (
            read_private_source_requirements(args.requirements)
            if args.requirements is not None else ([], None)
        )
        seen = set()
        families = []
        for family in required + args.family:
            normalized = normalize_name(family)
            if normalized not in seen:
                families.append(family)
                seen.add(normalized)
            elif family not in required:
                raise FontPacketError("duplicate/ambiguous manual --family value")
        manifest = build_packet(
            families, font_dir, args.output_dir, args.cloud_font_dir,
            source_requirements=provenance,
            allow_incomplete_styles=args.allow_incomplete_styles,
        )
    except (OSError, FontPacketError) as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 2
    print(json.dumps(manifest, indent=2, ensure_ascii=False))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
