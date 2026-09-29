#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
from pathlib import Path
import subprocess
import sys

from check_rustfmt_delta import changed_head_ranges, ranges_intersect


def normalized_path(value: str) -> str:
    return Path(value).as_posix().lstrip("./")


def diagnostic_ranges(message: dict) -> list[tuple[str, int, int, str]]:
    rendered = message.get("rendered") or message.get("message") or "clippy warning"
    out: list[tuple[str, int, int, str]] = []
    for span in message.get("spans", []):
        if not span.get("is_primary"):
            continue
        path = normalized_path(str(span.get("file_name", "")))
        start = int(span.get("line_start") or 0)
        end = int(span.get("line_end") or start)
        if path and start > 0:
            out.append((path, start, max(start, end), rendered))
    return out


def parse_clippy_messages(lines: list[str]) -> list[tuple[str, int, int, str]]:
    diagnostics: list[tuple[str, int, int, str]] = []
    for raw in lines:
        raw = raw.strip()
        if not raw.startswith("{"):
            continue
        try:
            event = json.loads(raw)
        except json.JSONDecodeError:
            continue
        if event.get("reason") != "compiler-message":
            continue
        message = event.get("message") or {}
        if message.get("level") != "warning":
            continue
        code = (message.get("code") or {}).get("code") or ""
        if not code.startswith("clippy::") and "clippy" not in (message.get("rendered") or ""):
            continue
        diagnostics.extend(diagnostic_ranges(message))
    return diagnostics


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--base", required=True)
    parser.add_argument("--head", required=True)
    parser.add_argument("--path", action="append", required=True)
    args = parser.parse_args()

    changed: dict[str, list[tuple[int, int]]] = {
        normalized_path(path): changed_head_ranges(args.base, args.head, normalized_path(path))
        for path in args.path
    }

    command = [
        "cargo",
        "clippy",
        "-p",
        "chaptera-desktop",
        "--features",
        "reader-only",
        "--all-targets",
        "--message-format=json",
    ]
    proc = subprocess.run(command, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    if proc.stderr:
        sys.stderr.write(proc.stderr)
    if proc.returncode != 0:
        sys.stderr.write("desktop-clippy-delta: clippy failed with a compile/error diagnostic\n")
        return proc.returncode

    blocking: list[tuple[str, int, int, str]] = []
    inherited = 0
    for path, start, end, rendered in parse_clippy_messages(proc.stdout.splitlines()):
        ranges = changed.get(path)
        if ranges is None:
            inherited += 1
            continue
        if ranges_intersect(ranges, [(start, end)]):
            blocking.append((path, start, end, rendered))
        else:
            inherited += 1

    if blocking:
        for path, start, end, rendered in blocking:
            sys.stderr.write(f"desktop-clippy-delta: changed-line warning {path}:{start}-{end}\n")
            sys.stderr.write(rendered.rstrip() + "\n")
        return 1

    print(
        "desktop-clippy-delta: no clippy warnings intersect changed Desktop lines; "
        f"{inherited} inherited/non-delta warning(s) allowed"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
