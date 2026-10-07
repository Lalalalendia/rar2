#!/usr/bin/env python3
from __future__ import annotations

import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile

MODULE = Path(__file__).with_name("dev_fast_loop.py")
spec = importlib.util.spec_from_file_location("dev_fast_loop", MODULE)
mod = importlib.util.module_from_spec(spec)
assert spec and spec.loader
spec.loader.exec_module(mod)


def commands(checks):
    return [check.command for check in checks]


def write(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")


def git(root: Path, *args: str) -> None:
    subprocess.run(["git", *args], cwd=root, check=True, stdout=subprocess.DEVNULL)


def test_discover_changed_paths() -> None:
    with tempfile.TemporaryDirectory() as raw:
        root = Path(raw)
        git(root, "init", "-b", "main")
        git(root, "config", "user.email", "fast-loop@example.invalid")
        git(root, "config", "user.name", "Fast Loop Test")
        write(root / "committed.py", "VALUE = 1\n")
        write(root / "staged.py", "VALUE = 1\n")
        write(root / "working.py", "VALUE = 1\n")
        git(root, "add", ".")
        git(root, "commit", "-m", "base")
        git(root, "checkout", "-b", "feature")

        write(root / "committed.py", "VALUE = 2\n")
        git(root, "add", "committed.py")
        git(root, "commit", "-m", "feature")
        write(root / "staged.py", "VALUE = 2\n")
        git(root, "add", "staged.py")
        write(root / "working.py", "VALUE = 2\n")
        write(root / "untracked.py", "VALUE = 2\n")

        assert mod.discover_changed_paths(root, base="main") == [
            "committed.py",
            "staged.py",
            "untracked.py",
            "working.py",
        ]


def test_plan_routing_and_dedupe() -> None:
    with tempfile.TemporaryDirectory() as raw:
        root = Path(raw)
        write(root / "Cargo.toml", "[workspace]\nmembers = [\"crates/foo\"]\nresolver = \"2\"\n")
        write(
            root / "crates/foo/Cargo.toml",
            "[package]\nname = \"foo\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        write(root / "crates/foo/src/lib.rs", "pub fn a() {}\n")
        write(root / "crates/foo/src/extra.rs", "pub fn b() {}\n")
        write(root / "crates/foo/tests/extra.rs", "#[test] fn smoke() {}\n")
        write(root / "tools/ci/foo.py", "VALUE = 1\n")
        write(root / "tools/ci/test_foo.py", "print('ok')\n")
        write(root / "web/a.mjs", "export const value = 1;\n")
        write(root / "web/a.test.mjs", "import assert from 'node:assert'; assert.ok(true);\n")
        write(root / ".github/workflows/x.yml", "name: x\non: workflow_dispatch\njobs: {}\n")
        write(root / "tools/ci/check_workflow_yaml_syntax.rb", "puts 'ok'\n")

        checks = mod.plan_for_paths(
            root,
            [
                "crates/foo/src/lib.rs",
                "crates/foo/src/extra.rs",
                "crates/foo/tests/extra.rs",
                "tools/ci/foo.py",
                "web/a.mjs",
                ".github/workflows/x.yml",
            ],
        )
        actual = commands(checks)
        manifest = "crates/foo/Cargo.toml"

        assert sum(command[:4] == ("cargo", "check", "--manifest-path", manifest) for command in actual) == 1
        assert sum(command[:4] == ("cargo", "fmt", "--manifest-path", manifest) for command in actual) == 1
        assert ("cargo", "test", "--manifest-path", manifest, "--test", "extra", "--no-fail-fast") in actual
        assert any(command[1:3] == ("-m", "py_compile") and "tools/ci/foo.py" in command for command in actual)
        assert any(command[-1] == "tools/ci/test_foo.py" for command in actual)
        assert ("node", "--check", "web/a.mjs") in actual
        assert ("node", "--test", "web/a.test.mjs") in actual
        assert ("ruby", "tools/ci/check_workflow_yaml_syntax.rb") in actual


def test_same_stem_rust_source_discovers_exact_integration_test() -> None:
    with tempfile.TemporaryDirectory() as raw:
        root = Path(raw)
        write(
            root / "crates/foo/Cargo.toml",
            "[package]\nname='foo'\nversion='0.1.0'\nedition='2024'\n",
        )
        write(root / "crates/foo/src/owned.rs", "pub fn owned() {}\n")
        write(root / "crates/foo/src/unowned.rs", "pub fn unowned() {}\n")
        write(root / "crates/foo/tests/owned.rs", "#[test] fn smoke() {}\n")

        owned = commands(mod.plan_for_paths(root, ["crates/foo/src/owned.rs"]))
        assert (
            "cargo",
            "test",
            "--manifest-path",
            "crates/foo/Cargo.toml",
            "--test",
            "owned",
            "--no-fail-fast",
        ) in owned

        unowned = commands(mod.plan_for_paths(root, ["crates/foo/src/unowned.rs"]))
        assert not any(
            command[:2] == ("cargo", "test") and "--test" in command
            for command in unowned
        )


def test_component_registry_routes_aliases_dedupes_and_ignores_unrelated() -> None:
    with tempfile.TemporaryDirectory() as raw:
        root = Path(raw)
        write(
            root / mod.COMPONENT_REGISTRY_PATH,
            """{
  "schema": "chaptera.dev-fast-components.v1",
  "rules": [
    {
      "name": "owned alias",
      "paths": ["crates/foo/src/owned.rs"],
      "commands": [["python", "-c", "print('owned')"]]
    },
    {
      "name": "duplicate command",
      "paths": ["crates/foo/src/owned.rs"],
      "commands": [["python", "-c", "print('owned')"]]
    }
  ]
}
""",
        )
        write(
            root / "crates/foo/Cargo.toml",
            "[package]\nname='foo'\nversion='0.1.0'\nedition='2024'\n",
        )
        write(root / "crates/foo/src/owned.rs", "pub fn owned() {}\n")
        write(root / "crates/foo/src/other.rs", "pub fn other() {}\n")

        owned = mod.plan_for_paths(root, ["crates/foo/src/owned.rs"])
        component = [check for check in owned if check.kind == "component-micro-test"]
        assert len(component) == 1
        assert component[0].command == ("python", "-c", "print('owned')")

        unrelated = mod.plan_for_paths(root, ["crates/foo/src/other.rs"])
        assert not any(check.kind == "component-micro-test" for check in unrelated)


def test_component_registry_malformed_fails_closed() -> None:
    with tempfile.TemporaryDirectory() as raw:
        root = Path(raw)
        write(root / mod.COMPONENT_REGISTRY_PATH, '{"schema":"wrong","rules":[]}\n')
        try:
            mod.plan_for_paths(root, ["README.md"])
        except RuntimeError as exc:
            assert "registry schema" in str(exc)
        else:
            raise AssertionError("malformed component registry must fail closed")


def test_rust_cache_auto_off_require_and_git_common_dir() -> None:
    with tempfile.TemporaryDirectory() as raw:
        root = Path(raw) / "main"
        root.mkdir()
        git(root, "init", "-b", "main")
        git(root, "config", "user.email", "fast-loop@example.invalid")
        git(root, "config", "user.name", "Fast Loop Test")
        write(root / "README.md", "base\n")
        git(root, "add", ".")
        git(root, "commit", "-m", "base")
        git(root, "branch", "side")

        worktree = Path(raw) / "side-worktree"
        subprocess.run(
            ["git", "worktree", "add", str(worktree), "side"],
            cwd=root,
            check=True,
            stdout=subprocess.DEVNULL,
        )
        assert mod.git_common_dir(root) == mod.git_common_dir(worktree)

        off, off_env = mod.configure_rust_cache(
            root,
            "off",
            which=lambda _: "/fake/sccache",
            environ={},
        )
        assert off.enabled is False
        assert "RUSTC_WRAPPER" not in off_env

        auto_missing, auto_missing_env = mod.configure_rust_cache(
            root,
            "auto",
            which=lambda _: None,
            environ={},
        )
        assert auto_missing.enabled is False
        assert "RUSTC_WRAPPER" not in auto_missing_env

        try:
            mod.configure_rust_cache(
                root,
                "require",
                which=lambda _: None,
                environ={},
            )
        except RuntimeError as exc:
            assert "required but not installed" in str(exc)
        else:
            raise AssertionError("require mode must reject a missing sccache")

        fake = str(Path(raw) / "bin" / "sccache")
        root_cache, root_env = mod.configure_rust_cache(
            root,
            "auto",
            which=lambda _: fake,
            environ={},
        )
        worktree_cache, worktree_env = mod.configure_rust_cache(
            worktree,
            "auto",
            which=lambda _: fake,
            environ={},
        )
        assert root_cache.enabled is True
        assert root_cache.directory == worktree_cache.directory
        assert root_env["RUSTC_WRAPPER"] == fake
        assert worktree_env["RUSTC_WRAPPER"] == fake
        assert root_env["SCCACHE_DIR"] == worktree_env["SCCACHE_DIR"]
        assert "CARGO_TARGET_DIR" not in root_env
        assert "CARGO_INCREMENTAL" not in root_env


def test_run_receipt_records_timings_and_cache_state() -> None:
    with tempfile.TemporaryDirectory() as raw:
        root = Path(raw)
        git(root, "init", "-b", "main")
        git(root, "config", "user.email", "fast-loop@example.invalid")
        git(root, "config", "user.name", "Fast Loop Test")
        write(root / "README.md", "base\n")
        git(root, "add", ".")
        git(root, "commit", "-m", "base")

        rust_cache, env = mod.configure_rust_cache(
            root,
            "off",
            which=lambda _: None,
            environ={},
        )
        receipt_path = Path("receipts") / "fast-loop.json"
        exit_code, receipt = mod.execute_plan(
            root,
            [
                mod.Check(
                    "synthetic",
                    (sys.executable, "-c", "print('ok')"),
                    "receipt contract",
                )
            ],
            paths=["README.md"],
            mode="edit",
            budget_seconds=60.0,
            rust_cache=rust_cache,
            env=env,
            standalone_receipt=receipt_path,
            write_history=True,
        )
        assert exit_code == 0
        assert receipt["schema"] == "chaptera.dev-fast-loop-run.v1"
        assert receipt["success"] is True
        assert receipt["head_sha"]
        assert receipt["changed_paths"] == ["README.md"]
        assert receipt["rust_cache"]["enabled"] is False
        assert receipt["checks"][0]["kind"] == "synthetic"
        assert receipt["checks"][0]["seconds"] >= 0.0

        standalone = json.loads((root / receipt_path).read_text(encoding="utf-8"))
        assert standalone["schema"] == receipt["schema"]

        history = root / ".chaptera-local" / "dev-fast-loop" / "history.jsonl"
        history_rows = [
            json.loads(line)
            for line in history.read_text(encoding="utf-8").splitlines()
            if line.strip()
        ]
        assert len(history_rows) == 1
        assert history_rows[0]["head_sha"] == receipt["head_sha"]


def test_workspace_manifest_edit_uses_metadata_not_full_workspace_compile() -> None:
    with tempfile.TemporaryDirectory() as raw:
        root = Path(raw)
        write(
            root / "Cargo.toml",
            "[workspace]\nmembers=['crates/foo']\nresolver='2'\n",
        )
        write(
            root / "crates/foo/Cargo.toml",
            "[package]\nname='foo'\nversion='0.1.0'\nedition='2024'\n",
        )
        write(root / "crates/foo/src/lib.rs", "pub fn value() -> u8 { 1 }\n")

        edit = mod.plan_for_paths(root, ["Cargo.toml"], mode="edit")
        edit_commands = commands(edit)
        assert (
            "cargo",
            "metadata",
            "--manifest-path",
            "Cargo.toml",
            "--no-deps",
            "--format-version",
            "1",
        ) in edit_commands
        assert not any(
            command[:2] == ("cargo", "fmt") for command in edit_commands
        )
        assert not any(
            command[:2] == ("cargo", "check") and "--workspace" in command
            for command in edit_commands
        )

        feature = mod.plan_for_paths(root, ["Cargo.toml"], mode="feature")
        feature_commands = commands(feature)
        assert (
            "cargo",
            "check",
            "--manifest-path",
            "Cargo.toml",
            "--workspace",
        ) in feature_commands


def test_pub_editor_facade_only_diff_skips_unfiltered_lib_test() -> None:
    with tempfile.TemporaryDirectory() as raw:
        root = Path(raw)
        git(root, "init", "-b", "main")
        git(root, "config", "user.email", "fast-loop@example.invalid")
        git(root, "config", "user.name", "Fast Loop Test")
        write(
            root / mod.COMPONENT_REGISTRY_PATH,
            """{
  "schema": "chaptera.dev-fast-components.v1",
  "rules": [
    {
      "name": "pub-editor facade/core",
      "paths": ["vendor/producer-a/crates/pub-editor/src/lib.rs"],
      "commands": [["cargo", "test", "--manifest-path", "vendor/producer-a/Cargo.toml", "-p", "pub-editor", "--lib"]]
    },
    {
      "name": "pub-editor registered module",
      "paths": ["vendor/producer-a/crates/pub-editor/src/registered.rs"],
      "commands": [["cargo", "test", "--manifest-path", "vendor/producer-a/Cargo.toml", "-p", "pub-editor", "registered_behavior", "--lib"]]
    }
  ]
}
""",
        )
        write(
            root / "vendor/producer-a/Cargo.toml",
            "[workspace]\nmembers=['crates/pub-editor']\nresolver='2'\n",
        )
        write(
            root / mod.PUB_EDITOR_PACKAGE_MANIFEST_PATH,
            "[package]\nname='pub-editor'\nversion='0.1.0'\nedition='2024'\n",
        )
        lib = root / mod.PUB_EDITOR_LIB_PATH
        write(lib, "mod owned;\npub use owned::Owned;\npub struct Core;\n")
        write(
            root / "vendor/producer-a/crates/pub-editor/src/owned.rs",
            "pub struct Owned;\n",
        )
        git(root, "add", ".")
        git(root, "commit", "-m", "base")
        git(root, "checkout", "-b", "feature")

        # Same-stem integration coverage permits the edit-mode facade fast path.
        write(
            lib,
            "mod owned;\nmod extra;\npub use owned::Owned;\npub use extra::Extra;\npub struct Core;\n",
        )
        write(
            root / "vendor/producer-a/crates/pub-editor/src/extra.rs",
            "pub struct Extra;\n",
        )
        write(
            root / "vendor/producer-a/crates/pub-editor/tests/extra.rs",
            "#[test] fn smoke() {}\n",
        )
        git(root, "add", ".")
        git(root, "commit", "-m", "facade")

        paths = mod.discover_changed_paths(root, base="main", head="HEAD")
        checks = mod.plan_for_paths(root, paths, mode="edit")
        narrowed = mod.narrow_checks_for_diff(
            root,
            paths,
            checks,
            base="main",
            head="HEAD",
            mode="edit",
        )
        actual = commands(narrowed)
        assert mod.PUB_EDITOR_UNFILTERED_LIB_TEST not in actual
        assert (
            "cargo",
            "test",
            "--manifest-path",
            "vendor/producer-a/crates/pub-editor/Cargo.toml",
            "--test",
            "extra",
            "--no-fail-fast",
        ) in actual

        # Feature mode remains the full package unit loop even for facade-only diffs.
        feature_checks = mod.plan_for_paths(root, paths, mode="feature")
        feature_narrowed = mod.narrow_checks_for_diff(
            root,
            paths,
            feature_checks,
            base="main",
            head="HEAD",
            mode="feature",
        )
        assert mod.PUB_EDITOR_UNFILTERED_LIB_TEST in commands(feature_narrowed)

        # Explicit component-registry coverage is also sufficient.
        write(
            lib,
            "mod owned;\nmod extra;\nmod registered;\npub use owned::Owned;\npub use extra::Extra;\npub use registered::Registered;\npub struct Core;\n",
        )
        write(
            root / "vendor/producer-a/crates/pub-editor/src/registered.rs",
            "pub struct Registered;\n",
        )
        git(root, "add", ".")
        git(root, "commit", "-m", "registered facade")

        paths = mod.discover_changed_paths(root, base="main", head="HEAD")
        checks = mod.plan_for_paths(root, paths, mode="edit")
        narrowed = mod.narrow_checks_for_diff(
            root,
            paths,
            checks,
            base="main",
            head="HEAD",
            mode="edit",
        )
        actual = commands(narrowed)
        assert mod.PUB_EDITOR_UNFILTERED_LIB_TEST not in actual
        assert (
            "cargo",
            "test",
            "--manifest-path",
            "vendor/producer-a/Cargo.toml",
            "-p",
            "pub-editor",
            "registered_behavior",
            "--lib",
        ) in actual

        # A facade-only lib.rs plus an uncovered semantic module must fail closed.
        write(
            lib,
            "mod owned;\nmod extra;\nmod registered;\nmod uncovered;\npub use owned::Owned;\npub use extra::Extra;\npub use registered::Registered;\npub use uncovered::Uncovered;\npub struct Core;\n",
        )
        write(
            root / "vendor/producer-a/crates/pub-editor/src/uncovered.rs",
            "pub struct Uncovered;\n",
        )
        paths = mod.discover_changed_paths(root, base="main", head=None)
        checks = mod.plan_for_paths(root, paths, mode="edit")
        narrowed = mod.narrow_checks_for_diff(
            root,
            paths,
            checks,
            base="main",
            head=None,
            mode="edit",
        )
        assert mod.PUB_EDITOR_UNFILTERED_LIB_TEST in commands(narrowed)

        # Any substantive core body change retains the full lib test.
        write(
            lib,
            "mod owned;\nmod extra;\nmod registered;\npub use owned::Owned;\npub use extra::Extra;\npub use registered::Registered;\npub struct Core;\npub fn core_value() -> u8 { 1 }\n",
        )
        checks = mod.plan_for_paths(root, [mod.PUB_EDITOR_LIB_PATH], mode="edit")
        narrowed = mod.narrow_checks_for_diff(
            root,
            [mod.PUB_EDITOR_LIB_PATH],
            checks,
            base="main",
            head=None,
            mode="edit",
        )
        assert mod.PUB_EDITOR_UNFILTERED_LIB_TEST in commands(narrowed)

        # Build-metadata changes never inherit the facade-only narrowing.
        write(
            lib,
            "mod owned;\nmod extra;\nmod registered;\npub use owned::Owned;\npub use extra::Extra;\npub use registered::Registered;\npub struct Core;\n",
        )
        write(
            root / mod.PUB_EDITOR_PACKAGE_MANIFEST_PATH,
            "[package]\nname='pub-editor'\nversion='0.1.1'\nedition='2024'\n",
        )
        metadata_paths = [mod.PUB_EDITOR_LIB_PATH, mod.PUB_EDITOR_PACKAGE_MANIFEST_PATH]
        checks = mod.plan_for_paths(root, metadata_paths, mode="edit")
        narrowed = mod.narrow_checks_for_diff(
            root,
            metadata_paths,
            checks,
            base="main",
            head=None,
            mode="edit",
        )
        assert mod.PUB_EDITOR_UNFILTERED_LIB_TEST in commands(narrowed)

def test_feature_mode_adds_package_unit_tests_once() -> None:
    with tempfile.TemporaryDirectory() as raw:
        root = Path(raw)
        write(root / "crates/foo/Cargo.toml", "[package]\nname='foo'\nversion='0.1.0'\nedition='2024'\n")
        write(root / "crates/foo/src/lib.rs", "pub fn a() {}\n")
        write(root / "crates/foo/src/other.rs", "pub fn b() {}\n")
        checks = mod.plan_for_paths(
            root,
            ["crates/foo/src/lib.rs", "crates/foo/src/other.rs"],
            mode="feature",
        )
        unit = [check for check in checks if check.kind == "rust-unit-tests"]
        assert len(unit) == 1
        assert unit[0].command[-1] == "--lib"


def test_feature_mode_discovers_binary_targets_and_respects_disabled_tests() -> None:
    with tempfile.TemporaryDirectory() as raw:
        root = Path(raw)
        manifest = root / "app/Cargo.toml"
        write(manifest, """[package]
name='app'
version='0.1.0'
edition='2024'
[[bin]]
name='editor'
path='src/main.rs'
[[bin]]
name='receipt'
path='src/bin/receipt.rs'
test=false
""")
        write(root / "app/src/main.rs", "fn main() {}\n")
        write(root / "app/src/bin/receipt.rs", "fn main() {}\n")
        write(root / "app/src/bin/probe/main.rs", "fn main() {}\n")
        checks = mod.plan_for_paths(root, ["app/src/main.rs"], mode="feature")
        unit = [check for check in checks if check.kind == "rust-unit-tests"]
        assert [check.command for check in unit] == [(
            "cargo", "test", "--manifest-path", "app/Cargo.toml",
            "--bin", "editor", "--bin", "probe",
        )]

        write(manifest, "[package]\nname='app'\nversion='0.1.0'\nautobins=false\n")
        assert mod.rust_unit_test_selectors(manifest) == ()
        write(manifest, "[package]\nname='app'\nversion='0.1.0'\n")
        assert mod.rust_unit_test_selectors(manifest) == (
            "--bin", "app", "--bin", "probe", "--bin", "receipt",
        )


def test_explicit_library_keeps_library_routing_without_default_path() -> None:
    with tempfile.TemporaryDirectory() as raw:
        manifest = Path(raw) / "Cargo.toml"
        write(manifest, "[package]\nname='app'\nversion='0.1.0'\nautolib=false\n[lib]\npath='core.rs'\n")
        assert mod.rust_unit_test_selectors(manifest) == ("--lib",)
        write(manifest, "[package]\nname='app'\nversion='0.1.0'\n[lib]\ntest=false\n")
        assert mod.rust_unit_test_selectors(manifest) == ()


def main() -> None:
    test_discover_changed_paths()
    test_plan_routing_and_dedupe()
    test_same_stem_rust_source_discovers_exact_integration_test()
    test_component_registry_routes_aliases_dedupes_and_ignores_unrelated()
    test_component_registry_malformed_fails_closed()
    test_rust_cache_auto_off_require_and_git_common_dir()
    test_run_receipt_records_timings_and_cache_state()
    test_workspace_manifest_edit_uses_metadata_not_full_workspace_compile()
    test_pub_editor_facade_only_diff_skips_unfiltered_lib_test()
    test_feature_mode_adds_package_unit_tests_once()
    test_feature_mode_discovers_binary_targets_and_respects_disabled_tests()
    test_explicit_library_keeps_library_routing_without_default_path()
    print("dev fast loop tests: ok")


if __name__ == "__main__":
    main()
