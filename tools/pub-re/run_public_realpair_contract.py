#!/usr/bin/env python3
"""Verify mature Publisher 0x2C real-pair attribution against the independent Apache POI corpus.

Downloads are private to the CI workspace and SHA/length checked before use.
Only numerical source-safe JSON receipts are retained. No Publisher COM.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import re
import subprocess
import sys
import urllib.request
from pathlib import Path

UPSTREAM_REVISION = "942d95d85b15d0dfdb3bc9ba1b4f273f277757c8"
UPSTREAM_BASE = (
    "https://raw.githubusercontent.com/apache/poi/"
    + UPSTREAM_REVISION
    + "/test-data/publisher/"
)

# Frozen independently in Notion's Apache POI corpus manifest.
FIXTURES = {
    "Sample.pub": (72192, "6fefdef46b87c767150878dc384549cb2d2ec2ac54de25f8ddb3a5628301107e"),
    "Sample_2010.pub": (72704, "f0a90aa566d72eb822eb48b4f378c33007610e35f3f1b999cdb88f2c9683858b"),
    "Sample2.pub": (72704, "c87ea7cc5606023c5fa99376631bbd45a8e9bdaba34c958057995b0e7ac8cc86"),
    "Sample2_2010.pub": (73216, "931ff20980b8f72aab227fa3cbe61b017be8d17c337fecb9aca3c97075940777"),
    "Sample3.pub": (72192, "424c69173ff08948c2529c8084b4ac2403f1ff1057146f4edd02fc29b44481fc"),
    "Sample3_2010.pub": (72704, "baa9555a72d9e4264e3fd722896e2e43b960d20b4afdb96f75cb721a6133ff91"),
    "Sample4.pub": (72192, "42195f7ad23d911219fea3ec88e66e867e9b9a6821a16dd1b535e2aa9d57a11b"),
    "Sample4_2010.pub": (72704, "544e2043f91aed664c0a09f9ae45da42291b6730f107f99f3cdf9462ed38fb02"),
}

# Known independently observed identical object counts, not derived from this analyzer.
MATRIX = (
    ("Sample-self", "Sample.pub", "Sample.pub", 45),
    ("Sample", "Sample.pub", "Sample_2010.pub", 38),
    ("Sample2", "Sample2.pub", "Sample2_2010.pub", 36),
    ("Sample3", "Sample3.pub", "Sample3_2010.pub", 38),
    ("Sample4", "Sample4.pub", "Sample4_2010.pub", 38),
)
CFB_SIGNATURE = bytes.fromhex("d0cf11e0a1b11ae1")


def write_json(path: Path, value: object) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def fetch_fixture(name: str, private_dir: Path) -> Path:
    byte_count, sha256 = FIXTURES[name]
    if not re.fullmatch(r"Sample[2-4]?(?:_2010)?\.pub", name):
        raise ValueError("fixture not in constrained allowlist")
    url = UPSTREAM_BASE + name
    request = urllib.request.Request(
        url, headers={"User-Agent": "chaptera-pub-re-realpair-contract/1"}
    )
    with urllib.request.urlopen(request, timeout=35) as response:
        raw = response.read(byte_count + 1)
    if len(raw) != byte_count or hashlib.sha256(raw).hexdigest() != sha256:
        raise ValueError(f"{name}: immutable upstream SHA/length check failed")
    if not raw.startswith(CFB_SIGNATURE):
        raise ValueError(f"{name}: not a CFB signature")
    path = private_dir / name
    path.write_bytes(raw)
    return path


def run_pair(
    label: str, left: str, right: str, expected_identical: int,
    tool: Path, private_dir: Path, evidence_dir: Path,
) -> dict:
    manifest = private_dir / (label + "-manifest.json")
    receipt = evidence_dir / ("realpair-" + label.lower() + ".json")
    write_json(manifest, {
        "schema": "chaptera.pub-re-experiment.v1",
        "experiment_id": "PUB-RE-REALPAIR-01-" + label,
        "question": "Does mature Contents retain independently known raw chunks?",
        "before": {
            "path": left,
            "expected_sha256": FIXTURES[left][1],
        },
        "after": {
            "path": right,
            "expected_sha256": FIXTURES[right][1],
        },
    })
    proc = subprocess.run(
        [str(tool), "attribute-contents", "--manifest", str(manifest),
         "--output", str(receipt)],
        check=False, capture_output=True, text=True, timeout=35,
    )
    if proc.returncode != 0:
        # All input paths remain under the private run directory.
        sanitized = (proc.stderr or proc.stdout).replace(str(private_dir), "[private]")
        raise RuntimeError(
            f"{label}: analyzer rejected exact upstream pair, exit={proc.returncode}: "
            + sanitized[:600]
        )
    result = json.loads(receipt.read_text(encoding="utf-8"))
    required = {
        "schema": "chaptera.pub-re-contents-diff.v1",
        "before_slot_count": 306,
        "after_slot_count": 306,
        "compared_chunks": 45,
        "unmatched_chunks": 0,
        "unresolved_slots": 0,
        "unchanged_chunks": expected_identical,
        "changed_chunks": 45 - expected_identical,
        "raw_bytes_emitted": False,
        "absolute_paths_emitted": False,
        "whole_pub_semantic_equality_claimed": False,
    }
    for field, expected in required.items():
        actual = result.get(field)
        if actual != expected:
            raise AssertionError(
                f"{label}: independent corpus mismatch {field}: "
                f"observed={actual!r} expected={expected!r}"
            )
    if result["before_sha256"] != FIXTURES[left][1]:
        raise AssertionError(f"{label}: incorrect before provenance")
    if result["after_sha256"] != FIXTURES[right][1]:
        raise AssertionError(f"{label}: incorrect after provenance")
    if label == "Sample-self" and result["status"] != "referenced_chunks_unchanged":
        raise AssertionError("self-pair must report unchanged referenced chunks")
    if label != "Sample-self" and result["status"] not in {
        "referenced_chunk_payload_changed", "reference_or_slot_changed"
    }:
        raise AssertionError(
            f"{label}: unexpected conversion diff status={result['status']}"
        )
    print(
        f"{label}: slots={result['before_slot_count']} "
        f"chunks={result['compared_chunks']} identical={result['unchanged_chunks']} "
        f"changed={result['changed_chunks']} status={result['status']}",
        flush=True,
    )
    return {
        "pair_id": label,
        "before_sha256": FIXTURES[left][1],
        "after_sha256": FIXTURES[right][1],
        "expected_identical": expected_identical,
        "measured_identical": result["unchanged_chunks"],
        "changed_chunks": result["changed_chunks"],
        "slot_count": result["before_slot_count"],
        "chunk_count": result["compared_chunks"],
        "status": result["status"],
        "reference_metadata_unchanged": result["reference_metadata_unchanged"],
        "trailer_offset_delta": result["trailer_offset_delta"],
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--tool", type=Path, required=True)
    parser.add_argument("--work-root", type=Path, required=True)
    parser.add_argument("--evidence-dir", type=Path, required=True)
    args = parser.parse_args()
    if args.work_root.resolve() == args.evidence_dir.resolve():
        raise ValueError("raw private files and source-safe evidence must be separated")
    args.work_root.mkdir(parents=True, exist_ok=False)
    args.evidence_dir.mkdir(parents=True, exist_ok=False)
    private = args.work_root.resolve()
    evidence = args.evidence_dir.resolve()
    for name in FIXTURES:
        fetch_fixture(name, private)
    outcomes = []
    for label, left, right, unchanged in MATRIX:
        outcomes.append(run_pair(
            label, left, right, unchanged, args.tool.resolve(), private, evidence
        ))
    summary = {
        "schema": "chaptera.pub-re-realpair-contract.v1",
        "upstream": "apache/poi test-data/publisher",
        "upstream_commit": UPSTREAM_REVISION,
        "run_count": len(outcomes),
        "independent_baseline": "OBS-PUBTOOL-021 / exact Apache POI manifest",
        "all_independent_preservation_counts_match": True,
        "full_pub_semantic_equality_claimed": False,
        "native_publisher_executed": False,
        "raw_pub_files_uploaded": False,
        "pairs": outcomes,
    }
    write_json(evidence / "realpair-summary.json", summary)
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except Exception as exc:
        # No tracebacks with private absolute paths in hosted workflow logs.
        print(f"REALPAIR_CONTRACT_FAILED: {type(exc).__name__}: {exc}", file=sys.stderr)
        sys.exit(1)
