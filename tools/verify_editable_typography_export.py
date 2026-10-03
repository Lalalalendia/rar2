#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import math
import re
import zipfile
from pathlib import Path
from xml.etree import ElementTree as ET

STYLE_NS = "urn:oasis:names:tc:opendocument:xmlns:style:1.0"
TEXT_NS = "urn:oasis:names:tc:opendocument:xmlns:text:1.0"
FO_NS = "urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0"
SVG_NS = "urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0"


def local(tag: str) -> str:
    return tag.rsplit("}", 1)[-1]


def norm_family(value: str | None) -> str:
    if value is None:
        return ""
    value = value.strip()
    if len(value) >= 2 and value[0] == value[-1] and value[0] in {"'", '"'}:
        value = value[1:-1]
    return re.sub(r"\s+", " ", value).casefold()


def parse_points(value: str | None) -> float | None:
    if not value:
        return None
    value = value.strip()
    if value.endswith("pt"):
        value = value[:-2]
    try:
        return float(value)
    except ValueError:
        return None


def expected(path: Path) -> list[dict]:
    data = json.loads(path.read_text())
    items = data.get("items", [])
    if not items:
        raise AssertionError("typography witness has no eligible items")
    return items


def verify_report(root: Path, expected_path: Path) -> dict:
    data = json.loads(expected_path.read_text())
    all_items = data.get("all_items")
    if not all_items:
        raise AssertionError("scoped LossReport proof requires all_items census for the witness source")

    proven_origins = {
        item["story_id"]
        for item in all_items
        if norm_family(item.get("font_family")) == "montserrat"
    }
    if not proven_origins:
        raise AssertionError("witness source has no consumer-proven Montserrat Story")

    target_results = {}
    family_feature = "story.typography.font_family"
    size_feature = "story.typography.font_size"
    color_feature = "story.typography.color"
    alignment_feature = "story.paragraph_alignment"
    scoped_features = {family_feature, size_feature}

    for target in ("idml", "odg"):
        report_path = root / f"{target}.report.json"
        if not report_path.is_file():
            raise AssertionError(f"missing {target} LossReport: {report_path}")
        report = json.loads(report_path.read_text())

        observed = {
            (item.get("origin"), item["feature"]): item["disposition"]
            for item in report.get("items", [])
            if item["feature"] in scoped_features
        }

        for origin in sorted(proven_origins):
            for feature in sorted(scoped_features):
                disposition = observed.get((origin, feature))
                if disposition != "preserved":
                    raise AssertionError(
                        f"{target}: proven {feature} @ {origin} is {disposition!r}, "
                        "expected preserved"
                    )

        leaked = sorted(
            (origin, feature, disposition)
            for (origin, feature), disposition in observed.items()
            if disposition == "preserved" and origin not in proven_origins
        )
        if leaked:
            raise AssertionError(
                f"{target}: scoped typography preservation leaked to unproven origins: {leaked[:40]!r}"
            )

        unrelated_preserved = [
            item
            for item in report.get("items", [])
            if item["feature"] in {color_feature, alignment_feature}
            and item["disposition"] == "preserved"
        ]
        if unrelated_preserved:
            raise AssertionError(
                f"{target}: color/alignment were unexpectedly promoted: "
                f"{unrelated_preserved[:20]!r}"
            )

        promoted = sum(
            disposition == "preserved"
            for (origin, _feature), disposition in observed.items()
            if origin in proven_origins
        )
        target_results[target] = {
            "proven_story_count": len(proven_origins),
            "promoted_feature_count": promoted,
            "unexpected_preserved_origin_count": len(leaked),
            "color_alignment_preserved_count": len(unrelated_preserved),
        }

    return {
        "proven_story_count": len(proven_origins),
        "targets": target_results,
    }


def assert_pair(pairs: set[tuple[str, float]], family: str, size: float, label: str) -> None:
    wanted = norm_family(family)
    for actual_family, actual_size in pairs:
        if norm_family(actual_family) == wanted and math.isclose(
            actual_size, size, rel_tol=0.0, abs_tol=0.02
        ):
            return
    raise AssertionError(
        f"{label}: missing font/size pair family={family!r} size={size}; "
        f"observed={sorted(pairs)[:80]!r}"
    )


def verify_wire(root: Path, items: list[dict]) -> dict:
    idml = root / "output.idml"
    odg = root / "output.odg"
    if not idml.is_file() or not odg.is_file():
        raise AssertionError("materialized IDML/ODG outputs are required")

    idml_pairs: set[tuple[str, float]] = set()
    with zipfile.ZipFile(idml) as archive:
        designmap = ET.fromstring(archive.read("designmap.xml"))
        font_refs = [
            node.attrib.get("src")
            for node in designmap.iter()
            if local(node.tag) == "Fonts"
        ]
        if font_refs != ["Resources/Fonts.xml"]:
            raise AssertionError(f"IDML typography wire has unexpected Fonts refs: {font_refs!r}")

        fonts_root = ET.fromstring(archive.read("Resources/Fonts.xml"))
        font_faces = {
            (
                family.attrib.get("Name"),
                face.attrib.get("FontStyleName"),
                face.attrib.get("PostScriptName"),
            )
            for family in fonts_root.iter()
            if local(family.tag) == "FontFamily"
            for face in list(family)
            if local(face.tag) == "Font"
        }

        for item in items:
            story_hex = item["story_id"].replace("-", "").lower()
            path = f"Stories/Story_us{story_hex}.xml"
            raw = archive.read(path)
            tree = ET.fromstring(raw)
            pairs: set[tuple[str, float]] = set()
            for node in tree.iter():
                if local(node.tag) != "CharacterStyleRange":
                    continue
                size = parse_points(node.attrib.get("PointSize"))
                family = None
                for child in node.iter():
                    if local(child.tag) == "AppliedFont" and child.text:
                        family = child.text
                        break
                if family and size is not None:
                    pairs.add((family, size))
            assert_pair(
                pairs,
                item["font_family"],
                float(item["font_size_pt"]),
                f"IDML Story {item['story_id']}",
            )
            if norm_family(item["font_family"]) == "montserrat":
                expected_face = ("Montserrat", "Regular", "Montserrat-Regular")
                if expected_face not in font_faces:
                    raise AssertionError(
                        f"IDML Fonts.xml missing bounded face {expected_face!r}; "
                        f"observed={sorted(font_faces)!r}"
                    )
            idml_pairs.update(pairs)

    with zipfile.ZipFile(odg) as archive:
        tree = ET.fromstring(archive.read("content.xml"))
    odg_pairs: set[tuple[str, float]] = set()
    span_styles = {
        node.attrib.get(f"{{{TEXT_NS}}}style-name")
        for node in tree.iter()
        if local(node.tag) == "span"
    }
    for item in items:
        story_hex = item["story_id"].replace("-", "").lower()
        style_name = f"PubStoryT_{story_hex}"
        style = next(
            (
                node
                for node in tree.iter()
                if local(node.tag) == "style"
                and node.attrib.get(f"{{{STYLE_NS}}}name") == style_name
            ),
            None,
        )
        if style is None:
            raise AssertionError(f"ODG missing automatic text style {style_name}")
        props = next((node for node in style.iter() if local(node.tag) == "text-properties"), None)
        if props is None:
            raise AssertionError(f"ODG style {style_name} has no text-properties")
        family = props.attrib.get(f"{{{FO_NS}}}font-family")
        size = parse_points(props.attrib.get(f"{{{FO_NS}}}font-size"))
        if family is None or size is None:
            raise AssertionError(f"ODG style {style_name} lacks font family/size")
        assert_pair(
            {(family, size)},
            item["font_family"],
            float(item["font_size_pt"]),
            f"ODG Story {item['story_id']}",
        )
        if style_name not in span_styles:
            raise AssertionError(f"ODG style {style_name} is not referenced by a text span")
        odg_pairs.add((family, size))

    return {
        "wire_idml_pair_count": len(idml_pairs),
        "wire_odg_pair_count": len(odg_pairs),
    }


def scribus_pairs(path: Path) -> set[tuple[str, float]]:
    root = ET.parse(path).getroot()
    document = next((node for node in root.iter() if local(node.tag) == "DOCUMENT"), None)
    doc_font = document.attrib.get("DFONT") if document is not None else None
    doc_size = parse_points(document.attrib.get("DSIZE")) if document is not None else None
    pairs: set[tuple[str, float]] = set()

    for story in (node for node in root.iter() if local(node.tag) == "StoryText"):
        default = next(
            (node for node in list(story) if local(node.tag) == "DefaultStyle"),
            None,
        )
        family = default.attrib.get("FONT", doc_font) if default is not None else doc_font
        size = (
            parse_points(default.attrib.get("FONTSIZE"))
            if default is not None and default.attrib.get("FONTSIZE") is not None
            else doc_size
        )
        for text in (node for node in story.iter() if local(node.tag) == "ITEXT"):
            effective_family = text.attrib.get("FONT", family)
            effective_size = parse_points(text.attrib.get("FONTSIZE"))
            if effective_size is None:
                effective_size = size
            if effective_family and effective_size is not None:
                pairs.add((effective_family, effective_size))
    return pairs


def scribus_family_name(value: str) -> str:
    normalized = norm_family(value)
    # Scribus SLA stores the installed regular face as a full face name
    # ("Family Regular"), while Publisher/our source authority stores the
    # family name. Only canonicalize the exact Regular suffix; do not collapse
    # Bold/Italic/etc. into the family because those faces are not proven here.
    suffix = " regular"
    if normalized.endswith(suffix):
        return normalized[: -len(suffix)]
    return normalized


def verify_scribus(path: Path, items: list[dict]) -> dict:
    pairs = scribus_pairs(path)
    for item in items:
        wanted_family = norm_family(item["font_family"])
        wanted_size = float(item["font_size_pt"])
        if not any(
            scribus_family_name(actual_family) == wanted_family
            and math.isclose(actual_size, wanted_size, rel_tol=0.0, abs_tol=0.02)
            for actual_family, actual_size in pairs
        ):
            raise AssertionError(
                "Scribus save/reopen: missing regular-face font/size pair "
                f"family={item['font_family']!r} size={wanted_size}; "
                f"observed={sorted(pairs)[:80]!r}"
            )
    return {"scribus_pair_count": len(pairs)}


def libreoffice_pairs(path: Path) -> set[tuple[str, float]]:
    root = ET.parse(path).getroot()
    font_faces: dict[str, str] = {}
    for node in root.iter():
        if local(node.tag) != "font-face":
            continue
        name = node.attrib.get(f"{{{STYLE_NS}}}name")
        family = node.attrib.get(f"{{{SVG_NS}}}font-family")
        if name and family:
            font_faces[name] = family

    pairs: set[tuple[str, float]] = set()
    for node in root.iter():
        if local(node.tag) != "text-properties":
            continue
        size = parse_points(node.attrib.get(f"{{{FO_NS}}}font-size"))
        family = node.attrib.get(f"{{{FO_NS}}}font-family")
        if not family:
            font_name = node.attrib.get(f"{{{STYLE_NS}}}font-name")
            if font_name:
                family = font_faces.get(font_name)
        if family and size is not None:
            pairs.add((family, size))
    return pairs


def verify_libreoffice(path: Path, items: list[dict]) -> dict:
    pairs = libreoffice_pairs(path)
    for item in items:
        assert_pair(
            pairs,
            item["font_family"],
            float(item["font_size_pt"]),
            "LibreOffice save/reopen",
        )
    return {"libreoffice_pair_count": len(pairs)}


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("mode", choices=("wire", "report", "scribus", "libreoffice"))
    parser.add_argument("--expected", required=True, type=Path)
    parser.add_argument("--root", type=Path)
    parser.add_argument("--input", type=Path)
    parser.add_argument("--receipt", type=Path)
    args = parser.parse_args()

    items = expected(args.expected)
    if args.mode == "wire":
        if args.root is None:
            parser.error("wire mode requires --root")
        result = verify_wire(args.root, items)
    elif args.mode == "report":
        if args.root is None:
            parser.error("report mode requires --root")
        result = verify_report(args.root, args.expected)
    elif args.mode == "scribus":
        if args.input is None:
            parser.error("scribus mode requires --input")
        result = verify_scribus(args.input, items)
    else:
        if args.input is None:
            parser.error("libreoffice mode requires --input")
        result = verify_libreoffice(args.input, items)

    payload = {
        "schema": "chaptera.editable-typography-consumer-check.v1",
        "mode": args.mode,
        "eligible_story_count": len(items),
        **result,
    }
    print(json.dumps(payload, indent=2, sort_keys=True))
    if args.receipt:
        args.receipt.parent.mkdir(parents=True, exist_ok=True)
        args.receipt.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
