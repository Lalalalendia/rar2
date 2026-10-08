#!/usr/bin/env python3
from __future__ import annotations

import argparse
import ast
import io
import json
import re
import subprocess
import tarfile
from functools import lru_cache
from pathlib import Path
from typing import Iterable

BUDGET_PATH = "tools/ci/source_fanout_budget.json"
ROOT_SHARD_DIR = "tools/ci/source_fanout_roots/"
WORKFLOW_DIR = ".github/workflows/"
WORKFLOW_SUFFIXES = (".yml", ".yaml")
BUDGET_SCHEMA_V1 = "chaptera.source-fanout-budget.v1"
BUDGET_SCHEMA_V2 = "chaptera.source-fanout-budget.v2"
ROOT_SHARD_SCHEMA = "chaptera.source-fanout-root.v1"


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


def _json_object(raw: str, *, path: str, revision: str) -> dict:
    try:
        data = json.loads(raw)
    except json.JSONDecodeError as exc:
        raise BudgetError(f"{path}: invalid JSON at {revision}: {exc}") from exc
    if not isinstance(data, dict):
        raise BudgetError(f"{path}: top-level value must be an object")
    return data


def _read_root_shards(repo: Path, revision: str) -> tuple[dict, dict]:
    shared: dict[str, dict[str, int]] = {}
    shard_paths: dict[str, str] = {}
    for shard_path in sorted(
        path
        for path in git_paths(repo, revision, ROOT_SHARD_DIR)
        if path.endswith(".json")
    ):
        raw = git_show(repo, revision, shard_path)
        if raw is None:
            raise BudgetError(f"{shard_path}: disappeared while reading {revision}")
        data = _json_object(raw, path=shard_path, revision=revision)
        if set(data) != {"schema", "path", "max_lines"}:
            raise BudgetError(
                f"{shard_path}: root shard must contain exactly schema/path/max_lines"
            )
        if data.get("schema") != ROOT_SHARD_SCHEMA:
            raise BudgetError(f"{shard_path}: unsupported root shard schema")
        source = data.get("path")
        ceiling = data.get("max_lines")
        if (
            not isinstance(source, str)
            or not source.endswith(".rs")
            or source.startswith("/")
            or ".." in Path(source).parts
        ):
            raise BudgetError(f"{shard_path}: invalid shared-root path: {source!r}")
        if not isinstance(ceiling, int) or ceiling < 1:
            raise BudgetError(f"{shard_path}: max_lines must be a positive integer")
        if source in shared:
            raise BudgetError(
                f"duplicate shared-root path {source}: "
                f"{shard_paths[source]} and {shard_path}"
            )
        shared[source] = {"max_lines": ceiling}
        shard_paths[source] = shard_path
    return shared, shard_paths


def read_budget(repo: Path, revision: str) -> dict | None:
    raw = git_show(repo, revision, BUDGET_PATH)
    if raw is None:
        return None
    data = _json_object(raw, path=BUDGET_PATH, revision=revision)
    schema = data.get("schema")
    if schema == BUDGET_SCHEMA_V1:
        result = dict(data)
        result["_format"] = "v1"
        result["_root_shards"] = {}
        return result
    if schema == BUDGET_SCHEMA_V2:
        if "shared_roots" in data:
            raise BudgetError(
                f"{BUDGET_PATH}: v2 policy must not inline shared_roots"
            )
        shared, shards = _read_root_shards(repo, revision)
        result = dict(data)
        result["shared_roots"] = shared
        result["_format"] = "v2"
        result["_root_shards"] = shards
        return result
    raise BudgetError(f"{BUDGET_PATH}: unsupported source fanout budget schema")


def validate_budget_shape(budget: dict) -> None:
    schema = budget.get("schema")
    if schema not in {BUDGET_SCHEMA_V1, BUDGET_SCHEMA_V2}:
        raise BudgetError("unsupported source fanout budget schema")
    limit = budget.get("new_leaf_max_conditional_workflows")
    if not isinstance(limit, int) or limit < 0:
        raise BudgetError("new_leaf_max_conditional_workflows must be a non-negative integer")
    roots = budget.get("tracked_roots")
    if not isinstance(roots, list) or not roots or not all(
        isinstance(x, str) and x for x in roots
    ):
        raise BudgetError("tracked_roots must be a non-empty string array")
    shared = budget.get("shared_roots")
    if not isinstance(shared, dict):
        raise BudgetError("shared_roots must be an object")
    for path, config in shared.items():
        if not isinstance(path, str) or not path.endswith(".rs"):
            raise BudgetError(f"shared root must be an .rs path: {path!r}")
        if schema == BUDGET_SCHEMA_V2 and not any(
            path.startswith(prefix) for prefix in roots
        ):
            raise BudgetError(f"shared root is outside tracked_roots: {path}")
        if not isinstance(config, dict):
            raise BudgetError(f"shared root config must be an object: {path}")
        max_lines = config.get("max_lines")
        if not isinstance(max_lines, int) or max_lines < 1:
            raise BudgetError(f"shared root max_lines must be a positive integer: {path}")
    if schema == BUDGET_SCHEMA_V2:
        shards = budget.get("_root_shards")
        if not isinstance(shards, dict) or set(shards) != set(shared):
            raise BudgetError("v2 shared roots must map one-to-one to root shards")


def validate_budget_ratchet(base: dict | None, head: dict) -> list[str]:
    validate_budget_shape(head)
    if base is None:
        return []
    validate_budget_shape(base)
    errors: list[str] = []

    base_schema = base["schema"]
    head_schema = head["schema"]
    if base_schema == BUDGET_SCHEMA_V2 and head_schema == BUDGET_SCHEMA_V1:
        errors.append("source fanout budget schema downgrade is forbidden")

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

    if base_schema == BUDGET_SCHEMA_V1 and head_schema == BUDGET_SCHEMA_V2:
        if base_limit != head_limit:
            errors.append(
                "v1 -> v2 migration must preserve new leaf workflow budget exactly"
            )
        if base["tracked_roots"] != head["tracked_roots"]:
            errors.append("v1 -> v2 migration must preserve tracked_roots exactly")
        if base_shared != head_shared:
            errors.append("v1 -> v2 migration must preserve all shared-root ceilings exactly")
        return errors

    new_shared = sorted(set(head_shared) - set(base_shared))
    if new_shared:
        errors.append(
            "new shared-root exemptions are forbidden in ordinary PRs: "
            + ", ".join(new_shared)
        )
    removed_shared = sorted(set(base_shared) - set(head_shared))
    if removed_shared:
        errors.append(
            "shared-root shard removed: " + ", ".join(removed_shared)
        )

    for path in sorted(set(base_shared) & set(head_shared)):
        old = base_shared[path]["max_lines"]
        new = head_shared[path]["max_lines"]
        if new > old:
            errors.append(f"monolith line ceiling increased for {path}: {old} -> {new}")

    if base_schema == BUDGET_SCHEMA_V2 and head_schema == BUDGET_SCHEMA_V2:
        base_shards = base.get("_root_shards", {})
        head_shards = head.get("_root_shards", {})
        for path in sorted(set(base_shared) & set(head_shared)):
            if base_shards.get(path) != head_shards.get(path):
                errors.append(
                    f"shared-root shard path changed for {path}: "
                    f"{base_shards.get(path)} -> {head_shards.get(path)}"
                )

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


@lru_cache(maxsize=None)
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
    try:
        archive = subprocess.check_output(
            [
                "git",
                "-C",
                str(repo),
                "archive",
                "--format=tar",
                revision,
                WORKFLOW_DIR.rstrip("/"),
            ],
            stderr=subprocess.STDOUT,
        )
    except subprocess.CalledProcessError as exc:
        raise BudgetError(exc.output.decode(errors="replace").strip()) from exc

    with tarfile.open(fileobj=io.BytesIO(archive), mode="r:") as bundle:
        for member in bundle.getmembers():
            path = member.name
            if not member.isfile() or not path.endswith(WORKFLOW_SUFFIXES):
                continue
            extracted = bundle.extractfile(member)
            if extracted is None:
                continue
            text = extracted.read().decode("utf-8")
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


def changed_workflow_paths(repo: Path, base_revision: str, head_revision: str) -> set[str]:
    output = git(repo, "diff", "--name-status", f"{base_revision}...{head_revision}")
    changed: set[str] = set()
    for line in output.splitlines():
        fields = line.split("\t")
        if len(fields) < 2:
            continue
        for path in fields[1:]:
            if path.startswith(WORKFLOW_DIR) and path.endswith(WORKFLOW_SUFFIXES):
                changed.add(path)
    return changed


def workflow_patterns_at(repo: Path, revision: str, path: str) -> list[str] | None:
    text = git_show(repo, revision, path)
    if text is None:
        return None
    return extract_pull_request_paths(text, workflow=path)


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
    existing_files = base_files & head_files
    new_files = head_files - base_files

    if base_budget is not None:
        for path in sorted(set(base_budget["shared_roots"]) - set(shared_roots)):
            if path in head_files:
                errors.append(
                    f"shared-root exemption removed while source still exists: {path}"
                )

    # Existing files may transfer ownership between conditional workflows, but
    # their total conditional-workflow fanout must not increase. This keeps the
    # ratchet compatible with semantic source splits where a new leaf replaces
    # unrelated old consumers with explicit true owners.
    workflow_deltas: dict[str, dict[str, list[str]]] = {
        path: {"added": [], "removed": []} for path in existing_files
    }
    for workflow in sorted(changed_workflow_paths(repo, base_revision, head_revision)):
        before_patterns = workflow_patterns_at(repo, base_revision, workflow)
        after_patterns = workflow_patterns_at(repo, head_revision, workflow)
        for path in sorted(existing_files):
            before = (
                admitted_by_patterns(path, before_patterns)
                if before_patterns is not None
                else False
            )
            after = (
                admitted_by_patterns(path, after_patterns)
                if after_patterns is not None
                else False
            )
            if after and not before:
                workflow_deltas[path]["added"].append(workflow)
            elif before and not after:
                workflow_deltas[path]["removed"].append(workflow)

    for path in sorted(existing_files):
        added = workflow_deltas[path]["added"]
        removed = workflow_deltas[path]["removed"]
        net = len(added) - len(removed)
        if net > 0:
            detail = [
                "FANOUT REGRESSION",
                f"source: {path}",
                f"net new conditional consumers: {net}",
                "added:",
                *[f"  - {workflow}" for workflow in added],
            ]
            if removed:
                detail.extend(
                    ["removed:", *[f"  - {workflow}" for workflow in removed]]
                )
            errors.append("\n".join(detail))

    # Only genuinely new leaf files need a full head fanout count. One git archive
    # loads all workflow YAML at once; ordinary source-only PRs with no new files skip it.
    new_leaves = sorted(path for path in new_files if path not in shared_roots)
    if new_leaves:
        head_workflows = workflows_at(repo, head_revision)
        for path in new_leaves:
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
        elif actual < ceiling:
            errors.append(
                "MONOLITH CEILING NOT RATCHETED\n"
                f"source: {path}\n"
                f"recorded ceiling: {ceiling}\n"
                f"head: {actual}\n"
                "lower max_lines to the exact current size in the same PR"
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

    changed_workflows = changed_workflow_paths(repo, args.base, args.head)
    base_files = tracked_rust_paths(repo, args.base, head_budget["tracked_roots"])
    head_files = tracked_rust_paths(repo, args.head, head_budget["tracked_roots"])
    print(
        "source fanout budget: ok "
        f"({len(head_files)} tracked Rust files, "
        f"{len(head_files - base_files)} new Rust files, "
        f"{len(changed_workflows)} changed workflows audited)"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
