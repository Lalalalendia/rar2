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

IDML_ALIGNMENT = {
    "left": "LeftAlign",
    "center": "CenterAlign",
    "right": "RightAlign",
}
SCRIBUS_ALIGNMENT = {
    "0": "left",
    "1": "center",
    "2": "right",
}


def local(tag: str) -> str:
    return tag.rsplit("}", 1)[-1]


def compact_id(value: str) -> str:
    return value.replace("-", "").lower()


def non_whitespace_sha256(text: str) -> str:
    normalized = "".join(character for character in text if not character.isspace())
    return hashlib.sha256(normalized.encode("utf-8")).hexdigest()


def load_expected(path: Path, state: str) -> tuple[dict, list[str]]:
    data = json.loads(path.read_text())
    if data.get("schema") != "chaptera.paragraph-scoped-alignment-consumer-fixture.v1":
        raise AssertionError(f"unexpected expected schema: {data.get('schema')!r}")
    state_data = data.get("states", {}).get(state)
    if not state_data:
        raise AssertionError(f"missing expected state {state!r}")
    sequence = [str(value).casefold() for value in state_data["alignment_sequence"]]
    if len(sequence) != int(data["paragraph_count"]):
        raise AssertionError("expected paragraph sequence length mismatch")
    unknown = set(sequence) - {"left", "center", "right"}
    if unknown:
        raise AssertionError(f"unsupported expected alignments: {sorted(unknown)!r}")
    return data, sequence


def idml_story_path(story_id: str) -> str:
    return f"Stories/Story_us{compact_id(story_id)}.xml"


def paragraph_style_table(root: ET.Element) -> dict[str, tuple[str | None, str | None]]:
    result: dict[str, tuple[str | None, str | None]] = {}
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
        align = None
        if props is not None:
            value = props.attrib.get(f"{{{FO_NS}}}text-align")
            align = value.casefold() if value else None
        result[name] = (align, parent)
    return result


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


def paragraph_alignment(node: ET.Element, styles: dict[str, tuple[str | None, str | None]]) -> str | None:
    direct = node.attrib.get(f"{{{FO_NS}}}text-align")
    if direct:
        return direct.casefold()
    return resolve_paragraph_alignment(styles, node.attrib.get(f"{{{TEXT_NS}}}style-name"))


def frame_paragraphs(frame: ET.Element) -> list[ET.Element]:
    return [node for node in frame.iter() if local(node.tag) == "p"]


def find_wire_story_frame(root: ET.Element, expected: dict, state: str) -> ET.Element:
    state_data = expected["states"][state]
    if state_data["wire_contract"] == "scoped":
        names = {
            f"PubParagraphP_{compact_id(item['paragraph_id'])}"
            for item in expected["paragraphs"]
        }
    else:
        names = {f"PubStoryP_{compact_id(expected['story_id'])}"}

    candidates = []
    for frame in (node for node in root.iter() if local(node.tag) == "frame"):
        style_names = {
            paragraph.attrib.get(f"{{{TEXT_NS}}}style-name")
            for paragraph in frame_paragraphs(frame)
        }
        if style_names & names:
            candidates.append(frame)
    if len(candidates) != 1:
        raise AssertionError(
            f"wire ODG expected exactly one target Story frame; observed={len(candidates)}"
        )
    return candidates[0]


def assert_odg_sequence(
    root: ET.Element,
    expected: dict,
    state: str,
    wanted: list[str],
) -> dict:
    styles = paragraph_style_table(root)
    frame = find_wire_story_frame(root, expected, state)
    paragraphs = frame_paragraphs(frame)
    if len(paragraphs) < len(wanted):
        raise AssertionError(
            f"ODG has fewer paragraph carriers than canonical paragraphs: "
            f"found={len(paragraphs)} expected={len(wanted)}"
        )
    observed = [paragraph_alignment(node, styles) for node in paragraphs[: len(wanted)]]
    if observed != wanted:
        raise AssertionError(
            "ODG ordered paragraph alignment mismatch: "
            + json.dumps({"expected": wanted, "observed": observed}, sort_keys=True)
        )
    extras = paragraphs[len(wanted) :]
    extra_nonempty = [
        "".join(node.itertext())
        for node in extras
        if any(not ch.isspace() for ch in "".join(node.itertext()))
    ]
    if extra_nonempty:
        raise AssertionError("ODG contains extra non-empty paragraph carriers")
    frame_name = frame.attrib.get(f"{{{DRAW_NS}}}name")
    if not frame_name:
        raise AssertionError("target ODG Story frame has no draw:name")
    return {
        "frame_name": frame_name,
        "ordered_alignment_sequence": observed,
        "paragraph_carrier_count": len(paragraphs),
        "extra_empty_carrier_count": len(extras),
    }


def verify_wire(root: Path, expected: dict, state: str, wanted: list[str]) -> dict:
    idml = root / "output.idml"
    odg = root / "output.odg"
    if not idml.is_file() or not odg.is_file():
        raise AssertionError("wire IDML and ODG are required")

    with zipfile.ZipFile(idml) as archive:
        story = ET.fromstring(archive.read(idml_story_path(expected["story_id"])))
    ranges = [node for node in story.iter() if local(node.tag) == "ParagraphStyleRange"]
    contract = expected["states"][state]["wire_contract"]
    if contract == "scoped":
        observed_idml = [node.attrib.get("Justification") for node in ranges]
        wanted_idml = [IDML_ALIGNMENT[value] for value in wanted]
        if observed_idml != wanted_idml:
            raise AssertionError(
                "IDML ordered scoped alignment mismatch: "
                + json.dumps(
                    {"expected": wanted_idml, "observed": observed_idml},
                    sort_keys=True,
                )
            )
    else:
        if len(ranges) != 1 or ranges[0].attrib.get("Justification") != "RightAlign":
            raise AssertionError(
                "Clear must return to exactly one full-Story RightAlign carrier"
            )
        observed_idml = ["RightAlign"]

    with zipfile.ZipFile(odg) as archive:
        odg_root = ET.fromstring(archive.read("content.xml"))
    odg_result = assert_odg_sequence(odg_root, expected, state, wanted)
    return {
        "idml_wire_contract": contract,
        "idml_paragraph_style_range_count": len(ranges),
        "idml_alignment_sequence": observed_idml,
        "odg": odg_result,
    }


def scribus_story_candidates(root: ET.Element, story_hash: str) -> list[ET.Element]:
    matches = []
    for story in (node for node in root.iter() if local(node.tag) == "StoryText"):
        text = "".join(
            node.attrib.get("CH", "")
            for node in story.iter()
            if local(node.tag) == "ITEXT"
        )
        if non_whitespace_sha256(text) == story_hash:
            matches.append(story)
    return matches


def scribus_sequence(story: ET.Element, paragraph_count: int) -> dict:
    default = next(
        (child for child in list(story) if local(child.tag) == "DefaultStyle"),
        None,
    )
    default_align = default.attrib.get("ALIGN") if default is not None else None
    markers = [
        child
        for child in list(story)
        if local(child.tag) in {"para", "trail"}
    ]
    if len(markers) != paragraph_count:
        return {
            "error": "paragraph_carrier_count_mismatch",
            "paragraph_carrier_count": len(markers),
            "expected_paragraph_count": paragraph_count,
        }

    observed = []
    raw = []
    for marker in markers:
        value = marker.attrib.get("ALIGN", default_align)
        if value is None:
            value = "0"
        raw.append(value)
        alignment = SCRIBUS_ALIGNMENT.get(value)
        if alignment is None:
            return {
                "error": "unsupported_align_value",
                "raw_align_sequence": raw,
                "unsupported_value": value,
            }
        observed.append(alignment)

    return {
        "ordered_alignment_sequence": observed,
        "raw_align_sequence": raw,
        "paragraph_carrier_count": len(markers),
    }


def verify_scribus_matrix(
    center_path: Path,
    clear_path: Path,
    mixed_path: Path,
    expected: dict,
) -> dict:
    paths = {
        "center": center_path,
        "clear": clear_path,
        "mixed": mixed_path,
    }
    wanted = {
        state: [str(value).casefold() for value in expected["states"][state]["alignment_sequence"]]
        for state in paths
    }
    paragraph_count = int(expected["paragraph_count"])
    candidates = {}
    summaries = {}

    for state, path in paths.items():
        root = ET.parse(path).getroot()
        state_candidates = scribus_story_candidates(
            root, expected["story_non_whitespace_sha256"]
        )
        candidates[state] = state_candidates
        summaries[state] = [
            scribus_sequence(story, paragraph_count)
            for story in state_candidates
        ]

    counts = {state: len(items) for state, items in candidates.items()}
    if not counts["center"] or len(set(counts.values())) != 1:
        raise AssertionError(
            "Scribus StoryText fingerprint candidate count is not stable across states: "
            + json.dumps({"counts": counts, "summaries": summaries}, sort_keys=True)
        )

    matching_indices = []
    for index in range(counts["center"]):
        if all(
            summaries[state][index].get("ordered_alignment_sequence") == wanted[state]
            for state in paths
        ):
            matching_indices.append(index)

    if len(matching_indices) != 1:
        raise AssertionError(
            "Scribus could not identify exactly one stable StoryText slot across Center/Clear/Mixed: "
            + json.dumps(
                {
                    "candidate_counts": counts,
                    "expected": wanted,
                    "matching_indices": matching_indices,
                    "candidate_summaries": summaries,
                },
                sort_keys=True,
            )
        )

    selected = matching_indices[0]
    return {
        "story_fingerprint_candidate_count": counts["center"],
        "selected_storytext_index": selected,
        "states": {
            state: summaries[state][selected]
            for state in paths
        },
    }


def verify_libreoffice(
    path: Path,
    wire_root: Path,
    expected: dict,
    state: str,
    wanted: list[str],
) -> dict:
    with zipfile.ZipFile(wire_root / "output.odg") as archive:
        wire = ET.fromstring(archive.read("content.xml"))
    wire_result = assert_odg_sequence(wire, expected, state, wanted)
    frame_name = wire_result["frame_name"]

    root = ET.parse(path).getroot()
    frames = [
        node
        for node in root.iter()
        if local(node.tag) == "frame"
        and node.attrib.get(f"{{{DRAW_NS}}}name") == frame_name
    ]
    if len(frames) != 1:
        raise AssertionError(
            f"LibreOffice lost exact target frame {frame_name!r}; observed={len(frames)}"
        )
    styles = paragraph_style_table(root)
    paragraphs = frame_paragraphs(frames[0])
    if len(paragraphs) < len(wanted):
        raise AssertionError(
            f"LibreOffice has fewer paragraphs than expected: "
            f"found={len(paragraphs)} expected={len(wanted)}"
        )
    observed = [paragraph_alignment(node, styles) for node in paragraphs[: len(wanted)]]
    if observed != wanted:
        raise AssertionError(
            "LibreOffice ordered paragraph alignment mismatch: "
            + json.dumps({"expected": wanted, "observed": observed}, sort_keys=True)
        )
    extras = paragraphs[len(wanted) :]
    if any(
        any(not ch.isspace() for ch in "".join(node.itertext()))
        for node in extras
    ):
        raise AssertionError("LibreOffice produced an extra non-empty paragraph")
    return {
        "frame_name": frame_name,
        "ordered_alignment_sequence": observed,
        "paragraph_carrier_count": len(paragraphs),
        "extra_empty_carrier_count": len(extras),
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("mode", choices=("wire", "scribus-matrix", "libreoffice"))
    parser.add_argument("--expected", type=Path, required=True)
    parser.add_argument("--state", choices=("center", "clear", "mixed"))
    parser.add_argument("--root", type=Path)
    parser.add_argument("--input", type=Path)
    parser.add_argument("--wire-root", type=Path)
    parser.add_argument("--center-input", type=Path)
    parser.add_argument("--clear-input", type=Path)
    parser.add_argument("--mixed-input", type=Path)
    parser.add_argument("--receipt", type=Path, required=True)
    args = parser.parse_args()

    if args.mode == "scribus-matrix":
        if args.center_input is None or args.clear_input is None or args.mixed_input is None:
            parser.error(
                "scribus-matrix mode requires --center-input --clear-input --mixed-input"
            )
        expected = json.loads(args.expected.read_text())
        if expected.get("schema") != "chaptera.paragraph-scoped-alignment-consumer-fixture.v1":
            raise AssertionError(f"unexpected expected schema: {expected.get('schema')!r}")
        result = verify_scribus_matrix(
            args.center_input,
            args.clear_input,
            args.mixed_input,
            expected,
        )
        payload = {
            "schema": "chaptera.paragraph-scoped-alignment-consumer-check.v1",
            "mode": args.mode,
            "state": "matrix",
            "story_id": expected["story_id"],
            "paragraph_count": expected["paragraph_count"],
            **result,
        }
    else:
        if args.state is None:
            parser.error(f"{args.mode} mode requires --state")
        expected, wanted = load_expected(args.expected, args.state)
        if args.mode == "wire":
            if args.root is None:
                parser.error("wire mode requires --root")
            result = verify_wire(args.root, expected, args.state, wanted)
        else:
            if args.input is None or args.wire_root is None:
                parser.error("libreoffice mode requires --input and --wire-root")
            result = verify_libreoffice(
                args.input,
                args.wire_root,
                expected,
                args.state,
                wanted,
            )

        payload = {
            "schema": "chaptera.paragraph-scoped-alignment-consumer-check.v1",
            "mode": args.mode,
            "state": args.state,
            "story_id": expected["story_id"],
            "paragraph_count": expected["paragraph_count"],
            "expected_alignment_sequence": wanted,
            **result,
        }

    args.receipt.parent.mkdir(parents=True, exist_ok=True)
    args.receipt.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n")
    print(json.dumps(payload, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
