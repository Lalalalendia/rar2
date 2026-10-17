#!/usr/bin/env python3
"""Local, resumable PUB salvage batch. The JSON queue is PRIVATE to this machine.

Uses the existing Rescue Salvage Bundle V1 as a per-file child process. Only
summary.json and entries.csv are source-free; NEVER upload queue-private.json,
its source paths, or bundles to public Actions artifacts.

This is an unattended CLI engine, not yet a standalone GUI or a repair writer.
"""
from __future__ import annotations

import argparse
from collections import Counter
from concurrent.futures import ThreadPoolExecutor, wait, FIRST_COMPLETED
from contextlib import contextmanager
import csv
import json
import os
from pathlib import Path
import re
import signal
import subprocess
import sys
import tempfile
import threading
import time
import uuid

from tools import rescue_salvage_bundle_v1 as salvage
from tools import rescue_auto_restore_v1 as autorestore
from tools import rescue_batch_text_v1 as text_batch
from tools import rescue_windows_job_guard as job_guard

SCHEMA = "chaptera.rescue-desktop-batch.v1"
REPORT_SCHEMA = "chaptera.rescue-desktop-batch-report.v1"
CONTROL_SCHEMA = "chaptera.rescue-desktop-batch-control.v1"
SHA = re.compile(r"[a-f0-9]{64}\Z")
# Run identity, not an authorization secret. Controls must specify the exact
# GUI-created run token or they cannot affect that batch incarnation.
RUN_TOKEN = re.compile(r"[A-Za-z0-9_-]{16,128}\Z")
MAX_UNIQUE_SOURCES = 10000
QUEUED = "pending"
STATES = frozenset({QUEUED, "running", "done_partial", "done_unsupported", "failed", "cancelled"})
DONE = frozenset({"done_partial", "done_unsupported"})
REASONS = frozenset({"source_missing", "source_drift", "unsafe_source", "worker_timeout",
                     "worker_error", "bundle_integrity", "auto_restore_integrity",
                     "cancelled", "worker_crash", "text_integrity"})
FIELDS = ("sha256", "source_bytes", "status", "outcome", "reason", "attempts",
          "stream_count", "asset_count", "auto_tier", "auto_index",
          "text_status", "text_count")
REPO_ROOT = Path(__file__).resolve().parents[1]


class BatchError(ValueError):
    pass


def no_symlinks(path: Path) -> None:
    salvage.no_symlink_components(path)


def safe_root(output: Path) -> Path:
    no_symlinks(output)
    if output.exists() and not output.is_dir():
        raise BatchError("output_not_directory")
    return output.resolve()


def atomic_write(path: Path, content: str) -> None:
    if path.is_symlink():
        raise BatchError("unsafe_report_target")
    fd, name = tempfile.mkstemp(prefix=".rescue-atomic-", suffix=".tmp", dir=path.parent)
    try:
        with os.fdopen(fd, "w", encoding="utf-8", newline="") as stream:
            stream.write(content)
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(name, path)
    finally:
        if os.path.exists(name):
            os.unlink(name)


def json_write(path: Path, value: dict) -> None:
    atomic_write(path, json.dumps(value, sort_keys=True, ensure_ascii=True, indent=2) + "\n")


def json_read(path: Path) -> dict:
    if path.is_symlink() or not path.is_file() or path.stat().st_size > 16 * 1024 * 1024:
        raise BatchError("queue_missing_or_unsafe")
    value = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(value, dict):
        raise BatchError("queue_not_object")
    return value


@contextmanager
def output_lock(output: Path):
    """Kernel lock; persist inode so crashed processes cannot strand a lock."""
    lock = output / ".rescue-batch.lock"
    if lock.is_symlink():
        raise BatchError("unsafe_lock")
    try:
        fd = os.open(lock, os.O_RDWR | os.O_CREAT | getattr(os, "O_NOFOLLOW", 0), 0o600)
    except OSError as exc:
        raise BatchError("unsafe_lock") from exc
    locked = False
    try:
        if os.name == "nt":
            import msvcrt
            if os.fstat(fd).st_size == 0:
                os.write(fd, b"\0")
            os.lseek(fd, 0, os.SEEK_SET)
            try:
                msvcrt.locking(fd, msvcrt.LK_NBLCK, 1)
            except OSError as exc:
                raise BatchError("already_running") from exc
        else:
            import fcntl
            try:
                fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
            except OSError as exc:
                raise BatchError("already_running") from exc
        locked = True
        yield
    finally:
        try:
            if locked:
                if os.name == "nt":
                    os.lseek(fd, 0, os.SEEK_SET)
                    msvcrt.locking(fd, msvcrt.LK_UNLCK, 1)
                else:
                    fcntl.flock(fd, fcntl.LOCK_UN)
        finally:
            os.close(fd)


def load_queue(output: Path) -> dict:
    state = json_read(output / "queue-private.json")
    if state.get("schema_version") != SCHEMA or not isinstance(state.get("entries"), list):
        raise BatchError("queue_schema_invalid")
    seen = set()
    for row in state["entries"]:
        if not isinstance(row, dict) or not isinstance(row.get("sha256"), str):
            raise BatchError("queue_row_invalid")
        if (not SHA.fullmatch(row["sha256"]) or row["sha256"] in seen
                or row.get("status") not in STATES or not isinstance(row.get("paths"), list)
                or not row["paths"] or not all(isinstance(p, str) for p in row["paths"])):
            raise BatchError("queue_row_invalid")
        if type(row.get("source_bytes")) is not int or row["source_bytes"] < 1:
            raise BatchError("queue_row_invalid")
        seen.add(row["sha256"])
    return state


def report(state: dict) -> dict:
    entries = []
    for row in sorted(state["entries"], key=lambda entry: entry["sha256"]):
        public = {key: row.get(key) for key in FIELDS}
        done = public["status"] in DONE
        count = row.get("text_count", 0)
        text_status = row.get("text_status", "not_requested")
        # Never echo an arbitrary private queue value in a source-free report.
        if not (done and type(count) is int and 0 <= count <= 16_384
                and text_status in ("not_requested", "unavailable", "verified")
                and ((text_status == "verified") == (count > 0))):
            public["text_status"] = None
            public["text_count"] = 0
        else:
            public["text_status"] = text_status
            public["text_count"] = count
        tier = public["auto_tier"]
        public["auto_tier"] = (
            tier if done and isinstance(tier, str) and tier in autorestore.TIERS else None
        )
        if public["auto_tier"] == "verified_text" and public["text_status"] != "verified":
            public["auto_tier"] = None
        digest = public["sha256"]
        if (public["auto_tier"] is not None and isinstance(digest, str)
                and SHA.fullmatch(digest) and done):
            prefix = "recovered-text-pages" if public["auto_tier"] == "verified_text" else "recovered"
            public["auto_index"] = f"{prefix}/{digest}/index.html"
        else:
            public["auto_index"] = None
        entries.append(public)
    counts = dict(sorted(Counter(row["status"] for row in entries).items()))
    return {"schema_version": REPORT_SCHEMA, "source_free": True,
            "unique_sources": len(entries), "duplicate_paths": state.get("duplicate_paths", 0),
            "rejected_paths": state.get("rejected_paths", {}), "counts": counts,
            "entries": entries}


def persist(output: Path, state: dict) -> None:
    # Queue is strictly PRIVATE. Source-free receipts are distinct files.
    json_write(output / "queue-private.json", state)
    public = report(state)
    json_write(output / "summary.json", public)
    field_headers = ",".join(FIELDS)
    import io
    f = io.StringIO(newline="")
    writer = csv.DictWriter(f, fieldnames=FIELDS)
    writer.writeheader()
    writer.writerows(public["entries"])
    atomic_write(output / "entries.csv", f.getvalue())


def validate_location(source: Path, output: Path) -> None:
    no_symlinks(source)
    a, b = source.resolve(), output.resolve()
    if a.is_dir():
        if b == a or b.is_relative_to(a) or a.is_relative_to(b):
            raise BatchError("output_overlaps_input_tree")
    elif a.is_file():
        if b == a or b == a.parent or a.is_relative_to(b):
            raise BatchError("output_overlaps_input_file")
    else:
        raise BatchError("input_missing")


def iter_pub_paths(source: Path):
    if source.is_file():
        if source.suffix.casefold() == ".pub":
            yield source
        return
    def on_error(_error):
        raise BatchError("input_walk_error")
    for base, dirs, files in os.walk(source, topdown=True, followlinks=False, onerror=on_error):
        good_dirs = []
        for directory in sorted(dirs):
            if not (Path(base) / directory).is_symlink():
                good_dirs.append(directory)
        dirs[:] = good_dirs
        for name in sorted(files):
            candidate = Path(base) / name
            if candidate.suffix.casefold() == ".pub":
                yield candidate


def scan(inputs: list[Path], output: Path, *, append: bool = False) -> dict:
    if not inputs:
        raise BatchError("no_input")
    output = safe_root(output)
    for source in inputs:
        validate_location(source, output)
    output.mkdir(parents=True, exist_ok=True)
    with output_lock(output):
        if (output / "queue-private.json").exists():
            if not append:
                raise BatchError("queue_exists_use_append")
            state = load_queue(output)
            # output_lock proves no other runner is alive. A persisted token is
            # stale after a crashed process; do not strand append/recovery.
            state["run_token"] = None
        else:
            state = {"schema_version": SCHEMA, "entries": [], "duplicate_paths": 0,
                     "rejected_paths": {}, "run_token": None}
        known = {r["sha256"]: r for r in state["entries"]}
        seen_paths = {p for row in state["entries"] for p in row["paths"]}
        reasons = Counter(state.get("rejected_paths", {}))
        for root in inputs:
            for path in iter_pub_paths(root):
                private_path = str(path.absolute())
                if private_path in seen_paths:
                    continue
                try:
                    digest, byte_len = salvage.require_source(path, output / "bundles")
                except (salvage.UnsafeInput, OSError, ValueError) as exc:
                    key = "unreadable_or_invalid"
                    if isinstance(exc, salvage.UnsafeInput):
                        key = "unsafe_or_invalid_pub"
                    reasons[key] += 1
                    continue
                if digest in known:
                    row = known[digest]
                    if row["source_bytes"] != byte_len:
                        raise BatchError("digest_size_conflict")
                    row["paths"].append(private_path)
                    state["duplicate_paths"] += 1
                else:
                    row = {"sha256": digest, "source_bytes": byte_len,
                           "paths": [private_path], "status": QUEUED, "attempts": 0,
                           "outcome": None, "reason": None, "stream_count": 0,
                           "asset_count": 0, "auto_tier": None,
                           "text_status": "not_requested", "text_count": 0}
                    if len(state["entries"]) >= MAX_UNIQUE_SOURCES:
                        raise BatchError("batch_source_limit")
                    state["entries"].append(row)
                    known[digest] = row
                seen_paths.add(private_path)
        state["entries"].sort(key=lambda row: row["sha256"])
        state["rejected_paths"] = dict(sorted(reasons.items()))
        persist(output, state)
        return report(state)


def choose_source(row: dict) -> tuple[Path | None, str | None]:
    missing = True
    for raw in row["paths"]:
        path = Path(raw)
        try:
            no_symlinks(path)
            if not path.is_file() or path.suffix.casefold() != ".pub":
                continue
            missing = False
            if path.stat().st_size != row["source_bytes"]:
                continue
            if salvage.sha_file(path) == row["sha256"]:
                return path, None
        except (OSError, salvage.UnsafeInput):
            continue
    return None, ("source_missing" if missing else "source_drift")


def request_action(output: Path, action: str, *, expected_run_token: str | None = None) -> None:
    output = safe_root(output)
    if action not in ("pause", "cancel"):
        raise BatchError("invalid_action")
    if expected_run_token is not None and not RUN_TOKEN.fullmatch(expected_run_token):
        raise BatchError("invalid_run_token")
    state = load_queue(output)
    token = state.get("run_token")
    if not isinstance(token, str) or not token:
        raise BatchError("no_running_batch")
    if expected_run_token is not None and token != expected_run_token:
        raise BatchError("run_token_mismatch")
    # A late control write contains only the previous run token; a later batch
    # ignores it even if it is written after that new batch starts.
    json_write(output / "control.json", {"schema_version": CONTROL_SCHEMA,
                                         "run_token": token, "action": action})


def control_action(output: Path, token: str) -> str | None:
    path = output / "control.json"
    if not path.exists():
        return None
    try:
        data = json_read(path)
    except (OSError, BatchError, ValueError):
        return None
    if data.get("schema_version") == CONTROL_SCHEMA and data.get("run_token") == token:
        action = data.get("action")
        return action if action in ("pause", "cancel") else None
    return None


def kill_tree(proc: subprocess.Popen) -> None:
    if proc.poll() is not None:
        return
    if os.name == "nt":
        try:
            subprocess.run(["taskkill", "/T", "/F", "/PID", str(proc.pid)],
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                           timeout=6, check=False)
        except (OSError, subprocess.TimeoutExpired):
            proc.kill()
    else:
        try:
            os.killpg(proc.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass


def process_one(row: dict, bundle_root: Path, *, timeout: int, file_timeout: int,
                pub_cli: Path | None, retain_original: bool,
                abort: threading.Event) -> dict:
    source, invalid = choose_source(row)
    if source is None:
        return {"status": "failed", "reason": invalid, "outcome": None,
                "stream_count": 0, "asset_count": 0}
    if abort.is_set():
        return {"status": "cancelled", "reason": "cancelled", "outcome": None,
                "stream_count": 0, "asset_count": 0}
    args = [sys.executable, "-m", "tools.rescue_salvage_bundle_v1", str(source),
            "--output", str(bundle_root), "--timeout", str(timeout)]
    if pub_cli is not None:
        args.extend(["--pub-cli", str(pub_cli)])
    if retain_original:
        args.append("--retain-original")
    options = {"cwd": REPO_ROOT, "stdin": subprocess.DEVNULL,
               "stdout": subprocess.PIPE, "stderr": subprocess.DEVNULL}
    if os.name == "nt":
        options["creationflags"] = subprocess.CREATE_NEW_PROCESS_GROUP
    else:
        options["start_new_session"] = True
    try:
        proc = subprocess.Popen(args, **options)
    except OSError:
        return {"status": "failed", "reason": "worker_error", "outcome": None,
                "stream_count": 0, "asset_count": 0}
    deadline = time.monotonic() + file_timeout
    stdout = b""
    while True:
        try:
            remaining = deadline - time.monotonic()
            if remaining <= 0 or abort.is_set():
                kill_tree(proc)
                try:
                    proc.communicate(timeout=3)
                except subprocess.TimeoutExpired:
                    proc.kill()
                    proc.communicate(timeout=2)
                reason = "cancelled" if abort.is_set() else "worker_timeout"
                return {"status": "cancelled" if abort.is_set() else "failed",
                        "reason": reason, "outcome": None, "stream_count": 0, "asset_count": 0}
            stdout, _ = proc.communicate(timeout=min(0.2, remaining))
            break
        except subprocess.TimeoutExpired:
            continue
        except OSError:
            kill_tree(proc)
            return {"status": "failed", "reason": "worker_crash", "outcome": None,
                    "stream_count": 0, "asset_count": 0}
    if abort.is_set():
        return {"status": "cancelled", "reason": "cancelled", "outcome": None,
                "stream_count": 0, "asset_count": 0}
    try:
        # Output is bounded to one small JSON line in the normal worker protocol.
        if len(stdout) > 16 * 1024:
            raise ValueError("worker_stdout_limit")
        answer = json.loads(stdout)
        if proc.returncode not in (0, 1) or answer.get("status") not in ("created", "reused"):
            raise ValueError("worker_failed")
        if answer.get("source_sha256") != row["sha256"]:
            raise ValueError("worker_wrong_source")
        # Do not trust worker status without independently verifying all artifacts.
        receipt = salvage.read_bundle_report(source, bundle_root / row["sha256"], timeout)
        if salvage.sha_file(source) != row["sha256"]:
            raise ValueError("source_drift")
        expected_outcome = receipt["outcome"]
        if expected_outcome != answer.get("outcome"):
            raise ValueError("worker_wrong_outcome")
        status = "done_partial" if expected_outcome == "partial" else "done_unsupported"
    except (salvage.UnsafeInput, OSError, ValueError, TypeError, KeyError, json.JSONDecodeError):
        return {"status": "failed", "reason": "bundle_integrity", "outcome": None,
                "stream_count": 0, "asset_count": 0}
    try:
        # The canonical independent verifier above owns evidence admission.
        # AutoRestore only materializes a local static presentation of it.
        promoted = autorestore.materialize(
            source, bundle_root / row["sha256"], bundle_root.parent / "recovered", receipt)
        if salvage.sha_file(source) != row["sha256"]:
            raise autorestore.AutoRestoreError("source_drift_after_presentation")
    except (autorestore.AutoRestoreError, salvage.UnsafeInput, OSError,
            ValueError, TypeError, KeyError, json.JSONDecodeError):
        return {"status": "failed", "reason": "auto_restore_integrity", "outcome": None,
                "stream_count": 0, "asset_count": 0, "auto_tier": None}
    try:
        # Same aggregate Windows Job and original per-file wall deadline.
        # A verified text view does not alter the immutable physical bundle.
        text_count, text_status = text_batch.admit(
            source, bundle_root.parent, row["sha256"], pub_cli=pub_cli,
            deadline=deadline, abort=abort, kill_tree=kill_tree)
    except text_batch.TextInterrupted as error:
        interrupted = str(error) == "cancelled"
        return {"status": "cancelled" if interrupted else "failed",
                "reason": "cancelled" if interrupted else "worker_timeout",
                "outcome": None, "stream_count": 0, "asset_count": 0, "auto_tier": None,
                "text_status": None, "text_count": 0}
    except (text_batch.TextRecoveryError, salvage.UnsafeInput, OSError, ValueError,
            TypeError, KeyError, json.JSONDecodeError):
        return {"status": "failed", "reason": "text_integrity", "outcome": None,
                "stream_count": 0, "asset_count": 0, "auto_tier": None,
                "text_status": None, "text_count": 0}
    composite_outcome = "partial" if text_count or expected_outcome == "partial" else "unsupported"
    return {"status": "done_partial" if composite_outcome == "partial" else "done_unsupported",
            "reason": None, "outcome": composite_outcome,
            "stream_count": receipt["physical"].get("stream_count", 0),
            "asset_count": receipt["images"].get("asset_count", 0),
            "auto_tier": "verified_text" if text_count else promoted["tier"],
            "text_status": text_status, "text_count": text_count}


def run(output: Path, *, workers: int = 2, timeout: int = 45,
        file_timeout: int = 600, pub_cli: Path | None = None,
        retain_original: bool = False, run_token: str | None = None) -> dict:
    if not 1 <= workers <= 4 or not 1 <= timeout <= 3600 or not 10 <= file_timeout <= 7200:
        raise BatchError("invalid_worker_limits")
    if run_token is not None and not RUN_TOKEN.fullmatch(run_token):
        raise BatchError("invalid_run_token")
    output = safe_root(output)
    if pub_cli is not None:
        no_symlinks(pub_cli)
        if not pub_cli.is_file():
            raise BatchError("pub_cli_unavailable")
    with output_lock(output):
        # The engine must enter a non-breakaway, aggregate-memory-capped
        # Windows Job before any untrusted child process can be spawned.
        # Failure is fatal; never fall back to an unfenced worker.
        try:
            job_guard.activate()
        except job_guard.JobGuardUnavailable as exc:
            raise BatchError("resource_guard_unavailable") from exc
        state = load_queue(output)
        bundle_root = output / "bundles"
        if bundle_root.is_symlink():
            raise BatchError("unsafe_bundle_root")
        bundle_root.mkdir(exist_ok=True)
        # Reverify completed results before treating them as durable evidence.
        for row in state["entries"]:
            if row["status"] in DONE:
                source, why = choose_source(row)
                if source is None:
                    row.update(status="failed", reason=why, outcome=None, auto_tier=None,
                               text_status=None, text_count=0)
                    continue
                try:
                    receipt = salvage.read_bundle_report(source, bundle_root / row["sha256"], timeout)
                    physical_outcome = receipt["outcome"]
                    stored_text_status = row.get("text_status", "not_requested")
                    stored_count = row.get("text_count", 0)
                    if (stored_text_status not in ("not_requested", "unavailable", "verified")
                            or type(stored_count) is not int
                            or not 0 <= stored_count <= 16_384
                            or ((stored_text_status == "verified") != (stored_count > 0))):
                        raise BatchError("invalid_stored_text_receipt")
                    previous_outcome = "partial" if stored_count or physical_outcome == "partial" else "unsupported"
                    if (row["status"] != ("done_partial" if previous_outcome == "partial"
                                          else "done_unsupported")
                            or row["outcome"] != previous_outcome
                            or row["stream_count"] != receipt["physical"].get("stream_count", 0)
                            or row["asset_count"] != receipt["images"].get("asset_count", 0)):
                        raise BatchError("integrity_mismatch")
                except (salvage.UnsafeInput, OSError, ValueError, TypeError, KeyError):
                    row.update(status="failed", reason="bundle_integrity", outcome=None,
                               auto_tier=None, text_status=None, text_count=0)
                    continue
                try:
                    promoted = autorestore.materialize(
                        source, bundle_root / row["sha256"], output / "recovered", receipt)
                except (autorestore.AutoRestoreError, salvage.UnsafeInput, OSError,
                        ValueError, TypeError, KeyError, json.JSONDecodeError):
                    row.update(status="failed", reason="auto_restore_integrity",
                               outcome=None, auto_tier=None, text_status=None, text_count=0)
                    continue
                try:
                    text_count, observed = text_batch.admit(
                        source, output, row["sha256"], pub_cli=None,
                        deadline=time.monotonic() + file_timeout,
                        abort=threading.Event(), kill_tree=kill_tree)
                    if (stored_text_status == "verified"
                            and (observed != "verified" or text_count != stored_count)):
                        raise text_batch.TextRecoveryError("stored_text_proof_missing")
                    outcome = "partial" if text_count or physical_outcome == "partial" else "unsupported"
                    row.update(status="done_partial" if outcome == "partial" else "done_unsupported",
                               outcome=outcome, auto_tier=("verified_text" if text_count
                                                           else promoted["tier"]),
                               text_count=text_count,
                               text_status="verified" if text_count else stored_text_status,
                               reason=None)
                    # Backfill an old completed row in the cancellable worker pool.
                    if pub_cli is not None and not text_count and stored_text_status == "not_requested":
                        row.update(status=QUEUED, outcome=None, auto_tier=None,
                                   text_count=0, text_status=None)
                except (text_batch.TextRecoveryError, text_batch.TextInterrupted,
                        salvage.UnsafeInput, OSError, ValueError, TypeError, KeyError):
                    row.update(status="failed", reason="text_integrity",
                               outcome=None, auto_tier=None, text_status=None, text_count=0)
            elif row["status"] == "running":
                row["status"] = QUEUED
                row["reason"] = None
        token = run_token if run_token is not None else uuid.uuid4().hex
        state["run_token"] = token
        persist(output, state)
        todo = [row for row in state["entries"] if row["status"] == QUEUED]
        in_flight = {}
        abort = threading.Event()
        with ThreadPoolExecutor(max_workers=workers) as pool:
            try:
                while todo or in_flight:
                    action = control_action(output, token)
                    if action == "cancel":
                        abort.set()
                    if not abort.is_set() and action is None:
                        while todo and len(in_flight) < workers:
                            row = todo.pop(0)
                            row["status"] = "running"
                            row["auto_tier"] = None
                            row["text_status"] = None
                            row["text_count"] = 0
                            row["attempts"] += 1
                            persist(output, state)
                            future = pool.submit(process_one, dict(row), bundle_root, timeout=timeout,
                                                 file_timeout=file_timeout, pub_cli=pub_cli,
                                                 retain_original=retain_original, abort=abort)
                            in_flight[future] = row
                    if not in_flight:
                        break
                    completed, _ = wait(in_flight, timeout=0.2, return_when=FIRST_COMPLETED)
                    for future in completed:
                        row = in_flight.pop(future)
                        try:
                            result = future.result()
                        except Exception:
                            result = {"status": "failed", "reason": "worker_crash", "outcome": None,
                                      "stream_count": 0, "asset_count": 0}
                        row.update(result)
                        persist(output, state)
                if abort.is_set():
                    for row in todo:
                        row.update(status="cancelled", reason="cancelled")
            finally:
                state["run_token"] = None
                persist(output, state)
        return report(state)


def retry(output: Path) -> dict:
    output = safe_root(output)
    with output_lock(output):
        state = load_queue(output)
        for row in state["entries"]:
            if row["status"] in ("failed", "cancelled"):
                row.update(status=QUEUED, reason=None, outcome=None,
                           stream_count=0, asset_count=0, auto_tier=None,
                           text_status=None, text_count=0)
        state["run_token"] = None
        persist(output, state)
        return report(state)


def status(output: Path) -> dict:
    output = safe_root(output)
    return report(load_queue(output))


def main(argv: list[str] | None = None) -> int:
    cli = argparse.ArgumentParser(description=__doc__)
    sub = cli.add_subparsers(dest="cmd", required=True)
    scan_cmd = sub.add_parser("scan", help="recursive file intake into a persistent private queue")
    scan_cmd.add_argument("--input", type=Path, action="append", required=True)
    scan_cmd.add_argument("--output", type=Path, required=True)
    scan_cmd.add_argument("--append", action="store_true")
    run_cmd = sub.add_parser("run", help="process/resume pending jobs")
    run_cmd.add_argument("--output", type=Path, required=True)
    run_cmd.add_argument("--workers", type=int, default=2)
    run_cmd.add_argument("--timeout", type=int, default=45)
    run_cmd.add_argument("--file-timeout", type=int, default=600)
    run_cmd.add_argument("--pub-cli", type=Path)
    run_cmd.add_argument("--retain-original", action="store_true")
    run_cmd.add_argument("--run-token", help="opaque per-GUI-run identity; not an access credential")
    for name in ("status", "retry", "pause", "cancel"):
        c = sub.add_parser(name)
        c.add_argument("--output", type=Path, required=True)
        if name in ("pause", "cancel"):
            c.add_argument("--expected-run-token", help="reject controls for another run")
    args = cli.parse_args(argv)
    try:
        if args.cmd == "scan":
            result = scan(args.input, args.output, append=args.append)
        elif args.cmd == "run":
            result = run(args.output, workers=args.workers, timeout=args.timeout,
                         file_timeout=args.file_timeout, pub_cli=args.pub_cli,
                         retain_original=args.retain_original, run_token=args.run_token)
        elif args.cmd == "retry":
            result = retry(args.output)
        elif args.cmd in ("pause", "cancel"):
            request_action(args.output, args.cmd, expected_run_token=args.expected_run_token)
            print(json.dumps({"schema_version": CONTROL_SCHEMA, "requested": args.cmd}))
            return 0
        else:
            result = status(args.output)
    except (BatchError, salvage.UnsafeInput, OSError, ValueError, json.JSONDecodeError) as exc:
        error = str(exc) if isinstance(exc, (BatchError, salvage.UnsafeInput)) else "invalid_state"
        print(json.dumps({"schema_version": REPORT_SCHEMA, "status": "rejected", "reason": error}))
        return 2
    print(json.dumps(result, sort_keys=True))
    return 1 if args.cmd == "run" and (not result["entries"] or any(
        row["status"] not in DONE for row in result["entries"])) else 0


if __name__ == "__main__":
    raise SystemExit(main())
