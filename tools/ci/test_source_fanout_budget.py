#!/usr/bin/env python3
from __future__ import annotations

import importlib.util
import subprocess
import tempfile
from pathlib import Path

MODULE = Path(__file__).with_name("source_fanout_budget.py")
spec = importlib.util.spec_from_file_location("source_fanout_budget", MODULE)
assert spec and spec.loader
mod = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mod)


def expect_error(errors: list[str], needle: str) -> None:
    assert any(needle in error for error in errors), errors


def test_patterns() -> None:
    patterns = ["src/**", "!src/generated/**", "src/generated/keep.rs"]
    assert mod.admitted_by_patterns("src/lib.rs", patterns)
    assert not mod.admitted_by_patterns("src/generated/a.rs", patterns)
    assert mod.admitted_by_patterns("src/generated/keep.rs", patterns)
    assert mod.path_matches("**/*.rs", "lib.rs")
    assert mod.path_matches("**/*.rs", "src/deep/lib.rs")
    assert mod.path_matches("foo/**/bar.rs", "foo/bar.rs")
    assert mod.path_matches("foo/**/bar.rs", "foo/a/b/bar.rs")


def test_yaml_paths() -> None:
    text = """name: x
on:
  pull_request:
    paths:
      - "src/**"
      - "!src/generated/**"
  workflow_dispatch:
"""
    assert mod.extract_pull_request_paths(text) == ["src/**", "!src/generated/**"]

    inline = """name: x
on:
  pull_request:
    paths: ["src/**", "!src/generated/**"]
"""
    assert mod.extract_pull_request_paths(inline) == ["src/**", "!src/generated/**"]

    always = """name: x
on:
  pull_request:
  workflow_dispatch:
"""
    assert mod.extract_pull_request_paths(always) is None


def test_budget_ratchet() -> None:
    base = {
        "schema": "chaptera.source-fanout-budget.v1",
        "new_leaf_max_conditional_workflows": 6,
        "tracked_roots": ["src/"],
        "shared_roots": {"src/lib.rs": {"max_lines": 100}},
    }
    assert mod.validate_budget_ratchet(base, base) == []

    raised = {**base, "new_leaf_max_conditional_workflows": 7}
    expect_error(mod.validate_budget_ratchet(base, raised), "budget increased")

    raised_lines = {
        **base,
        "shared_roots": {"src/lib.rs": {"max_lines": 101}},
    }
    expect_error(mod.validate_budget_ratchet(base, raised_lines), "line ceiling increased")

    removed_tracked = {**base, "tracked_roots": ["other/"]}
    expect_error(mod.validate_budget_ratchet(base, removed_tracked), "tracked roots removed")

    new_shared = {
        **base,
        "shared_roots": {
            "src/lib.rs": {"max_lines": 100},
            "src/new.rs": {"max_lines": 10},
        },
    }
    expect_error(
        mod.validate_budget_ratchet(base, new_shared),
        "new shared-root exemptions",
    )


def init_repo() -> tuple[Path, str]:
    root = Path(tempfile.mkdtemp(prefix="fanout-budget-"))
    subprocess.run(["git", "-C", str(root), "init", "-q"], check=True)
    subprocess.run(
        ["git", "-C", str(root), "config", "user.email", "ci@example.invalid"],
        check=True,
    )
    subprocess.run(["git", "-C", str(root), "config", "user.name", "CI"], check=True)
    (root / ".github/workflows").mkdir(parents=True)
    (root / "src").mkdir()
    (root / "tools/ci").mkdir(parents=True)
    (root / "src/lib.rs").write_text("fn root() {}\n", encoding="utf-8")
    (root / "src/leaf.rs").write_text("fn leaf() {}\n", encoding="utf-8")
    budget = {
        "schema": "chaptera.source-fanout-budget.v1",
        "new_leaf_max_conditional_workflows": 1,
        "tracked_roots": ["src/"],
        "shared_roots": {"src/lib.rs": {"max_lines": 1}},
    }
    (root / mod.BUDGET_PATH).write_text(
        __import__("json").dumps(budget) + "\n",
        encoding="utf-8",
    )
    (root / ".github/workflows/a.yml").write_text(
        'name: a\non:\n  pull_request:\n    paths:\n      - "src/leaf.rs"\n',
        encoding="utf-8",
    )
    subprocess.run(["git", "-C", str(root), "add", "."], check=True)
    subprocess.run(["git", "-C", str(root), "commit", "-qm", "base"], check=True)
    base = subprocess.check_output(
        ["git", "-C", str(root), "rev-parse", "HEAD"],
        text=True,
    ).strip()
    return root, base


def evaluate(root: Path, base: str, head: str) -> list[str]:
    return mod.evaluate(
        repo=root,
        base_revision=base,
        head_revision=head,
        base_budget=mod.read_budget(root, base),
        head_budget=mod.read_budget(root, head),
    )


def commit(root: Path, message: str) -> str:
    subprocess.run(["git", "-C", str(root), "add", "."], check=True)
    subprocess.run(["git", "-C", str(root), "commit", "-qm", message], check=True)
    return subprocess.check_output(
        ["git", "-C", str(root), "rev-parse", "HEAD"],
        text=True,
    ).strip()


def test_existing_fanout_increase() -> None:
    root, base = init_repo()
    (root / ".github/workflows/b.yml").write_text(
        'name: b\non:\n  pull_request:\n    paths:\n      - "src/**"\n',
        encoding="utf-8",
    )
    head = commit(root, "head")
    expect_error(evaluate(root, base, head), "FANOUT REGRESSION")


def test_existing_fanout_swap_passes() -> None:
    root, base = init_repo()
    (root / ".github/workflows/a.yml").write_text(
        'name: a\non:\n  pull_request:\n    paths:\n      - "other/**"\n',
        encoding="utf-8",
    )
    (root / ".github/workflows/b.yml").write_text(
        'name: b\non:\n  pull_request:\n    paths:\n      - "src/leaf.rs"\n',
        encoding="utf-8",
    )
    head = commit(root, "head")
    assert evaluate(root, base, head) == []


def test_existing_fanout_net_growth_fails() -> None:
    root, base = init_repo()
    (root / ".github/workflows/a.yml").write_text(
        'name: a\non:\n  pull_request:\n    paths:\n      - "other/**"\n',
        encoding="utf-8",
    )
    for name in ("b", "c"):
        (root / f".github/workflows/{name}.yml").write_text(
            f'name: {name}\non:\n  pull_request:\n    paths:\n      - "src/leaf.rs"\n',
            encoding="utf-8",
        )
    head = commit(root, "head")
    expect_error(evaluate(root, base, head), "net new conditional consumers: 1")


def test_new_leaf_budget() -> None:
    root, base = init_repo()
    (root / "src/new.rs").write_text("fn new_leaf() {}\n", encoding="utf-8")
    for name in ("b", "c"):
        (root / f".github/workflows/{name}.yml").write_text(
            f'name: {name}\non:\n  pull_request:\n    paths:\n      - "src/new.rs"\n',
            encoding="utf-8",
        )
    head = commit(root, "head")
    expect_error(evaluate(root, base, head), "NEW LEAF EXCEEDS FANOUT BUDGET")


def test_monolith_growth() -> None:
    root, base = init_repo()
    (root / "src/lib.rs").write_text(
        "fn root() {}\nfn grew() {}\n",
        encoding="utf-8",
    )
    head = commit(root, "head")
    expect_error(evaluate(root, base, head), "MONOLITH GROWTH REGRESSION")


def test_monolith_shrink_requires_ceiling_drop() -> None:
    root, base = init_repo()
    (root / "src/lib.rs").write_text("", encoding="utf-8")
    head = commit(root, "head")
    expect_error(evaluate(root, base, head), "MONOLITH CEILING NOT RATCHETED")


def test_fanout_reduction_passes() -> None:
    root, base = init_repo()
    (root / ".github/workflows/a.yml").write_text(
        'name: a\non:\n  pull_request:\n    paths:\n      - "other/**"\n',
        encoding="utf-8",
    )
    head = commit(root, "head")
    assert evaluate(root, base, head) == []


def main() -> int:
    test_patterns()
    test_yaml_paths()
    test_budget_ratchet()
    test_existing_fanout_increase()
    test_existing_fanout_swap_passes()
    test_existing_fanout_net_growth_fails()
    test_new_leaf_budget()
    test_monolith_growth()
    test_monolith_shrink_requires_ceiling_drop()
    test_fanout_reduction_passes()
    print("source fanout budget tests: ok")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
