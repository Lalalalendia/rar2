#!/usr/bin/env python3
"""Offline, fail-closed Publisher 2019 visual verdict for exact LINE405 browser A/B.

Inputs are the two *original ZIP artifacts* from Actions run 38260303184 and
the external PDF-only Publisher oracle ZIP. Nothing is uploaded to GitHub.
Only a source-safe numerical JSON receipt is written to --output.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import stat
import tempfile
import zipfile
from pathlib import Path

from reader_logo_publisher_native_verdict_v1 import evaluate, require_exact_publisher_bundle

SCHEMA = "chaptera.reader-line405-offline-native-verdict.v1"
ACTIONS_RUN_ID = 38260303184
BASE_COMMIT = "47eb72faac780692335eef7f00ac0dbd9daaf206"
CANDIDATE_COMMIT = "846c735f2f7eb2ea92451236bdb146cb0eb481d8"
BASE_ARTIFACT_ID = 11752465627
CANDIDATE_ARTIFACT_ID = 11751779333
BASE_ARTIFACT_SHA = "fd21f8041019ea8c5d52b7ba86c9c4b3d3b7e48fbfc28e28d297150fe2226943"
CANDIDATE_ARTIFACT_SHA = "91fe08f33e22a776422c4c04f7f6e5580e9b6743f280ec044eede2317f5e4d31"
NATIVE_ZIP_FAMILY = "Microsoft Publisher 2019 PDF-only, 10 PDF / 20 pages"
FILES = frozenset(["receipt.json"] + [f"SampleNewsletter-page-{i}.png" for i in range(1, 5)])
MAX_RECEIPT_BYTES = 4 * 1024 * 1024
MAX_PNG_BYTES = 32 * 1024 * 1024
MAX_CAPTURE_BYTES = MAX_RECEIPT_BYTES + 4 * MAX_PNG_BYTES


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def extract_capture_artifact(archive_path: Path, destination: Path, expected_sha: str) -> Path:
    """Extract exactly the five verified GitHub artifact members, never paths."""
    if not archive_path.is_file() or not zipfile.is_zipfile(archive_path):
        raise ValueError("Reader capture artifact must be an existing ZIP")
    if sha256_file(archive_path) != expected_sha:
        raise ValueError("Reader capture artifact ZIP SHA256 mismatch")
    with zipfile.ZipFile(archive_path) as archive:
        members = archive.infolist()
        if len(members) != len(FILES):
            raise ValueError("Reader artifact must contain precisely four PNGs and one receipt")
        seen: set[str] = set()
        total = 0
        for member in members:
            filename = member.filename
            mode = (member.external_attr >> 16) & 0xFFFF
            limit = MAX_RECEIPT_BYTES if filename == "receipt.json" else MAX_PNG_BYTES
            if (
                filename not in FILES
                or filename in seen
                or member.is_dir()
                or "\\" in filename
                or member.flag_bits & 1
                or stat.S_ISLNK(mode)
                or (stat.S_IFMT(mode) not in (0, stat.S_IFREG))
                or member.compress_type not in (zipfile.ZIP_STORED, zipfile.ZIP_DEFLATED)
                or not 0 < member.file_size <= limit
            ):
                raise ValueError("unsafe, duplicated or unexpected Reader artifact member")
            seen.add(filename)
            total += member.file_size
            if total > MAX_CAPTURE_BYTES:
                raise ValueError("Reader artifact decompression budget exceeded")
        if seen != FILES:
            raise ValueError("Reader artifact missing expected receipt or page PNG")
        destination.mkdir(parents=True, exist_ok=True)
        for member in members:
            # Member names are now checked against a fixed literal whitelist.
            with archive.open(member) as stream:
                data = stream.read(member.file_size + 1)
            if len(data) != member.file_size:
                raise ValueError("Reader artifact decompressed size drift")
            (destination / member.filename).write_bytes(data)
    return destination / "receipt.json"


def run_verdict(before_zip: Path, after_zip: Path, publisher_zip: Path, output: Path) -> dict:
    """No network, no inferred source identities, no raw Publisher bytes in output."""
    inputs = (before_zip.resolve(), after_zip.resolve(), publisher_zip.resolve())
    if output.resolve() in inputs:
        raise ValueError("numerical output must never overwrite a source artifact")
    # Reject a wrong external oracle before extracting or processing either PNG set.
    require_exact_publisher_bundle(publisher_zip)
    with tempfile.TemporaryDirectory(prefix="reader-line405-native-") as temp:
        root = Path(temp)
        before = extract_capture_artifact(before_zip, root / "before", BASE_ARTIFACT_SHA)
        after = extract_capture_artifact(after_zip, root / "after", CANDIDATE_ARTIFACT_SHA)
        verdict = evaluate(
            before, after, publisher_zip,
            baseline_commit=BASE_COMMIT,
            candidate_commit=CANDIDATE_COMMIT,
            expected_added_nodes=1,
        )
        verdict["offline_runner_schema"] = SCHEMA
        verdict["source_capture_artifacts"] = {
            "github_actions_run_id": ACTIONS_RUN_ID,
            "baseline_artifact_id": BASE_ARTIFACT_ID,
            "baseline_zip_sha256": BASE_ARTIFACT_SHA,
            "candidate_artifact_id": CANDIDATE_ARTIFACT_ID,
            "candidate_zip_sha256": CANDIDATE_ARTIFACT_SHA,
            "artifact_zip_hashes_verified": True,
        }
        verdict["limitations"] = [
            "Publisher 2019 PDF is visual reference only, never PUB semantic evidence.",
            "ZIP has no cryptographic source-PUB to PDF pairing; external 2026-10-01 attestation remains the only pair link.",
            "Publisher build and native export stage not independently confirmed.",
            "No exact Publisher pixel parity claimed even if relative residual improves.",
            "Only derived numerical metadata is written; no PNG, PDF or PUB bytes in output.",
        ]
        output.parent.mkdir(parents=True, exist_ok=True)
        temporary_path = None
        try:
            with tempfile.NamedTemporaryFile(
                mode="w", encoding="utf-8", dir=output.parent,
                prefix=f".{output.name}.", suffix=".tmp", delete=False,
            ) as handle:
                temporary_path = Path(handle.name)
                json.dump(verdict, handle, indent=2, sort_keys=True)
                handle.write("\n")
            os.replace(temporary_path, output)
        finally:
            if temporary_path is not None:
                temporary_path.unlink(missing_ok=True)
        return verdict


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline-browser-zip", required=True, type=Path,
                        help=f"Original Actions artifact ZIP #{BASE_ARTIFACT_ID}")
    parser.add_argument("--candidate-browser-zip", required=True, type=Path,
                        help=f"Original Actions artifact ZIP #{CANDIDATE_ARTIFACT_ID}")
    parser.add_argument("--publisher-pdf-zip", required=True, type=Path,
                        help=NATIVE_ZIP_FAMILY + ", external/local only")
    parser.add_argument("--output", required=True, type=Path,
                        help="Numerical JSON only; never a raw PDF/PNG/PUB")
    args = parser.parse_args()
    verdict = run_verdict(
        args.baseline_browser_zip,
        args.candidate_browser_zip,
        args.publisher_pdf_zip,
        args.output,
    )
    print(
        "LINE405_PUBLISHER_NATIVE_PAGE4 "
        f"baseline={verdict['pages'][3]['baseline_significant_fraction_vs_publisher']:.8f} "
        f"candidate={verdict['pages'][3]['candidate_significant_fraction_vs_publisher']:.8f} "
        f"state={verdict['page4_disposition']}"
    )


if __name__ == "__main__":
    main()
