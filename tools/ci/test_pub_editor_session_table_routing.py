#!/usr/bin/env python3
from __future__ import annotations

import importlib.util
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
GUARD = ROOT / "tools" / "ci" / "source_fanout_budget.py"
SESSION_TABLE = "vendor/producer-a/crates/pub-editor/src/session_table.rs"

EXPECTED = {
    ".github/workflows/pub-editor-fast-pr.yml",
    ".github/workflows/authoring-table-rowcol-v1.yml",
}

spec = importlib.util.spec_from_file_location("source_fanout_budget", GUARD)
if spec is None or spec.loader is None:
    raise SystemExit("cannot load source_fanout_budget.py")
mod = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mod)


def main() -> int:
    workflows = mod.workflows_at(ROOT, "HEAD")
    actual = mod.matching_workflows(SESSION_TABLE, workflows)
    if actual != EXPECTED:
        added = sorted(actual - EXPECTED)
        missing = sorted(EXPECTED - actual)
        details: list[str] = []
        if added:
            details.append("unexpected consumers: " + ", ".join(added))
        if missing:
            details.append("missing required consumers: " + ", ".join(missing))
        raise SystemExit(
            "pub-editor session-table routing regression: " + "; ".join(details)
        )

    print(
        "pub-editor session-table routing: ok "
        f"({len(actual)} conditional workflows: {', '.join(sorted(actual))})"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
