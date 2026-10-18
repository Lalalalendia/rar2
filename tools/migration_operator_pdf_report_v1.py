#!/usr/bin/env python3
"""Read-only integrity/exception report for an existing Chaptera batch PDF receipt.

This is a local operator triage tool, NOT a migration certificate or delivery
bundle. It never reads or modifies PUB input files, and does not perform export.
"""
from __future__ import annotations

import argparse
import csv
import hashlib
import io
import json
import os
from pathlib import Path
import re
import stat
import sys
import tempfile

SCHEMA = "chaptera.rescue-batch-pdf.v1"
REPORT_SCHEMA = "chaptera.migration-operator-pdf-report.v1"
MAX_FILES = 2_000
MAX_RECEIPT_BYTES = 16 * 1024 * 1024
MAX_PDF_BYTES = 1024 * 1024 * 1024
MAX_LOSS_BYTES = 16 * 1024 * 1024
HASH = re.compile(r"[0-9a-f]{64}\Z")
PDF_NAME = re.compile(r"[A-Za-z0-9_-]{1,48}--[0-9a-f]{12}--[0-9a-f]{16}\.pdf\Z")
REASON = re.compile(r"[a-z0-9_]{1,96}\Z")
FIELDS = ("source_relative_path", "source_sha256", "output_pdf", "state",
          "reason", "pdf_sha256", "loss_json_sha256", "loss_text_sha256")


class ReportRejected(ValueError):
    """Unsafe or inconsistent input; the report cannot be trusted."""


def safe_root(path: Path, *, create: bool = False) -> Path:
    if path.is_symlink():
        raise ReportRejected("root_symlink")
    if create:
        path.mkdir(parents=True, exist_ok=True)
    if not path.is_dir():
        raise ReportRejected("root_not_directory")
    return path.resolve(strict=True)


def verified_regular(path: Path, limit: int) -> bool:
    try:
        info = path.lstat()
        return stat.S_ISREG(info.st_mode) and 0 < info.st_size <= limit
    except OSError:
        return False


def read_manifest(root: Path) -> dict:
    path = root / "manifest.json"
    if not verified_regular(path, MAX_RECEIPT_BYTES):
        raise ReportRejected("missing_or_unsafe_manifest")
    try:
        with path.open("rb") as reader:
            raw = reader.read(MAX_RECEIPT_BYTES + 1)
        data = json.loads(raw)
    except (OSError, UnicodeError, ValueError, TypeError) as error:
        raise ReportRejected("invalid_manifest_json") from error
    if not isinstance(data, dict) or data.get("schema") != SCHEMA:
        raise ReportRejected("unexpected_manifest_schema")
    total, entries = data.get("total_sources"), data.get("entries")
    if (type(total) is not int or not 1 <= total <= MAX_FILES
            or not isinstance(entries, list) or len(entries) > total
            or data.get("input_kind") not in ("directory", "zip")):
        raise ReportRejected("invalid_manifest_denominator")
    seen: set[str] = set()
    for entry in entries:
        if not isinstance(entry, dict):
            raise ReportRejected("malformed_manifest_entry")
        relative, sha, state = (entry.get("source_relative_path"),
                                entry.get("source_sha256"), entry.get("status"))
        if (not isinstance(relative, str) or len(relative) > 1024
                or not relative.lower().endswith(".pub")
                or "\\" in relative or relative.startswith("/")
                or any(part in ("", ".", "..") for part in relative.split("/"))
                or any(ord(ch) < 32 for ch in relative)):
            raise ReportRejected("unsafe_source_relative_path")
        if relative.casefold() in seen:
            raise ReportRejected("duplicate_source_identity")
        seen.add(relative.casefold())
        if state not in ("converted", "reused", "failed"):
            raise ReportRejected("unsupported_batch_row_state")
        if sha is not None and (not isinstance(sha, str) or not HASH.fullmatch(sha)):
            raise ReportRejected("malformed_source_hash")
        if state != "failed" and not HASH.fullmatch(sha or ""):
            raise ReportRejected("missing_success_source_hash")
        if state == "failed" and (not isinstance(entry.get("reason"), str)
                                  or not REASON.fullmatch(entry["reason"])):
            raise ReportRejected("unsafe_failure_reason")
    return data


def file_digest(path: Path, max_bytes: int) -> str | None:
    if not verified_regular(path, max_bytes):
        return None
    digest = hashlib.sha256()
    try:
        with path.open("rb") as stream:
            for chunk in iter(lambda: stream.read(1024 * 1024), b""):
                digest.update(chunk)
        if not verified_regular(path, max_bytes):
            return None
    except OSError:
        return None
    return digest.hexdigest()


def inspect_row(row: dict, root: Path) -> dict:
    result = {k: "" for k in FIELDS}
    result["source_relative_path"] = row["source_relative_path"]
    result["source_sha256"] = row.get("source_sha256") or ""
    if row["status"] == "failed":
        result.update(state="exception", reason=row["reason"])
        return result

    name = row.get("output_pdf")
    if not isinstance(name, str) or not PDF_NAME.fullmatch(name):
        raise ReportRejected("unsafe_pdf_name")
    result["output_pdf"] = name
    artifacts = (("pdf_sha256", name, MAX_PDF_BYTES),
                 ("loss_json_sha256", name + ".loss.json", MAX_LOSS_BYTES),
                 ("loss_text_sha256", name + ".loss.txt", MAX_LOSS_BYTES))
    for key, filename, limit in artifacts:
        expected = row.get(key)
        if not isinstance(expected, str) or not HASH.fullmatch(expected):
            raise ReportRejected("invalid_recorded_output_hash")
        actual = file_digest(root / filename, limit)
        if actual != expected:
            result.update(state="integrity_rejected", reason="output_missing_or_hash_mismatch")
            return result
        result[key] = actual
    try:
        path = root / (name + ".loss.json")
        if path.stat().st_size > MAX_LOSS_BYTES:
            raise ValueError("too_large")
        loss = json.loads(path.read_text(encoding="utf-8"))
        if not isinstance(loss, dict):
            raise ValueError("not an object")
        with (root / name).open("rb") as stream:
            if stream.read(5) != b"%PDF-":
                raise ValueError("bad PDF marker")
    except (UnicodeError, OSError, ValueError, TypeError):
        result.update(state="integrity_rejected", reason="invalid_pdf_or_loss_structure")
        return result
    # Verified artifact bytes != visual parity, standards compliance or an
    # independently accepted target application. These all require other owners.
    result.update(state="integrity_confirmed_review_required", reason="visual_and_loss_review_required")
    return result


def generate(batch_output: Path) -> dict:
    root = safe_root(batch_output)
    if (root / ".rescue-batch.lock").exists():
        raise ReportRejected("batch_writer_active_or_lock_present")
    first_manifest_hash = file_digest(root / "manifest.json", MAX_RECEIPT_BYTES)
    manifest = read_manifest(root)
    entries = sorted(manifest["entries"], key=lambda r: r["source_relative_path"])
    output = [inspect_row(row, root) for row in entries]
    pending = manifest["total_sources"] - len(entries)
    counts = {state: sum(x["state"] == state for x in output) for state in
              ("integrity_confirmed_review_required", "integrity_rejected", "exception")}
    counts["not_yet_in_checkpoint"] = pending
    last_manifest_hash = file_digest(root / "manifest.json", MAX_RECEIPT_BYTES)
    if first_manifest_hash is None or last_manifest_hash != first_manifest_hash:
        raise ReportRejected("manifest_changed_during_report")
    return {"schema": REPORT_SCHEMA, "source_receipt_schema": SCHEMA,
            "source_receipt_sha256": first_manifest_hash,
            "total_sources": manifest["total_sources"], "checkpoint_rows": len(output),
            "counts": counts, "entries": output,
            "claim_boundary": "operator_only_integrity_not_visual_fidelity_or_migration_certification"}


def spreadsheet_safe(value: object) -> str:
    cell = str(value if value is not None else "")
    return "'" + cell if cell.startswith(("=", "+", "-", "@", "\t", "\r", "\n")) else cell


def csv_content(rows: list[dict]) -> str:
    file = io.StringIO(newline="")
    writer = csv.DictWriter(file, fieldnames=FIELDS, lineterminator="\n", extrasaction="ignore")
    writer.writeheader()
    writer.writerows({key: spreadsheet_safe(row.get(key, "")) for key in FIELDS} for row in rows)
    return file.getvalue()


def atomic_text(root: Path, name: str, content: str) -> None:
    target = root / name
    if target.is_symlink() or (target.exists() and not target.is_file()):
        raise ReportRejected("unsafe_report_target")
    fd, temporary = tempfile.mkstemp(prefix=".operator-report-", dir=root)
    try:
        with os.fdopen(fd, "w", encoding="utf-8", newline="") as stream:
            stream.write(content)
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, target)
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)


def write_reports(receipt: dict, report_dir: Path, batch_dir: Path) -> None:
    src = safe_root(batch_dir)
    # Reject overlap before creating any report directory under the source.
    prospective = report_dir.resolve()
    if (src == prospective or src in prospective.parents
            or prospective in src.parents):
        raise ReportRejected("report_and_batch_trees_overlap")
    dst = safe_root(report_dir, create=True)
    for name in ("operator-report.json", "operator-report.csv", "exceptions.csv"):
        if (dst / name).is_symlink() or ((dst / name).exists() and not (dst / name).is_file()):
            raise ReportRejected("unsafe_report_target")
    values = receipt["entries"]
    problems = [row for row in values if row["state"] != "integrity_confirmed_review_required"]
    atomic_text(dst, "operator-report.json", json.dumps(receipt, sort_keys=True, indent=2, ensure_ascii=True)+"\n")
    atomic_text(dst, "operator-report.csv", csv_content(values))
    atomic_text(dst, "exceptions.csv", csv_content(problems))


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("batch_output", type=Path, help="private output from existing rescue_batch_pdf_v1.py")
    parser.add_argument("--report-dir", required=True, type=Path, help="separate private operator reports directory")
    args = parser.parse_args(argv)
    try:
        receipt = generate(args.batch_output)
        write_reports(receipt, args.report_dir, args.batch_output)
    except (ReportRejected, OSError) as error:
        reason = str(error) if isinstance(error, ReportRejected) else "filesystem_failure"
        print("migration report rejected: "+reason, file=sys.stderr)
        return 2
    counts = receipt["counts"]
    print("migration operator integrity="+str(counts["integrity_confirmed_review_required"])
          +" rejected="+str(counts["integrity_rejected"])
          +" failed="+str(counts["exception"])
          +" pending="+str(counts["not_yet_in_checkpoint"]))
    return 1 if any(counts[s] for s in ("integrity_rejected", "exception", "not_yet_in_checkpoint")) else 0


if __name__ == "__main__":
    sys.exit(main())
