#!/usr/bin/env python3
"""Real Scribus text edit acceptance helper.

Scribus opens the input document before this script is invoked. The script
mutates document text only through the Scribus scripting API, saves through
Scribus, and emits a source-safe receipt without copying source text.
"""

from __future__ import annotations

import hashlib
import json
import math
import os
from pathlib import Path
import re
import sys

import scribus


def required_env(name: str) -> str:
    value = os.environ.get(name)
    if not value:
        print(f"{name} is required", file=sys.stderr)
        raise SystemExit(2)
    return value


def all_text_frames() -> list[str]:
    result: list[str] = []
    for page in range(1, scribus.pageCount() + 1):
        scribus.gotoPage(page)
        for item in scribus.getAllObjects():
            if scribus.getObjectType(item) == "TextFrame":
                result.append(item)
    return result


def frame_text(frame: str) -> str:
    try:
        return scribus.getAllText(frame)
    except Exception:
        return scribus.getText(frame)


def frames_with_marker(frames: list[str], marker: str) -> list[str]:
    return [frame for frame in frames if marker in frame_text(frame)]


def normalized_family(value: str) -> str:
    value = re.sub(r"\s+", " ", value.strip().strip("'\"")).casefold()
    if value.endswith(" regular"):
        value = value[: -len(" regular")]
    return value


def frame_matches_typography(frame: str, family: str, size_pt: float) -> bool:
    length = int(scribus.getTextLength(frame))
    if length <= 0:
        return False
    try:
        scribus.selectText(0, 1, frame)
        actual_family = scribus.getFont(frame)
        actual_size = float(scribus.getFontSize(frame))
    finally:
        try:
            scribus.selectText(0, 0, frame)
        except Exception:
            pass
    return normalized_family(actual_family) == normalized_family(family) and math.isclose(
        actual_size, size_pt, rel_tol=0.0, abs_tol=0.02
    )


def main() -> int:
    output = Path(required_env("CHAPTERA_SCRIBUS_EDIT_OUT")).resolve()
    receipt_path = Path(required_env("CHAPTERA_SCRIBUS_EDIT_RECEIPT")).resolve()
    marker = required_env("CHAPTERA_EDIT_MARKER")
    expected_family = required_env("CHAPTERA_EXPECTED_FONT_FAMILY")
    expected_size_pt = float(required_env("CHAPTERA_EXPECTED_FONT_SIZE_PT"))
    mode = os.environ.get("CHAPTERA_SCRIBUS_EDIT_MODE", "edit")

    if mode not in {"edit", "verify"}:
        print(f"unsupported CHAPTERA_SCRIBUS_EDIT_MODE={mode!r}", file=sys.stderr)
        return 2
    if not scribus.haveDoc():
        print("scribus did not auto-open the input document", file=sys.stderr)
        return 3

    frames = all_text_frames()
    if not frames:
        print("scribus document contains no TextFrame objects", file=sys.stderr)
        return 4

    edited_frame = None
    before_length = None
    after_length = None

    if mode == "edit":
        if frames_with_marker(frames, marker):
            print("edit marker already exists before requested edit", file=sys.stderr)
            return 5

        for frame in frames:
            length = scribus.getTextLength(frame)
            if length <= 0 or not frame_matches_typography(
                frame, expected_family, expected_size_pt
            ):
                continue
            before_length = int(length)
            # Insert through Scribus itself into the consumer-resolved target
            # typography Story. Appending is deliberate; setText would replace
            # all text and can reset style.
            scribus.insertText(marker, length, frame)
            try:
                scribus.layoutTextChain(frame)
            except Exception:
                try:
                    scribus.layoutText(frame)
                except Exception:
                    pass
            after_length = int(scribus.getTextLength(frame))
            edited_frame = frame
            break

        if edited_frame is None:
            print(
                "scribus document has no editable TextFrame matching expected typography",
                file=sys.stderr,
            )
            return 6
        marker_frames = frames_with_marker(all_text_frames(), marker)
        if not marker_frames:
            print("scribus edit marker is not observable after insertText", file=sys.stderr)
            return 7
        if not any(
            frame_matches_typography(frame, expected_family, expected_size_pt)
            for frame in marker_frames
        ):
            print(
                "scribus edited frame no longer matches expected typography after edit",
                file=sys.stderr,
            )
            return 8
    else:
        marker_frames = frames_with_marker(frames, marker)
        if not marker_frames:
            print("scribus fresh reopen did not retain edit marker", file=sys.stderr)
            return 9
        if not any(
            frame_matches_typography(frame, expected_family, expected_size_pt)
            for frame in marker_frames
        ):
            print(
                "scribus fresh reopen retained marker but lost expected typography",
                file=sys.stderr,
            )
            return 10

    output.parent.mkdir(parents=True, exist_ok=True)
    if output.exists():
        output.unlink()
    scribus.saveDocAs(str(output))
    scribus.closeDoc()
    if not output.is_file() or output.stat().st_size == 0:
        print("scribus produced no saved document", file=sys.stderr)
        return 11

    receipt = {
        "schema": "chaptera.scribus-text-edit-consumer.v1",
        "mode": mode,
        "marker_sha256": hashlib.sha256(marker.encode("utf-8")).hexdigest(),
        "marker_observed": True,
        "expected_font_family": expected_family,
        "expected_font_size_pt": expected_size_pt,
        "edited_frame_name_sha256": (
            hashlib.sha256(edited_frame.encode("utf-8")).hexdigest()
            if edited_frame is not None
            else None
        ),
        "before_text_length": before_length,
        "after_text_length": after_length,
        "saved_bytes": output.stat().st_size,
    }
    receipt_path.parent.mkdir(parents=True, exist_ok=True)
    receipt_path.write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
