#!/usr/bin/env python3
"""Fail-closed routing laws for owned pub-editor integration-test paths."""

from pathlib import Path
import importlib.util

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location(
    "source_fanout_budget", ROOT / "tools/ci/source_fanout_budget.py"
)
assert SPEC and SPEC.loader
fanout = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(fanout)

DELETE_TEST = "vendor/producer-a/crates/pub-editor/tests/delete_node_runtime_v1.rs"
PUB_EDITOR_ROOT = "vendor/producer-a/crates/pub-editor/src/lib.rs"

OWNER_WORKFLOWS = (
    ".github/workflows/authoring-delete-node-runtime-v1.yml",
    ".github/workflows/pub-editor-fast-pr.yml",
)

UNRELATED_HEAVY_WORKFLOWS = (
    ".github/workflows/editable-typography-export-v1.yml",
    ".github/workflows/editable-typography-scoped-preserved-v2.yml",
    ".github/workflows/w2-project-fork-receipt-v1.yml",
    ".github/workflows/editable-source-image-export-v1.yml",
    ".github/workflows/migration-1050-corpus-matrix.yml",
    ".github/workflows/authoring-picture-frame-receipt.yml",
)


def patterns(path: str) -> list[str]:
    text = (ROOT / path).read_text(encoding="utf-8")
    result = fanout.extract_pull_request_paths(text, workflow=path)
    assert result is not None, f"{path}: pull_request trigger missing"
    return result


def admitted(path: str, source: str) -> bool:
    return fanout.admitted_by_patterns(source, patterns(path))


def main() -> int:
    for workflow in OWNER_WORKFLOWS:
        assert admitted(workflow, DELETE_TEST), (
            f"{workflow}: exact DeleteNode test owner must remain admitted"
        )

    for workflow in UNRELATED_HEAVY_WORKFLOWS:
        assert not admitted(workflow, DELETE_TEST), (
            f"{workflow}: unrelated heavy workflow must reject DeleteNode test"
        )
        assert admitted(workflow, PUB_EDITOR_ROOT), (
            f"{workflow}: product pub-editor root coverage must remain intact"
        )

    print("pub-editor integration-test path ownership: ok")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
