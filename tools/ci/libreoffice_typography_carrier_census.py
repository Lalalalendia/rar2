#!/usr/bin/env python3
"""Source-safe LibreOffice Draw carrier census through the real UNO API."""

from __future__ import annotations

import argparse
import collections
import json
import re
import time
from pathlib import Path

import uno


EMU_PER_POINT = 12_700


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


def normalize_family(value: str) -> str:
    value = re.sub(r"\s+", " ", value.strip().strip("'\"")).casefold()
    if value.endswith(" regular"):
        value = value[: -len(" regular")]
    return value


def expected_counts(path: Path) -> collections.Counter[tuple[str, int]]:
    data = json.loads(path.read_text())
    return collections.Counter(
        (normalize_family(item["font_family"]), int(item["font_size_emu"]))
        for item in data.get("items", [])
    )


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--input", type=Path, required=True)
    parser.add_argument("--expected", type=Path, required=True)
    parser.add_argument("--receipt", type=Path, required=True)
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", type=int, required=True)
    parser.add_argument("--connect-timeout", type=float, default=20.0)
    args = parser.parse_args()

    expected = expected_counts(args.expected)
    ctx = connect(args.host, args.port, args.connect_timeout)
    desktop = ctx.ServiceManager.createInstanceWithContext(
        "com.sun.star.frame.Desktop", ctx
    )
    doc = desktop.loadComponentFromURL(
        uno.systemPathToFileUrl(str(args.input.resolve())),
        "_blank",
        0,
        (property_value("Hidden", True),),
    )
    if doc is None:
        raise RuntimeError(f"LibreOffice failed to load {args.input}")

    observed: collections.Counter[tuple[str, int]] = collections.Counter()
    try:
        for shape in all_shapes(doc):
            try:
                text = shape.String
            except Exception:
                continue
            if not isinstance(text, str) or not text:
                continue
            try:
                cursor = shape.createTextCursor()
                cursor.gotoStart(False)
                cursor.gotoEnd(True)
                family = str(cursor.CharFontName)
                size_pt = float(cursor.CharHeight)
            except Exception:
                continue
            if not family or size_pt <= 0:
                continue
            key = (normalize_family(family), int(round(size_pt * EMU_PER_POINT)))
            observed[key] += 1
    finally:
        try:
            doc.close(True)
        except Exception:
            doc.dispose()

    relevant = collections.Counter({key: observed.get(key, 0) for key in expected})
    match = relevant == expected
    receipt = {
        "schema": "chaptera.libreoffice-typography-carrier-census.v1",
        "expected_carrier_count": sum(expected.values()),
        "observed_carrier_count": sum(relevant.values()),
        "expected_family_size_counts": {
            f"{family}|{size}": count for (family, size), count in sorted(expected.items())
        },
        "observed_family_size_counts": {
            f"{family}|{size}": count for (family, size), count in sorted(relevant.items())
        },
        "family_size_counts_match": match,
        "story_text_recorded": False,
    }
    args.receipt.parent.mkdir(parents=True, exist_ok=True)
    args.receipt.write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n")
    print(json.dumps(receipt, indent=2, sort_keys=True))
    if not match:
        raise SystemExit(1)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
