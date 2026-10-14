#!/usr/bin/env python3
"""Local opt-in Chaptera task handoff: test committed work, push one branch, draft PR.

This is a developer command, never a self-hosted Actions workflow. It cannot
claim Notion tasks, create commits, use the Publisher oracle, or merge a PR.
"""
from __future__ import annotations

import argparse
from datetime import datetime, timezone
import json
import os
from pathlib import Path, PurePosixPath
import re
import subprocess
import sys
import tempfile
import time


REPO = "Lalalalendia/rar2"
SCHEMA = "chaptera.local-dev-handoff.v1"
TASK_RE = re.compile(r"^[A-Z][A-Z0-9]*(?:-[A-Z0-9]+){1,15}$")
SHA_RE = re.compile(r"^[0-9a-f]{40}$")
PULL_URL_RE = re.compile(r"^https://github\.com/Lalalalendia/rar2/pull/[0-9]+$")
ORIGINS = {
    "https://github.com/Lalalalendia/rar2.git",
    "https://github.com/Lalalalendia/rar2",
    "git@github.com:Lalalalendia/rar2.git",
    "ssh://git@github.com/Lalalalendia/rar2.git",
}
PATH_PREFIXES = ("apps/web/", "services/editor-api/", "vendor/producer-a/crates/pub-editor/")
SUFFIXES = {".rs", ".py", ".mjs", ".js", ".ts", ".tsx", ".jsx", ".html",
            ".css", ".json", ".toml", ".md", ".yml", ".yaml", ".txt"}
FORBIDDEN_BYTES = (b"-----BEGIN OPENSSH PRIVATE KEY-----",
                   b"-----BEGIN RSA PRIVATE KEY-----",
                   b"-----BEGIN PRIVATE KEY-----", b"sk-proj-", b"ghp_", b"gho_")
MAX_FILES = 120
MAX_FILE_BYTES = 2_000_000


class HandoffError(RuntimeError):
    """An error code that cannot contain a local path or credential."""


def valid_task(task: str) -> str:
    if not TASK_RE.fullmatch(task) or len(task) > 100:
        raise HandoffError("invalid_task_identity")
    return task


def task_branch(task: str) -> str:
    return "localdev/" + valid_task(task).lower()


def validate_origin(value: str) -> None:
    if value not in ORIGINS:
        raise HandoffError("origin_not_canonical")


def run(root: Path, argv: list[str], *, timeout: int = 60) -> subprocess.CompletedProcess:
    try:
        return subprocess.run(argv, cwd=root, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                              timeout=timeout, check=False)
    except (OSError, subprocess.TimeoutExpired) as exc:
        raise HandoffError("command_unavailable_or_timeout") from exc


def git(root: Path, *arguments: str, timeout: int = 60) -> str:
    result = run(root, ["git", *arguments], timeout=timeout)
    if result.returncode != 0:
        raise HandoffError("git_command_failed")
    return result.stdout.decode("utf-8", "replace").strip()


def sha(value: str) -> str:
    if SHA_RE.fullmatch(value) is None:
        raise HandoffError("invalid_git_sha")
    return value


def diff_paths(root: Path, older: str, newer: str) -> list[str]:
    result = run(root, ["git", "diff", "--name-only", "-z", "--diff-filter=ACMRTD",
                        sha(older), sha(newer), "--"])
    if result.returncode != 0:
        raise HandoffError("git_diff_failed")
    paths = sorted({os.fsdecode(p) for p in result.stdout.split(b"\0") if p})
    if len(paths) > MAX_FILES:
        raise HandoffError("too_many_files")
    return paths


def validate_public_delta(root: Path, paths: list[str]) -> None:
    if not paths:
        raise HandoffError("empty_feature_delta")
    for name in paths:
        if ("\r" in name or "\n" in name or "\\" in name or name.startswith("-")
                or name.startswith("/") or "/../" in f"/{name}/"):
            raise HandoffError("unsafe_path")
        if not name.startswith(PATH_PREFIXES):
            raise HandoffError("path_not_local_editor_allowlist")
        if PurePosixPath(name).suffix.lower() not in SUFFIXES:
            raise HandoffError("non_text_source_extension")
        file = root / name
        if file.is_symlink() or not file.resolve().is_relative_to(root):
            raise HandoffError("source_symlink")
        if not file.exists():
            continue  # A source deletion is still an owned change.
        if not file.is_file() or file.stat().st_size > MAX_FILE_BYTES:
            raise HandoffError("source_not_small_regular_file")
        content = file.read_bytes()
        if b"\0" in content:
            raise HandoffError("binary_content")
        if any(marker in content for marker in FORBIDDEN_BYTES):
            raise HandoffError("private_key_or_token_marker")
    # This is not a substitute for human/agent source-review before publication.


def preview(root: Path, task: str, *, fetch_main: bool) -> tuple[str, str, list[str]]:
    branch = task_branch(task)
    if Path(git(root, "rev-parse", "--show-toplevel")).resolve() != root:
        raise HandoffError("wrong_git_root")
    if git(root, "symbolic-ref", "--quiet", "--short", "HEAD") != branch:
        raise HandoffError("wrong_feature_branch")
    if git(root, "status", "--porcelain", "--untracked-files=normal"):
        raise HandoffError("worktree_not_clean")
    validate_origin(git(root, "remote", "get-url", "origin"))
    head = sha(git(root, "rev-parse", "HEAD"))
    if fetch_main:
        git(root, "fetch", "--no-tags", "origin",
            "refs/heads/main:refs/remotes/origin/main", timeout=120)
    main = sha(git(root, "rev-parse", "refs/remotes/origin/main"))
    if main == head:
        raise HandoffError("no_local_change")
    base = sha(git(root, "merge-base", main, head))
    owned = diff_paths(root, base, head)
    upstream = diff_paths(root, base, main)
    if set(owned).intersection(upstream):
        raise HandoffError("intersecting_upstream_drift")
    validate_public_delta(root, owned)
    return head, main, owned


def fast_loop(root: Path, head: str, paths: list[str]) -> int:
    with tempfile.TemporaryDirectory(prefix="chaptera-handoff-") as temporary:
        result_path = Path(temporary) / "fast-receipt.json"
        argv = [
            sys.executable, "tools/dev_fast_loop.py", "--paths", *paths,
            "--mode", "feature", "--run", "--no-history", "--receipt", str(result_path),
        ]
        try:
            result = subprocess.run(argv, cwd=root, stdout=subprocess.PIPE,
                                    stderr=subprocess.PIPE, check=False, timeout=30 * 60)
        except (OSError, subprocess.TimeoutExpired) as exc:
            raise HandoffError("fast_loop_unavailable_or_timeout") from exc
        if result.returncode != 0:
            raise HandoffError("fast_loop_failed")
        try:
            receipt = json.loads(result_path.read_text(encoding="utf-8"))
            if receipt.get("head_sha") != head or receipt.get("success") is not True:
                raise HandoffError("fast_loop_wrong_head_or_result")
            return len(receipt["checks"])
        except (OSError, ValueError, KeyError, TypeError) as exc:
            raise HandoffError("fast_loop_receipt_invalid") from exc


def push_branch(root: Path, branch: str) -> None:
    # Ordinary, non-force push. GitHub rejects branch updates that race or diverge.
    git(root, "push", "--porcelain", "origin", f"HEAD:refs/heads/{branch}", timeout=120)


def ensure_draft_pr(root: Path, branch: str, task: str, head: str) -> str:
    lookup = run(root, ["gh", "pr", "view", branch, "--repo", REPO,
                        "--json", "url", "--jq", ".url"])
    if lookup.returncode == 0:
        url = lookup.stdout.decode("utf-8", "replace").strip()
        if not PULL_URL_RE.fullmatch(url):
            raise HandoffError("untrusted_existing_pr_url")
        return url
    body = ("Task: " + task + "\nExact local commit: " + head
            + "\nLocal feature fast-loop PASS; product acceptance and merge remain separate.")
    command = ["gh", "pr", "create", "--repo", REPO, "--draft", "--base", "main",
               "--head", branch, "--title", f"{task}: local developer handoff",
               "--body", body]
    outcome = run(root, command, timeout=90)
    if outcome.returncode != 0:
        raise HandoffError("draft_pr_creation_failed")
    url = outcome.stdout.decode("utf-8", "replace").strip()
    if not PULL_URL_RE.fullmatch(url):
        raise HandoffError("untrusted_created_pr_url")
    return url


def write_report(path: Path, root: Path, report: dict) -> None:
    dst = path.resolve()
    if dst == root or dst.is_relative_to(root):
        raise HandoffError("receipt_inside_source_tree")
    dst.parent.mkdir(parents=True, exist_ok=True)
    tmp = None
    try:
        with tempfile.NamedTemporaryFile("w", dir=dst.parent, encoding="utf-8",
                                         prefix=".handoff-", suffix=".json",
                                         delete=False) as stream:
            tmp = Path(stream.name)
            json.dump(report, stream, sort_keys=True, indent=2)
            stream.write("\n")
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(tmp, dst)
    finally:
        if tmp is not None:
            tmp.unlink(missing_ok=True)


def handoff(args: argparse.Namespace) -> tuple[int, dict]:
    started = time.monotonic()
    report = {"schema": SCHEMA, "at": datetime.now(timezone.utc).isoformat(),
              "source_safe": True, "task": args.task, "head_sha": None,
              "main_sha": None, "branch": None, "changed_count": 0,
              "fast_check_count": 0, "pushed": False, "pr_url": None,
              "status": "BLOCKED", "error_code": None}
    code = 2
    try:
        root = args.repo.resolve(strict=True)
        task = valid_task(args.task)
        branch = task_branch(task)
        report["branch"] = branch
        if args.draft_pr and not args.publish:
            raise HandoffError("draft_pr_requires_publish")
        if args.receipt.resolve() == root or args.receipt.resolve().is_relative_to(root):
            raise HandoffError("receipt_inside_source_tree")
        head, main, owned = preview(root, task, fetch_main=args.publish)
        report.update(head_sha=head, main_sha=main, changed_count=len(owned))
        if not args.publish:
            report["status"] = "READY_TO_TEST"
            code = 0
        else:
            report["fast_check_count"] = fast_loop(root, head, owned)
            if git(root, "rev-parse", "HEAD") != head:
                raise HandoffError("head_changed_during_tests")
            if git(root, "status", "--porcelain", "--untracked-files=normal"):
                raise HandoffError("worktree_changed_during_tests")
            push_branch(root, branch)
            report["pushed"] = True
            if args.draft_pr:
                report["pr_url"] = ensure_draft_pr(root, branch, task, head)
            report["status"] = "PUBLISHED"
            code = 0
    except (HandoffError, OSError, ValueError) as exc:
        report["error_code"] = str(exc) if isinstance(exc, HandoffError) else "io_or_argument_error"
        report["status"] = "PUSHED_PR_BLOCKED" if report["pushed"] else "BLOCKED"
        code = 3 if report["pushed"] else 2
    report["duration_seconds"] = round(time.monotonic() - started, 3)
    return code, report


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", type=Path, default=Path.cwd())
    parser.add_argument("--task", required=True)
    parser.add_argument("--receipt", type=Path, required=True)
    parser.add_argument("--publish", action="store_true",
                        help="fetch main, run feature fast-loop, then push task branch")
    parser.add_argument("--draft-pr", action="store_true",
                        help="create/reuse a draft PR through existing local gh authentication")
    args = parser.parse_args()
    code, report = handoff(args)
    try:
        write_report(args.receipt, args.repo.resolve(), report)
    except (OSError, HandoffError):
        print("CHAPTERA_HANDOFF receipt_write_failed", file=sys.stderr)
        return 2
    print(f"CHAPTERA_HANDOFF status={report['status']} "
          f"changed={report['changed_count']} tests={report['fast_check_count']}")
    return code


if __name__ == "__main__":
    raise SystemExit(main())
