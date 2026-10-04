#!/usr/bin/env python3
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import time

import uno


def property_value(name: str, value):
    prop = uno.createUnoStruct("com.sun.star.beans.PropertyValue")
    prop.Name = name
    prop.Value = value
    return prop


def connect(host: str, port: int, timeout_seconds: float):
    local_ctx = uno.getComponentContext()
    resolver = local_ctx.ServiceManager.createInstanceWithContext(
        "com.sun.star.bridge.UnoUrlResolver", local_ctx
    )
    target = f"uno:socket,host={host},port={port};urp;StarOffice.ComponentContext"
    deadline = time.monotonic() + timeout_seconds
    last_error = None
    while time.monotonic() < deadline:
        try:
            return resolver.resolve(target)
        except Exception as exc:
            last_error = exc
            time.sleep(0.25)
    raise RuntimeError(f"could not connect to LibreOffice UNO: {last_error}")


def iter_shapes(container):
    try:
        count = int(container.getCount())
    except Exception:
        return
    for index in range(count):
        shape = container.getByIndex(index)
        yield shape
        try:
            child_count = int(shape.getCount())
        except Exception:
            child_count = 0
        if child_count:
            yield from iter_shapes(shape)


def all_shapes(document):
    pages = document.getDrawPages()
    for page_index in range(pages.getCount()):
        yield from iter_shapes(pages.getByIndex(page_index))


def shape_name(shape) -> str:
    try:
        value = shape.Name
        if isinstance(value, str):
            return value
    except Exception:
        pass
    try:
        value = shape.getName()
        if isinstance(value, str):
            return value
    except Exception:
        pass
    return ""


def shape_text(shape) -> str | None:
    try:
        value = shape.String
    except Exception:
        return None
    return value if isinstance(value, str) else None


def is_center(shape) -> bool:
    try:
        cursor = shape.createTextCursor()
        cursor.gotoStart(False)
        cursor.gotoEnd(True)
        actual = cursor.ParaAdjust
        wanted = uno.Enum("com.sun.star.style.ParagraphAdjust", "CENTER")
        return actual == wanted
    except Exception:
        return False


def load_document(ctx, path: Path):
    desktop = ctx.ServiceManager.createInstanceWithContext(
        "com.sun.star.frame.Desktop", ctx
    )
    doc = desktop.loadComponentFromURL(
        uno.systemPathToFileUrl(str(path.resolve())),
        "_blank",
        0,
        (property_value("Hidden", True),),
    )
    if doc is None:
        raise RuntimeError(f"LibreOffice failed to load {path}")
    return doc


def save_odg(document, output: Path) -> None:
    output.parent.mkdir(parents=True, exist_ok=True)
    if output.exists():
        output.unlink()
    document.storeAsURL(
        uno.systemPathToFileUrl(str(output.resolve())),
        (
            property_value("FilterName", "draw8"),
            property_value("Overwrite", True),
        ),
    )
    if not output.is_file() or output.stat().st_size == 0:
        raise RuntimeError("LibreOffice produced no ODG output")


def close_document(document) -> None:
    try:
        document.close(True)
    except Exception:
        document.dispose()


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("mode", choices=("edit", "verify"))
    parser.add_argument("--input", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--target-frame", required=True)
    parser.add_argument("--marker", required=True)
    parser.add_argument("--receipt", type=Path, required=True)
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", type=int, required=True)
    parser.add_argument("--connect-timeout", type=float, default=20.0)
    args = parser.parse_args()

    ctx = connect(args.host, args.port, args.connect_timeout)
    doc = load_document(ctx, args.input)
    before = None
    after = None
    try:
        matches = [shape for shape in all_shapes(doc) if shape_name(shape) == args.target_frame]
        if len(matches) != 1:
            raise RuntimeError(
                f"expected one exact LibreOffice target frame; observed={len(matches)}"
            )
        shape = matches[0]
        before = shape_text(shape)
        if before is None:
            raise RuntimeError("target LibreOffice frame is not text-bearing")
        if not is_center(shape):
            raise RuntimeError("target LibreOffice frame is not effectively centered")

        if args.mode == "edit":
            if args.marker in before:
                raise RuntimeError("edit marker already exists")
            cursor = shape.createTextCursor()
            cursor.gotoEnd(False)
            shape.insertString(cursor, args.marker, False)
            after = shape_text(shape)
            if after is None or args.marker not in after:
                raise RuntimeError("UNO insertString was not observable")
            if not is_center(shape):
                raise RuntimeError("UNO edit changed target Center alignment")
        else:
            if args.marker not in before:
                raise RuntimeError("fresh LibreOffice reopen lost edit marker")
            after = before
            if not is_center(shape):
                raise RuntimeError("fresh LibreOffice reopen lost Center alignment")

        save_odg(doc, args.output)
    finally:
        close_document(doc)

    receipt = {
        "schema": "chaptera.libreoffice-paragraph-alignment-edit.v1",
        "mode": args.mode,
        "target_frame_name_sha256": hashlib.sha256(
            args.target_frame.encode()
        ).hexdigest(),
        "marker_sha256": hashlib.sha256(args.marker.encode()).hexdigest(),
        "marker_observed": True,
        "center_alignment_observed": True,
        "before_text_length": len(before) if before is not None else None,
        "after_text_length": len(after) if after is not None else None,
        "saved_bytes": args.output.stat().st_size,
        "real_libreoffice_uno_insert_string_used": args.mode == "edit",
        "story_text_recorded": False,
    }
    args.receipt.parent.mkdir(parents=True, exist_ok=True)
    args.receipt.write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n")
    print(json.dumps(receipt, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
