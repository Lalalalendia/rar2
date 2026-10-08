#!/usr/bin/env python3
"""Fail-closed required check for dynamically selected pull-request workflows.

A single always-registered check can be required on a protected main branch.
It waits for all PR workflows selected by the base revision's path rules,
rather than only checking whichever jobs have already registered.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path
from typing import Iterable

from source_fanout_budget import admitted_by_patterns, path_matches, extract_pull_request_paths

SELF_WORKFLOW = ".github/workflows/required-merge-gate.yml"
SUCCESS = {"success"}
PR_ACTIONS_DEFAULT = {"opened", "synchronize", "reopened"}


class GateError(RuntimeError):
    pass


def git(repo: Path, *args: str) -> str:
    return subprocess.check_output(["git", "-C", str(repo), *args], text=True).strip()


def workflow_paths(repo: Path, base_sha: str) -> list[str]:
    return [
        p for p in git(repo, "ls-tree", "-r", "--name-only", base_sha, "--", ".github/workflows").splitlines()
        if p.endswith((".yml", ".yaml"))
    ]


def base_file(repo: Path, base_sha: str, path: str) -> str:
    return git(repo, "show", f"{base_sha}:{path}")


def _top_level_on(text: str) -> list[str]:
    lines = text.splitlines()
    start = next((i for i, line in enumerate(lines) if re.match(r"^on\s*:", line)), None)
    if start is None:
        return []
    first = lines[start].split(":", 1)[1].strip()
    if first:
        return [first]
    result: list[str] = []
    for line in lines[start + 1:]:
        if line.strip() and not line.startswith((" ", "\t", "#")):
            break
        result.append(line)
    return result


def _pr_block(text: str) -> tuple[bool, list[str]]:
    lines = _top_level_on(text)
    if not lines:
        return False, []
    if len(lines) == 1 and not lines[0].startswith((" ", "\t", "#")):
        events = re.split(r"[\s,\[\]]+", lines[0])
        return "pull_request" in events, []
    for i, line in enumerate(lines):
        m = re.match(r"^  pull_request\s*:(.*)$", line)
        if m:
            if m.group(1).strip() in ("", "null", "~"):
                result: list[str] = []
                for child in lines[i + 1:]:
                    if re.match(r"^  \S", child):
                        break
                    result.append(child)
                return True, result
            raise GateError("unsupported pull_request trigger syntax")
    return False, []


def _list_attr(lines: list[str], attr: str) -> list[str] | None:
    for i, line in enumerate(lines):
        m = re.match(r"^    " + re.escape(attr) + r"\s*:(.*)$", line)
        if not m:
            continue
        value = m.group(1).split("#", 1)[0].strip()
        if value:
            if not value.startswith("[") or not value.endswith("]"):
                raise GateError(f"unsupported {attr} inline syntax: {value}")
            content = value[1:-1].strip()
            return [s.strip().strip("\"'") for s in content.split(",") if s.strip()]
        values: list[str] = []
        for child in lines[i + 1:]:
            if re.match(r"^    \S", child):
                break
            cm = re.match(r"^\s+-\s+(.+?)\s*(?:#.*)?$", child)
            if cm:
                values.append(cm.group(1).strip().strip("\"'"))
            elif child.strip() and not child.lstrip().startswith("#"):
                raise GateError(f"unsupported {attr} entry: {child}")
        if not values:
            raise GateError(f"empty {attr}")
        return values
    return None


def workflow_selected(text: str, paths: Iterable[str], action: str, target: str) -> bool:
    is_pr, block = _pr_block(text)
    if not is_pr:
        return False
    types = _list_attr(block, "types")
    if action not in (set(types) if types is not None else PR_ACTIONS_DEFAULT):
        return False
    branches = _list_attr(block, "branches")
    if branches is not None and not any(path_matches(p, target) for p in branches):
        return False
    ignore_branches = _list_attr(block, "branches-ignore")
    if ignore_branches is not None and any(path_matches(p, target) for p in ignore_branches):
        return False
    patterns = extract_pull_request_paths(text)
    if patterns is not None:
        return any(admitted_by_patterns(p, patterns) for p in paths)
    ignored = _list_attr(block, "paths-ignore")
    if ignored is not None:
        return any(not any(path_matches(ignore, p) for ignore in ignored) for p in paths)
    return True


def expected_workflows(repo: Path, base_sha: str, paths: list[str], action: str, target: str) -> set[str]:
    expected: set[str] = set()
    for path in workflow_paths(repo, base_sha):
        if path == SELF_WORKFLOW:
            continue
        text = base_file(repo, base_sha, path)
        try:
            selected = workflow_selected(text, paths, action, target)
        except Exception as exc:
            raise GateError(f"cannot classify {path}: {exc}") from exc
        if selected:
            expected.add(path)
    if ".github/workflows/public-boundary-guard.yml" not in expected:
        raise GateError("unconditional public-boundary guard is not selected")
    return expected


def request_json(repository: str, route: str, token: str) -> object:
    url = f"https://api.github.com/repos/{repository}/{route}"
    req = urllib.request.Request(url, headers={
        "Accept": "application/vnd.github+json",
        "Authorization": f"Bearer {token}",
        "X-GitHub-Api-Version": "2022-11-28",
        "User-Agent": "chaptera-merge-authority",
    })
    try:
        with urllib.request.urlopen(req, timeout=30) as response:
            return json.load(response)
    except urllib.error.HTTPError as exc:
        raise GateError(f"GitHub API {route}: HTTP {exc.code}") from exc


def changed_paths(repository: str, pr_number: int, token: str) -> list[str]:
    paths: set[str] = set()
    for page in range(1, 32):
        items = request_json(repository, f"pulls/{pr_number}/files?per_page=100&page={page}", token)
        if not isinstance(items, list):
            raise GateError("pull request files API returned non-list")
        for item in items:
            paths.add(item["filename"])
            if item.get("previous_filename"):
                paths.add(item["previous_filename"])
        if len(items) < 100:
            return sorted(paths)
    raise GateError("PR file listing exceeds safe 3000-file limit")


def exact_head_runs(repository: str, branch: str, sha: str, token: str) -> dict[str, dict]:
    candidates: dict[str, dict] = {}
    query = urllib.parse.urlencode({"event": "pull_request", "branch": branch, "per_page": 100})
    for page in range(1, 32):
        data = request_json(repository, f"actions/runs?{query}&page={page}", token)
        if not isinstance(data, dict) or not isinstance(data.get("workflow_runs"), list):
            raise GateError("workflow runs API returned malformed data")
        items = data["workflow_runs"]
        for item in items:
            if item.get("head_sha") != sha:
                continue
            path = item.get("path", "").split("@", 1)[0]
            if not path.startswith(".github/workflows/") or path == SELF_WORKFLOW:
                continue
            previous = candidates.get(path)
            key = (item.get("created_at", ""), item.get("run_attempt", 1), item.get("id", 0))
            if previous is None or key > (
                previous.get("created_at", ""), previous.get("run_attempt", 1), previous.get("id", 0)
            ):
                candidates[path] = item
        if len(items) < 100:
            return candidates
    raise GateError("workflow run listing exceeds safe pagination limit")


def classify(expected: set[str], runs: dict[str, dict]) -> tuple[list[str], list[str]]:
    failures: list[str] = []
    pending: list[str] = []
    for path in sorted(expected | set(runs)):
        run = runs.get(path)
        if run is None:
            pending.append(f"{path}: not registered")
        elif run.get("status") != "completed":
            pending.append(f"{path}: {run.get('status', 'unknown')}")
        elif run.get("conclusion") not in SUCCESS:
            failures.append(f"{path}: {run.get('conclusion', 'unknown')}")
    return failures, pending


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--repository", required=True)
    parser.add_argument("--pr-number", type=int, required=True)
    parser.add_argument("--head-sha", required=True)
    parser.add_argument("--head-branch", required=True)
    parser.add_argument("--base-sha", required=True)
    parser.add_argument("--base-branch", default="main")
    parser.add_argument("--action", required=True)
    parser.add_argument("--deadline-seconds", type=int, default=6400)
    parser.add_argument("--poll-seconds", type=int, default=25)
    args = parser.parse_args()
    token = os.environ.get("GH_TOKEN")
    if not token:
        raise GateError("GH_TOKEN missing; cannot verify required checks")
    paths = changed_paths(args.repository, args.pr_number, token)
    if not paths:
        raise GateError("cannot prove a nonempty PR diff")
    expected = expected_workflows(Path("."), args.base_sha, paths, args.action, args.base_branch)
    print(f"Required merge gate: head={args.head_sha} paths={len(paths)} expected={len(expected)}", flush=True)
    for name in sorted(expected):
        print(f"  EXPECT {name}", flush=True)
    limit = time.monotonic() + args.deadline_seconds
    previous = None
    while True:
        runs = exact_head_runs(args.repository, args.head_branch, args.head_sha, token)
        failures, pending = classify(expected, runs)
        state = (tuple(failures), tuple(pending))
        if state != previous:
            print(f"  state: failed={len(failures)} pending={len(pending)}", flush=True)
            for item in (failures + pending)[:40]:
                print(f"    {item}", flush=True)
            previous = state
        if failures:
            raise GateError("failed PR workflow(s) on exact head")
        if not pending:
            print("Required merge gate: PASS; all applicable workflows green", flush=True)
            return 0
        if time.monotonic() >= limit:
            raise GateError(f"timed out waiting for {len(pending)} required PR workflow(s)")
        time.sleep(args.poll_seconds)


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (GateError, subprocess.CalledProcessError, KeyError) as error:
        print(f"::error::{error}", file=sys.stderr)
        sys.exit(1)
