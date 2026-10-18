#!/usr/bin/env python3
"""Explicit, version-bound AutoRestore pass for source-verified table text.

Runs only against the existing private Rescue Batch queue and *never* rewrites
old immutable CFB, galleries or stories.json/txt. Table text uses a separate
SHA-keyed directory and immutable local receipt, guarded by the same OS lock.
"""
from __future__ import annotations

from collections import Counter
import json
from pathlib import Path
import subprocess

from tools import rescue_desktop_batch_v1 as batch
from tools import rescue_legacy_table_cell_proof_census_v1 as census
from tools import verify_rescue_table_text_evidence_v1 as verifier

SCHEMA = "chaptera.rescue-batch-table-text-profile.v1"
PROFILE = "legacy_table_cells_v1"
STATES = frozenset({"verified", "unavailable"})
RECEIPT_FIELDS = frozenset({
    "schema_version", "source_sha256", "profile", "status",
    "story_count", "manifest_sha256", "stories_txt_sha256",
    "source_immutable", "native_pub_repair_attempted",
})


class TableBatchError(batch.BatchError):
    pass


def _receipt(sha: str, status: str, count: int,
             manifest_sha: str | None = None, txt_sha: str | None = None) -> dict:
    if (not batch.SHA.fullmatch(sha) or status not in STATES
            or type(count) is not int or not 0 <= count <= 16384
            or ((status == "verified") != (count > 0))
            or ((manifest_sha is not None) != (txt_sha is not None))
            or ((status == "verified") != (manifest_sha is not None))
            or (manifest_sha is not None and (
                not batch.SHA.fullmatch(manifest_sha) or not batch.SHA.fullmatch(txt_sha)))):
        raise TableBatchError("invalid_table_profile_receipt")
    return {
        "schema_version": SCHEMA, "source_sha256": sha,
        "profile": PROFILE, "status": status, "story_count": count,
        "manifest_sha256": manifest_sha,
        "stories_txt_sha256": txt_sha, "source_immutable": True,
        "native_pub_repair_attempted": False,
    }


def _read_receipt(path: Path, sha: str) -> dict | None:
    if not path.exists() and not path.is_symlink():
        return None
    batch.no_symlinks(path)
    try:
        if not path.is_file() or path.stat().st_size > 8192:
            raise TableBatchError("untrusted_table_profile_receipt")
        saved = json.loads(path.read_text(encoding="utf-8"))
        if (type(saved) is not dict or set(saved) != RECEIPT_FIELDS
                or saved != _receipt(
                    sha, saved["status"], saved["story_count"],
                    saved["manifest_sha256"], saved["stories_txt_sha256"],
                )):
            raise TableBatchError("untrusted_table_profile_receipt")
        return saved
    except (OSError, KeyError, ValueError, TypeError) as error:
        raise TableBatchError("untrusted_table_profile_receipt") from error


def _save_receipt(path: Path, value: dict) -> None:
    before = _read_receipt(path, value["source_sha256"])
    if before is not None:
        if before != value:
            raise TableBatchError("immutable_table_profile_receipt_changed")
        return
    batch.atomic_write(path, json.dumps(value, sort_keys=True, indent=2) + "\n")


def _worker(pub_cli: Path, command: str, source: Path, target: Path,
            timeout: int) -> tuple[bool, dict | None]:
    args = [str(pub_cli), command, str(source)]
    if command in ("extract-legacy-table-text", "verify-legacy-table-text"):
        args.extend(["--output", str(target)])
    options = dict(
        cwd=batch.REPO_ROOT, stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
        timeout=timeout, check=False,
    )
    if batch.os.name == "nt":
        options["creationflags"] = subprocess.CREATE_NEW_PROCESS_GROUP
    else:
        options["start_new_session"] = True
    try:
        proc = subprocess.run(args, **options)
    except (OSError, subprocess.TimeoutExpired) as error:
        raise TableBatchError("table_cli_worker_unavailable") from error
    if len(proc.stdout) > 16384:
        raise TableBatchError("table_worker_stdout_exceeded_limit")
    if command in ("probe-legacy-text", "probe-legacy-table-cells"):
        if proc.returncode != 0:
            raise TableBatchError("canonical_table_proof_unavailable")
        try:
            return True, json.loads(proc.stdout)
        except ValueError as error:
            raise TableBatchError("invalid_table_probe_protocol") from error
    return proc.returncode == 0, None


def _validate_existing(source: Path, target: Path, pub_cli: Path,
                       sha: str, expected_count: int | None = None) -> dict:
    batch.no_symlinks(target)
    try:
        result = verifier.verify(source, target, pub_cli)
    except (OSError, ValueError) as error:
        raise TableBatchError("table_private_evidence_integrity") from error
    if (result["source_sha256"] != sha
            or (expected_count is not None and result["verified_table_stories"] != expected_count)):
        raise TableBatchError("table_private_source_or_count_mismatch")
    return _receipt(
        sha, "verified", result["verified_table_stories"],
        batch.salvage.sha_file(target / "stories.json"),
        batch.salvage.sha_file(target / "stories.txt"),
    )


def admit_completed(output: Path, pub_cli: Path, *, timeout: int = 45,
                    file_timeout: int = 600) -> dict:
    if not 1 <= timeout <= 3600 or not 10 <= file_timeout <= 7200:
        raise TableBatchError("table_profile_invalid_timeouts")
    output = batch.safe_root(output)
    batch.no_symlinks(pub_cli)
    if not pub_cli.is_file():
        raise TableBatchError("table_profile_missing_pub_cli")

    with batch.output_lock(output):
        try:
            batch.job_guard.activate()
        except batch.job_guard.JobGuardUnavailable as error:
            raise TableBatchError("table_resource_guard_unavailable") from error
        state = batch.load_queue(output)
        root = output / "recovered-table-text-v1"
        receipt_root = output / "recovered-table-text-profiles"
        for path in (root, receipt_root):
            batch.no_symlinks(path)
            path.mkdir(exist_ok=True)
            if not path.is_dir() or path.is_symlink():
                raise TableBatchError("unsafe_table_profile_root")
        counts = Counter()
        verified_ranges = 0
        source_counts = 0
        for row in state["entries"]:
            if row["status"] not in batch.DONE:
                counts["skipped_nonterminal"] += 1
                continue
            sha = row["sha256"]
            if not batch.SHA.fullmatch(sha):
                raise TableBatchError("table_queue_source_sha_invalid")
            source, invalid = batch.choose_source(row)
            if source is None or invalid:
                raise TableBatchError("table_queue_original_unavailable")
            if batch.salvage.sha_file(source) != sha:
                raise TableBatchError("table_queue_original_sha_drift")
            # Refuse to process table evidence unless old immutable bundle
            # is still independently revalidated by the existing authority.
            try:
                batch.salvage.read_bundle_report(
                    source, output / "bundles" / sha, timeout,
                )
            except (batch.salvage.UnsafeInput, OSError, ValueError) as error:
                raise TableBatchError("table_original_bundle_not_verified") from error
            source_counts += 1
            target = root / sha
            receipt_path = receipt_root / f"{sha}.json"
            batch.no_symlinks(target)
            batch.no_symlinks(receipt_path)
            stored = _read_receipt(receipt_path, sha)
            if stored is not None:
                if stored["status"] == "verified":
                    checked = _validate_existing(
                        source, target, pub_cli, sha, stored["story_count"],
                    )
                    if checked != stored:
                        raise TableBatchError("table_saved_receipt_integrity_mismatch")
                    verified_ranges += stored["story_count"]
                    counts["verified_reused"] += 1
                else:
                    if target.exists() or target.is_symlink():
                        raise TableBatchError("unavailable_table_profile_has_evidence")
                    counts["unavailable_reused"] += 1
                continue
            if target.exists() or target.is_symlink():
                # Crash after atomic private export but before receipt: recover
                # only after complete source-byte verification, never overwrite.
                saved = _validate_existing(source, target, pub_cli, sha)
                _save_receipt(receipt_path, saved)
                verified_ranges += saved["story_count"]
                counts["verified_recovered"] += 1
                continue
            ok, previous = _worker(pub_cli, "probe-legacy-text", source, target, file_timeout)
            if not ok:
                raise TableBatchError("table_baseline_probe_failed")
            original = census.check_probe(previous, sha, source.stat().st_size)
            ok, raw = _worker(pub_cli, "probe-legacy-table-cells", source, target, file_timeout)
            if not ok:
                raise TableBatchError("table_cell_probe_failed")
            cell_proof = census.check_cell_proof(raw, original, sha, source.stat().st_size)
            expected_count = cell_proof["proven_table_stories"]
            if expected_count == 0:
                _save_receipt(receipt_path, _receipt(sha, "unavailable", 0))
                counts["unavailable_new"] += 1
                continue
            succeeded, _ = _worker(
                pub_cli, "extract-legacy-table-text", source, target, file_timeout,
            )
            if not succeeded:
                raise TableBatchError("proven_table_export_failed")
            saved = _validate_existing(source, target, pub_cli, sha, expected_count)
            if batch.salvage.sha_file(source) != sha:
                raise TableBatchError("table_source_modified_by_worker")
            _save_receipt(receipt_path, saved)
            verified_ranges += saved["story_count"]
            counts["verified_new"] += 1

        return {
            "schema_version": "chaptera.rescue-batch-table-text-summary.v1",
            "profile": PROFILE,
            "source_free": True,
            "verified_table_story_ranges": verified_ranges,
            "source_entries_checked": source_counts,
            "statuses": dict(sorted(counts.items())),
            "source_immutable": True,
            "native_pub_repair_attempted": False,
        }
