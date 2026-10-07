#!/usr/bin/env python3
from __future__ import annotations

import importlib.util
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
FAST_LOOP = ROOT / "tools" / "dev_fast_loop.py"

spec = importlib.util.spec_from_file_location("dev_fast_loop", FAST_LOOP)
if spec is None or spec.loader is None:
    raise SystemExit("cannot load dev_fast_loop.py")
mod = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mod)

PUB_EDITOR_SRC = ROOT / "vendor" / "producer-a" / "crates" / "pub-editor" / "src"
AUTHORING_CORE_SRC = ROOT / "vendor" / "producer-a" / "crates" / "pub-editor-authoring-core" / "src"
TABLE_CORE_SRC = ROOT / "vendor" / "producer-a" / "crates" / "pub-editor-table-core" / "src"
PREFIX = "vendor/producer-a/crates/pub-editor/src/"
AUTHORING_CORE_PREFIX = "vendor/producer-a/crates/pub-editor-authoring-core/src/"
TABLE_CORE_PREFIX = "vendor/producer-a/crates/pub-editor-table-core/src/"


def is_pub_editor_test(command: tuple[str, ...]) -> bool:
    if not command or command[0] != "cargo" or "test" not in command:
        return False
    if "-p" in command:
        index = command.index("-p")
        if index + 1 < len(command) and command[index + 1] in {
            "pub-editor",
            "pub-editor-authoring-core",
            "pub-editor-table-core",
        }:
            return True
    if "--manifest-path" in command:
        index = command.index("--manifest-path")
        if index + 1 < len(command):
            manifest = command[index + 1].replace("\\", "/")
            return (
                manifest.endswith("/pub-editor/Cargo.toml")
                or manifest.endswith("/pub-editor-authoring-core/Cargo.toml")
                or manifest.endswith("/pub-editor-table-core/Cargo.toml")
            )
    return False


def is_bounded_test(command: tuple[str, ...]) -> bool:
    if not is_pub_editor_test(command):
        return False
    return "--test" in command or "--lib" in command


def main() -> int:
    uncovered: list[str] = []
    unbounded: list[tuple[str, tuple[str, ...]]] = []

    for source in sorted(PUB_EDITOR_SRC.glob("*.rs")):
        rel = PREFIX + source.name
        checks = mod.plan_for_paths(ROOT, [rel], mode="edit")
        test_commands = [check.command for check in checks if is_pub_editor_test(check.command)]

        if source.name != "lib.rs" and not test_commands:
            uncovered.append(rel)

        for command in test_commands:
            if not is_bounded_test(command):
                unbounded.append((rel, command))

    for source in sorted(AUTHORING_CORE_SRC.glob("*.rs")):
        rel = AUTHORING_CORE_PREFIX + source.name
        checks = mod.plan_for_paths(ROOT, [rel], mode="edit")
        test_commands = [check.command for check in checks if is_pub_editor_test(check.command)]

        if source.name != "lib.rs" and not test_commands:
            uncovered.append(rel)

        for command in test_commands:
            if not is_bounded_test(command):
                unbounded.append((rel, command))

    for source in sorted(TABLE_CORE_SRC.glob("*.rs")):
        rel = TABLE_CORE_PREFIX + source.name
        checks = mod.plan_for_paths(ROOT, [rel], mode="edit")
        test_commands = [check.command for check in checks if is_pub_editor_test(check.command)]

        if source.name != "lib.rs" and not test_commands:
            uncovered.append(rel)

        for command in test_commands:
            if not is_bounded_test(command):
                unbounded.append((rel, command))

    lib_checks = mod.plan_for_paths(ROOT, [PREFIX + "lib.rs"], mode="edit")
    lib_tests = [check.command for check in lib_checks if is_pub_editor_test(check.command)]
    if not lib_tests or not all("--lib" in command for command in lib_tests):
        raise SystemExit(
            "pub-editor lib.rs must use bounded --lib tests in the fast loop"
        )

    if uncovered:
        raise SystemExit(
            "pub-editor fast plan has source modules without bounded tests: "
            + ", ".join(uncovered)
        )
    if unbounded:
        details = "; ".join(
            f'{path}: {" ".join(command)}' for path, command in unbounded
        )
        raise SystemExit("pub-editor fast plan contains full-crate tests: " + details)

    representative = {
        PREFIX + "session_geometry.rs": "move_nodes_v1",
        PREFIX + "duplicate_authored_rectangle_v1.rs": "duplicate_authored_rectangle_v1",
        AUTHORING_CORE_PREFIX + "create_table_runtime_v1.rs": "create_table_runtime_v1",
        AUTHORING_CORE_PREFIX + "authored_stack_lifecycle_v1.rs": "authored_stack_lifecycle_v1",
        TABLE_CORE_PREFIX + "table_track_extent_v1.rs": "table_track_extent_v1",
        AUTHORING_CORE_PREFIX + "authored_stack_runtime_v1.rs": "authored_stack_runtime_v1",
        AUTHORING_CORE_PREFIX + "create_line_runtime_v1.rs": "create_line_runtime_v1",
    }
    for path, target in representative.items():
        checks = mod.plan_for_paths(ROOT, [path], mode="edit")
        commands = [check.command for check in checks]
        if not any("--test" in command and target in command for command in commands):
            raise SystemExit(f"{path}: expected exact integration target {target!r}")

    print(
        "pub-editor fast plan contract: ok "
        f"({len(list(PUB_EDITOR_SRC.glob('*.rs'))) + len(list(AUTHORING_CORE_SRC.glob('*.rs'))) + len(list(TABLE_CORE_SRC.glob('*.rs')))} source modules audited)"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
