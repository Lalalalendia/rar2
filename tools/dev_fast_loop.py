#!/usr/bin/env python3
from __future__ import annotations

import argparse
import fnmatch
import json
from pathlib import Path
import shlex
import subprocess
import sys
import time
import tomllib
from typing import Iterable, NamedTuple, Sequence


PYTHON_SUFFIX = ".py"
RUST_SUFFIX = ".rs"
NODE_SUFFIXES = {".js", ".mjs", ".cjs"}
WORKFLOW_PREFIX = ".github/workflows/"
COMPONENT_REGISTRY_PATH = "tools/dev_fast_loop_components.json"


class Check(NamedTuple):
    kind: str
    command: tuple[str, ...]
    reason: str

    def display(self) -> str:
        return shlex.join(self.command)


class ComponentRule(NamedTuple):
    name: str
    paths: tuple[str, ...]
    commands: tuple[tuple[str, ...], ...]


def load_component_registry(root: Path) -> list[ComponentRule]:
    path = root / COMPONENT_REGISTRY_PATH
    if not path.exists():
        return []
    try:
        payload = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise RuntimeError(f"invalid fast-loop component registry: {exc}") from exc

    if not isinstance(payload, dict) or payload.get("schema") != "chaptera.dev-fast-components.v1":
        raise RuntimeError("invalid fast-loop component registry schema")
    raw_rules = payload.get("rules")
    if not isinstance(raw_rules, list):
        raise RuntimeError("fast-loop component registry rules must be a list")

    rules: list[ComponentRule] = []
    for index, raw_rule in enumerate(raw_rules):
        if not isinstance(raw_rule, dict):
            raise RuntimeError(f"component rule {index} must be an object")
        name = raw_rule.get("name")
        patterns = raw_rule.get("paths")
        commands = raw_rule.get("commands")
        if not isinstance(name, str) or not name.strip():
            raise RuntimeError(f"component rule {index} requires a non-empty name")
        if (
            not isinstance(patterns, list)
            or not patterns
            or not all(isinstance(item, str) and item for item in patterns)
        ):
            raise RuntimeError(f"component rule {name!r} requires non-empty string paths")
        if not isinstance(commands, list) or not commands:
            raise RuntimeError(f"component rule {name!r} requires commands")

        normalized_commands: list[tuple[str, ...]] = []
        for command in commands:
            if (
                not isinstance(command, list)
                or not command
                or not all(isinstance(item, str) and item for item in command)
            ):
                raise RuntimeError(f"component rule {name!r} has an invalid argv command")
            normalized_commands.append(tuple(command))

        rules.append(
            ComponentRule(
                name=name,
                paths=tuple(patterns),
                commands=tuple(normalized_commands),
            )
        )
    return rules


def component_registry_checks(root: Path, paths: Iterable[str]) -> list[Check]:
    normalized = tuple(paths)
    checks: list[Check] = []
    for rule in load_component_registry(root):
        matched = any(
            fnmatch.fnmatchcase(path, pattern)
            for path in normalized
            for pattern in rule.paths
        )
        if not matched:
            continue
        for command in rule.commands:
            checks.append(
                Check(
                    "component-micro-test",
                    command,
                    f"component registry: {rule.name}",
                )
            )
    return checks


def _run_lines(root: Path, args: Sequence[str], *, check: bool = True) -> list[str]:
    result = subprocess.run(
        list(args),
        cwd=root,
        check=check,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    return [line.strip() for line in result.stdout.splitlines() if line.strip()]


def _git_ref_exists(root: Path, ref: str) -> bool:
    return subprocess.run(
        ["git", "rev-parse", "--verify", "--quiet", ref],
        cwd=root,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        check=False,
    ).returncode == 0


def resolve_base_ref(root: Path, base: str) -> str | None:
    for candidate in (base, f"origin/{base}"):
        if _git_ref_exists(root, candidate):
            return candidate
    return None


def resolve_commit_sha(root: Path, ref: str) -> str:
    try:
        lines = _run_lines(root, ["git", "rev-parse", "--verify", f"{ref}^{{commit}}"])
    except subprocess.CalledProcessError as exc:
        raise RuntimeError(f"cannot resolve commit ref {ref!r}") from exc
    if len(lines) != 1:
        raise RuntimeError(f"commit ref {ref!r} did not resolve to one SHA")
    return lines[0]


def build_pass_receipt(
    root: Path,
    *,
    base: str,
    head: str | None,
    mode: str,
    paths: Sequence[str],
    checks: Sequence[Check],
) -> dict:
    resolved_base = resolve_base_ref(root, base)
    if resolved_base is None:
        raise RuntimeError(f"cannot resolve base ref {base!r}")
    return {
        "schema": "chaptera.dev-fast-loop-receipt.v1",
        "status": "PASS",
        "mode": mode,
        "base_ref": base,
        "base_sha": resolve_commit_sha(root, resolved_base),
        "source_head_sha": resolve_commit_sha(root, head or "HEAD"),
        "explicit_head": head is not None,
        "changed_paths": list(paths),
        "checks": [
            {"kind": check.kind, "command": list(check.command), "reason": check.reason}
            for check in checks
        ],
        "deep_acceptance_scheduled": False,
    }


def discover_changed_paths(root: Path, *, base: str = "main", head: str | None = None) -> list[str]:
    """Return committed + staged + unstaged + untracked paths for the edit loop."""
    paths: set[str] = set()
    resolved_base = resolve_base_ref(root, base)

    if head:
        if resolved_base is None:
            raise RuntimeError(f"cannot resolve base ref {base!r}")
        paths.update(_run_lines(root, ["git", "diff", "--name-only", f"{resolved_base}...{head}"]))
    else:
        if resolved_base is not None and _git_ref_exists(root, "HEAD"):
            paths.update(_run_lines(root, ["git", "diff", "--name-only", f"{resolved_base}...HEAD"]))
        paths.update(_run_lines(root, ["git", "diff", "--name-only"]))
        paths.update(_run_lines(root, ["git", "diff", "--cached", "--name-only"]))
        paths.update(_run_lines(root, ["git", "ls-files", "--others", "--exclude-standard"]))

    return sorted(_normalize_path(path) for path in paths if path.strip())


def _normalize_path(path: str) -> str:
    value = Path(path).as_posix()
    while value.startswith("./"):
        value = value[2:]
    return value


def _manifest_table(manifest: Path) -> dict:
    try:
        return tomllib.loads(manifest.read_text(encoding="utf-8"))
    except (OSError, tomllib.TOMLDecodeError):
        return {}


def manifest_kind(manifest: Path) -> str | None:
    parsed = _manifest_table(manifest)
    if "package" in parsed:
        return "package"
    if "workspace" in parsed:
        return "workspace"
    return None


def nearest_package_manifest(root: Path, repo_path: str) -> Path | None:
    target = root / repo_path
    cursor = target.parent if target.name != "Cargo.toml" else target.parent
    root = root.resolve()
    cursor = cursor.resolve()
    while True:
        manifest = cursor / "Cargo.toml"
        if manifest.exists() and manifest_kind(manifest) == "package":
            return manifest
        if cursor == root:
            return None
        if root not in cursor.parents:
            return None
        cursor = cursor.parent


def nearest_workspace_manifest(root: Path, repo_path: str) -> Path | None:
    target = root / repo_path
    cursor = target.parent if target.name != "Cargo.toml" else target.parent
    root = root.resolve()
    cursor = cursor.resolve()
    while True:
        manifest = cursor / "Cargo.toml"
        if manifest.exists() and manifest_kind(manifest) == "workspace":
            return manifest
        if cursor == root:
            return None
        if root not in cursor.parents:
            return None
        cursor = cursor.parent


def _rel(root: Path, path: Path) -> str:
    return path.resolve().relative_to(root.resolve()).as_posix()


def python_companion_tests(root: Path, repo_path: str) -> list[str]:
    path = root / repo_path
    if not path.exists() or path.suffix != PYTHON_SUFFIX:
        return []
    candidates: list[Path] = []
    if path.name.startswith("test_"):
        candidates.append(path)
    else:
        candidates.append(path.with_name(f"test_{path.name}"))
        if path.parent.as_posix().endswith("tools/ci"):
            candidates.append(path.parent / f"test_{path.name}")
    return sorted({_rel(root, item) for item in candidates if item.exists()})


def node_companion_tests(root: Path, repo_path: str) -> list[str]:
    path = root / repo_path
    if not path.exists() or path.suffix not in NODE_SUFFIXES:
        return []
    candidates: list[Path] = []
    if ".test." in path.name:
        candidates.append(path)
    else:
        candidates.append(path.with_name(f"{path.stem}.test{path.suffix}"))
    return sorted({_rel(root, item) for item in candidates if item.exists()})


def rust_same_stem_integration_target(
    root: Path,
    manifest: Path,
    repo_path: str,
) -> str | None:
    """Return tests/<source-stem>.rs when the changed Rust source owns one."""
    source = (root / repo_path).resolve()
    package_root = manifest.resolve().parent
    try:
        relative = source.relative_to(package_root)
    except ValueError:
        return None
    if source.suffix != RUST_SUFFIX or not relative.parts or relative.parts[0] != "src":
        return None
    candidate = package_root / "tests" / f"{source.stem}.rs"
    return source.stem if candidate.is_file() else None


def _dedupe(checks: Iterable[Check]) -> list[Check]:
    seen: set[tuple[str, ...]] = set()
    result: list[Check] = []
    for check in checks:
        if check.command in seen:
            continue
        seen.add(check.command)
        result.append(check)
    return result


def plan_for_paths(root: Path, paths: Iterable[str], *, mode: str = "edit") -> list[Check]:
    root = root.resolve()
    normalized = sorted({_normalize_path(path) for path in paths if path})
    checks: list[Check] = component_registry_checks(root, normalized)

    existing_python = [path for path in normalized if path.endswith(PYTHON_SUFFIX) and (root / path).exists()]
    if existing_python:
        checks.append(Check(
            "python-syntax",
            (sys.executable, "-m", "py_compile", *existing_python),
            "compile changed Python files",
        ))
        for test in sorted({test for path in existing_python for test in python_companion_tests(root, path)}):
            checks.append(Check("python-micro-test", (sys.executable, test), f"companion test for changed Python code: {test}"))

    existing_node = [path for path in normalized if Path(path).suffix in NODE_SUFFIXES and (root / path).exists()]
    for path in existing_node:
        checks.append(Check("node-syntax", ("node", "--check", path), f"syntax-check changed Node file: {path}"))
    for test in sorted({test for path in existing_node for test in node_companion_tests(root, path)}):
        checks.append(Check("node-micro-test", ("node", "--test", test), f"companion test for changed Node code: {test}"))

    if any(path.startswith(WORKFLOW_PREFIX) and Path(path).suffix in {".yml", ".yaml"} for path in normalized):
        guard = root / "tools/ci/check_workflow_yaml_syntax.rb"
        if guard.exists():
            checks.append(Check(
                "workflow-yaml",
                ("ruby", "tools/ci/check_workflow_yaml_syntax.rb"),
                "validate workflow YAML before any hosted run",
            ))

    package_manifests: set[str] = set()
    workspace_manifests: set[str] = set()
    rust_test_targets: set[tuple[str, str]] = set()

    for path in normalized:
        disk_path = root / path
        suffix = Path(path).suffix
        if suffix == RUST_SUFFIX and disk_path.exists():
            manifest = nearest_package_manifest(root, path)
            if manifest:
                manifest_rel = _rel(root, manifest)
                package_manifests.add(manifest_rel)
                parts = Path(path).parts
                if "tests" in parts and Path(path).parent.name == "tests":
                    rust_test_targets.add((manifest_rel, Path(path).stem))
                companion = rust_same_stem_integration_target(root, manifest, path)
                if companion:
                    rust_test_targets.add((manifest_rel, companion))
        elif Path(path).name == "Cargo.toml" and disk_path.exists():
            kind = manifest_kind(disk_path)
            if kind == "package":
                package_manifests.add(path)
            elif kind == "workspace":
                workspace_manifests.add(path)
        elif Path(path).name == "Cargo.lock":
            workspace = nearest_workspace_manifest(root, path)
            if workspace:
                workspace_manifests.add(_rel(root, workspace))

    for manifest in sorted(workspace_manifests):
        checks.append(Check(
            "rustfmt-workspace",
            ("cargo", "fmt", "--manifest-path", manifest, "--all", "--", "--check"),
            f"workspace manifest/lock changed: {manifest}",
        ))
        checks.append(Check(
            "rust-check-workspace",
            ("cargo", "check", "--manifest-path", manifest, "--workspace"),
            f"workspace manifest/lock changed: {manifest}",
        ))

    for manifest in sorted(package_manifests):
        checks.append(Check(
            "rustfmt-package",
            ("cargo", "fmt", "--manifest-path", manifest, "--", "--check"),
            f"format affected Rust package: {manifest}",
        ))
        checks.append(Check(
            "rust-check-package",
            ("cargo", "check", "--manifest-path", manifest),
            f"compile affected Rust package: {manifest}",
        ))
        if mode == "feature":
            checks.append(Check(
                "rust-unit-tests",
                ("cargo", "test", "--manifest-path", manifest, "--lib"),
                f"feature-loop unit tests for affected Rust package: {manifest}",
            ))

    for manifest, target in sorted(rust_test_targets):
        checks.append(Check(
            "rust-micro-test",
            ("cargo", "test", "--manifest-path", manifest, "--test", target, "--no-fail-fast"),
            f"exact integration micro-test for affected Rust component: {target}",
        ))

    priority = {
        "python-syntax": 10,
        "node-syntax": 10,
        "workflow-yaml": 10,
        "rustfmt-workspace": 10,
        "rustfmt-package": 10,
        "python-micro-test": 20,
        "node-micro-test": 20,
        "rust-check-workspace": 30,
        "rust-check-package": 30,
        "component-micro-test": 35,
        "rust-micro-test": 40,
        "rust-unit-tests": 50,
    }
    return sorted(_dedupe(checks), key=lambda item: (priority.get(item.kind, 99), item.display()))


def print_plan(paths: list[str], checks: list[Check], *, mode: str, as_json: bool = False) -> None:
    if as_json:
        print(json.dumps({
            "schema": "chaptera.dev-fast-loop.v1",
            "mode": mode,
            "changed_paths": paths,
            "checks": [
                {"kind": check.kind, "command": list(check.command), "reason": check.reason}
                for check in checks
            ],
            "deep_acceptance_scheduled": False,
        }, indent=2, sort_keys=True))
        return

    print(f"Chaptera fast loop ({mode})")
    print(f"changed paths: {len(paths)}")
    for path in paths:
        print(f"  - {path}")
    print(f"fast checks: {len(checks)}")
    for index, check in enumerate(checks, start=1):
        print(f"  {index}. [{check.kind}] {check.display()}")
        print(f"     {check.reason}")
    if not checks:
        print("  (no supported code/workflow changes detected)")
    print("deep/product acceptance scheduled: no")


def execute_plan(root: Path, checks: list[Check], *, budget_seconds: float) -> int:
    started = time.monotonic()
    for index, check in enumerate(checks, start=1):
        print(f"[{index}/{len(checks)}] {check.kind}: {check.display()}", flush=True)
        check_started = time.monotonic()
        try:
            result = subprocess.run(check.command, cwd=root, check=False)
        except FileNotFoundError as exc:
            print(f"fast-loop tool missing: {exc.filename}", file=sys.stderr)
            return 127
        elapsed = time.monotonic() - check_started
        print(f"  -> {elapsed:.2f}s", flush=True)
        if result.returncode != 0:
            print(f"fast loop failed at {check.kind} (exit {result.returncode})", file=sys.stderr)
            return result.returncode
    total = time.monotonic() - started
    print(f"fast loop green in {total:.2f}s")
    if total > budget_seconds:
        print(
            f"warning: edit-loop budget exceeded ({total:.2f}s > {budget_seconds:.2f}s); "
            "measure the slow check before adding more coverage",
            file=sys.stderr,
        )
    return 0


def repo_root() -> Path:
    result = subprocess.run(
        ["git", "rev-parse", "--show-toplevel"],
        check=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    return Path(result.stdout.strip()).resolve()


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Plan or execute Chaptera's local diff-to-fast-check development loop."
    )
    parser.add_argument("--base", default="main", help="base ref for committed changes (default: main)")
    parser.add_argument("--head", help="optional explicit head ref; working-tree changes are omitted when set")
    parser.add_argument("--paths", nargs="+", help="explicit repository paths; bypass git diff discovery")
    parser.add_argument("--mode", choices=("edit", "feature"), default="edit")
    action = parser.add_mutually_exclusive_group()
    action.add_argument("--plan", action="store_true", help="print checks without executing them (default)")
    action.add_argument("--run", action="store_true", help="execute the planned fast checks fail-fast")
    parser.add_argument("--json", action="store_true", help="emit the plan as JSON")
    parser.add_argument("--budget-seconds", type=float, default=60.0)
    parser.add_argument(
        "--receipt",
        type=Path,
        help="write a PASS receipt after --run succeeds; intended for exact-head integration-train boarding",
    )
    args = parser.parse_args()
    if args.receipt and not args.run:
        parser.error("--receipt requires --run")

    root = repo_root()
    paths = sorted({_normalize_path(path) for path in args.paths}) if args.paths else discover_changed_paths(
        root, base=args.base, head=args.head
    )
    checks = plan_for_paths(root, paths, mode=args.mode)
    print_plan(paths, checks, mode=args.mode, as_json=args.json)
    if args.run:
        code = execute_plan(root, checks, budget_seconds=args.budget_seconds)
        if code == 0 and args.receipt:
            receipt = build_pass_receipt(
                root,
                base=args.base,
                head=args.head,
                mode=args.mode,
                paths=paths,
                checks=checks,
            )
            args.receipt.parent.mkdir(parents=True, exist_ok=True)
            args.receipt.write_text(
                json.dumps(receipt, indent=2, sort_keys=True) + "\n",
                encoding="utf-8",
            )
            print(f"fast-loop receipt: {args.receipt}")
        return code
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
