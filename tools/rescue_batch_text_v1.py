#!/usr/bin/env python3
"""Optional source-proven Story evidence in the bounded Rescue batch.

The only semantic producer is the existing Rust pub extract-text CLI.
This module checks its private output with the independent byte-integrity
validator and creates a separate immutable local index. It does not decode
Quill, modify the physical salvage bundle, or repair a Publisher document.
"""
from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
import threading
import time

from tools import rescue_salvage_bundle_v1 as salvage
from tools import verify_rescue_text_evidence_v1 as verifier

SCHEMA = "chaptera.rescue-text-presentation.v1"
MAX_INDEX_BYTES = 16 * 1024
MAX_MANIFEST_BYTES = 16 * 1024


class TextRecoveryError(ValueError):
    """A source, output or previously published text result is untrusted."""


class TextInterrupted(Exception):
    """A batch cancellation or per-file deadline stopped the text producer."""


def _safe_directory(path: Path) -> None:
    try:
        salvage.no_symlink_components(path)
    except salvage.UnsafeInput as error:
        raise TextRecoveryError("unsafe_text_path") from error
    if path.exists() and not path.is_dir():
        raise TextRecoveryError("text_path_not_directory")


def _sha(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def _reap(proc: subprocess.Popen, kill_tree) -> None:
    kill_tree(proc)
    try:
        proc.wait(timeout=3)
    except subprocess.TimeoutExpired:
        proc.kill()
        proc.wait(timeout=2)


def run_cli(pub_cli: Path, source: Path, target: Path, deadline: float,
            abort: threading.Event, kill_tree) -> bool:
    """Run the exact existing Rust producer in the parent job's process tree.

    No stdout or stderr is retained (either stream may contain private data).
    The caller has already entered the non-breakaway aggregate Windows Job.
    """
    options = dict(cwd=salvage.REPO_ROOT, stdin=subprocess.DEVNULL,
                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    if os.name == "nt":
        options["creationflags"] = subprocess.CREATE_NEW_PROCESS_GROUP
    else:
        options["start_new_session"] = True
    try:
        proc = subprocess.Popen(
            [str(pub_cli), "extract-text", str(source), "--output", str(target)],
            **options)
    except OSError as error:
        raise TextRecoveryError("text_worker_unavailable") from error
    while True:
        remaining = deadline - time.monotonic()
        if abort.is_set() or remaining <= 0:
            _reap(proc, kill_tree)
            raise TextInterrupted("cancelled" if abort.is_set() else "worker_timeout")
        try:
            result = proc.wait(timeout=min(0.2, remaining))
            if abort.is_set():
                raise TextInterrupted("cancelled")
            return result == 0
        except subprocess.TimeoutExpired:
            continue
        except OSError as error:
            _reap(proc, kill_tree)
            raise TextRecoveryError("text_worker_crash") from error


def _index_bytes(digest: str, count: int) -> bytes:
    # Never embed a Story key, filename, document name or textual payload.
    page = f"""<!doctype html>
<html lang="en"><head><meta charset="utf-8">
<meta http-equiv="Content-Security-Policy"
 content="default-src 'none'; style-src 'unsafe-inline'">
<title>Chaptera Rescue - verified Story ranges</title>
<style>body{{font:16px system-ui,sans-serif;max-width:960px;margin:3rem auto;
padding:1rem;background:#e9ebf0;color:#222536}}
main{{background:white;padding:1.5rem;border-radius:14px}}
a{{color:#5540c3;overflow-wrap:anywhere}}</style></head><body><main>
<h1>Verified source-backed text ranges</h1>
<p>Source SHA-256: <code>{digest}</code></p>
<p>Independently byte-verified Story ranges: <strong>{count}</strong>.</p>
<p>This is partial extracted text, not reconstructed page order, typography,
text frames or a repaired native PUB.</p>
<ul><li><a href="../../recovered-text/{digest}/stories.txt">Open extracted text</a></li>
<li><a href="../../recovered-text/{digest}/stories.json">Open private provenance and ranges</a></li>
<li><a href="../../recovered/{digest}/index.html">Open separately verified image/stream gallery</a></li></ul>
<p>Local private evidence only. No network access or upload is needed.</p>
</main></body></html>
"""
    result = page.encode("utf-8")
    if len(result) > MAX_INDEX_BYTES:
        raise TextRecoveryError("text_index_size_limit")
    return result


def _verify_saved(target: Path, manifest: bytes, page: bytes) -> None:
    try:
        salvage.check_no_symlinks(target)
        if {p.name for p in target.iterdir()} != {"recovery.json", "index.html"}:
            raise TextRecoveryError("unexpected_text_presentation_artifact")
        if ((target / "recovery.json").stat().st_size > MAX_MANIFEST_BYTES
                or (target / "index.html").stat().st_size > MAX_INDEX_BYTES
                or (target / "recovery.json").read_bytes() != manifest
                or (target / "index.html").read_bytes() != page):
            raise TextRecoveryError("text_presentation_integrity_mismatch")
    except (OSError, salvage.UnsafeInput) as error:
        raise TextRecoveryError("unsafe_text_presentation") from error


def _save_index(output: Path, digest: str, story_count: int) -> None:
    evidence = output / "recovered-text" / digest
    # This is the previous, separately admitted immutable AutoRestore preview.
    gallery = output / "recovered" / digest / "recovery.json"
    if gallery.is_symlink() or not gallery.is_file():
        raise TextRecoveryError("missing_verified_base_gallery")
    page = _index_bytes(digest, story_count)
    manifest_data = {
        "schema_version": SCHEMA,
        "source_sha256": digest,
        "story_count": story_count,
        "story_manifest_sha256": salvage.sha_file(evidence / "stories.json"),
        "story_text_sha256": salvage.sha_file(evidence / "stories.txt"),
        "base_gallery_receipt_sha256": salvage.sha_file(gallery),
        "html_sha256": _sha(page),
        "source_immutable": True,
        "native_pub_delivery_allowed": False,
        "native_pub_repair_attempted": False,
        "page_geometry_reconstructed": False,
    }
    manifest = (json.dumps(manifest_data, sort_keys=True, indent=2, ensure_ascii=True) + "\n").encode("utf-8")
    if len(manifest) > MAX_MANIFEST_BYTES:
        raise TextRecoveryError("text_manifest_size_limit")
    root = output / "recovered-text-pages"
    _safe_directory(root)
    root.mkdir(parents=True, exist_ok=True)
    target = root / digest
    _safe_directory(target)
    if target.exists():
        _verify_saved(target, manifest, page)
        return
    with tempfile.TemporaryDirectory(prefix=".stage-rescue-text-view-", dir=root) as temp:
        stage = Path(temp) / "presentation"
        stage.mkdir()
        (stage / "index.html").write_bytes(page)
        (stage / "recovery.json").write_bytes(manifest)
        _verify_saved(stage, manifest, page)
        if target.exists() or target.is_symlink():
            raise TextRecoveryError("text_presentation_collision")
        stage.rename(target)


def admit(source: Path, output: Path, digest: str, *, pub_cli: Path | None,
          deadline: float, abort: threading.Event, kill_tree) -> tuple[int, str]:
    """Return (independently verified Story count, typed attempt status).

    Existing evidence is always checked, even when --pub-cli is not supplied.
    An unsupported PUB produces no false text claim; existing tampering fails
    closed and is never replaced with fresh output.
    """
    if not salvage.SHA.fullmatch(digest):
        raise TextRecoveryError("invalid_text_source_sha")
    _safe_directory(output)
    root = output / "recovered-text"
    _safe_directory(root)
    target = root / digest
    _safe_directory(target)
    _safe_directory(output / "recovered-text-pages")
    published = output / "recovered-text-pages" / digest
    _safe_directory(published)
    if abort.is_set():
        raise TextInterrupted("cancelled")
    if salvage.sha_file(source) != digest:
        raise TextRecoveryError("text_source_drift")

    if not target.exists():
        if published.exists():
            raise TextRecoveryError("orphan_text_presentation")
        if pub_cli is None:
            return 0, "not_requested"
        if deadline - time.monotonic() <= 0:
            raise TextInterrupted("worker_timeout")
        root.mkdir(parents=True, exist_ok=True)
        succeeded = run_cli(pub_cli, source, target, deadline, abort, kill_tree)
        if abort.is_set():
            raise TextInterrupted("cancelled")
        if not succeeded:
            if target.exists() or target.is_symlink():
                raise TextRecoveryError("unverified_text_producer_output")
            return 0, "unavailable"
        if not target.is_dir() or target.is_symlink():
            raise TextRecoveryError("missing_text_producer_output")

    try:
        proof = verifier.verify(source, target)
    except (verifier.StoryEvidenceError, OSError, ValueError) as error:
        raise TextRecoveryError("text_evidence_integrity") from error
    if proof["source_sha256"] != digest or proof["story_count"] < 1:
        raise TextRecoveryError("text_source_or_count_mismatch")
    if abort.is_set():
        raise TextInterrupted("cancelled")
    if salvage.sha_file(source) != digest:
        raise TextRecoveryError("text_source_drift_after_verification")
    _save_index(output, digest, proof["story_count"])
    if abort.is_set():
        raise TextInterrupted("cancelled")
    return proof["story_count"], "verified"
