#!/usr/bin/env python3
from __future__ import annotations

from pathlib import Path
import re
import sys


WORKFLOW_ROOT = Path(".github/workflows")
RUNS_ON_RE = re.compile(r"^(?P<indent>\s*)runs-on:\s*(?P<inline>.*)$")
PUBLISHER_LABEL_RE = re.compile(r"publisher-\d{4}", re.IGNORECASE)


def leading_spaces(value: str) -> int:
    return len(value) - len(value.lstrip(" "))


def runs_on_blocks(text: str):
    lines = text.splitlines()
    for index, line in enumerate(lines):
        match = RUNS_ON_RE.match(line)
        if not match:
            continue
        indent = len(match.group("indent"))
        block = []
        inline = match.group("inline").strip()
        if inline:
            block.append(inline)
        cursor = index + 1
        while cursor < len(lines):
            candidate = lines[cursor]
            if candidate.strip() and leading_spaces(candidate) <= indent:
                break
            block.append(candidate.strip())
            cursor += 1
        yield index + 1, "\n".join(block)


def main() -> int:
    offenders = []
    for path in sorted(
        list(WORKFLOW_ROOT.glob("*.yml")) + list(WORKFLOW_ROOT.glob("*.yaml"))
    ):
        text = path.read_text(encoding="utf-8")
        for line_no, block in runs_on_blocks(text):
            normalized = block.lower()
            if "self-hosted" not in normalized:
                continue
            publisher_native = (
                "pub-research" in normalized
                or "publisher_environment" in normalized
                or PUBLISHER_LABEL_RE.search(block) is not None
            )
            if publisher_native:
                offenders.append((path.as_posix(), line_no, block))

    if offenders:
        print(
            "Publisher-native execution must not wait on GitHub self-hosted runners.",
            file=sys.stderr,
        )
        print(
            "GitHub may validate/prepare a handoff; exact Publisher execution is local/manual.",
            file=sys.stderr,
        )
        for path, line_no, block in offenders:
            print(f"{path}:{line_no}: forbidden runs-on block:\n{block}", file=sys.stderr)
        return 1

    print("Publisher native Actions boundary: ok (no self-hosted Publisher jobs)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
