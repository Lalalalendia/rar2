#!/usr/bin/env python3
"""Source-safe acceptance of the real Chaptera PUB CLI behind rescue batch.

Only pinned, publicly redistributable samples may be used in public CI. This
script deliberately never prints source bytes, PDF bytes, document text, or
absolute local paths. It exercises the already-shipped conversion CLI; it is
not a second PDF renderer.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import zipfile

RECEIPT_SCHEMA = "chaptera.rescue-batch-real-cli-acceptance.v1"
BATCH_SCHEMA = "chaptera.rescue-batch-pdf.v1"
PDF_REPORT_SCHEMA = "free-pub-pdf-v0.1"


class AcceptanceFailure(Exception):
    """Stable, source-free error code, suitable for shared CI logs."""


def require(ok: bool, code: str) -> None:
    if not ok:
        raise AcceptanceFailure(code)


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def digest_file(path: Path) -> str:
    return digest(path.read_bytes())


def batch_once(batch_tool: Path, root: Path, output: Path, font: Path,
               cli: Path, expected_status: int) -> dict:
    command = [sys.executable, str(batch_tool), str(root), "--output", str(output),
               "--fallback-font", str(font), "--pub-cli", str(cli),
               "--timeout", "120"]
    try:
        run = subprocess.run(command, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                             check=False, timeout=360)
    except (OSError, subprocess.TimeoutExpired) as exc:
        raise AcceptanceFailure("batch_subprocess_failed") from exc
    require(run.returncode == expected_status, "batch_exit_code_mismatch")
    try:
        receipt = json.loads((output / "manifest.json").read_text(encoding="utf-8"))
    except (OSError, ValueError) as exc:
        raise AcceptanceFailure("batch_manifest_missing_or_invalid") from exc
    require(receipt.get("schema") == BATCH_SCHEMA, "batch_schema_mismatch")
    require(isinstance(receipt.get("entries"), list), "batch_entries_not_list")
    return receipt


def rows(manifest: dict, paths: tuple[str, ...], kind: str) -> dict[str, dict]:
    require(manifest.get("input_kind") == kind, "input_kind_mismatch")
    require(manifest.get("total_sources") == len(paths), "source_count_mismatch")
    values = manifest["entries"]
    require(all(isinstance(row, dict) for row in values), "batch_row_invalid")
    keyed = {row.get("source_relative_path"): row for row in values}
    require(set(keyed) == set(paths) and len(values) == len(paths), "source_identity_mismatch")
    return keyed


def successful_pdf(row: dict, source_sha: str, font_sha: str,
                   output: Path, expected_source_label: str) -> tuple[dict, int]:
    require(row.get("source_sha256") == source_sha, "source_hash_mismatch")
    require(row.get("fallback_font_sha256") == font_sha, "font_hash_mismatch")
    name = row.get("output_pdf")
    require(isinstance(name, str) and name.endswith(".pdf") and
            "/" not in name and "\\" not in name and name != ".pdf",
            "unsafe_output_pdf_name")
    paths = (output / name, output / (name + ".loss.json"),
             output / (name + ".loss.txt"))
    hashes = ("pdf_sha256", "loss_json_sha256", "loss_text_sha256")
    for path, key in zip(paths, hashes):
        require(path.is_file() and not path.is_symlink(), "missing_output_or_sidecar")
        require(path.stat().st_size > 0, "empty_output_or_sidecar")
        require(row.get(key) == digest_file(path), "output_digest_mismatch")
    require(paths[0].read_bytes().startswith(b"%PDF-1.7"), "real_pdf_header_missing")
    try:
        report = json.loads(paths[1].read_text(encoding="utf-8"))
    except (ValueError, OSError, UnicodeError) as exc:
        raise AcceptanceFailure("fixed_pdf_report_invalid") from exc
    require(report.get("schema_version") == PDF_REPORT_SCHEMA, "fixed_pdf_schema_mismatch")
    require(report.get("source", {}).get("label") == expected_source_label,
            "fixed_pdf_source_label_mismatch")
    require(report.get("conversion_profile", {}).get("source", {}).get("source_sha256") == source_sha,
            "fixed_pdf_source_hash_mismatch")
    require(report.get("target", {}).get("format") == "pdf", "fixed_pdf_target_mismatch")
    typography = report.get("typography", {})
    require(typography.get("fallback_font", {}).get("fingerprint_sha256") == font_sha,
            "fixed_pdf_fallback_font_mismatch")
    require(typography.get("disposition") == "explicit_user_fallback_not_source_font",
            "fixed_pdf_font_disposition_mismatch")
    return ({key: row[key] for key in hashes}, paths[0].stat().st_mtime_ns)


def accept(source: Path, expected_sha: str, font: Path, cli: Path,
           batch_tool: Path) -> dict:
    require(source.is_file() and not source.is_symlink(), "source_not_regular")
    require(font.is_file() and not font.is_symlink(), "font_not_regular")
    require(cli.is_file() and not cli.is_symlink(), "compiled_cli_not_regular")
    require(batch_tool.is_file() and not batch_tool.is_symlink(), "batch_tool_not_regular")
    require(expected_sha == digest_file(source), "pinned_source_sha_mismatch")
    source_sha = expected_sha
    font_sha = digest_file(font)
    with tempfile.TemporaryDirectory(prefix="chaptera-rescue-real-accept-") as temporary:
        work = Path(temporary)
        folder = work / "input"
        folder.mkdir()
        valid = folder / "SampleNewsletter.pub"
        shutil.copyfile(source, valid)
        (folder / "bad.pub").write_bytes(b"not-a-valid-cfb-publisher-file\n")
        invalid_sha = digest_file(folder / "bad.pub")
        out = work / "folder-output"
        manifest1 = batch_once(batch_tool, folder, out, font, cli, 1)
        first = rows(manifest1, ("SampleNewsletter.pub", "bad.pub"), "directory")
        require(first["SampleNewsletter.pub"].get("status") == "converted",
                "real_folder_not_converted")
        require(first["bad.pub"].get("status") == "failed", "invalid_pub_not_isolated")
        require(first["bad.pub"].get("source_sha256") == invalid_sha,
                "invalid_pub_source_unbound")
        require(first["bad.pub"].get("reason") == "converter_failed",
                "invalid_pub_failure_class_wrong")
        positive1, mtime1 = successful_pdf(first["SampleNewsletter.pub"], source_sha,
                                           font_sha, out, "SampleNewsletter.pub")
        require(manifest1.get("fallback_font_sha256") == font_sha,
                "manifest_font_hash_mismatch")

        manifest2 = batch_once(batch_tool, folder, out, font, cli, 1)
        second = rows(manifest2, ("SampleNewsletter.pub", "bad.pub"), "directory")
        require(second["SampleNewsletter.pub"].get("status") == "reused",
                "real_folder_not_reused")
        require(second["bad.pub"].get("status") == "failed", "failed_file_not_retried")
        positive2, mtime2 = successful_pdf(second["SampleNewsletter.pub"], source_sha,
                                           font_sha, out, "SampleNewsletter.pub")
        require(positive2 == positive1 and mtime2 == mtime1,
                "reused_output_was_modified")

        # A successful sidecar digest cannot remain authoritative after tamper.
        pdf_name = first["SampleNewsletter.pub"]["output_pdf"]
        (out / (pdf_name + ".loss.txt")).write_text("tampered synthetic loss sidecar\n",
                                                     encoding="utf-8")
        manifest3 = batch_once(batch_tool, folder, out, font, cli, 1)
        third = rows(manifest3, ("SampleNewsletter.pub", "bad.pub"), "directory")
        require(third["SampleNewsletter.pub"].get("status") == "converted",
                "loss_tamper_was_reused")
        positive3, _ = successful_pdf(third["SampleNewsletter.pub"], source_sha,
                                      font_sha, out, "SampleNewsletter.pub")
        require(positive3 == positive1, "repair_is_not_deterministic")

        archive = work / "inputs.zip"
        with zipfile.ZipFile(archive, "w", compression=zipfile.ZIP_STORED) as stream:
            stream.writestr("nested/SampleNewsletter.pub", source.read_bytes())
        zipped = work / "zip-output"
        zip1 = rows(batch_once(batch_tool, archive, zipped, font, cli, 0),
                    ("nested/SampleNewsletter.pub",), "zip")
        require(zip1["nested/SampleNewsletter.pub"].get("status") == "converted",
                "real_zip_not_converted")
        ziphash1, zipmtime1 = successful_pdf(zip1["nested/SampleNewsletter.pub"],
                                              source_sha, font_sha, zipped, "source.pub")
        zip2 = rows(batch_once(batch_tool, archive, zipped, font, cli, 0),
                    ("nested/SampleNewsletter.pub",), "zip")
        require(zip2["nested/SampleNewsletter.pub"].get("status") == "reused",
                "real_zip_not_reused")
        ziphash2, zipmtime2 = successful_pdf(zip2["nested/SampleNewsletter.pub"],
                                              source_sha, font_sha, zipped, "source.pub")
        require(ziphash2 == ziphash1 and zipmtime2 == zipmtime1,
                "reused_zip_output_was_modified")
        return {
            "schema": RECEIPT_SCHEMA,
            "source": "pinned-public-apache-poi-SampleNewsletter",
            "source_sha256": source_sha,
            "fallback_font_sha256": font_sha,
            "compiled_cli_sha256": digest_file(cli),
            "folder": {"first": "converted+failed", "second": "reused+failed",
                       "tamper": "reconverted+failed", **positive3},
            "zip": {"first": "converted", "second": "reused", **ziphash2},
            "invariants": {
                "pinned_source_identity": True,
                "real_cli_pdf": True,
                "per_file_error_isolation": True,
                "source_and_font_hash_bound": True,
                "loss_sidecars_complete": True,
                "resumption_does_not_rewrite": True,
                "sidecar_tamper_causes_reconversion": True,
                "folder_and_zip_supported": True,
                "raw_pub_or_pdf_artifacts_uploaded": False,
            },
        }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", required=True, type=Path,
                        help="already pinned public PUB sample")
    parser.add_argument("--source-sha256", required=True,
                        help="expected 64 lowercase hex SHA-256 of exact source")
    parser.add_argument("--font", required=True, type=Path)
    parser.add_argument("--pub-cli", required=True, type=Path)
    parser.add_argument("--batch-tool", type=Path,
                        default=Path(__file__).with_name("rescue_batch_pdf_v1.py"))
    parser.add_argument("--receipt", required=True, type=Path)
    options = parser.parse_args(argv)
    if len(options.source_sha256) != 64 or any(c not in "0123456789abcdef" for c in options.source_sha256):
        parser.error("--source-sha256 must be 64 lowercase hex digits")
    try:
        receipt = accept(options.source.resolve(), options.source_sha256,
                         options.font.resolve(), options.pub_cli.resolve(),
                         options.batch_tool.resolve())
        destination = options.receipt.resolve()
        destination.parent.mkdir(parents=True, exist_ok=True)
        with destination.open("x", encoding="utf-8") as stream:
            json.dump(receipt, stream, sort_keys=True, indent=2)
            stream.write("\n")
        # Source-safe facts only, no local paths or decoded PDF text.
        print(json.dumps(receipt, sort_keys=True))
        return 0
    except (AcceptanceFailure, OSError, ValueError) as exc:
        code = str(exc) if isinstance(exc, AcceptanceFailure) else "acceptance_io_or_validation_error"
        print("RESCUE_BATCH_REAL_CLI_ACCEPTANCE_FAILED:" + code, file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
