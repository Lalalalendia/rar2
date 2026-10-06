#!/usr/bin/env python3
from __future__ import annotations

import importlib.util
from pathlib import Path
import subprocess
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


def test_pass_receipt_binds_exact_feature_head() -> None:
    with tempfile.TemporaryDirectory() as raw:
        root = Path(raw)
        git(root, "init", "-b", "main")
        git(root, "config", "user.email", "fast-loop@example.invalid")
        git(root, "config", "user.name", "Fast Loop Test")
        write(root / "tool.py", "VALUE = 1\n")
        git(root, "add", ".")
        git(root, "commit", "-m", "base")
        base_sha = subprocess.check_output(
            ["git", "rev-parse", "HEAD"], cwd=root, text=True
        ).strip()

        git(root, "checkout", "-b", "feature")
        write(root / "tool.py", "VALUE = 2\n")
        git(root, "add", "tool.py")
        git(root, "commit", "-m", "feature")
        head_sha = subprocess.check_output(
            ["git", "rev-parse", "HEAD"], cwd=root, text=True
        ).strip()

        checks = mod.plan_for_paths(root, ["tool.py"], mode="feature")
        receipt = mod.build_pass_receipt(
            root,
            base="main",
            head=head_sha,
            mode="feature",
            paths=["tool.py"],
            checks=checks,
        )
        assert receipt["schema"] == "chaptera.dev-fast-loop-receipt.v1"
        assert receipt["status"] == "PASS"
        assert receipt["mode"] == "feature"
        assert receipt["base_sha"] == base_sha
        assert receipt["source_head_sha"] == head_sha
        assert receipt["explicit_head"] is True
        assert receipt["changed_paths"] == ["tool.py"]


def main() -> None:
    test_discover_changed_paths()
    test_plan_routing_and_dedupe()
    test_same_stem_rust_source_discovers_exact_integration_test()
    test_component_registry_routes_aliases_dedupes_and_ignores_unrelated()
    test_component_registry_malformed_fails_closed()
    test_feature_mode_adds_package_unit_tests_once()
    test_pass_receipt_binds_exact_feature_head()
    print("dev fast loop tests: ok")


if __name__ == "__main__":
    main()
