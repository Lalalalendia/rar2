#!/usr/bin/env python3
from __future__ import annotations

import argparse
from pathlib import Path
import re
import sys

GLOBAL_PR_MARKER = "# fanout: global-pr-required"
EVENT_RE = re.compile(r"^  ([A-Za-z_][A-Za-z0-9_-]*):(?:\s*(.*))?$")


def pull_request_block(text: str) -> list[str] | None:
    lines = text.splitlines()
    for index, line in enumerate(lines):
        match = EVENT_RE.match(line)
        if not match or match.group(1) != "pull_request":
            continue
        block = [line]
        for following in lines[index + 1 :]:
            if EVENT_RE.match(following):
                break
            if following and not following.startswith(" "):
                break
            block.append(following)
        return block
    return None


def is_scoped_pull_request(text: str) -> bool:
    block = pull_request_block(text)
    if block is None:
        return True
    body = "\n".join(block[1:])
    return bool(re.search(r"^\s{4}(paths|paths-ignore):\s*$", body, re.MULTILINE))


def check_path(path: Path) -> str | None:
    text = path.read_text(encoding="utf-8")
    if pull_request_block(text) is None:
        return None
    if is_scoped_pull_request(text):
        return None
    if GLOBAL_PR_MARKER in text:
        return None
    return (
        f"{path.as_posix()}: unscoped pull_request trigger requires paths/paths-ignore "
        f"or explicit {GLOBAL_PR_MARKER}"
    )


def self_test() -> None:
    assert pull_request_block("on:\n  workflow_dispatch:\n") is None
    assert is_scoped_pull_request(
        "on:\n  pull_request:\n    paths:\n      - 'src/**'\n  workflow_dispatch:\n"
    )
    assert is_scoped_pull_request(
        "on:\n  pull_request:\n    paths-ignore:\n      - 'docs/**'\n"
    )
    assert not is_scoped_pull_request(
        "on:\n  pull_request:\n  workflow_dispatch:\n"
    )
    assert not is_scoped_pull_request(
        "on:\n  pull_request:\n    branches:\n      - main\n"
    )
    print("changed PR workflow scope self-test: ok")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("paths", nargs="*")
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()

    if args.self_test:
        self_test()

    violations = []
    for raw in args.paths:
        path = Path(raw)
        if not path.exists():
            continue
        violation = check_path(path)
        if violation:
            violations.append(violation)

    if violations:
        print("\n".join(violations), file=sys.stderr)
        return 1

    if args.paths:
        print(f"changed PR workflow scope guard: ok ({len(args.paths)} paths checked)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
