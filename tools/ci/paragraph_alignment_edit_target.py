#!/usr/bin/env python3
from __future__ import annotations

import argparse
import hashlib
import json
import zipfile
from pathlib import Path
from xml.etree import ElementTree as ET

STYLE_NS = "urn:oasis:names:tc:opendocument:xmlns:style:1.0"
TEXT_NS = "urn:oasis:names:tc:opendocument:xmlns:text:1.0"
FO_NS = "urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0"
DRAW_NS = "urn:oasis:names:tc:opendocument:xmlns:drawing:1.0"


def local(tag: str) -> str:
    return tag.rsplit("}", 1)[-1]


def digest(value: str) -> str:
    return hashlib.sha256(value.encode("utf-8")).hexdigest()


def scribus_story_center(story: ET.Element) -> bool:
    values = {
        child.attrib.get("ALIGN")
        for child in list(story)
        if local(child.tag) in {"DefaultStyle", "para", "trail"}
        and child.attrib.get("ALIGN") is not None
    }
    return "1" in values


def scribus_marker_present(story: ET.Element, marker: str) -> bool:
    chunks = [
        node.attrib.get("CH", "")
        for node in story.iter()
        if local(node.tag) == "ITEXT"
    ]
    return marker in "".join(chunks)


def scribus_mode(
    path: Path,
    receipt_path: Path,
    target_frame: str | None,
    marker: str | None,
) -> str:
    root = ET.parse(path).getroot()
    candidates: list[tuple[str, ET.Element]] = []
    all_named_story_frames = 0
    for owner in root.iter():
        story = next(
            (child for child in list(owner) if local(child.tag) == "StoryText"),
            None,
        )
        if story is None:
            continue
        name = owner.attrib.get("ANNAME")
        if not name:
            continue
        all_named_story_frames += 1
        if scribus_story_center(story):
            candidates.append((name, story))

    if target_frame is None:
        if len(candidates) != 1:
            raise AssertionError(
                f"expected exactly one centered Scribus StoryText frame; observed={len(candidates)}"
            )
        name, story = candidates[0]
    else:
        matches = [(name, story) for name, story in candidates if name == target_frame]
        if len(matches) != 1:
            raise AssertionError(
                f"target Scribus frame is not the unique centered carrier: {target_frame!r}"
            )
        name, story = matches[0]
        if marker is not None and not scribus_marker_present(story, marker):
            raise AssertionError("target Scribus centered frame lost edit marker")

    receipt = {
        "schema": "chaptera.paragraph-alignment-edit-target.v1",
        "consumer": "scribus",
        "center_storytext_count": len(candidates),
        "named_story_frame_count": all_named_story_frames,
        "target_frame_name_sha256": digest(name),
        "target_center_alignment_observed": True,
        "marker_observed": marker is not None,
        "marker_sha256": digest(marker) if marker is not None else None,
        "story_text_recorded": False,
    }
    receipt_path.parent.mkdir(parents=True, exist_ok=True)
    receipt_path.write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n")
    return name


def odg_mode(
    path: Path,
    story_id: str,
    receipt_path: Path,
) -> str:
    with zipfile.ZipFile(path) as archive:
        root = ET.fromstring(archive.read("content.xml"))

    style_name = "PubStoryP_" + story_id.replace("-", "").lower()
    style = next(
        (
            node
            for node in root.iter()
            if local(node.tag) == "style"
            and node.attrib.get(f"{{{STYLE_NS}}}family") == "paragraph"
            and node.attrib.get(f"{{{STYLE_NS}}}name") == style_name
        ),
        None,
    )
    if style is None:
        raise AssertionError(f"ODG missing target Story paragraph style {style_name}")
    props = next(
        (node for node in style.iter() if local(node.tag) == "paragraph-properties"),
        None,
    )
    actual = (
        props.attrib.get(f"{{{FO_NS}}}text-align").casefold()
        if props is not None and props.attrib.get(f"{{{FO_NS}}}text-align")
        else None
    )
    if actual != "center":
        raise AssertionError(
            f"ODG target Story style is not centered: expected='center' actual={actual!r}"
        )

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
    if len(names) != 1:
        raise AssertionError(
            f"expected one target ODG frame for witness Story; observed={sorted(names)!r}"
        )
    name = next(iter(names))
    receipt = {
        "schema": "chaptera.paragraph-alignment-edit-target.v1",
        "consumer": "libreoffice",
        "story_id": story_id,
        "source_style_name_sha256": digest(style_name),
        "target_frame_name_sha256": digest(name),
        "target_center_alignment_observed": True,
        "story_text_recorded": False,
    }
    receipt_path.parent.mkdir(parents=True, exist_ok=True)
    receipt_path.write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n")
    return name


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("mode", choices=("scribus", "odg"))
    parser.add_argument("--input", type=Path, required=True)
    parser.add_argument("--receipt", type=Path, required=True)
    parser.add_argument("--story-id")
    parser.add_argument("--target-frame")
    parser.add_argument("--marker")
    args = parser.parse_args()

    if args.mode == "scribus":
        name = scribus_mode(
            args.input,
            args.receipt,
            args.target_frame,
            args.marker,
        )
    else:
        if not args.story_id:
            parser.error("odg mode requires --story-id")
        name = odg_mode(args.input, args.story_id, args.receipt)

    print(name)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
