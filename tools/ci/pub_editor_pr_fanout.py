#!/usr/bin/env python3
from __future__ import annotations

import argparse
import fnmatch
import json
from pathlib import Path
import subprocess
from typing import Iterable


PUB_EDITOR_PREFIX = "vendor/producer-a/crates/pub-editor/"
PUB_EDITOR_LIB = PUB_EDITOR_PREFIX + "src/lib.rs"

# Deliberately tiny first allowlist. These modules are feature-owned and are
# not consumed by the Desktop Continuity V2 acceptance transaction.
SAFE_CONTINUITY_V2_MODULES = {
    "duplicate_authored_rectangle_v1": PUB_EDITOR_PREFIX + "src/duplicate_authored_rectangle_v1.rs",
    "imported_paragraph_alignment_v1": PUB_EDITOR_PREFIX + "src/imported_paragraph_alignment_v1.rs",
}

DIRECT_CONTINUITY_V2_OWNERS = (
    ".github/workflows/editor-desktop-continuity-v2-windows.yml",
    "apps/chaptera-desktop/src/acceptance.rs",
    "apps/chaptera-desktop/src/acceptance_v2.rs",
    "apps/chaptera-desktop/src/acceptance_v2_cli.rs",
    "packages/product/editor-desktop-continuity/v2/**",
    "tools/run_editor_desktop_continuity_v2.py",
    "tools/validate_editor_desktop_continuity_v2_receipt.py",
    "tools/verify_editable_export_geometry.py",
    "vendor/producer-a/crates/pub-odg/**",
    "crates/chaptera-scene-instance/**",
)


def changed_paths(base: str, head: str) -> list[str]:
    return sorted(
        dict.fromkeys(
            path
            for path in subprocess.check_output(
                ["git", "diff", "--name-only", f"{base}...{head}"],
                text=True,
            ).splitlines()
            if path
        )
    )


def matches(path: str, patterns: Iterable[str]) -> bool:
    for pattern in patterns:
        if pattern.endswith("/**") and path.startswith(pattern[:-3]):
            return True
        if fnmatch.fnmatchcase(path, pattern):
            return True
    return False


def strip_safe_facade_blocks(source: str) -> str:
    """Remove only declarations/re-exports for explicitly safe modules."""
    lines = source.splitlines(keepends=True)
    output: list[str] = []
    index = 0

    while index < len(lines):
        stripped = lines[index].strip()
        module = next(
            (
                name
                for name in SAFE_CONTINUITY_V2_MODULES
                if stripped == f"mod {name};"
            ),
            None,
        )
        if module is not None:
            index += 1
            continue

        module = next(
            (
                name
                for name in SAFE_CONTINUITY_V2_MODULES
                if stripped.startswith(f"pub use {name}::")
            ),
            None,
        )
        if module is not None:
            if stripped.endswith(";"):
                index += 1
                continue
            index += 1
            while index < len(lines):
                if lines[index].strip() == "};":
                    index += 1
                    break
                index += 1
            continue

        output.append(lines[index])
        index += 1

    return "".join(output)


def facade_change_is_safe(base_source: str, head_source: str) -> bool:
    return strip_safe_facade_blocks(base_source) == strip_safe_facade_blocks(head_source)


def classify_continuity_v2_windows(
    paths: list[str],
    *,
    base_lib_source: str | None = None,
    head_lib_source: str | None = None,
) -> tuple[bool, str]:
    if any(matches(path, DIRECT_CONTINUITY_V2_OWNERS) for path in paths):
        return True, "direct_continuity_owner_changed"

    pub_editor_paths = [
        path for path in paths if path.startswith(PUB_EDITOR_PREFIX)
    ]
    if not pub_editor_paths:
        return False, "no_continuity_owner_changed"

    allowed_paths = set(SAFE_CONTINUITY_V2_MODULES.values()) | {PUB_EDITOR_LIB}
    unknown = sorted(set(pub_editor_paths) - allowed_paths)
    if unknown:
        return True, "unknown_or_core_pub_editor_path"

    if PUB_EDITOR_LIB in pub_editor_paths:
        if base_lib_source is None or head_lib_source is None:
            return True, "lib_changed_without_source_proof"
        if not facade_change_is_safe(base_lib_source, head_lib_source):
            return True, "pub_editor_lib_core_change"

    return False, "proven_non_continuity_pub_editor_slice"


def git_show(revision: str, path: str) -> str | None:
    try:
        return subprocess.check_output(
            ["git", "show", f"{revision}:{path}"],
            text=True,
        )
    except subprocess.CalledProcessError:
        return None


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--base", required=True)
    parser.add_argument("--head", required=True)
    parser.add_argument("--receipt", type=Path)
    parser.add_argument("--github-output", type=Path)
    args = parser.parse_args()

    paths = changed_paths(args.base, args.head)
    base_lib = git_show(args.base, PUB_EDITOR_LIB) if PUB_EDITOR_LIB in paths else None
    head_lib = git_show(args.head, PUB_EDITOR_LIB) if PUB_EDITOR_LIB in paths else None
    run_windows, reason = classify_continuity_v2_windows(
        paths,
        base_lib_source=base_lib,
        head_lib_source=head_lib,
    )

    receipt = {
        "schema": "chaptera.pub-editor-pr-fanout.v1",
        "base": args.base,
        "head": args.head,
        "changed_paths": paths,
        "continuity_v2_windows": run_windows,
        "reason": reason,
        "safe_continuity_v2_modules": sorted(SAFE_CONTINUITY_V2_MODULES.values()),
    }

    payload = json.dumps(receipt, indent=2, sort_keys=True) + "\n"
    if args.receipt:
        args.receipt.parent.mkdir(parents=True, exist_ok=True)
        args.receipt.write_text(payload, encoding="utf-8")
    else:
        print(payload, end="")

    if args.github_output:
        with args.github_output.open("a", encoding="utf-8") as handle:
            handle.write(f"continuity_v2_windows={'true' if run_windows else 'false'}\n")
            handle.write(f"reason={reason}\n")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
