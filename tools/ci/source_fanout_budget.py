#!/usr/bin/env python3
from __future__ import annotations

import argparse
import ast
import json
import re
import subprocess
from pathlib import Path
from typing import Iterable

BUDGET_PATH = "tools/ci/source_fanout_budget.json"
WORKFLOW_DIR = ".github/workflows/"
WORKFLOW_SUFFIXES = (".yml", ".yaml")


class BudgetError(RuntimeError):
    pass


def git(repo: Path, *args: str) -> str:
    try:
        return subprocess.check_output(
            ["git", "-C", str(repo), *args], text=True, stderr=subprocess.STDOUT
        )
    except subprocess.CalledProcessError as exc:
        raise BudgetError(exc.output.strip() or f"git {' '.join(args)} failed") from exc


def git_show(repo: Path, revision: str, path: str) -> str | None:
    try:
        return subprocess.check_output(
            ["git", "-C", str(repo), "show", f"{revision}:{path}"],
            text=True,
            stderr=subprocess.DEVNULL,
        )
    except subprocess.CalledProcessError:
        return None


def git_paths(repo: Path, revision: str, prefix: str | None = None) -> list[str]:
    args = ["ls-tree", "-r", "--name-only", revision]
    if prefix:
        args.extend(["--", prefix])
    return [line for line in git(repo, *args).splitlines() if line]


def read_budget(repo: Path, revision: str) -> dict | None:
    raw = git_show(repo, revision, BUDGET_PATH)
    if raw is None:
        return None
    try:
        data = json.loads(raw)
    except json.JSONDecodeError as exc:
        raise BudgetError(f"{BUDGET_PATH}: invalid JSON at {revision}: {exc}") from exc
    if not isinstance(data, dict):
        raise BudgetError(f"{BUDGET_PATH}: top-level value must be an object")
    return data


def validate_budget_shape(budget: dict) -> None:
    if budget.get("schema") != "chaptera.source-fanout-budget.v1":
        raise BudgetError("unsupported source fanout budget schema")
    limit = budget.get("new_leaf_max_conditional_workflows")
    if not isinstance(limit, int) or limit < 0:
        raise BudgetError("new_leaf_max_conditional_workflows must be a non-negative integer")
    roots = budget.get("tracked_roots")
    if not isinstance(roots, list) or not roots or not all(isinstance(x, str) and x for x in roots):
        raise BudgetError("tracked_roots must be a non-empty string array")
    shared = budget.get("shared_roots")
    if not isinstance(shared, dict):
        raise BudgetError("shared_roots must be an object")
    for path, config in shared.items():
        if not isinstance(path, str) or not path.endswith(".rs"):
            raise BudgetError(f"shared root must be an .rs path: {path!r}")
        if not isinstance(config, dict):
            raise BudgetError(f"shared root config must be an object: {path}")
        max_lines = config.get("max_lines")
        if not isinstance(max_lines, int) or max_lines < 1:
            raise BudgetError(f"shared root max_lines must be a positive integer: {path}")


def validate_budget_ratchet(base: dict | None, head: dict) -> list[str]:
    validate_budget_shape(head)
    if base is None:
        return []
    validate_budget_shape(base)
    errors: list[str] = []

    base_limit = base["new_leaf_max_conditional_workflows"]
    head_limit = head["new_leaf_max_conditional_workflows"]
    if head_limit > base_limit:
        errors.append(
            f"new leaf workflow budget increased: {base_limit} -> {head_limit}"
        )

    base_tracked = set(base["tracked_roots"])
    head_tracked = set(head["tracked_roots"])
    removed_tracked = sorted(base_tracked - head_tracked)
    if removed_tracked:
        errors.append("tracked roots removed: " + ", ".join(removed_tracked))

    base_shared = base["shared_roots"]
    head_shared = head["shared_roots"]
    new_shared = sorted(set(head_shared) - set(base_shared))
    if new_shared:
        errors.append(
            "new shared-root exemptions are forbidden in ordinary PRs: "
            + ", ".join(new_shared)
        )

    for path, config in base_shared.items():
        if path not in head_shared:
            continue
        old = config["max_lines"]
        new = head_shared[path]["max_lines"]
        if new > old:
            errors.append(f"monolith line ceiling increased for {path}: {old} -> {new}")

    return errors


def _indent(line: str) -> int:
    return len(line) - len(line.lstrip(" "))


def _mapping_key(line: str) -> tuple[str, str] | None:
    stripped = line.strip()
    if not stripped or stripped.startswith("#") or ":" not in stripped:
        return None
    key, value = stripped.split(":", 1)
    key = key.strip()
    if len(key) >= 2 and key[0] == key[-1] and key[0] in {"'", '"'}:
        key = key[1:-1]
    return key, value.strip()


def _strip_unquoted_comment(value: str) -> str:
    quote: str | None = None
    escaped = False
    for index, char in enumerate(value):
        if escaped:
            escaped = False
            continue
        if char == "\\" and quote == '"':
            escaped = True
            continue
        if quote:
            if char == quote:
                quote = None
            continue
        if char in {"'", '"'}:
            quote = char
            continue
        if char == "#" and (index == 0 or value[index - 1].isspace()):
            return value[:index].rstrip()
    return value.strip()


def _yaml_scalar(value: str) -> str:
    value = _strip_unquoted_comment(value.strip())
    if not value:
        raise BudgetError("empty workflow path pattern")
    if value.startswith('"'):
        try:
            parsed = json.loads(value)
        except json.JSONDecodeError as exc:
            raise BudgetError(f"unsupported quoted workflow path: {value}") from exc
        if not isinstance(parsed, str):
            raise BudgetError(f"workflow path must be a string: {value}")
        return parsed
    if value.startswith("'"):
        if len(value) < 2 or not value.endswith("'"):
            raise BudgetError(f"unsupported quoted workflow path: {value}")
        return value[1:-1].replace("''", "'")
    return value


def _inline_paths(value: str) -> list[str]:
    raw = _strip_unquoted_comment(value).strip()
    if not (raw.startswith("[") and raw.endswith("]")):
        raise BudgetError(f"unsupported inline pull_request.paths value: {value}")
    try:
        items = ast.literal_eval(raw)
    except (SyntaxError, ValueError) as exc:
        raise BudgetError(f"unsupported inline pull_request.paths value: {value}") from exc
    if not isinstance(items, (list, tuple)) or not all(isinstance(x, str) for x in items):
        raise BudgetError("inline pull_request.paths must be a string list")
    return list(items)


def extract_pull_request_paths(text: str, *, workflow: str = "<workflow>") -> list[str] | None:
    lines = text.splitlines()
    on_index = None
    on_indent = None
    for index, line in enumerate(lines):
        item = _mapping_key(line)
        if item and item[0] == "on" and _indent(line) == 0:
            if item[1]:
                return None
            on_index = index
            on_indent = 0
            break
    if on_index is None:
        return None

    on_end = len(lines)
    for index in range(on_index + 1, len(lines)):
        line = lines[index]
        if not line.strip() or line.lstrip().startswith("#"):
            continue
        if _indent(line) <= on_indent:
            on_end = index
            break

    pr_index = None
    pr_indent = None
    for index in range(on_index + 1, on_end):
        line = lines[index]
        item = _mapping_key(line)
        if item and item[0] == "pull_request":
            if item[1] not in {"", "null", "~"}:
                return None
            pr_index = index
            pr_indent = _indent(line)
            break
    if pr_index is None or pr_indent is None:
        return None

    pr_end = on_end
    for index in range(pr_index + 1, on_end):
        line = lines[index]
        if not line.strip() or line.lstrip().startswith("#"):
            continue
        if _indent(line) <= pr_indent:
            pr_end = index
            break

    paths_index = None
    paths_indent = None
    inline_value = ""
    for index in range(pr_index + 1, pr_end):
        line = lines[index]
        item = _mapping_key(line)
        if item and item[0] == "paths":
            paths_index = index
            paths_indent = _indent(line)
            inline_value = item[1]
            break
    if paths_index is None or paths_indent is None:
        return None
    if inline_value:
        return _inline_paths(inline_value)

    patterns: list[str] = []
    for index in range(paths_index + 1, pr_end):
        line = lines[index]
        stripped = line.strip()
        if not stripped or stripped.startswith("#"):
            continue
        if _indent(line) <= paths_indent:
            break
        if not stripped.startswith("- "):
            raise BudgetError(
                f"{workflow}: unsupported pull_request.paths entry at line {index + 1}: {stripped}"
            )
        patterns.append(_yaml_scalar(stripped[2:].strip()))
    if not patterns:
        raise BudgetError(f"{workflow}: explicit pull_request.paths is empty")
    return patterns


def glob_regex(pattern: str) -> re.Pattern[str]:
    result = ["^"]
    i = 0
    while i < len(pattern):
        char = pattern[i]
        if char == "*":
            if i + 1 < len(pattern) and pattern[i + 1] == "*":
                i += 2
                if i < len(pattern) and pattern[i] == "/":
                    i += 1
                    result.append("(?:.*/)?")
                else:
                    result.append(".*")
                continue
            result.append("[^/]*")
        elif char == "?":
            result.append("[^/]")
        elif char == "[":
            end = pattern.find("]", i + 1)
            if end < 0:
                result.append(r"\[")
            else:
                content = pattern[i + 1 : end]
                if content.startswith("!"):
                    content = "^" + content[1:]
                elif content.startswith("^"):
                    content = "\\" + content
                content = content.replace("\\", r"\\")
                result.append("[" + content + "]")
                i = end
        else:
            result.append(re.escape(char))
        i += 1
    result.append("$")
    return re.compile("".join(result))


def path_matches(pattern: str, path: str) -> bool:
    return glob_regex(pattern).match(path) is not None


def admitted_by_patterns(path: str, patterns: Iterable[str]) -> bool:
    admitted = False
    saw_positive = False
    for raw in patterns:
        negative = raw.startswith("!")
        pattern = raw[1:] if negative else raw
        if not negative:
            saw_positive = True
        if path_matches(pattern, path):
            admitted = not negative
    return saw_positive and admitted


def workflows_at(repo: Path, revision: str) -> dict[str, list[str]]:
    result: dict[str, list[str]] = {}
    for path in git_paths(repo, revision, WORKFLOW_DIR):
        if not path.endswith(WORKFLOW_SUFFIXES):
            continue
        text = git_show(repo, revision, path)
        if text is None:
            continue
        patterns = extract_pull_request_paths(text, workflow=path)
        if patterns is not None:
            result[path] = patterns
    return result


def tracked_rust_paths(repo: Path, revision: str, roots: Iterable[str]) -> set[str]:
    all_paths = git_paths(repo, revision)
    prefixes = tuple(roots)
    return {
        path
        for path in all_paths
        if path.endswith(".rs") and any(path.startswith(prefix) for prefix in prefixes)
    }


def matching_workflows(path: str, workflows: dict[str, list[str]]) -> set[str]:
    return {
        workflow
        for workflow, patterns in workflows.items()
        if admitted_by_patterns(path, patterns)
    }


def line_count(text: str) -> int:
    return len(text.splitlines())


def evaluate(
    *,
    repo: Path,
    base_revision: str,
    head_revision: str,
    base_budget: dict | None,
    head_budget: dict,
) -> list[str]:
    errors = validate_budget_ratchet(base_budget, head_budget)
    tracked_roots = head_budget["tracked_roots"]
    shared_roots = head_budget["shared_roots"]
    leaf_limit = head_budget["new_leaf_max_conditional_workflows"]

    base_files = tracked_rust_paths(repo, base_revision, tracked_roots)
    head_files = tracked_rust_paths(repo, head_revision, tracked_roots)
    base_workflows = workflows_at(repo, base_revision)
    head_workflows = workflows_at(repo, head_revision)

    if base_budget is not None:
        for path in sorted(set(base_budget["shared_roots"]) - set(shared_roots)):
            if path in head_files:
                errors.append(
                    f"shared-root exemption removed while source still exists: {path}"
                )

    for path in sorted(base_files & head_files):
        before = matching_workflows(path, base_workflows)
        after = matching_workflows(path, head_workflows)
        if len(after) > len(before):
            added = sorted(after - before)
            errors.append(
                "FANOUT REGRESSION\n"
                f"source: {path}\n"
                f"base conditional workflows: {len(before)}\n"
                f"head conditional workflows: {len(after)}\n"
                "new consumers:\n  - "
                + "\n  - ".join(added or ["<count increased without a new named workflow>"])
            )

    for path in sorted(head_files - base_files):
        if path in shared_roots:
            continue
        after = matching_workflows(path, head_workflows)
        if len(after) > leaf_limit:
            errors.append(
                "NEW LEAF EXCEEDS FANOUT BUDGET\n"
                f"source: {path}\n"
                f"conditional workflows: {len(after)}\n"
                f"budget: {leaf_limit}\n"
                "consumers:\n  - "
                + "\n  - ".join(sorted(after))
            )

    for path, config in sorted(shared_roots.items()):
        text = git_show(repo, head_revision, path)
        if text is None:
            errors.append(f"shared root missing from head: {path}")
            continue
        actual = line_count(text)
        ceiling = config["max_lines"]
        if actual > ceiling:
            errors.append(
                "MONOLITH GROWTH REGRESSION\n"
                f"source: {path}\n"
                f"ceiling: {ceiling}\n"
                f"head: {actual}\n"
                "new feature logic must move to an owned module/crate"
            )

    return errors


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--base", required=True)
    parser.add_argument("--head", required=True)
    parser.add_argument("--repo", type=Path, default=Path("."))
    args = parser.parse_args()

    repo = args.repo.resolve()
    base_budget = read_budget(repo, args.base)
    head_budget = read_budget(repo, args.head)
    if head_budget is None:
        raise SystemExit(f"{BUDGET_PATH} is missing from head revision")

    try:
        errors = evaluate(
            repo=repo,
            base_revision=args.base,
            head_revision=args.head,
            base_budget=base_budget,
            head_budget=head_budget,
        )
    except BudgetError as exc:
        raise SystemExit(f"source fanout budget guard error: {exc}") from exc

    if errors:
        raise SystemExit("source fanout budget guard failed:\n\n" + "\n\n".join(errors))

    tracked = tracked_rust_paths(repo, args.head, head_budget["tracked_roots"])
    conditional = workflows_at(repo, args.head)
    worst_path = None
    worst_count = -1
    for path in tracked:
        count = len(matching_workflows(path, conditional))
        if count > worst_count:
            worst_path = path
            worst_count = count
    print(
        "source fanout budget: ok "
        f"({len(tracked)} tracked Rust files, {len(conditional)} conditional workflows, "
        f"worst={worst_count} at {worst_path})"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
