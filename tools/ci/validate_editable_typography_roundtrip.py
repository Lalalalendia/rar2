#!/usr/bin/env python3
"""Fail-closed consumer roundtrip proof for bounded editable typography.

The validator never reads PUB source bytes. It compares typography that the
editable adapters actually serialized against consumer-produced documents:
IDML -> Scribus SLA and ODG -> LibreOffice FODG.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import uuid
import zipfile
from decimal import Decimal, InvalidOperation
from pathlib import Path
import xml.etree.ElementTree as ET

FAMILY_FEATURE = "story.typography.font_family"
SIZE_FEATURE = "story.typography.font_size"
SCHEMA = "chaptera.editable-typography-consumer-roundtrip.v1"

STYLE_NS = "urn:oasis:names:tc:opendocument:xmlns:style:1.0"
FO_NS = "urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0"
SVG_NS = "urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0"


def local_name(tag: str) -> str:
    return tag.rsplit("}", 1)[-1]


def normalized_font(value: str) -> str:
    value = " ".join(value.strip().split())
    if len(value) >= 2 and value[0] == value[-1] and value[0] in {"'", '"'}:
        value = value[1:-1].strip()
    return value.casefold()


def normalized_size(value: str) -> Decimal:
    value = value.strip()
    if value.casefold().endswith("pt"):
        value = value[:-2].strip()
    try:
        return Decimal(value).normalize()
    except InvalidOperation as exc:
        raise ValueError(f"invalid point size {value!r}") from exc


def pair(font: str, size: str) -> tuple[str, Decimal]:
    return normalized_font(font), normalized_size(size)


def printable_pair(value: tuple[str, Decimal]) -> dict[str, str]:
    font, size = value
    return {
        "font_family": font,
        "point_size": format(size, "f"),
        "pair_sha256": hashlib.sha256(
            f"{font}\0{format(size, 'f')}".encode("utf-8")
        ).hexdigest(),
    }


def preserved_typography_origins(report_path: Path) -> set[str]:
    report = json.loads(report_path.read_text(encoding="utf-8"))
    families = {
        item["origin"]
        for item in report["items"]
        if item["feature"] == FAMILY_FEATURE
        and item["disposition"] == "preserved"
        and item.get("origin")
    }
    sizes = {
        item["origin"]
        for item in report["items"]
        if item["feature"] == SIZE_FEATURE
        and item["disposition"] == "preserved"
        and item.get("origin")
    }
    if families != sizes:
        raise AssertionError(
            f"{report_path}: font-family/size preserved origin sets diverge"
        )
    return families


def idml_origin(self_value: str) -> str:
    if not re.fullmatch(r"us[0-9a-fA-F]{32}", self_value):
        raise ValueError(f"unexpected IDML Story Self={self_value!r}")
    return str(uuid.UUID(hex=self_value[2:]))


def idml_expected(path: Path) -> dict[str, tuple[str, Decimal]]:
    result: dict[str, tuple[str, Decimal]] = {}
    with zipfile.ZipFile(path) as archive:
        for name in sorted(archive.namelist()):
            if not (name.startswith("Stories/") and name.endswith(".xml")):
                continue
            root = ET.fromstring(archive.read(name))
            story = next((el for el in root.iter() if local_name(el.tag) == "Story"), None)
            if story is None:
                continue
            origin = idml_origin(story.attrib["Self"])
            ranges = [
                el
                for el in story.iter()
                if local_name(el.tag) == "CharacterStyleRange"
                and "PointSize" in el.attrib
            ]
            if not ranges:
                continue
            if len(ranges) != 1:
                raise AssertionError(
                    f"{path}: bounded full-Story typography expected one styled range for {origin}"
                )
            style_range = ranges[0]
            applied_font = next(
                (
                    el.text or ""
                    for el in style_range.iter()
                    if local_name(el.tag) == "AppliedFont"
                ),
                "",
            ).strip()
            if not applied_font:
                raise AssertionError(f"{path}: styled Story {origin} has no AppliedFont")
            result[origin] = pair(applied_font, style_range.attrib["PointSize"])
    return result


def scribus_pairs(path: Path) -> set[tuple[str, Decimal]]:
    root = ET.parse(path).getroot()
    result: set[tuple[str, Decimal]] = set()

    for story_text in (el for el in root.iter() if local_name(el.tag) == "StoryText"):
        default = next(
            (el for el in story_text if local_name(el.tag) == "DefaultStyle"),
            None,
        )
        default_font = default.attrib.get("FONT") if default is not None else None
        default_size = default.attrib.get("FONTSIZE") if default is not None else None
        for element in story_text.iter():
            font = element.attrib.get("FONT") or default_font
            size = element.attrib.get("FONTSIZE") or default_size
            if font and size:
                result.add(pair(font, size))

    # Some Scribus versions materialize a style outside StoryText. Retain this
    # conservative fallback while still requiring an exact family/size pair.
    for element in root.iter():
        font = element.attrib.get("FONT")
        size = element.attrib.get("FONTSIZE")
        if font and size:
            result.add(pair(font, size))

    return result


def odg_expected(path: Path) -> dict[str, tuple[str, Decimal]]:
    style_name = f"{{{STYLE_NS}}}name"
    style_family = f"{{{STYLE_NS}}}family"
    font_family = f"{{{FO_NS}}}font-family"
    font_size = f"{{{FO_NS}}}font-size"
    result: dict[str, tuple[str, Decimal]] = {}

    with zipfile.ZipFile(path) as archive:
        root = ET.fromstring(archive.read("content.xml"))

    for element in root.iter():
        if local_name(element.tag) != "style" or element.attrib.get(style_family) != "text":
            continue
        name = element.attrib.get(style_name, "")
        if not re.fullmatch(r"TextStyle_[0-9a-fA-F]{32}", name):
            continue
        props = next(
            (child for child in element if local_name(child.tag) == "text-properties"),
            None,
        )
        if props is None or font_family not in props.attrib or font_size not in props.attrib:
            raise AssertionError(f"{path}: incomplete bounded ODG text style {name}")
        origin = str(uuid.UUID(hex=name.removeprefix("TextStyle_")))
        result[origin] = pair(props.attrib[font_family], props.attrib[font_size])

    return result


def fodg_pairs(path: Path) -> set[tuple[str, Decimal]]:
    root = ET.parse(path).getroot()
    style_name = f"{{{STYLE_NS}}}name"
    style_font_name = f"{{{STYLE_NS}}}font-name"
    svg_font_family = f"{{{SVG_NS}}}font-family"
    fo_font_family = f"{{{FO_NS}}}font-family"
    fo_font_size = f"{{{FO_NS}}}font-size"

    faces: dict[str, str] = {}
    for element in root.iter():
        if local_name(element.tag) == "font-face":
            name = element.attrib.get(style_name)
            family = element.attrib.get(svg_font_family)
            if name and family:
                faces[name] = family

    result: set[tuple[str, Decimal]] = set()
    for element in root.iter():
        if local_name(element.tag) != "text-properties":
            continue
        size = element.attrib.get(fo_font_size)
        if not size or size.endswith("%"):
            continue
        family = element.attrib.get(fo_font_family)
        if not family:
            face_name = element.attrib.get(style_font_name)
            family = faces.get(face_name or "")
        if family:
            result.add(pair(family, size))
    return result


def validate_target(
    name: str,
    report_path: Path,
    expected: dict[str, tuple[str, Decimal]],
    consumer_pairs: set[tuple[str, Decimal]],
) -> dict[str, object]:
    preserved = preserved_typography_origins(report_path)
    expected_origins = set(expected)
    if preserved != expected_origins:
        missing_from_target = sorted(preserved - expected_origins)
        unplanned_in_target = sorted(expected_origins - preserved)
        raise AssertionError(
            f"{name}: target/report origin mismatch "
            f"missing={missing_from_target} unplanned={unplanned_in_target}"
        )

    expected_pairs = set(expected.values())
    missing_pairs = sorted(
        expected_pairs - consumer_pairs,
        key=lambda item: (item[0], item[1]),
    )
    result = {
        "preserved_story_count": len(preserved),
        "expected_pair_count": len(expected_pairs),
        "consumer_pair_count": len(consumer_pairs),
        "matched_pair_count": len(expected_pairs) - len(missing_pairs),
        "missing_pairs": [printable_pair(value) for value in missing_pairs],
        "passed": not missing_pairs,
    }
    if missing_pairs:
        raise AssertionError(
            f"{name}: consumer roundtrip lost {len(missing_pairs)} exact typography pairs: "
            + json.dumps(result["missing_pairs"], sort_keys=True)
        )
    return result


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--idml", type=Path)
    parser.add_argument("--idml-report", type=Path)
    parser.add_argument("--sla", type=Path)
    parser.add_argument("--odg", type=Path)
    parser.add_argument("--odg-report", type=Path)
    parser.add_argument("--fodg", type=Path)
    parser.add_argument("--receipt", type=Path, required=True)
    args = parser.parse_args()

    receipt: dict[str, object] = {"schema": SCHEMA}
    failure: Exception | None = None

    try:
        if args.idml and args.idml.is_file():
            if not args.idml_report or not args.sla or not args.sla.is_file():
                raise AssertionError("IDML validation requires report + Scribus SLA")
            receipt["idml"] = validate_target(
                "idml",
                args.idml_report,
                idml_expected(args.idml),
                scribus_pairs(args.sla),
            )

        if args.odg and args.odg.is_file():
            if not args.odg_report or not args.fodg or not args.fodg.is_file():
                raise AssertionError("ODG validation requires report + LibreOffice FODG")
            receipt["odg"] = validate_target(
                "odg",
                args.odg_report,
                odg_expected(args.odg),
                fodg_pairs(args.fodg),
            )
    except Exception as exc:  # write a source-safe receipt before failing CI
        failure = exc
        receipt["error"] = str(exc)

    args.receipt.parent.mkdir(parents=True, exist_ok=True)
    args.receipt.write_text(
        json.dumps(receipt, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    print(json.dumps(receipt, indent=2, sort_keys=True))

    if failure is not None:
        raise failure
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
