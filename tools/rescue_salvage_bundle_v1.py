#!/usr/bin/env python3
"""Bounded local Chaptera Rescue salvage bundle, not a native PUB repair.

Strict CFB SID extraction and image export are delegated to *existing* tools.
No semantic text recovery is claimed. No input file is ever written or moved.
The bundle is private/local; only counts and exact SHA identities may be shared.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile

from tools import rescue_windows_job_guard as job_guard

SCHEMA = "chaptera.rescue-salvage-bundle.v1"
ASSET_SCHEMA = "pub-assets-v0.1"
PHYSICAL_SCHEMA = "chaptera.recovery-cfb-stream-export.v1"
REPO_ROOT = Path(__file__).resolve().parents[1]
SHA = re.compile(r"[a-f0-9]{64}\Z")
SAFE_NAME = re.compile(r"[a-zA-Z0-9][a-zA-Z0-9._-]{0,127}\Z")
MAX_FILE_BYTES = 128 * 1024 * 1024


class UnsafeInput(ValueError):
    pass


def sha_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def no_symlink_components(path: Path) -> None:
    if any(p.is_symlink() for p in (path, *path.parents)):
        raise UnsafeInput("symlink_path")


def require_source(source: Path, output: Path) -> tuple[str, int]:
    no_symlink_components(source)
    no_symlink_components(output)
    if not source.is_file() or source.suffix.casefold() != ".pub":
        raise UnsafeInput("input_must_be_regular_pub")
    if output.resolve() == source.resolve() or output.resolve() in source.resolve().parents:
        raise UnsafeInput("output_overlaps_source")
    if output.resolve() == source.parent.resolve():
        raise UnsafeInput("output_must_be_dedicated_directory")
    n = source.stat().st_size
    if not 0 < n <= MAX_FILE_BYTES:
        raise UnsafeInput("invalid_source_size")
    digest = sha_file(source)
    if SHA.fullmatch(source.stem.casefold()) and source.stem.casefold() != digest:
        raise UnsafeInput("hash_named_source_drift")
    return digest, n


def run_command(args: list[str], timeout: int) -> str | None:
    try:
        result = subprocess.run(args, cwd=REPO_ROOT, stdin=subprocess.DEVNULL,
                                stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                                timeout=timeout, check=False)
    except subprocess.TimeoutExpired:
        return "timeout"
    except OSError:
        return "unavailable"
    return None if result.returncode == 0 else "failed"


def load_json(path: Path) -> dict:
    if path.is_symlink() or not path.is_file() or path.stat().st_size > 8 * 1024 * 1024:
        raise UnsafeInput("missing_or_unsafe_manifest")
    payload = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(payload, dict):
        raise UnsafeInput("manifest_not_object")
    return payload


def check_asset_output(asset_dir: Path) -> tuple[int, int, str]:
    """Verify the exact asset manifest and detect extra/untrusted output files.

    Image span correctness is owned by the Rust producer, not reconstructed here.
    """
    info = load_json(asset_dir / "manifest.json")
    if info.get("schema_version") != ASSET_SCHEMA or not isinstance(info.get("assets"), list):
        raise UnsafeInput("unexpected_asset_schema")
    diagnostics = info.get("diagnostics", [])
    if not isinstance(diagnostics, list):
        raise UnsafeInput("invalid_asset_diagnostics")
    expected = {"manifest.json"}
    for item in info["assets"]:
        if not isinstance(item, dict):
            raise UnsafeInput("invalid_asset_record")
        name, digest, length = item.get("filename"), item.get("sha256"), item.get("byte_len")
        if (not isinstance(name, str) or name in (".", "..") or not SAFE_NAME.fullmatch(name)
                or name in expected or name == "manifest.json"):
            raise UnsafeInput("unsafe_or_duplicate_asset_name")
        if not isinstance(digest, str) or not SHA.fullmatch(digest):
            raise UnsafeInput("invalid_asset_sha")
        if type(length) is not int or length < 0 or length > MAX_FILE_BYTES:
            raise UnsafeInput("invalid_asset_size")
        artifact = asset_dir / name
        if artifact.is_symlink() or not artifact.is_file() or artifact.stat().st_size != length:
            raise UnsafeInput("asset_missing_or_wrong_size")
        if sha_file(artifact) != digest:
            raise UnsafeInput("asset_sha_mismatch")
        expected.add(name)
    if set(p.name for p in asset_dir.iterdir()) != expected:
        raise UnsafeInput("unexpected_extra_asset")
    return len(info["assets"]), len(diagnostics), sha_file(asset_dir / "manifest.json")


def check_no_symlinks(folder: Path) -> None:
    if folder.is_symlink() or not folder.is_dir():
        raise UnsafeInput("bundle_not_directory")
    for root, dirs, files in os.walk(folder, followlinks=False):
        for name in dirs + files:
            if (Path(root) / name).is_symlink():
                raise UnsafeInput("symlink_in_bundle")


def independent_physical_verify(source: Path, base: Path, timeout: int) -> tuple[int, str]:
    manifest = base / "physical" / "manifest.json"
    streams = base / "physical" / "streams"
    info = load_json(manifest)
    if info.get("schema_version") != PHYSICAL_SCHEMA or not isinstance(info.get("streams"), list):
        raise UnsafeInput("unexpected_physical_schema")
    if not streams.is_dir() or streams.is_symlink():
        raise UnsafeInput("missing_physical_streams")
    if any(p.is_symlink() for p in streams.iterdir()):
        raise UnsafeInput("symlink_physical_stream")
    expected_stream_names = [x.get("artifact_path") for x in info["streams"]]
    if (any(not isinstance(n, str) or not SAFE_NAME.fullmatch(n)
            or n in (".", "..") for n in expected_stream_names)
            or len(expected_stream_names) != len(set(expected_stream_names))
            or set(p.name for p in streams.iterdir()) != set(expected_stream_names)
            or set(p.name for p in (base / "physical").iterdir()) != {"manifest.json", "streams"}):
        raise UnsafeInput("unmanifested_physical_artifact")
    # Independent validator re-reads exact physical directory entries and bytes.
    # Receipt belongs to a temporary location, NOT to an immutable bundle.
    with tempfile.TemporaryDirectory(prefix=".verify-", dir=base.parent) as temporary:
        receipt = Path(temporary) / "verification.json"
        error = run_command([sys.executable, "-m", "tools.verify_rescue_cfb_stream_export",
                             str(source), "--artifact-dir", str(streams),
                             "--manifest", str(manifest), "--receipt", str(receipt)], timeout)
        if error:
            raise UnsafeInput("physical_verifier_" + error)
        verified = load_json(receipt)
        if (verified.get("pass") is not True or verified.get("source_sha256") != sha_file(source)
                or type(verified.get("verified_stream_count")) is not int
                or verified["verified_stream_count"] != info.get("artifact_count")):
            raise UnsafeInput("physical_verifier_receipt_invalid")
        return verified["verified_stream_count"], sha_file(manifest)


def build_gaps(physical: dict, images: dict) -> list[str]:
    gaps = ["semantic_text_not_implemented", "metadata_not_implemented",
            "native_pub_repair_not_attempted"]
    if physical.get("status") != "verified":
        gaps.append("physical_streams_unavailable")
    if images.get("status") != "verified":
        gaps.append("semantic_images_unavailable")
    elif images.get("diagnostic_count"):
        gaps.append("some_images_not_promoted")
    return sorted(gaps)


def outcome_for(physical: dict, images: dict) -> str:
    if ((physical.get("status") == "verified" and physical.get("stream_count", 0) > 0)
            or (images.get("status") == "verified" and images.get("asset_count", 0) > 0)):
        return "partial"
    return "unsupported"


def read_bundle_report(source: Path, bundle: Path, timeout: int) -> dict:
    check_no_symlinks(bundle)
    report = load_json(bundle / "bundle.json")
    if report.get("schema_version") != SCHEMA:
        raise UnsafeInput("bundle_schema_invalid")
    source_info = report.get("source", {})
    if not isinstance(source_info, dict):
        raise UnsafeInput("bundle_source_invalid")
    if (source_info.get("sha256") != sha_file(source)
            or source_info.get("byte_len") != source.stat().st_size):
        raise UnsafeInput("bundle_source_drift")
    allowed = {"bundle.json"}
    physical = report.get("physical", {})
    if physical.get("status") == "verified":
        count, digest = independent_physical_verify(source, bundle, timeout)
        if count != physical.get("stream_count") or digest != physical.get("manifest_sha256"):
            raise UnsafeInput("bundle_physical_mismatch")
        allowed.add("physical")
    elif physical.get("status") != "unavailable":
        raise UnsafeInput("bundle_physical_status_invalid")
    images = report.get("images", {})
    if images.get("status") == "verified":
        count, gaps, digest = check_asset_output(bundle / "images")
        if (count != images.get("asset_count") or gaps != images.get("diagnostic_count")
                or digest != images.get("manifest_sha256")):
            raise UnsafeInput("bundle_images_mismatch")
        allowed.add("images")
    elif images.get("status") not in ("not_requested", "unavailable"):
        raise UnsafeInput("bundle_images_status_invalid")
    original = report.get("original", {})
    if original.get("status") == "retained":
        p = bundle / "original.pub"
        if p.is_symlink() or not p.is_file() or sha_file(p) != source_info["sha256"]:
            raise UnsafeInput("bundle_original_mismatch")
        allowed.add("original.pub")
    elif original.get("status") != "external_source":
        raise UnsafeInput("bundle_original_status_invalid")
    if report.get("text") != {"status": "not_implemented"}:
        raise UnsafeInput("unproven_text_status")
    if report.get("native_pub_delivery_allowed") is not False or report.get("source_immutable") is not True:
        raise UnsafeInput("unsupported_native_or_immutability_claim")
    expected_gaps = build_gaps(physical, images)
    if report.get("known_gaps") != expected_gaps:
        raise UnsafeInput("bundle_gaps_inconsistent")
    if report.get("outcome") != outcome_for(physical, images):
        raise UnsafeInput("bundle_outcome_inconsistent")
    if set(p.name for p in bundle.iterdir()) != allowed:
        raise UnsafeInput("unexpected_bundle_payload")
    return report


def execute(source: Path, output: Path, *, timeout: int = 45,
            pub_cli: Path | None = None, retain_original: bool = False) -> tuple[str, dict]:
    if not 1 <= timeout <= 3600:
        raise UnsafeInput("invalid_timeout")
    # Direct salvage CLI must have the same Windows process-tree guard as
    # the GUI batch engine before starting its own exporter/verifier/pub CLI.
    try:
        job_guard.activate()
    except job_guard.JobGuardUnavailable as exc:
        raise UnsafeInput("resource_guard_unavailable") from exc
    digest, length = require_source(source, output)
    output.mkdir(parents=True, exist_ok=True)
    target = output / digest
    if target.is_symlink():
        raise UnsafeInput("symlink_existing_bundle")
    if target.exists():
        report = read_bundle_report(source, target, timeout)
        if retain_original and report["original"]["status"] != "retained":
            raise UnsafeInput("existing_bundle_missing_requested_original")
        if pub_cli is not None and report["images"]["status"] == "not_requested":
            raise UnsafeInput("existing_bundle_missing_requested_images")
        return "reused", report

    if pub_cli is not None:
        no_symlink_components(pub_cli)
        if not pub_cli.is_file():
            raise UnsafeInput("missing_pub_cli")
    with tempfile.TemporaryDirectory(prefix=".stage-rescue-", dir=output) as temp:
        stage = Path(temp) / "bundle"
        stage.mkdir()
        physical_dir = stage / "physical"
        physical_dir.mkdir()
        streams = physical_dir / "streams"
        physical_result = {"status": "unavailable", "reason": "not_attempted"}
        err = run_command([sys.executable, "-m", "tools.rescue_cfb_stream_export", str(source),
                           "--output-dir", str(streams), "--manifest", str(physical_dir / "manifest.json")], timeout)
        if err is None:
            try:
                count, manifest_hash = independent_physical_verify(source, stage, timeout)
                physical_result = {"status": "verified", "stream_count": count,
                                   "manifest_sha256": manifest_hash}
            except (UnsafeInput, OSError, ValueError, KeyError):
                err = "verification_failed"
        if err is not None:
            shutil.rmtree(physical_dir)
            physical_result = {"status": "unavailable", "reason": "physical_" + err}

        image_result = {"status": "not_requested"}
        if pub_cli is not None:
            image_dir = stage / "images"
            err = run_command([str(pub_cli), "extract-assets", str(source),
                               "--output", str(image_dir)], timeout)
            if err is None:
                try:
                    count, gaps, manifest_hash = check_asset_output(image_dir)
                    image_result = {"status": "verified", "asset_count": count,
                                    "diagnostic_count": gaps, "manifest_sha256": manifest_hash}
                except (UnsafeInput, OSError, ValueError, KeyError):
                    err = "verification_failed"
            if err is not None:
                if image_dir.exists():
                    shutil.rmtree(image_dir)
                image_result = {"status": "unavailable", "reason": "image_" + err}

        original = {"status": "external_source"}
        if retain_original:
            saved = stage / "original.pub"
            shutil.copyfile(source, saved)
            if sha_file(saved) != digest:
                raise UnsafeInput("original_copy_mismatch")
            original = {"status": "retained", "sha256": digest}

        if sha_file(source) != digest:
            raise UnsafeInput("source_changed_during_export")
        gaps = build_gaps(physical_result, image_result)
        report = {
            "schema_version": SCHEMA,
            "source": {"sha256": digest, "byte_len": length},
            "outcome": outcome_for(physical_result, image_result),
            "physical": physical_result,
            "images": image_result,
            "text": {"status": "not_implemented"},
            "original": original,
            "known_gaps": gaps,
            "native_pub_delivery_allowed": False,
            "source_immutable": True,
        }
        (stage / "bundle.json").write_text(
            json.dumps(report, indent=2, sort_keys=True, ensure_ascii=True) + "\n", encoding="utf-8")
        read_bundle_report(source, stage, timeout)
        # The output identity is SHA-named. Existing bundles are never overwritten.
        if target.exists():
            raise UnsafeInput("concurrent_bundle_collision")
        stage.rename(target)
    return "created", report


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path, help="source .pub (private; never uploaded)")
    parser.add_argument("--output", type=Path, required=True, help="output root outside source directory")
    parser.add_argument("--pub-cli", type=Path, help="optional Chaptera pub CLI for exact images")
    parser.add_argument("--retain-original", action="store_true", help="include exact full source as opaque evidence")
    parser.add_argument("--timeout", type=int, default=45)
    args = parser.parse_args(argv)
    try:
        status, report = execute(args.source, args.output, timeout=args.timeout,
                                 pub_cli=args.pub_cli, retain_original=args.retain_original)
    except (UnsafeInput, ValueError, OSError, json.JSONDecodeError) as exc:
        reason = str(exc) if isinstance(exc, UnsafeInput) else type(exc).__name__
        print(json.dumps({"schema_version": SCHEMA, "status": "rejected", "reason": reason}))
        return 2
    print(json.dumps({"schema_version": SCHEMA, "status": status,
                      "source_sha256": report["source"]["sha256"],
                      "outcome": report["outcome"],
                      "stream_count": report["physical"].get("stream_count", 0),
                      "asset_count": report["images"].get("asset_count", 0)}, sort_keys=True))
    return 0 if report["outcome"] == "partial" else 1


if __name__ == "__main__":
    raise SystemExit(main())
