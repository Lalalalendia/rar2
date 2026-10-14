#!/usr/bin/env python3
"""Source-safe transport for a dedicated, unprivileged Chaptera Windows dev runner.

This is not a code-writing agent, a native Publisher oracle, or a release gate.
GitHub checkout supplies an immutable trusted main SHA. The existing fast loop
owns all impact selection and check semantics.
"""
from __future__ import annotations

import argparse
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import time


SCHEMA = "chaptera.local-dev-transport.v1"
OWNER = "Lalalalendia"
REPOSITORY = "Lalalalendia/rar2"
MAIN_REF = "refs/heads/main"
SHA_RE = re.compile(r"^[0-9a-f]{40}$")
MAX_CHANGED_PATHS = 300
TIMEOUT_SECONDS = 18 * 60


class TransportError(RuntimeError):
    """Only a bounded error identifier may be serialized or printed."""


def require_sha(value: object) -> str:
    if not isinstance(value, str) or SHA_RE.fullmatch(value) is None:
        raise TransportError("invalid_sha")
    return value


def admit(*, repository: str, ref: str, event: str, actor: str) -> None:
    if repository != REPOSITORY or ref != MAIN_REF:
        raise TransportError("untrusted_repository_or_ref")
    if event not in {"schedule", "workflow_dispatch", "push"}:
        raise TransportError("untrusted_event")
    if event == "workflow_dispatch" and actor != OWNER:
        raise TransportError("untrusted_dispatch_actor")
    # Push events are admitted only through the main-only workflow trigger.
    # Never accept pull_request, issue_comment, or arbitrary head refs.


def git(repo: Path, *args: str) -> str:
    try:
        result = subprocess.run(
            ["git", *args], cwd=repo, capture_output=True, check=False,
            text=True, encoding="utf-8", errors="replace", timeout=30,
        )
    except (OSError, subprocess.TimeoutExpired) as exc:
        raise TransportError("git_unavailable") from exc
    if result.returncode != 0:
        raise TransportError("git_command_failed")
    return result.stdout.strip()


def load_state(path: Path) -> dict:
    if not path.exists():
        return {"schema": SCHEMA, "last_seen": None, "last_success": None}
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, ValueError) as exc:
        raise TransportError("invalid_state") from exc
    if not isinstance(data, dict) or data.get("schema") != SCHEMA:
        raise TransportError("invalid_state_schema")
    for key in ("last_seen", "last_success"):
        if data.get(key) is not None:
            require_sha(data[key])
    return data


def atomic_json(path: Path, payload: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp_path = None
    try:
        with tempfile.NamedTemporaryFile(
            "w", encoding="utf-8", dir=path.parent, prefix=".chaptera-",
            suffix=".json", delete=False,
        ) as stream:
            tmp_path = Path(stream.name)
            json.dump(payload, stream, sort_keys=True, indent=2)
            stream.write("\n")
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(tmp_path, path)
    finally:
        if tmp_path is not None:
            tmp_path.unlink(missing_ok=True)


def baseline(repo: Path, head: str, last_success: str | None) -> tuple[str, str]:
    if last_success is not None and last_success != head:
        require_sha(last_success)
        git(repo, "cat-file", "-e", f"{last_success}^{{commit}}")
        try:
            result = subprocess.run(
                ["git", "merge-base", "--is-ancestor", last_success, head],
                cwd=repo, check=False, capture_output=True, timeout=30,
            )
        except (OSError, subprocess.TimeoutExpired) as exc:
            raise TransportError("merge_base_unavailable") from exc
        if result.returncode != 0:
            raise TransportError("non_fast_forward_main")
        return last_success, "since_last_success"
    try:
        parent = git(repo, "rev-parse", f"{head}^")
    except TransportError as exc:
        raise TransportError("missing_bootstrap_parent") from exc
    return require_sha(parent), "bootstrap_last_commit_only"


def changed_paths(repo: Path, base: str, head: str) -> list[str]:
    try:
        data = subprocess.run(
            ["git", "diff", "--name-only", "--diff-filter=ACMRTD", "-z", base, head, "--"],
            cwd=repo, capture_output=True, check=False, timeout=45,
        )
    except (OSError, subprocess.TimeoutExpired) as exc:
        raise TransportError("git_diff_unavailable") from exc
    if data.returncode != 0:
        raise TransportError("git_diff_failed")
    names = sorted(set(item.decode("utf-8", "surrogateescape")
                       for item in data.stdout.split(b"\x00") if item))
    if len(names) > MAX_CHANGED_PATHS:
        raise TransportError("change_set_too_large")
    if any(name.startswith("-") or "\n" in name or "\r" in name for name in names):
        raise TransportError("unsafe_changed_path")
    return names


def run_checks(repo: Path, head: str, paths: list[str], mode: str, logs_root: Path,
               run_id: str) -> tuple[int, int]:
    logs = logs_root / "private-logs"
    logs.mkdir(parents=True, exist_ok=True)
    captured_receipt = logs / f"fast-{head[:12]}-{run_id}.json"
    if mode == "smoke":
        command = [sys.executable, "tools/test_dev_fast_loop.py"]
    else:
        command = [
            sys.executable, "tools/dev_fast_loop.py", "--paths", *paths,
            "--mode", mode, "--run", "--no-history", "--receipt",
            str(captured_receipt),
        ]
    env = os.environ.copy()
    env["CARGO_BUILD_JOBS"] = "2"
    env["RUST_TEST_THREADS"] = "2"
    env["PYTHONDONTWRITEBYTECODE"] = "1"
    try:
        result = subprocess.run(
            command, cwd=repo, env=env, capture_output=True, check=False,
            timeout=TIMEOUT_SECONDS,
        )
        code = result.returncode
        private_output = (result.stdout + b"\n--- stderr ---\n" + result.stderr)[-65536:]
    except subprocess.TimeoutExpired as exc:
        code = 124
        private_output = (exc.stdout or b"")[-32768:] + b"\nTIMEOUT\n"
    except OSError:
        code = 127
        private_output = b"process unavailable\n"
    (logs / f"output-{head[:12]}-{run_id}.log").write_bytes(private_output)

    check_count = 0
    if mode != "smoke" and captured_receipt.exists():
        try:
            payload = json.loads(captured_receipt.read_text(encoding="utf-8"))
            if (payload.get("head_sha") != head
                    or payload.get("success") != (code == 0)):
                raise TransportError("fast_loop_receipt_mismatch")
            check_count = len(payload.get("checks", []))
        except (ValueError, OSError, TypeError) as exc:
            raise TransportError("fast_loop_receipt_invalid") from exc
    elif mode != "smoke" and code == 0:
        raise TransportError("fast_loop_receipt_missing")
    return code, check_count


def execute(args: argparse.Namespace, environ: dict[str, str] | None = None) -> tuple[int, dict]:
    env = os.environ if environ is None else environ
    started = time.monotonic()
    report: dict = {
        "schema": SCHEMA,
        "observed_at": datetime.now(timezone.utc).isoformat(),
        "source_safe": True,
        "private_logs_uploaded": False,
        "product_acceptance": False,
        "repository": REPOSITORY,
        "head_sha": args.sha,
        "run_id": args.run_id,
        "profile": args.profile,
        "status": "BLOCKED",
        "error_code": None,
        "base_sha": None,
        "coverage": None,
        "changed_path_count": 0,
        "check_count": 0,
        "exit_code": 2,
    }
    code = 2
    try:
        admit(
            repository=env.get("GITHUB_REPOSITORY", ""),
            ref=env.get("GITHUB_REF", ""),
            event=env.get("GITHUB_EVENT_NAME", ""),
            actor=env.get("GITHUB_ACTOR", ""),
        )
        head = require_sha(args.sha)
        if env.get("GITHUB_SHA") != head or not re.fullmatch(r"[0-9]{1,20}", args.run_id):
            raise TransportError("workflow_identity_mismatch")
        repo = args.repo.resolve(strict=True)
        root = Path(git(repo, "rev-parse", "--show-toplevel")).resolve(strict=True)
        if root != repo:
            raise TransportError("checkout_root_mismatch")
        if git(repo, "rev-parse", "HEAD") != head:
            raise TransportError("checkout_sha_mismatch")
        if git(repo, "status", "--porcelain", "--untracked-files=normal"):
            raise TransportError("dirty_checkout")
        state_root = args.state_root.resolve()
        receipt_path = args.receipt.resolve()
        logs_root = args.logs_root.resolve()
        if (state_root == repo or state_root.is_relative_to(repo)
                or receipt_path == repo or receipt_path.is_relative_to(repo)
                or logs_root == repo or logs_root.is_relative_to(repo)):
            raise TransportError("outputs_inside_checkout")
        state_path = state_root / "checkpoint.json"
        state = load_state(state_path)
        if state["last_success"] == head and not args.force:
            report.update(status="UNCHANGED", exit_code=0, coverage="same_head")
            return 0, report
        old, coverage = baseline(repo, head, state["last_success"])
        paths = changed_paths(repo, old, head)
        report.update(base_sha=old, coverage=coverage, changed_path_count=len(paths))
        if not paths:
            code, count = 0, 0
            status = "NO_SOURCE_CHANGE"
        else:
            code, count = run_checks(repo, head, paths, args.profile, logs_root, args.run_id)
            status = "PASS" if code == 0 else "FAIL"
        report.update(status=status, check_count=count, exit_code=code)
        state["last_seen"] = head
        if code == 0:
            state["last_success"] = head
        atomic_json(state_path, state)
    except (TransportError, OSError, ValueError) as exc:
        report["error_code"] = str(exc) if isinstance(exc, TransportError) else "io_or_value_error"
        report["status"] = "BLOCKED"
        report["exit_code"] = 2
        code = 2
    finally:
        report["duration_seconds"] = round(time.monotonic() - started, 3)
    return code, report


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", type=Path, required=True)
    parser.add_argument("--state-root", type=Path, required=True)
    parser.add_argument("--logs-root", type=Path, required=True)
    parser.add_argument("--receipt", type=Path, required=True)
    parser.add_argument("--sha", required=True)
    parser.add_argument("--run-id", required=True)
    parser.add_argument("--profile", choices=("edit", "feature", "smoke"), default="edit")
    parser.add_argument("--force", action="store_true")
    args = parser.parse_args()
    exit_code, report = execute(args)
    atomic_json(args.receipt, report)
    print(f"CHAPTERA_LOCAL_DEV status={report['status']} sha={args.sha[:12]} "
          f"checks={report['check_count']} exit={exit_code}")
    return exit_code


if __name__ == "__main__":
    raise SystemExit(main())
