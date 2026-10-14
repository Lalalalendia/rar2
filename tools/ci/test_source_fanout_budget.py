#!/usr/bin/env python3
from __future__ import annotations

import importlib.util
import json
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



def write_v2_budget(
    root: Path,
    shared: dict[str, int],
    *,
    leaf_limit: int = 1,
    tracked_roots: list[str] | None = None,
    shard_names: dict[str, str] | None = None,
) -> dict[str, str]:
    tracked = tracked_roots or ["src/"]
    policy = {
        "schema": mod.BUDGET_SCHEMA_V2,
        "new_leaf_max_conditional_workflows": leaf_limit,
        "tracked_roots": tracked,
    }
    (root / mod.BUDGET_PATH).write_text(
        json.dumps(policy, indent=2) + "\n", encoding="utf-8"
    )
    shard_dir = root / mod.ROOT_SHARD_DIR
    shard_dir.mkdir(parents=True, exist_ok=True)
    for existing in shard_dir.glob("*.json"):
        existing.unlink()
    result: dict[str, str] = {}
    for index, (source, ceiling) in enumerate(sorted(shared.items())):
        name = (shard_names or {}).get(source, f"root-{index}.json")
        shard = shard_dir / name
        shard.write_text(
            json.dumps(
                {
                    "schema": mod.ROOT_SHARD_SCHEMA,
                    "path": source,
                    "max_lines": ceiling,
                },
                indent=2,
            )
            + "\n",
            encoding="utf-8",
        )
        result[source] = shard.relative_to(root).as_posix()
    return result


def init_repo_v2() -> tuple[Path, str, dict[str, str]]:
    root, _ = init_repo()
    shards = write_v2_budget(root, {"src/lib.rs": 1})
    base = commit(root, "v2 base")
    return root, base, shards


def test_v1_to_v2_migration_preserves_authority() -> None:
    root, base = init_repo()
    write_v2_budget(root, {"src/lib.rs": 1})
    head = commit(root, "migrate to shards")
    assert evaluate(root, base, head) == []
    budget = mod.read_budget(root, head)
    assert budget is not None
    assert budget["schema"] == mod.BUDGET_SCHEMA_V2
    assert budget["shared_roots"] == {"src/lib.rs": {"max_lines": 1}}


def test_v1_to_v2_migration_cannot_change_ceiling() -> None:
    root, base = init_repo()
    write_v2_budget(root, {"src/lib.rs": 2})
    head = commit(root, "bad migration")
    expect_error(
        evaluate(root, base, head),
        "v1 -> v2 migration must preserve all shared-root ceilings exactly",
    )


def test_v2_removed_shard_fails() -> None:
    root, base, shards = init_repo_v2()
    (root / shards["src/lib.rs"]).unlink()
    head = commit(root, "remove root shard")
    expect_error(evaluate(root, base, head), "shared-root shard removed")


def test_v2_new_exemption_fails() -> None:
    root, base, _ = init_repo_v2()
    shard_dir = root / mod.ROOT_SHARD_DIR
    (shard_dir / "leaf.json").write_text(
        json.dumps(
            {
                "schema": mod.ROOT_SHARD_SCHEMA,
                "path": "src/leaf.rs",
                "max_lines": 1,
            },
            indent=2,
        )
        + "\n",
        encoding="utf-8",
    )
    head = commit(root, "add exemption")
    expect_error(
        evaluate(root, base, head),
        "new shared-root exemptions are forbidden",
    )


def test_v2_duplicate_root_fails_closed() -> None:
    root, _, shards = init_repo_v2()
    original = json.loads((root / shards["src/lib.rs"]).read_text(encoding="utf-8"))
    (root / mod.ROOT_SHARD_DIR / "duplicate.json").write_text(
        json.dumps(original, indent=2) + "\n",
        encoding="utf-8",
    )
    head = commit(root, "duplicate root")
    try:
        mod.read_budget(root, head)
    except mod.BudgetError as exc:
        assert "duplicate shared-root path" in str(exc)
    else:
        raise AssertionError("duplicate shared-root shard must fail closed")


def test_v2_malformed_shard_fails_closed() -> None:
    root, _, shards = init_repo_v2()
    (root / shards["src/lib.rs"]).write_text("{not-json\n", encoding="utf-8")
    head = commit(root, "malformed root")
    try:
        mod.read_budget(root, head)
    except mod.BudgetError as exc:
        assert "invalid JSON" in str(exc)
    else:
        raise AssertionError("malformed shared-root shard must fail closed")


def test_v2_path_spoof_fails() -> None:
    root, base, shards = init_repo_v2()
    shard = root / shards["src/lib.rs"]
    data = json.loads(shard.read_text(encoding="utf-8"))
    data["path"] = "src/spoof.rs"
    shard.write_text(json.dumps(data, indent=2) + "\n", encoding="utf-8")
    head = commit(root, "spoof root path")
    errors = evaluate(root, base, head)
    expect_error(errors, "new shared-root exemptions are forbidden")
    expect_error(errors, "shared-root shard removed")


def test_v2_shard_rename_fails() -> None:
    root, base, shards = init_repo_v2()
    old = root / shards["src/lib.rs"]
    new = old.with_name("renamed.json")
    old.rename(new)
    head = commit(root, "rename shard")
    expect_error(evaluate(root, base, head), "shared-root shard path changed")


def test_v2_ceiling_increase_fails() -> None:
    root, base, shards = init_repo_v2()
    shard = root / shards["src/lib.rs"]
    data = json.loads(shard.read_text(encoding="utf-8"))
    data["max_lines"] = 2
    shard.write_text(json.dumps(data, indent=2) + "\n", encoding="utf-8")
    head = commit(root, "raise ceiling")
    expect_error(evaluate(root, base, head), "line ceiling increased")


def test_v2_global_leaf_budget_increase_fails() -> None:
    root, base, _ = init_repo_v2()
    policy = json.loads((root / mod.BUDGET_PATH).read_text(encoding="utf-8"))
    policy["new_leaf_max_conditional_workflows"] = 2
    (root / mod.BUDGET_PATH).write_text(
        json.dumps(policy, indent=2) + "\n", encoding="utf-8"
    )
    head = commit(root, "raise leaf budget")
    expect_error(evaluate(root, base, head), "budget increased")


def test_v2_shrink_requires_own_shard_ratchet() -> None:
    root, base, _ = init_repo_v2()
    (root / "src/lib.rs").write_text("", encoding="utf-8")
    head = commit(root, "shrink without shard")
    expect_error(evaluate(root, base, head), "MONOLITH CEILING NOT RATCHETED")


def test_disjoint_roots_have_disjoint_budget_paths() -> None:
    root, _, _ = init_repo_v2()
    (root / "src/a.rs").write_text("fn a() {}\nfn a2() {}\n", encoding="utf-8")
    (root / "src/b.rs").write_text("fn b() {}\nfn b2() {}\n", encoding="utf-8")
    shards = write_v2_budget(
        root,
        {"src/a.rs": 2, "src/b.rs": 2},
        shard_names={"src/a.rs": "a.json", "src/b.rs": "b.json"},
    )
    base = commit(root, "two root base")

    subprocess.run(
        ["git", "-C", str(root), "checkout", "-qb", "root-a", base], check=True
    )
    (root / "src/a.rs").write_text("fn a() {}\n", encoding="utf-8")
    a_shard = root / shards["src/a.rs"]
    a_data = json.loads(a_shard.read_text(encoding="utf-8"))
    a_data["max_lines"] = 1
    a_shard.write_text(json.dumps(a_data, indent=2) + "\n", encoding="utf-8")
    a_head = commit(root, "shrink a")
    assert evaluate(root, base, a_head) == []

    subprocess.run(
        ["git", "-C", str(root), "checkout", "-qb", "root-b", base], check=True
    )
    (root / "src/b.rs").write_text("fn b() {}\n", encoding="utf-8")
    b_shard = root / shards["src/b.rs"]
    b_data = json.loads(b_shard.read_text(encoding="utf-8"))
    b_data["max_lines"] = 1
    b_shard.write_text(json.dumps(b_data, indent=2) + "\n", encoding="utf-8")
    b_head = commit(root, "shrink b")
    assert evaluate(root, base, b_head) == []

    def budget_paths(head: str) -> set[str]:
        changed = subprocess.check_output(
            ["git", "-C", str(root), "diff", "--name-only", f"{base}...{head}"],
            text=True,
        ).splitlines()
        return {
            path
            for path in changed
            if path == mod.BUDGET_PATH or path.startswith(mod.ROOT_SHARD_DIR)
        }

    a_budget = budget_paths(a_head)
    b_budget = budget_paths(b_head)
    assert a_budget == {shards["src/a.rs"]}
    assert b_budget == {shards["src/b.rs"]}
    assert a_budget.isdisjoint(b_budget)


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
    test_v1_to_v2_migration_preserves_authority()
    test_v1_to_v2_migration_cannot_change_ceiling()
    test_v2_removed_shard_fails()
    test_v2_new_exemption_fails()
    test_v2_duplicate_root_fails_closed()
    test_v2_malformed_shard_fails_closed()
    test_v2_path_spoof_fails()
    test_v2_shard_rename_fails()
    test_v2_ceiling_increase_fails()
    test_v2_global_leaf_budget_increase_fails()
    test_v2_shrink_requires_own_shard_ratchet()
    test_disjoint_roots_have_disjoint_budget_paths()
    print("source fanout budget tests: ok")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
