#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import zipfile
from collections import Counter
from pathlib import Path
from xml.etree import ElementTree as ET

STYLE_NS = "urn:oasis:names:tc:opendocument:xmlns:style:1.0"
TEXT_NS = "urn:oasis:names:tc:opendocument:xmlns:text:1.0"
FO_NS = "urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0"
DRAW_NS = "urn:oasis:names:tc:opendocument:xmlns:drawing:1.0"


def local(tag: str) -> str:
    return tag.rsplit("}", 1)[-1]


def load_expected(path: Path) -> list[dict]:
    data = json.loads(path.read_text())
    items = data.get("items", [])
    if not items:
        raise AssertionError("paragraph-alignment expected set is empty")
    return items


def expected_counts(items: list[dict]) -> Counter:
    counts = Counter(str(item["alignment"]).casefold() for item in items)
    unknown = set(counts) - {"center", "right"}
    if unknown:
        raise AssertionError(f"unexpected alignment values: {sorted(unknown)!r}")
    return counts


def idml_story_path(story_id: str) -> str:
    return f"Stories/Story_us{story_id.replace('-', '').lower()}.xml"


def odg_style_name(story_id: str) -> str:
    return f"PubStoryP_{story_id.replace('-', '').lower()}"


def verify_wire(root: Path, items: list[dict]) -> dict:
    counts = expected_counts(items)
    idml = root / "output.idml"
    odg = root / "output.odg"
    if not idml.is_file() or not odg.is_file():
        raise AssertionError("materialized IDML/ODG outputs are required")

    idml_observed = Counter()
    with zipfile.ZipFile(idml) as archive:
        for item in items:
            tree = ET.fromstring(archive.read(idml_story_path(item["story_id"])))
            ranges = [
                node
                for node in tree.iter()
                if local(node.tag) == "ParagraphStyleRange"
            ]
            if len(ranges) != 1:
                raise AssertionError(
                    f"IDML Story {item['story_id']} expected one ParagraphStyleRange; "
                    f"observed={len(ranges)}"
                )
            actual = ranges[0].attrib.get("Justification")
            wanted = "CenterAlign" if item["alignment"] == "center" else "RightAlign"
            if actual != wanted:
                raise AssertionError(
                    f"IDML Story {item['story_id']} alignment mismatch: "
                    f"expected={wanted!r} actual={actual!r}"
                )
            idml_observed[item["alignment"]] += 1

    with zipfile.ZipFile(odg) as archive:
        odg_root = ET.fromstring(archive.read("content.xml"))

    styles: dict[str, str] = {}
    for node in odg_root.iter():
        if local(node.tag) != "style":
            continue
        name = node.attrib.get(f"{{{STYLE_NS}}}name")
        family = node.attrib.get(f"{{{STYLE_NS}}}family")
        if not name or family != "paragraph":
            continue
        props = next(
            (child for child in node.iter() if local(child.tag) == "paragraph-properties"),
            None,
        )
        if props is None:
            continue
        align = props.attrib.get(f"{{{FO_NS}}}text-align")
        if align:
            styles[name] = align.casefold()

    paragraph_refs = Counter()
    for node in odg_root.iter():
        if local(node.tag) != "p":
            continue
        style_name = node.attrib.get(f"{{{TEXT_NS}}}style-name")
        align = styles.get(style_name or "")
        if align in {"center", "right"}:
            paragraph_refs[align] += 1

    for item in items:
        style_name = odg_style_name(item["story_id"])
        actual = styles.get(style_name)
        if actual != item["alignment"]:
            raise AssertionError(
                f"ODG Story {item['story_id']} style {style_name} mismatch: "
                f"expected={item['alignment']!r} actual={actual!r}"
            )
        if not any(
            local(node.tag) == "p"
            and node.attrib.get(f"{{{TEXT_NS}}}style-name") == style_name
            for node in odg_root.iter()
        ):
            raise AssertionError(
                f"ODG Story {item['story_id']} paragraph style {style_name} is unreferenced"
            )

    if idml_observed != counts:
        raise AssertionError(
            f"IDML alignment counts mismatch expected={dict(counts)} "
            f"observed={dict(idml_observed)}"
        )
    for alignment, count in counts.items():
        if paragraph_refs[alignment] < count:
            raise AssertionError(
                f"ODG has fewer {alignment} paragraph carriers than eligible Stories: "
                f"stories={count} paragraphs={paragraph_refs[alignment]}"
            )

    return {
        "expected_story_counts": dict(sorted(counts.items())),
        "idml_story_counts": dict(sorted(idml_observed.items())),
        "odg_paragraph_carrier_counts": dict(sorted(paragraph_refs.items())),
    }


def scribus_story_alignment_counts(path: Path) -> Counter:
    root = ET.parse(path).getroot()
    counts = Counter()
    for story in (node for node in root.iter() if local(node.tag) == "StoryText"):
        values = {
            child.attrib.get("ALIGN")
            for child in list(story)
            if local(child.tag) in {"DefaultStyle", "para", "trail"}
            and child.attrib.get("ALIGN") is not None
        }
        if "1" in values:
            counts["center"] += 1
        if "2" in values:
            counts["right"] += 1
    return counts


def verify_scribus(path: Path, items: list[dict]) -> dict:
    expected = expected_counts(items)
    observed = scribus_story_alignment_counts(path)
    for alignment, count in expected.items():
        if observed[alignment] != count:
            raise AssertionError(
                f"Scribus save/reopen {alignment} Story carrier count mismatch: "
                f"expected exactly {count}, observed {observed[alignment]}"
            )
    return {
        "expected_story_counts": dict(sorted(expected.items())),
        "scribus_storytext_alignment_counts": dict(sorted(observed.items())),
        "scribus_expected_alignment_counts_match_exactly": True,
        "accepted_sla_carriers": ["DefaultStyle", "para", "trail"],
    }


def libreoffice_paragraph_alignment_counts(path: Path) -> Counter:
    root = ET.parse(path).getroot()
    styles: dict[str, str] = {}
    for node in root.iter():
        if local(node.tag) != "style":
            continue
        name = node.attrib.get(f"{{{STYLE_NS}}}name")
        family = node.attrib.get(f"{{{STYLE_NS}}}family")
        if not name or family != "paragraph":
            continue
        props = next(
            (child for child in node.iter() if local(child.tag) == "paragraph-properties"),
            None,
        )
        if props is None:
            continue
        align = props.attrib.get(f"{{{FO_NS}}}text-align")
        if align:
            styles[name] = align.casefold()

    counts = Counter()
    for node in root.iter():
        if local(node.tag) != "p":
            continue
        direct = node.attrib.get(f"{{{FO_NS}}}text-align")
        if direct and direct.casefold() in {"center", "right"}:
            counts[direct.casefold()] += 1
            continue
        style_name = node.attrib.get(f"{{{TEXT_NS}}}style-name")
        align = styles.get(style_name or "")
        if align in {"center", "right"}:
            counts[align] += 1
    return counts


def odg_story_frame_names(root: ET.Element, story_id: str) -> set[str]:
    style_name = odg_style_name(story_id)
    names: set[str] = set()
    for frame in (node for node in root.iter() if local(node.tag) == "frame"):
        if any(
            local(node.tag) == "p"
            and node.attrib.get(f"{{{TEXT_NS}}}style-name") == style_name
            for node in frame.iter()
        ):
            name = frame.attrib.get(f"{{{DRAW_NS}}}name")
            if name:
                names.add(name)
    return names


def odg_root_from_package(root: Path) -> ET.Element:
    path = root / "output.odg"
    if not path.is_file():
        raise AssertionError("wire ODG output is required for consumer identity")
    with zipfile.ZipFile(path) as archive:
        return ET.fromstring(archive.read("content.xml"))


def paragraph_style_table(root: ET.Element) -> dict[str, tuple[str | None, str | None]]:
    styles: dict[str, tuple[str | None, str | None]] = {}
    for node in root.iter():
        if local(node.tag) != "style":
            continue
        name = node.attrib.get(f"{{{STYLE_NS}}}name")
        family = node.attrib.get(f"{{{STYLE_NS}}}family")
        if not name or family != "paragraph":
            continue
        parent = node.attrib.get(f"{{{STYLE_NS}}}parent-style-name")
        props = next(
            (child for child in node.iter() if local(child.tag) == "paragraph-properties"),
            None,
        )
        align = (
            props.attrib.get(f"{{{FO_NS}}}text-align").casefold()
            if props is not None and props.attrib.get(f"{{{FO_NS}}}text-align")
            else None
        )
        styles[name] = (align, parent)
    return styles


def resolve_paragraph_alignment(
    styles: dict[str, tuple[str | None, str | None]],
    style_name: str | None,
) -> str | None:
    seen: set[str] = set()
    current = style_name
    while current and current not in seen:
        seen.add(current)
        item = styles.get(current)
        if item is None:
            return None
        align, parent = item
        if align:
            return align
        current = parent
    return None


def verify_libreoffice(path: Path, items: list[dict], wire_root: Path) -> dict:
    source_root = odg_root_from_package(wire_root)
    root = ET.parse(path).getroot()
    styles = paragraph_style_table(root)
    frames = {
        node.attrib.get(f"{{{DRAW_NS}}}name"): node
        for node in root.iter()
        if local(node.tag) == "frame"
        and node.attrib.get(f"{{{DRAW_NS}}}name")
    }

    verified_stories = 0
    verified_frames = 0
    for item in items:
        expected_frames = odg_story_frame_names(source_root, item["story_id"])
        if not expected_frames:
            raise AssertionError(
                f"wire ODG Story {item['story_id']} has no named frame carrier"
            )
        for frame_name in sorted(expected_frames):
            frame = frames.get(frame_name)
            if frame is None:
                raise AssertionError(
                    f"LibreOffice fresh reopen lost Story frame {frame_name}"
                )
            paragraphs = [node for node in frame.iter() if local(node.tag) == "p"]
            if not paragraphs:
                raise AssertionError(
                    f"LibreOffice Story frame {frame_name} has no paragraph carriers"
                )
            observed: list[str | None] = []
            for paragraph in paragraphs:
                direct = paragraph.attrib.get(f"{{{FO_NS}}}text-align")
                if direct:
                    alignment = direct.casefold()
                else:
                    alignment = resolve_paragraph_alignment(
                        styles,
                        paragraph.attrib.get(f"{{{TEXT_NS}}}style-name"),
                    )
                observed.append(alignment)
            if any(value != item["alignment"] for value in observed):
                raise AssertionError(
                    f"LibreOffice Story frame {frame_name} alignment mismatch: "
                    f"expected={item['alignment']!r} observed={observed!r}"
                )
            verified_frames += 1
        verified_stories += 1

    observed = libreoffice_paragraph_alignment_counts(path)
    return {
        "expected_story_counts": dict(sorted(expected_counts(items).items())),
        "libreoffice_exact_story_frame_count": verified_frames,
        "libreoffice_exact_story_count": verified_stories,
        "libreoffice_exact_story_frames_match": verified_stories == len(items),
        "libreoffice_paragraph_alignment_counts": dict(sorted(observed.items())),
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("mode", choices=("wire", "scribus", "libreoffice"))
    parser.add_argument("--expected", required=True, type=Path)
    parser.add_argument("--root", type=Path)
    parser.add_argument("--input", type=Path)
    parser.add_argument("--receipt", type=Path)
    parser.add_argument("--wire-root", type=Path)
    args = parser.parse_args()

    items = load_expected(args.expected)
    if args.mode == "wire":
        if args.root is None:
            parser.error("wire mode requires --root")
        result = verify_wire(args.root, items)
    elif args.mode == "scribus":
        if args.input is None:
            parser.error("scribus mode requires --input")
        result = verify_scribus(args.input, items)
    else:
        if args.input is None:
            parser.error("libreoffice mode requires --input")
        if args.wire_root is None:
            parser.error("libreoffice mode requires --wire-root")
        result = verify_libreoffice(args.input, items, args.wire_root)

    payload = {
        "schema": "chaptera.editable-paragraph-alignment-consumer-check.v1",
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
