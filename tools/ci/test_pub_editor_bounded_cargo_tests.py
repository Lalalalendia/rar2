#!/usr/bin/env python3
from __future__ import annotations

import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
WORKFLOWS = ROOT / ".github" / "workflows"
ALLOWED_FULL = "pub-editor-full-regression.yml"

PUB_EDITOR_TEST = re.compile(r"cargo\s+test\b", re.MULTILINE)
PUB_EDITOR_PACKAGE = re.compile(r"(?:-p|--package)\s+pub-editor\b")


def command_window(text: str, start: int) -> str:
    # Every legitimate --lib/--test selector appears immediately after the
    # package selector, including folded YAML commands. Keep the window small
    # so a later unrelated step cannot accidentally bless an unbounded test.
    return text[start : start + 260]


def main() -> int:
    violations: list[str] = []
    audited = 0

    for path in sorted(WORKFLOWS.glob("*.yml")):
        text = path.read_text(encoding="utf-8")
        for match in PUB_EDITOR_TEST.finditer(text):
            window = command_window(text, match.start())
            if not PUB_EDITOR_PACKAGE.search(window):
                continue
            audited += 1
            if path.name == ALLOWED_FULL:
                continue
            if "--lib" in window or "--test" in window:
                continue
            line = text.count("\n", 0, match.start()) + 1
            violations.append(f"{path.relative_to(ROOT)}:{line}")

    if violations:
        raise SystemExit(
            "unbounded pub-editor cargo test outside canonical deep workflow: "
            + ", ".join(violations)
        )

    print(
        "pub-editor bounded cargo-test guard: ok "
        f"({audited} pub-editor cargo test invocation(s) audited)"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
