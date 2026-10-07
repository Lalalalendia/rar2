#!/usr/bin/env python3
from __future__ import annotations

import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
WORKFLOWS = ROOT / ".github" / "workflows"
ALLOWED_FULL = "pub-editor-full-regression.yml"

RUN_RE = re.compile(r"^(?P<indent>\s*)run:\s*(?P<body>.*)$")
PUB_EDITOR_PACKAGE = re.compile(r"(?:-p|--package)\s+pub-editor\b")


def run_blocks(text: str) -> list[str]:
    lines = text.splitlines()
    blocks: list[str] = []
    i = 0
    while i < len(lines):
        match = RUN_RE.match(lines[i])
        if not match:
            i += 1
            continue

        base_indent = len(match.group("indent"))
        body = match.group("body").strip()
        if body and body not in {"|", "|-", ">", ">-"}:
            blocks.append(body)
            i += 1
            continue

        style = body or "|"
        raw: list[str] = []
        i += 1
        while i < len(lines):
            line = lines[i]
            if line.strip() and len(line) - len(line.lstrip()) <= base_indent:
                break
            if line.strip():
                raw.append(line.strip())
            i += 1

        if style.startswith(">"):
            blocks.append(" ".join(raw))
            continue

        # Literal shell blocks may contain many commands. Keep a cargo command
        # with explicit backslash continuations together, but never let a
        # later unrelated command donate --lib/--test to it.
        current = ""
        for line in raw:
            if current:
                current += " " + line
            else:
                current = line
            if current.endswith("\\"):
                current = current[:-1].rstrip()
                continue
            blocks.append(current)
            current = ""
        if current:
            blocks.append(current)

    return blocks


def main() -> int:
    violations: list[str] = []
    audited = 0

    for path in sorted(WORKFLOWS.glob("*.yml")):
        text = path.read_text(encoding="utf-8")
        for command in run_blocks(text):
            if "cargo test" not in command or not PUB_EDITOR_PACKAGE.search(command):
                continue
            audited += 1
            if path.name == ALLOWED_FULL:
                continue
            if "--lib" in command or "--test" in command:
                continue
            violations.append(f"{path.relative_to(ROOT)}: {command[:180]}")

    if violations:
        raise SystemExit(
            "unbounded pub-editor cargo test outside canonical deep workflow:\n  - "
            + "\n  - ".join(violations)
        )

    print(
        "pub-editor bounded cargo-test guard: ok "
        f"({audited} pub-editor cargo test invocation(s) audited)"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
