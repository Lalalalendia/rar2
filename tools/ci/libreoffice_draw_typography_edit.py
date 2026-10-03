#!/usr/bin/env python3
"""Real LibreOffice Draw text edit/reopen acceptance helper.

Connects to a separately started headless LibreOffice instance over UNO,
mutates a text-bearing Draw shape through UNO, saves as ODG, and emits only a
source-safe receipt.
"""

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
    target = (
        f"uno:socket,host={host},port={port};urp;StarOffice.ComponentContext"
    )
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
        page = pages.getByIndex(page_index)
        yield from iter_shapes(page)


def shape_text(shape) -> str | None:
    try:
        value = shape.String
    except Exception:
        return None
    return value if isinstance(value, str) else None


def marker_present(document, marker: str) -> bool:
    return any(
        marker in text
        for shape in all_shapes(document)
        if (text := shape_text(shape)) is not None
    )


def load_document(ctx, input_path: Path):
    desktop = ctx.ServiceManager.createInstanceWithContext(
        "com.sun.star.frame.Desktop", ctx
    )
    url = uno.systemPathToFileUrl(str(input_path.resolve()))
    doc = desktop.loadComponentFromURL(
        url,
        "_blank",
        0,
        (property_value("Hidden", True),),
    )
    if doc is None:
        raise RuntimeError(f"LibreOffice failed to load {input_path}")
    return desktop, doc


def save_odg(document, output_path: Path) -> None:
    output_path.parent.mkdir(parents=True, exist_ok=True)
    if output_path.exists():
        output_path.unlink()
    url = uno.systemPathToFileUrl(str(output_path.resolve()))
    document.storeAsURL(
        url,
        (
            property_value("FilterName", "draw8"),
            property_value("Overwrite", True),
        ),
    )
    if not output_path.is_file() or output_path.stat().st_size == 0:
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
    parser.add_argument("--marker", required=True)
    parser.add_argument("--receipt", type=Path, required=True)
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", type=int, required=True)
    parser.add_argument("--connect-timeout", type=float, default=20.0)
    args = parser.parse_args()

    ctx = connect(args.host, args.port, args.connect_timeout)
    _desktop, doc = load_document(ctx, args.input)

    edited_shape_index = None
    before_length = None
    after_length = None

    try:
        shapes = list(all_shapes(doc))
        if not shapes:
            raise RuntimeError("LibreOffice Draw document contains no shapes")

        if args.mode == "edit":
            if marker_present(doc, args.marker):
                raise RuntimeError("edit marker already exists before requested edit")

            for index, shape in enumerate(shapes):
                text = shape_text(shape)
                if not text:
                    continue
                before_length = len(text)
                try:
                    cursor = shape.createTextCursor()
                    cursor.gotoEnd(False)
                    shape.insertString(cursor, args.marker, False)
                except Exception as exc:
                    raise RuntimeError(
                        "text-bearing Draw shape does not support bounded XText insertion"
                    ) from exc
                after = shape_text(shape)
                if after is None or args.marker not in after:
                    raise RuntimeError("UNO XText insertion was not observable")
                after_length = len(after)
                edited_shape_index = index
                break

            if edited_shape_index is None:
                raise RuntimeError("no non-empty UNO text-bearing shape was editable")
        else:
            if not marker_present(doc, args.marker):
                raise RuntimeError("fresh LibreOffice reopen did not retain edit marker")

        save_odg(doc, args.output)
    finally:
        close_document(doc)

    receipt = {
        "schema": "chaptera.libreoffice-draw-text-edit-consumer.v1",
        "mode": args.mode,
        "marker_sha256": hashlib.sha256(args.marker.encode("utf-8")).hexdigest(),
        "marker_observed": True,
        "edited_shape_index": edited_shape_index,
        "before_text_length": before_length,
        "after_text_length": after_length,
        "saved_bytes": args.output.stat().st_size,
    }
    args.receipt.parent.mkdir(parents=True, exist_ok=True)
    args.receipt.write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n")
    print(json.dumps(receipt, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
