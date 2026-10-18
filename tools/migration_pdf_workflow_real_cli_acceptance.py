#!/usr/bin/env python3
"""Source-free physical Rust CLI acceptance for the local Chaptera Migration PDF route.

Real PUB, generated PDF and licensed fallback font remain in an ephemeral private
CI workspace. Only exact hashes, counts and typed outcomes enter a public receipt.
This verifies actual execution/reuse, not Publisher visual or semantic fidelity.
"""
from __future__ import annotations

import argparse
import csv
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import zipfile

from rescue_batch_pdf_real_cli_acceptance import (
    AcceptanceFailure,
    digest_file,
    require,
    rows,
    successful_pdf,
)

SCHEMA = "chaptera.migration-pdf-workflow-real-cli-acceptance.v1"
OPERATOR_SCHEMA = "chaptera.migration-operator-pdf-report.v1"
BATCH_SCHEMA = "chaptera.rescue-batch-pdf.v1"
PUBLIC_FIXTURE_SHA = "6a825ba26ba35d6e885acdc62e859591ed37cb0ff7480b554b9cb362b644dfcf"
PUBLIC_FIXTURE_LENGTH = 291840


def read_json(path: Path, cap: int = 16 * 1024 * 1024) -> dict:
    try:
        require(path.is_file() and not path.is_symlink(), "missing_or_unsafe_receipt")
        require(0 < path.stat().st_size <= cap, "oversized_or_empty_receipt")
        document = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as exc:
        raise AcceptanceFailure("invalid_receipt_json") from exc
    require(isinstance(document, dict), "receipt_not_object")
    return document


def invoke(route: Path, action: str, source: Path, pdf: Path | None = None,
           reports: Path | None = None, font: Path | None = None,
           cli: Path | None = None, expected: int = 0) -> dict | None:
    command = [sys.executable, str(route), action, str(source)]
    if action == "run":
        require(all(item is not None for item in (pdf, reports, font, cli)),
                "missing_run_argument")
        command += ["--pdf-dir", str(pdf), "--report-dir", str(reports),
                    "--fallback-font", str(font), "--pub-cli", str(cli),
                    "--timeout", "120"]
    try:
        process = subprocess.run(command, capture_output=True, check=False,
                                 timeout=600)
    except (OSError, subprocess.TimeoutExpired) as exc:
        raise AcceptanceFailure("physical_route_subprocess_failed") from exc
    require(process.returncode == expected, "physical_route_exit_mismatch")
    # Do not forward captured document-relative paths from the local operator.
    if action == "inventory":
        try:
            result = json.loads(process.stdout)
        except (ValueError, UnicodeError) as exc:
            raise AcceptanceFailure("inventory_invalid_json") from exc
        require(isinstance(result, dict), "inventory_not_object")
        return result
    return None


def read_csv(path: Path) -> list[dict]:
    require(path.is_file() and not path.is_symlink(), "missing_operator_csv")
    require(path.stat().st_size <= 4 * 1024 * 1024, "operator_csv_too_large")
    try:
        with path.open(encoding="utf-8", newline="") as source:
            records = list(csv.DictReader(source))
    except (OSError, UnicodeError, csv.Error) as exc:
        raise AcceptanceFailure("invalid_operator_csv") from exc
    return records


def verify_batch(pdf: Path, report_dir: Path, expected_names: tuple[str, ...],
                 kind: str, valid_name: str, valid_state: str,
                 source_sha: str, font_sha: str, source_label: str,
                 expect_failed: bool) -> tuple[dict, int]:
    manifest = read_json(pdf / "manifest.json")
    require(manifest.get("schema") == BATCH_SCHEMA, "batch_schema_drift")
    actual = rows(manifest, expected_names, kind)
    positive = actual[valid_name]
    require(positive.get("status") == valid_state, "positive_status_drift")
    if expect_failed:
        failed = actual["bad.pub"]
        require(failed.get("status") == "failed", "malformed_file_was_not_isolated")
        require(failed.get("reason") == "converter_failed",
                "malformed_file_failure_reason_drift")
    hashes, mtime = successful_pdf(positive, source_sha, font_sha,
                                   pdf, source_label)

    report = read_json(report_dir / "operator-report.json")
    require(report.get("schema") == OPERATOR_SCHEMA, "operator_schema_drift")
    require(report.get("source_receipt_sha256") == digest_file(pdf / "manifest.json"),
            "operator_manifest_hash_not_bound")
    require(report.get("total_sources") == len(expected_names), "operator_denominator_drift")
    counts = report.get("counts")
    require(isinstance(counts, dict), "operator_counts_missing")
    require(counts.get("integrity_confirmed_review_required") == 1,
            "physical_pdf_not_confirmed_for_review")
    require(counts.get("exception") == int(expect_failed),
            "failed_input_not_reported")
    require(counts.get("integrity_rejected") == 0
            and counts.get("not_yet_in_checkpoint") == 0,
            "silent_pending_or_integrity_failure")
    require(report.get("claim_boundary")
            == "operator_only_integrity_not_visual_fidelity_or_migration_certification",
            "operator_claim_boundary_broadened")
    require(report.get("checkpoint_rows") == len(expected_names),
            "checkpoint_rows_drift")
    entries = report.get("entries")
    require(isinstance(entries, list) and len(entries) == len(expected_names),
            "operator_entries_drift")
    by_relative = {entry["source_relative_path"]: entry for entry in entries}
    require(set(by_relative) == set(expected_names), "operator_source_identity_drift")
    require(by_relative[valid_name].get("state")
            == "integrity_confirmed_review_required", "visual_review_was_waived")
    require(by_relative[valid_name].get("pdf_sha256") == hashes["pdf_sha256"],
            "operator_pdf_hash_not_bound")
    if expect_failed:
        require(by_relative["bad.pub"].get("state") == "exception",
                "failed_pub_not_exception")

    csv_rows = read_csv(report_dir / "operator-report.csv")
    exceptions = read_csv(report_dir / "exceptions.csv")
    require(len(csv_rows) == len(expected_names), "operator_csv_row_count")
    require({row["source_relative_path"] for row in csv_rows} == set(expected_names),
            "operator_csv_identity_drift")
    require(len(exceptions) == int(expect_failed), "operator_exception_count_drift")
    if expect_failed:
        require(exceptions[0].get("source_relative_path") == "bad.pub",
                "operator_exception_identity_drift")
    return hashes, mtime


def accept(source: Path, exact_sha: str, font: Path, cli: Path,
           route: Path) -> dict:
    for path in (source, font, cli, route):
        require(path.is_file() and not path.is_symlink(), "required_input_not_regular")
    require(source.stat().st_size == PUBLIC_FIXTURE_LENGTH,
            "pinned_real_pub_size_drift")
    require(exact_sha == PUBLIC_FIXTURE_SHA and digest_file(source) == exact_sha,
            "pinned_real_pub_sha_drift")
    require(source.open("rb").read(8) == bytes.fromhex("d0cf11e0a1b11ae1"),
            "not_CFB_original")
    font_sha = digest_file(font)
    cli_sha = digest_file(cli)
    with tempfile.TemporaryDirectory(prefix="chaptera-migration-real-") as temporary:
        root = Path(temporary)
        folder = root / "originals"
        folder.mkdir()
        actual = folder / "SampleNewsletter.pub"
        shutil.copyfile(source, actual)
        (folder / "bad.pub").write_bytes(b"not-a-valid-cfb-publisher-file\n")
        bad_sha = digest_file(folder / "bad.pub")

        inv = invoke(route, "inventory", folder)
        require(inv.get("schema") == "chaptera.migration-pdf-workflow-inventory.v1",
                "inventory_schema_drift")
        require(inv.get("candidate_pub_count") == 2, "real_folder_inventory_drift")
        require(inv.get("claim_boundary") == "inventory_only_no_decode_or_format_admission",
                "inventory_claim_broadened")

        pdf = root / "batch-pdfs"
        reports = root / "batch-reports"
        invoke(route, "run", folder, pdf, reports, font, cli, 1)
        h1, t1 = verify_batch(pdf, reports, ("SampleNewsletter.pub", "bad.pub"),
                              "directory", "SampleNewsletter.pub", "converted",
                              exact_sha, font_sha, "SampleNewsletter.pub", True)
        invoke(route, "run", folder, pdf, reports, font, cli, 1)
        h2, t2 = verify_batch(pdf, reports, ("SampleNewsletter.pub", "bad.pub"),
                              "directory", "SampleNewsletter.pub", "reused",
                              exact_sha, font_sha, "SampleNewsletter.pub", True)
        require(h2 == h1 and t2 == t1, "resumed_pdf_or_sidecars_changed")

        current_manifest = read_json(pdf / "manifest.json")
        ok = rows(current_manifest, ("SampleNewsletter.pub", "bad.pub"), "directory")
        name = ok["SampleNewsletter.pub"].get("output_pdf")
        require(isinstance(name, str) and "/" not in name and "\\" not in name,
                "unsafe_output_name")
        (pdf / (name + ".loss.txt")).write_text("tampered synthetic sidecar\n",
                                               encoding="utf-8")
        invoke(route, "run", folder, pdf, reports, font, cli, 1)
        h3, _t3 = verify_batch(pdf, reports, ("SampleNewsletter.pub", "bad.pub"),
                               "directory", "SampleNewsletter.pub", "converted",
                               exact_sha, font_sha, "SampleNewsletter.pub", True)
        require(h3 == h1, "real_reconversion_not_deterministic")
        require(digest_file(actual) == exact_sha, "source_pub_was_mutated")
        require(digest_file(folder / "bad.pub") == bad_sha, "invalid_pub_was_mutated")

        archive = root / "input.zip"
        with zipfile.ZipFile(archive, "w", compression=zipfile.ZIP_STORED) as zipped:
            zipped.writestr("nested/SampleNewsletter.pub", source.read_bytes())
        archive_sha = digest_file(archive)
        inventory = invoke(route, "inventory", archive)
        require(inventory.get("input_kind") == "zip"
                and inventory.get("candidate_pub_count") == 1,
                "real_zip_inventory_drift")
        pdfzip, reportzip = root / "zip-pdfs", root / "zip-reports"
        invoke(route, "run", archive, pdfzip, reportzip, font, cli, 0)
        hz1, zt1 = verify_batch(pdfzip, reportzip, ("nested/SampleNewsletter.pub",),
                                "zip", "nested/SampleNewsletter.pub", "converted",
                                exact_sha, font_sha, "source.pub", False)
        invoke(route, "run", archive, pdfzip, reportzip, font, cli, 0)
        hz2, zt2 = verify_batch(pdfzip, reportzip, ("nested/SampleNewsletter.pub",),
                                "zip", "nested/SampleNewsletter.pub", "reused",
                                exact_sha, font_sha, "source.pub", False)
        require(hz1 == hz2 and zt1 == zt2, "real_zip_reuse_changed_bytes")
        require(archive_sha == digest_file(archive), "original_zip_changed")
        return {
            "schema": SCHEMA,
            "fixture": "pinned-public-Apache-POI-SampleNewsletter",
            "fixture_sha256": exact_sha,
            "fixture_size_bytes": PUBLIC_FIXTURE_LENGTH,
            "compiled_pub_cli_sha256": cli_sha,
            "fallback_font_sha256": font_sha,
            "route": "real_compiled_pub_then_existing_batch_then_operator_report",
            "folder": {
                "first": "converted_and_one_exception",
                "second": "reused_and_one_exception",
                "after_tamper": "reconverted_and_one_exception",
                "pdf_sha256": h3["pdf_sha256"],
                "loss_json_sha256": h3["loss_json_sha256"],
                "loss_text_sha256": h3["loss_text_sha256"],
            },
            "zip": {
                "first": "converted_review_required",
                "second": "reused_review_required",
                "pdf_sha256": hz2["pdf_sha256"],
                "loss_json_sha256": hz2["loss_json_sha256"],
                "loss_text_sha256": hz2["loss_text_sha256"],
            },
            "invariants": {
                "genuine_compiled_rust_pub": True,
                "exact_public_CFB_source": True,
                "source_and_font_bound": True,
                "real_pdf_and_sidecars": True,
                "per_file_failure_in_exception_csv": True,
                "folder_zip_inventory_run": True,
                "operator_report_bound_to_manifest": True,
                "reused_outputs_untouched": True,
                "tamper_forces_reconversion": True,
                "source_inputs_unchanged": True,
                "requires_visual_loss_review": True,
                "publisher_layout_equivalence_proven": False,
                "raw_pub_pdf_font_upload": False,
            },
        }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--source-sha256", required=True)
    parser.add_argument("--font", type=Path, required=True)
    parser.add_argument("--pub-cli", type=Path, required=True)
    parser.add_argument("--workflow", type=Path, default=Path(__file__).with_name("migration_pdf_workflow_v1.py"))
    parser.add_argument("--receipt", type=Path, required=True)
    args = parser.parse_args(argv)
    try:
        require(args.source_sha256 == PUBLIC_FIXTURE_SHA, "fixture_identity_argument_drift")
        receipt = accept(args.source.resolve(), args.source_sha256,
                         args.font.resolve(), args.pub_cli.resolve(), args.workflow.resolve())
        destination = args.receipt.resolve()
        destination.parent.mkdir(parents=True, exist_ok=True)
        with destination.open("x", encoding="utf-8") as stream:
            json.dump(receipt, stream, sort_keys=True, indent=2)
            stream.write("\n")
        print(json.dumps(receipt, sort_keys=True))
        return 0
    except (AcceptanceFailure, OSError, ValueError, TypeError) as error:
        reason = str(error) if isinstance(error, AcceptanceFailure) else "acceptance_io_failure"
        print("MIGRATION_REAL_CLI_ACCEPTANCE_FAILED:" + reason, file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
