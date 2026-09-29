#!/usr/bin/env python3
from __future__ import annotations

import argparse
import difflib
from pathlib import Path
import re
import subprocess
import sys
import tempfile

HUNK_RE = re.compile(r"^@@ -\d+(?:,\d+)? \+(\d+)(?:,(\d+))? @@")


def git_text(ref: str, path: str) -> str | None:
    result = subprocess.run(
        ["git", "show", f"{ref}:{path}"],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    if result.returncode == 0:
        return result.stdout
    return None


def changed_head_ranges(base: str, head: str, path: str) -> list[tuple[int, int]]:
    diff = subprocess.check_output(
        [
            "git",
            "diff",
            "--unified=0",
            "--no-ext-diff",
            f"{base}...{head}",
            "--",
            path,
        ],
        text=True,
    )
    return parse_changed_head_ranges(diff)


def parse_changed_head_ranges(diff: str) -> list[tuple[int, int]]:
    ranges: list[tuple[int, int]] = []
    for line in diff.splitlines():
        match = HUNK_RE.match(line)
        if not match:
            continue
        start = int(match.group(1))
        count = int(match.group(2) or "1")
        if count == 0:
            # A deletion can require formatting at the surviving boundary.
            anchor = max(1, start)
            ranges.append((anchor, anchor))
        else:
            ranges.append((start, start + count - 1))
    return ranges


def rustfmt_text(source: str, *, edition: str) -> str:
    with tempfile.TemporaryDirectory(prefix="reader-rustfmt-delta-") as temp:
        path = Path(temp) / "source.rs"
        path.write_text(source, encoding="utf-8")
        result = subprocess.run(
            [
                "rustfmt",
                "--edition",
                edition,
                "--config",
                "skip_children=true",
                str(path),
            ],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
        if result.returncode != 0:
            sys.stderr.write(result.stdout)
            sys.stderr.write(result.stderr)
            raise SystemExit(result.returncode)
        return path.read_text(encoding="utf-8")


def formatting_change_ranges(source: str, formatted: str) -> list[tuple[int, int]]:
    before = source.splitlines()
    after = formatted.splitlines()
    matcher = difflib.SequenceMatcher(a=before, b=after, autojunk=False)
    ranges: list[tuple[int, int]] = []
    for tag, i1, i2, _j1, _j2 in matcher.get_opcodes():
        if tag == "equal":
            continue
        if i1 == i2:
            anchor = max(1, i1 + 1)
            ranges.append((anchor, anchor))
        else:
            ranges.append((i1 + 1, i2))
    return ranges


def ranges_intersect(
    left: list[tuple[int, int]], right: list[tuple[int, int]]
) -> bool:
    return any(a0 <= b1 and b0 <= a1 for a0, a1 in left for b0, b1 in right)


def print_diff(path: str, source: str, formatted: str) -> None:
    for line in difflib.unified_diff(
        source.splitlines(keepends=True),
        formatted.splitlines(keepends=True),
        fromfile=f"{path} (head)",
        tofile=f"{path} (rustfmt)",
    ):
        sys.stderr.write(line)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--base", required=True)
    parser.add_argument("--head", required=True)
    parser.add_argument("--path", required=True)
    parser.add_argument("--edition", required=True)
    args = parser.parse_args()

    if not args.path.endswith(".rs"):
        raise SystemExit("--path must name a Rust source file")

    head_source = git_text(args.head, args.path)
    if head_source is None:
        print(f"rustfmt-delta: {args.path} is absent at head; nothing to format")
        return 0

    formatted = rustfmt_text(head_source, edition=args.edition)
    if head_source == formatted:
        print(f"rustfmt-delta: {args.path} is fully rustfmt-clean")
        return 0

    base_source = git_text(args.base, args.path)
    if base_source is None:
        print_diff(args.path, head_source, formatted)
        print(
            f"rustfmt-delta: new Rust file {args.path} is not rustfmt-clean",
            file=sys.stderr,
        )
        return 1

    changed = changed_head_ranges(args.base, args.head, args.path)
    fmt_changes = formatting_change_ranges(head_source, formatted)
    if ranges_intersect(changed, fmt_changes):
        print_diff(args.path, head_source, formatted)
        print(
            "rustfmt-delta: formatting rewrite intersects PR-changed lines; "
            "the head introduces or touches rustfmt debt",
            file=sys.stderr,
        )
        return 1

    print(
        "rustfmt-delta: head still contains inherited rustfmt debt outside "
        f"PR-changed lines for {args.path}; allowed without weakening changed-line checks"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
