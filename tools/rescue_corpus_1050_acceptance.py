#!/usr/bin/env python3
"""Exact 1,050 real PUB Rescue batch acceptance from the existing GitHub artifact.

Only source-free aggregate receipts leave this process. Never upload source
files, private queue, extracted Story text or salvage bundles.
"""
from __future__ import annotations

import argparse
from collections import Counter
import hashlib
import json
import os
from pathlib import Path
import re
import tempfile
import time
from zipfile import ZipFile

from tools import rescue_desktop_batch_v1 as batch
from tools import rescue_salvage_bundle_v1 as salvage

EXPECTED_ARCHIVE_SHA = "687f1a5a5225e41344814a671cccd3ed9073203e5244c13a768baff566ea288e"
EXPECTED_LIST_SHA = "21e9c00963410e1004d407e6476f2e909c6561a4374b4ddadad052499e448d43"
EXPECTED_PUBS = 1050
MEMBER = re.compile(r"materialized/native/([0-9a-f]{64})\.pub\Z")
DONE = {"done_partial", "done_unsupported"}


def sha_file(file: Path) -> str:
    digest = hashlib.sha256()
    with file.open("rb") as fh:
        for block in iter(lambda: fh.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def prepare(archive: Path, directory: Path, shard: int, shards: int) -> tuple[list[str], int, int]:
    if sha_file(archive) != EXPECTED_ARCHIVE_SHA:
        raise ValueError("corpus_zip_identity_mismatch")
    hashes: list[str] = []
    selected: list[str] = []
    with ZipFile(archive) as z:
        infos = {m.filename: m for m in z.infolist()}
        items = {m.group(1): info for name, info in infos.items() if (m := MEMBER.fullmatch(name))}
        if len(items) != EXPECTED_PUBS or len(infos) != EXPECTED_PUBS + 6:
            raise ValueError("corpus_entry_count_or_names_mismatch")
        sidecar = z.read("materialized/sha256.txt")
        if hashlib.sha256(sidecar).hexdigest() != EXPECTED_LIST_SHA:
            raise ValueError("corpus_sidecar_changed")
        registered = [line.decode("ascii") for line in sidecar.splitlines()]
        if len(registered) != EXPECTED_PUBS or len(set(registered)) != EXPECTED_PUBS:
            raise ValueError("corpus_sidecar_not_unique")
        if registered != sorted(items):
            raise ValueError("corpus_sidecar_not_exact_files")
        summary = json.loads(z.read("materialized/summary.json"))
        if summary.get("unique_publisher_cfb") != EXPECTED_PUBS or summary.get("rejections") != 0:
            raise ValueError("corpus_manifest_wrong_authority")
        for i, digest in enumerate(registered):
            info = items[digest]
            if info.file_size < 1 or info.file_size > salvage.MAX_FILE_BYTES:
                raise ValueError("corpus_unsafe_file_size")
            if info.is_dir() or (info.external_attr >> 16) & 0o170000 == 0o120000:
                raise ValueError("corpus_symlink_or_directory")
            with z.open(info) as reader:
                content = reader.read(salvage.MAX_FILE_BYTES + 1)
            if (len(content) != info.file_size
                    or content[:8] != bytes.fromhex("d0cf11e0a1b11ae1")
                    or hashlib.sha256(content).hexdigest() != digest):
                raise ValueError("corpus_source_hash_or_magic_mismatch")
            hashes.append(digest)
            if i % shards == shard:
                (directory / (digest + ".pub")).write_bytes(content)
                selected.append(digest)
    return selected, len(hashes), sum((directory / (sha + ".pub")).stat().st_size for sha in selected)


def run_shard(args: argparse.Namespace) -> dict:
    if not 0 <= args.shard < args.shards <= 8:
        raise ValueError("invalid_shard_partition")
    if not args.pub_cli.is_file():
        raise ValueError("canonical_pub_cli_unavailable")
    start = time.monotonic()
    with tempfile.TemporaryDirectory(prefix="chaptera-real-corpus-") as td:
        root = Path(td)
        sources = root / "source"
        output = root / "out"
        sources.mkdir()
        selected, full_count, input_bytes = prepare(args.archive, sources, args.shard, args.shards)
        if not selected or full_count != EXPECTED_PUBS:
            raise ValueError("empty_or_incomplete_corpus_shard")
        observed = batch.scan([sources], output)
        if (observed["unique_sources"] != len(selected)
                or observed["duplicate_paths"] != 0
                or observed["rejected_paths"]):
            raise ValueError("batch_scan_incomplete")
        first = batch.run(output, workers=4, timeout=45, file_timeout=180,
                          pub_cli=args.pub_cli)
        statuses = Counter(row["status"] for row in first["entries"])
        if len(first["entries"]) != len(selected) or any(s not in DONE for s in statuses):
            # Capture exact failure reasons only as bounded reason enums.
            failure = Counter(str(row.get("reason")) for row in first["entries"]
                              if row["status"] not in DONE)
        else:
            failure = Counter()
        if not (all(row["sha256"] in selected for row in first["entries"])
                and first["source_free"]):
            raise ValueError("public_batch_receipt_invalid")
        text_files = sum(row["text_status"] == "verified" for row in first["entries"])
        text_ranges = sum(row["text_count"] or 0 for row in first["entries"])
        for digest in selected:
            if sha_file(sources / (digest + ".pub")) != digest:
                raise ValueError("original_PUB_changed")
        # If safe terminal, rerun the same 4-worker job to prove verified reuse.
        restart_ok = False
        if not failure:
            second = batch.run(output, workers=4, timeout=45, file_timeout=180,
                               pub_cli=args.pub_cli)
            restart_ok = all(
                a["sha256"] == b["sha256"] and a["status"] == b["status"]
                and a["attempts"] == b["attempts"] and a["text_count"] == b["text_count"]
                for a, b in zip(first["entries"], second["entries"])
            )
        tamper_ok = None
        if restart_ok and text_files:
            witness = next(row for row in first["entries"] if row["text_status"] == "verified")
            evidence = output / "recovered-text" / witness["sha256"] / "stories.txt"
            # Disposable output only. Never corrupt an original PUB.
            evidence.write_bytes(b"tampered_private_story_evidence")
            third = batch.run(output, workers=4, timeout=45, file_timeout=180)
            observed_row = next(row for row in third["entries"]
                                if row["sha256"] == witness["sha256"])
            tamper_ok = observed_row["status"] == "failed" and observed_row["reason"] == "text_integrity"
            if sha_file(sources / (witness["sha256"] + ".pub")) != witness["sha256"]:
                raise ValueError("tamper_test_modified_source")
        result = {
            "schema_version": "chaptera.rescue-real-corpus-1050.v1",
            "source_free": True,
            "artifact_id": 11038217075,
            "artifact_sha256": EXPECTED_ARCHIVE_SHA,
            "registered_in_artifact": full_count,
            "shard": args.shard,
            "shards": args.shards,
            "input_unique": len(selected),
            "input_bytes": input_bytes,
            "status_counts": dict(sorted(statuses.items())),
            "failure_reasons": dict(sorted(failure.items())),
            "text_verified_files": text_files,
            "text_ranges": text_ranges,
            "verified_image_assets": sum(row["asset_count"] or 0 for row in first["entries"]),
            "verified_physical_streams": sum(row["stream_count"] or 0 for row in first["entries"]),
            "tier_counts": dict(sorted(Counter(row["auto_tier"] for row in first["entries"]).items(),
                                        key=lambda pair: str(pair[0]))),
            "restart_idempotent": restart_ok,
            "tamper_rejected": tamper_ok,
            "source_immutable": True,
            "elapsed_seconds": round(time.monotonic() - start, 3),
            "native_pub_repaired": False,
            "publisher_layout_certified": False,
        }
        # The output is aggregate-only. Never echo or persist text or customer paths.
        args.receipt.write_text(json.dumps(result, ensure_ascii=True, sort_keys=True, indent=2) + "\n",
                                encoding="utf-8")
        print(json.dumps(result, ensure_ascii=True, sort_keys=True))
        if failure or not restart_ok or tamper_ok is False:
            raise ValueError("real_corpus_integrity_gate_failed")
        return result


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--archive", type=Path, required=True)
    parser.add_argument("--pub-cli", type=Path, required=True)
    parser.add_argument("--shard", type=int, required=True)
    parser.add_argument("--shards", type=int, default=2)
    parser.add_argument("--receipt", type=Path, required=True)
    args = parser.parse_args()
    try:
        run_shard(args)
    except (OSError, ValueError, batch.BatchError) as error:
        # The failure is typed. Do not print source paths or private output.
        print(json.dumps({"schema_version": "chaptera.rescue-real-corpus-1050.v1",
                          "source_free": True, "status": "rejected",
                          "reason": str(error) if isinstance(error, ValueError) else "execution_error",
                          "shard": args.shard}))
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
