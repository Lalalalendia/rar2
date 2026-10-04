#!/usr/bin/env python3
from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import sys

import scribus


def required(name: str) -> str:
    value = os.environ.get(name)
    if not value:
        print(f"{name} is required", file=sys.stderr)
        raise SystemExit(2)
    return value


def frame_text(frame: str) -> str:
    try:
        return scribus.getAllText(frame)
    except Exception:
        return scribus.getText(frame)


def main() -> int:
    output = Path(required("CHAPTERA_SCRIBUS_ALIGNMENT_OUT")).resolve()
    receipt_path = Path(required("CHAPTERA_SCRIBUS_ALIGNMENT_RECEIPT")).resolve()
    target = required("CHAPTERA_ALIGNMENT_TARGET_FRAME")
    marker = required("CHAPTERA_EDIT_MARKER")
    mode = os.environ.get("CHAPTERA_ALIGNMENT_EDIT_MODE", "edit")
    if mode not in {"edit", "verify"}:
        print(f"unsupported mode {mode!r}", file=sys.stderr)
        return 2
    if not scribus.haveDoc():
        print("scribus did not auto-open the input document", file=sys.stderr)
        return 3

    found = False
    for page in range(1, scribus.pageCount() + 1):
        scribus.gotoPage(page)
        if target in scribus.getAllObjects():
            found = True
            break
    if not found:
        print("target Scribus frame is absent", file=sys.stderr)
        return 4
    if scribus.getObjectType(target) != "TextFrame":
        print("target Scribus object is not a TextFrame", file=sys.stderr)
        return 5

    before = frame_text(target)
    if mode == "edit":
        if marker in before:
            print("edit marker already exists", file=sys.stderr)
            return 6
        length = int(scribus.getTextLength(target))
        scribus.insertText(marker, length, target)
        try:
            scribus.layoutTextChain(target)
        except Exception:
            try:
                scribus.layoutText(target)
            except Exception:
                pass
        after = frame_text(target)
        if marker not in after:
            print("scribus insertText was not observable", file=sys.stderr)
            return 7
    else:
        if marker not in before:
            print("fresh Scribus reopen lost edit marker", file=sys.stderr)
            return 8
        after = before

    output.parent.mkdir(parents=True, exist_ok=True)
    if output.exists():
        output.unlink()
    scribus.saveDocAs(str(output))
    scribus.closeDoc()
    if not output.is_file() or output.stat().st_size == 0:
        print("scribus produced no saved document", file=sys.stderr)
        return 9

    receipt = {
        "schema": "chaptera.scribus-paragraph-alignment-edit.v1",
        "mode": mode,
        "target_frame_name_sha256": hashlib.sha256(target.encode()).hexdigest(),
        "marker_sha256": hashlib.sha256(marker.encode()).hexdigest(),
        "marker_observed": True,
        "before_text_length": len(before),
        "after_text_length": len(after),
        "saved_bytes": output.stat().st_size,
        "real_scribus_insert_text_used": mode == "edit",
        "story_text_recorded": False,
    }
    receipt_path.parent.mkdir(parents=True, exist_ok=True)
    receipt_path.write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
