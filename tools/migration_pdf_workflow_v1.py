#!/usr/bin/env python3
"""One-command local PDF archive workflow over existing Chaptera authorities.

Inventory uses the accepted RESCUE-BATCH-PDF intake, execution reuses its
converter/checkpoint, and reports reuse MIGRATION-OPERATOR-PDF-REPORT-01.
This entrypoint does not certify migration fidelity or repair native PUB.
"""
from __future__ import annotations

import argparse
import json
from pathlib import Path
import sys
import zipfile

import migration_operator_pdf_report_v1 as reporter
import rescue_batch_pdf_v1 as batch

INVENTORY_SCHEMA = "chaptera.migration-pdf-workflow-inventory.v1"


class WorkflowRejected(ValueError):
    pass


def _canonical(path: Path) -> Path:
    if path.is_symlink():
        raise WorkflowRejected("symlink_path")
    return path.resolve()


def _overlaps(left: Path, right: Path) -> bool:
    return left == right or left in right.parents or right in left.parents


def _source_kind(root: Path) -> str:
    if root.is_symlink() or not root.exists():
        raise WorkflowRejected("input_missing_or_symlink")
    if root.is_dir():
        return "directory"
    if root.is_file() and root.suffix.lower() == ".zip":
        return "zip"
    raise WorkflowRejected("input_must_be_directory_or_zip")


def _preflight(root: Path, pdf_dir: Path, report_dir: Path) -> str:
    kind = _source_kind(root)
    src, pdf, reports = (_canonical(root), _canonical(pdf_dir),
                         _canonical(report_dir))
    if _overlaps(pdf, reports):
        raise WorkflowRejected("pdf_and_report_trees_overlap")
    if kind == "directory" and (_overlaps(src, pdf) or _overlaps(src, reports)):
        raise WorkflowRejected("source_and_output_trees_overlap")
    # A source ZIP may live alongside outputs, but its exact file path must not
    # become a result directory. The accepted batch authority opens it read-only.
    if kind == "zip" and (src == pdf or src == reports):
        raise WorkflowRejected("zip_and_output_path_collide")
    for candidate in (pdf_dir, report_dir):
        if candidate.exists() and not candidate.is_dir():
            raise WorkflowRejected("output_must_be_directory")
    return kind


def inventory(source: Path) -> dict:
    kind = _source_kind(source)
    if kind == "directory":
        sources = batch.inventory_folder(source)
    else:
        with zipfile.ZipFile(source, "r") as archive:
            sources = batch.inventory_zip(archive)
    if not sources:
        raise WorkflowRejected("no_pub_sources")
    return {"schema": INVENTORY_SCHEMA, "input_kind": kind,
            "candidate_pub_count": len(sources),
            "claim_boundary": "inventory_only_no_decode_or_format_admission"}


def run(source: Path, pdf_dir: Path, report_dir: Path,
        fallback_font: Path, pub_cli: str, timeout: int) -> tuple[int, dict]:
    _preflight(source, pdf_dir, report_dir)
    if timeout < 1 or timeout > 3600:
        raise WorkflowRejected("timeout_outside_1_to_3600")
    # Do not implement conversion or artifact validation here: the two merged
    # owner modules enforce their own admission, hashes and source fences.
    args = argparse.Namespace(root=source, output=pdf_dir,
                              fallback_font=fallback_font, pub_cli=pub_cli,
                              timeout=timeout)
    status, _batch_receipt = batch.run(args)
    if status not in (0, 1):
        raise WorkflowRejected("unexpected_batch_exit_status")
    operator = reporter.generate(pdf_dir)
    reporter.write_reports(operator, report_dir, pdf_dir)
    counts = operator["counts"]
    if (counts["integrity_rejected"] or counts["exception"]
            or counts["not_yet_in_checkpoint"] or status == 1):
        return 1, counts
    if counts["integrity_confirmed_review_required"] != operator["total_sources"]:
        raise WorkflowRejected("batch_denominator_mismatch")
    return 0, counts


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    actions = parser.add_subparsers(dest="action", required=True)
    scan = actions.add_parser("inventory", help="count candidate PUBs without parsing")
    scan.add_argument("input", type=Path, help="source folder or ZIP")
    convert = actions.add_parser("run", help="run the existing batch converter and report")
    convert.add_argument("input", type=Path, help="source folder or ZIP")
    convert.add_argument("--pdf-dir", type=Path, required=True)
    convert.add_argument("--report-dir", type=Path, required=True)
    convert.add_argument("--fallback-font", type=Path, required=True)
    convert.add_argument("--pub-cli", required=True)
    convert.add_argument("--timeout", type=int, default=300)
    options = parser.parse_args(argv)
    try:
        if options.action == "inventory":
            print(json.dumps(inventory(options.input), sort_keys=True))
            return 0
        code, counts = run(options.input, options.pdf_dir, options.report_dir,
                           options.fallback_font, options.pub_cli, options.timeout)
    except (WorkflowRejected, batch.InputRejected, reporter.ReportRejected,
            OSError, ValueError, zipfile.BadZipFile, zipfile.LargeZipFile) as error:
        # Never reveal full source paths, source contents or converter stderr.
        reason = (str(error) if isinstance(error, (WorkflowRejected,
                  batch.InputRejected, reporter.ReportRejected))
                  else "source_or_io_rejected")
        print("migration workflow rejected: " + reason, file=sys.stderr)
        return 2
    print("migration workflow integrity_review="
          + str(counts["integrity_confirmed_review_required"])
          + " rejected=" + str(counts["integrity_rejected"])
          + " failed=" + str(counts["exception"])
          + " pending=" + str(counts["not_yet_in_checkpoint"]))
    return code


if __name__ == "__main__":
    sys.exit(main())
