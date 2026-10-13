#!/usr/bin/env python3
"""Produce and replay-check a physical 9-up A4 sheet from the real Editor runner.

This composes the existing exact-current-state fixed-PDF producer; it does not
reparse PUB, execute a second rendering authority, or mutate EditorProject.
Use an admitted 85x55 mm, single-logical-page project as input.
"""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]
EDITOR_API = ROOT / "services" / "editor-api"
if str(EDITOR_API) not in sys.path:
    sys.path.insert(0, str(EDITOR_API))

from print_composer_nup_current_revision_v1 import compose_verified_current_editor_card_nup_v1
import os
import stat
from print_composer_nup_pdf_v1 import MAX_FIXED_PDF_BYTES
from print_composer_nup_v1 import PrintComposerNupError

CANONICAL_RUNNER = ROOT / "tools" / "run_editor_fixed_pdf_current_revision_v2.py"
PROTOCOL = "chaptera.editor-nine-up-fresh-replay.v1"
MAX_SOURCE = 256 * 1024 * 1024
MAX_PROJECT = 32 * 1024 * 1024
MAX_RECEIPT = 256 * 1024
MAX_STAGE01 = 256 * 1024
MAX_RESOURCE_INPUT = 32 * 1024 * 1024
MAX_REPLACEMENT = 16 * 1024 * 1024


def digest(payload: bytes) -> str:
    return hashlib.sha256(payload).hexdigest()


def read_regular_bounded(path: Path, limit: int) -> bytes:
    """Reject links/devices/oversized files even if contents look valid."""
    info = path.lstat()
    if not stat.S_ISREG(info.st_mode) or not (0 < info.st_size <= limit):
        raise PrintComposerNupError("input_file_class_or_size_invalid")
    flags = os.O_RDONLY | getattr(os, "O_NONBLOCK", 0) | getattr(os, "O_NOFOLLOW", 0) | getattr(os, "O_BINARY", 0)
    fd = os.open(path, flags)
    with os.fdopen(fd, "rb") as source:
        info = os.fstat(source.fileno())
        if not stat.S_ISREG(info.st_mode) or not (0 < info.st_size <= limit):
            raise PrintComposerNupError("input_file_class_or_size_invalid")
        result = source.read(limit + 1)
    if not (0 < len(result) <= limit):
        raise PrintComposerNupError("input_file_size_changed")
    return result


def _run_canonical_runner(command: list[str]) -> None:
    """Keep potentially private producer stdout/stderr off public receipts."""
    try:
        completed = subprocess.run(
            command, cwd=ROOT, capture_output=True, timeout=1800, check=False
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        raise PrintComposerNupError("upstream_fixed_pdf_execution_unavailable") from error
    if completed.returncode != 0:
        raise PrintComposerNupError("upstream_fixed_pdf_replay_failed")


def _fresh_run(inputs: dict[str, Path], temp: Path, run_index: int,
               source: bytes, project: bytes, logical_page_id: str, gap_mm: int):
    run_dir = temp / f"run-{run_index}"
    run_dir.mkdir()
    request = run_dir / "request.json"
    renderer_result = run_dir / "renderer-result.json"
    fixed_pdf = run_dir / "current.pdf"
    fixed_receipt = run_dir / "current-receipt.json"
    command = [
        sys.executable, str(CANONICAL_RUNNER),
        "--source", str(inputs["source"]),
        "--project", str(inputs["project"]),
        "--replacement", str(inputs["replacement"]),
        "--stage01-receipt", str(inputs["stage01_receipt"]),
        "--resource-input", str(inputs["resource_input"]),
        "--yab-checkout", str(inputs["yab_checkout"]),
        "--request-output", str(request),
        "--renderer-result-output", str(renderer_result),
        "--pdf-output", str(fixed_pdf),
        "--receipt-output", str(fixed_receipt),
    ]
    _run_canonical_runner(command)
    current_pdf = read_regular_bounded(fixed_pdf, MAX_FIXED_PDF_BYTES)
    raw_receipt = read_regular_bounded(fixed_receipt, MAX_RECEIPT)
    try:
        current_receipt = json.loads(raw_receipt)
    except (ValueError, UnicodeError) as error:
        raise PrintComposerNupError("upstream_fixed_pdf_receipt_invalid") from error
    return compose_verified_current_editor_card_nup_v1(
        upstream_receipt=current_receipt, original_pub_bytes=source,
        editor_project_bytes=project, current_fixed_pdf_bytes=current_pdf,
        logical_page_id=logical_page_id, gap_mm=gap_mm,
    )


def run_two_fresh_replays(*, source: Path, project: Path, replacement: Path,
                          stage01_receipt: Path, resource_input: Path,
                          yab_checkout: Path, logical_page_id: str, gap_mm: int):
    inputs = dict(source=source, project=project, replacement=replacement,
                  stage01_receipt=stage01_receipt, resource_input=resource_input,
                  yab_checkout=yab_checkout)
    bounds = dict(source=MAX_SOURCE, project=MAX_PROJECT,
                  replacement=MAX_REPLACEMENT, stage01_receipt=MAX_STAGE01,
                  resource_input=MAX_RESOURCE_INPUT)
    snapshots = {key: read_regular_bounded(inputs[key], limit)
                 for key, limit in bounds.items()}
    # The external native producer must exist. Tests patch only the private
    # _run_canonical_runner seam to check the orchestration without Publisher.
    if not CANONICAL_RUNNER.is_file():
        raise PrintComposerNupError("canonical_fixed_pdf_runner_missing")
    results = []
    with tempfile.TemporaryDirectory(prefix="chaptera-nine-up-") as tmp:
        for run_index in (1, 2):
            result = _fresh_run(inputs, Path(tmp), run_index,
                                snapshots["source"], snapshots["project"],
                                logical_page_id, gap_mm)
            for key, expected in snapshots.items():
                if read_regular_bounded(inputs[key], bounds[key]) != expected:
                    raise PrintComposerNupError("input_changed_during_replay")
            results.append(result)
    first, second = results
    if (first.pdf_bytes != second.pdf_bytes or
        first.preview_png_bytes != second.preview_png_bytes or
        first.receipt != second.receipt):
        raise PrintComposerNupError("fresh_replay_output_mismatch")
    return first, {
        **first.receipt,
        "fresh_replay": {
            "schema_version": PROTOCOL,
            "runs": 2,
            "exact_pdf_bytes_equal": True,
            "exact_preview_bytes_equal": True,
            "normalized_receipts_equal": True,
            "input_snapshots_unchanged": True,
            "fixed_pdf_producer": "tools/run_editor_fixed_pdf_current_revision_v2.py",
            "source_sha256": digest(snapshots["source"]),
            "project_sha256": digest(snapshots["project"]),
        },
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("source", "project", "replacement", "stage01-receipt",
                 "resource-input", "yab-checkout", "pdf-output", "preview-output",
                 "receipt-output"):
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--logical-page-id", required=True)
    parser.add_argument("--gap-mm", type=int, default=5)
    args = parser.parse_args()
    inputs = [args.source, args.project, args.replacement,
              args.stage01_receipt, args.resource_input]
    outputs = [args.pdf_output, args.preview_output, args.receipt_output]
    try:
        if len({p.resolve() for p in [*inputs, *outputs]}) != len(inputs) + len(outputs):
            raise PrintComposerNupError("source_output_path_collision")
        if any(p.exists() or p.is_symlink() or not p.parent.is_dir() for p in outputs):
            raise PrintComposerNupError("output_must_be_new_file")
        product, receipt = run_two_fresh_replays(
            source=args.source, project=args.project,
            replacement=args.replacement, stage01_receipt=args.stage01_receipt,
            resource_input=args.resource_input, yab_checkout=args.yab_checkout,
            logical_page_id=args.logical_page_id, gap_mm=args.gap_mm,
        )
        data = [product.pdf_bytes, product.preview_png_bytes,
                (json.dumps(receipt, sort_keys=True, indent=2) + "\n").encode()]
        created: list[Path] = []
        try:
            for path, payload in zip(outputs, data):
                with path.open("xb") as stream:
                    created.append(path)
                    stream.write(payload)
        except OSError:
            for path in created:
                path.unlink(missing_ok=True)
            raise
        print(json.dumps({"ok": True, "pdf_sha256": digest(product.pdf_bytes),
                          "preview_sha256": digest(product.preview_png_bytes),
                          "verified_fresh_replays": 2}, sort_keys=True))
        return 0
    except (OSError, PrintComposerNupError) as error:
        print(json.dumps({"ok": False, "error":
                          str(error) if isinstance(error, PrintComposerNupError)
                          else "file_io_error"}, sort_keys=True))
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
