#!/usr/bin/env python3
"""Pinned real-CFB, real-Story batch replay: source-free two-pass admission.

The first pass uses the unchanged mature-only producer. The second pass opts
into a one-time legacy-v1 retry of unavailable files. Neither raw PUB bytes,
private text nor private queue state are written to public CI artifacts.
"""
from __future__ import annotations
import argparse
from collections import Counter
import json
from pathlib import Path
import tempfile
import time

from tools import rescue_corpus_1050_acceptance as cohort
from tools import rescue_desktop_batch_v1 as batch
from tools import rescue_salvage_bundle_v1 as salvage


def run(archive: Path, pub_cli: Path, shard: int, receipt: Path) -> dict:
    if shard not in (0, 1) or not pub_cli.is_file():
        raise ValueError("bad_batch_legacy_arguments")
    begin = time.monotonic()
    with tempfile.TemporaryDirectory(prefix="chaptera-legacy-batch-") as td:
        root = Path(td)
        sources = root / "sources"
        output = root / "output"
        sources.mkdir()
        sha_set, corpus_n, nbytes = cohort.prepare(archive, sources, shard, 2)
        if corpus_n != 1050 or len(sha_set) != 525:
            raise ValueError("incomplete_pinned_corpus_shard")
        scanned = batch.scan([sources], output)
        if scanned.get("unique_sources") != 525 or scanned.get("rejected_paths"):
            raise ValueError("batch_scan_not_exhaustive")
        baseline = batch.run(output, workers=4, timeout=45, file_timeout=180,
                             pub_cli=pub_cli)
        base = {r["sha256"]: r for r in baseline["entries"]}
        if set(base) != set(sha_set) or any(r["status"] not in batch.DONE for r in base.values()):
            raise ValueError("baseline_batch_incomplete")
        first_verified = sum(r["text_status"] == "verified" for r in base.values())
        first_ranges = sum(r["text_count"] or 0 for r in base.values())

        upgraded = batch.run(output, workers=4, timeout=45, file_timeout=180,
                             pub_cli=pub_cli, legacy_text_v1=True)
        post = {r["sha256"]: r for r in upgraded["entries"]}
        if set(post) != set(sha_set) or any(r["status"] not in batch.DONE for r in post.values()):
            raise ValueError("legacy_batch_incomplete")
        final_verified = sum(r["text_status"] == "verified" for r in post.values())
        final_ranges = sum(r["text_count"] or 0 for r in post.values())
        newly_verified = [
            sha for sha, row in post.items()
            if row["text_status"] == "verified" and base[sha]["text_status"] != "verified"
        ]
        if (final_verified < first_verified or final_ranges < first_ranges
                or any(r["text_status"] != "verified"
                       for sha, r in base.items() if r["text_status"] == "verified")):
            raise ValueError("mature_text_regression")
        # The PRIVATE queue should have independently recorded an exact
        # versioned re-probe for new text-positive files, not a public label.
        private = batch.load_queue(output)
        by_sha = {r["sha256"]: r for r in private["entries"]}
        if any(by_sha[sha].get("text_profile") != "legacy_0x22_v1" for sha in newly_verified):
            raise ValueError("legacy_reprobe_profile_missing")
        attempt_counts = {sha: r["attempts"] for sha, r in post.items()}
        repeat = batch.run(output, workers=4, timeout=45, file_timeout=180,
                           pub_cli=pub_cli, legacy_text_v1=True)
        by_repeat = {r["sha256"]: r for r in repeat["entries"]}
        if (set(by_repeat) != set(sha_set)
                or any(row["attempts"] != attempt_counts[sha]
                       or row["text_status"] != post[sha]["text_status"]
                       or row["text_count"] != post[sha]["text_count"]
                       for sha, row in by_repeat.items())):
            raise ValueError("legacy_batch_restart_not_idempotent")

        # Deliberate corruption is confined to disposable private output.
        tamper_ok = False
        if newly_verified:
            witness = newly_verified[0]
            path = output / "recovered-text" / witness / "stories.txt"
            path.write_bytes(b"corrupt_private_story_evidence")
            rejected = batch.run(output, workers=4, timeout=45, file_timeout=180)
            r = next(r for r in rejected["entries"] if r["sha256"] == witness)
            tamper_ok = r["status"] == "failed" and r["reason"] == "text_integrity"
        for sha in sha_set:
            if salvage.sha_file(sources / (sha + ".pub")) != sha:
                raise ValueError("real_source_sha_drift")
        for filename in ("summary.json", "entries.csv"):
            exported = (output / filename).read_text(encoding="utf-8")
            if "sources/" in exported or ".pub" in exported or "story_key" in exported:
                raise ValueError("private_content_in_source_free_report")
        result = {
            "schema_version": "chaptera.rescue-legacy-batch-1050.v1",
            "source_free": True, "artifact_id": 11038217075,
            "pinned_archive_sha256": cohort.EXPECTED_ARCHIVE_SHA,
            "shard": shard, "shards": 2, "source_count": 525,
            "source_bytes": nbytes, "baseline_verified_files": first_verified,
            "baseline_ranges": first_ranges, "final_verified_files": final_verified,
            "final_ranges": final_ranges, "new_legacy_verified_files": len(newly_verified),
            "status_counts": dict(sorted(Counter(r["status"] for r in post.values()).items())),
            "same_profile_reuse": True, "tamper_fail_closed": tamper_ok,
            "originals_unchanged": True, "native_pub_repaired": False,
            "elapsed_seconds": round(time.monotonic() - begin, 3),
        }
        receipt.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n",
                           encoding="utf-8")
        print(json.dumps(result, sort_keys=True))
        if not tamper_ok or not newly_verified:
            raise ValueError("legacy_batch_positive_tamper_gate_failed")
        return result


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--archive", type=Path, required=True)
    p.add_argument("--pub-cli", type=Path, required=True)
    p.add_argument("--shard", type=int, required=True)
    p.add_argument("--receipt", type=Path, required=True)
    opts = p.parse_args()
    try:
        run(opts.archive, opts.pub_cli, opts.shard, opts.receipt)
    except Exception:
        print(json.dumps({"schema_version": "chaptera.rescue-legacy-batch-1050.v1",
                          "source_free": True, "status": "rejected",
                          "reason": "typed_private_batch_gate_failure", "shard": opts.shard}))
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
