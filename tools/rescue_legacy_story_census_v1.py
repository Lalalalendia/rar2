#!/usr/bin/env python3
"""Source-free exact-byte census using only the canonical Rust legacy Story probe."""
from __future__ import annotations

import argparse
from collections import Counter
import hashlib
import json
from pathlib import Path
import re
import subprocess
import tempfile
from zipfile import ZipFile

CORPUS_SHA = "687f1a5a5225e41344814a671cccd3ed9073203e5244c13a768baff566ea288e"
SIDECAR_SHA = "21e9c00963410e1004d407e6476f2e909c6561a4374b4ddadad052499e448d43"
EXPECTED = 1050
MEMBER = re.compile(r"materialized/native/([0-9a-f]{64})\.pub\Z")
FAMILIES = {"mature_0x2c", "legacy_0x22_quill", "legacy_0x22_noquill", "unclassified"}
STATUSES = {"verified", "verified_partial", "ambiguous", "no_proven_story", "ineligible"}
SCHEMA = "chaptera.rescue-legacy-story-probe.v1"


def digest(content: bytes) -> str:
    return hashlib.sha256(content).hexdigest()


def check_probe(record: object, sha: str, length: int) -> dict:
    if not isinstance(record, dict):
        raise ValueError("invalid_probe_json")
    exact_keys = {"schema_version", "source_sha256", "source_byte_len",
                  "family", "status", "confirmed_story_ranges",
                  "ambiguous_story_ranges", "graph_diagnostic_count",
                  "source_immutable", "native_pub_repair_attempted",
                  "semantic_text_exposed"}
    if set(record) != exact_keys:
        raise ValueError("probe_unexpected_output_fields")
    family = record.get("family")
    status = record.get("status")
    count = record.get("confirmed_story_ranges")
    ambiguous = record.get("ambiguous_story_ranges")
    diagnostics = record.get("graph_diagnostic_count")
    if (record.get("schema_version") != SCHEMA
            or record.get("source_sha256") != sha
            or record.get("source_byte_len") != length
            or family not in FAMILIES or status not in STATUSES
            or record.get("source_immutable") is not True
            or record.get("native_pub_repair_attempted") is not False
            or record.get("semantic_text_exposed") is not False
            or any(type(n) is not int or not 0 <= n <= 16384
                   for n in (count, ambiguous, diagnostics))):
        raise ValueError("probe_evidence_invalid")
    if (status in ("verified", "verified_partial")) != (count > 0):
        raise ValueError("probe_status_count_mismatch")
    if family == "unclassified" and (status != "ineligible" or count or ambiguous):
        raise ValueError("unclassified_story_promoted")
    if family != "unclassified" and status == "ineligible":
        raise ValueError("classified_story_mislabeled")
    if status == "verified" and ambiguous:
        raise ValueError("ambiguous_marked_verified")
    if status == "verified_partial" and not ambiguous:
        raise ValueError("partial_without_ambiguity")
    if status == "ambiguous" and not ambiguous:
        raise ValueError("ambiguous_without_evidence")
    if status == "no_proven_story" and ambiguous:
        raise ValueError("missing_ambiguity_label")
    return record


def sha_file(file: Path) -> str:
    digestor = hashlib.sha256()
    with file.open("rb") as fh:
        for block in iter(lambda: fh.read(1024 * 1024), b""):
            digestor.update(block)
    return digestor.hexdigest()


def census(binary: Path, archive: Path, receipt_dir: Path) -> dict:
    if not binary.is_file() or sha_file(archive) != CORPUS_SHA:
        raise ValueError("missing_binary_or_archive_identity")
    statuses, families, matrix = Counter(), Counter(), Counter()
    rows = []
    ranges = 0
    with ZipFile(archive) as z:
        names = z.namelist()
        members = {m.group(1): name for name in names if (m := MEMBER.fullmatch(name))}
        sidecar = z.read("materialized/sha256.txt")
        if (len(members) != EXPECTED or len(names) != EXPECTED + 6
                or digest(sidecar) != SIDECAR_SHA):
            raise ValueError("invalid_corpus_membership")
        registered = [line.decode("ascii") for line in sidecar.splitlines()]
        if len(registered) != EXPECTED or len(set(registered)) != EXPECTED or registered != sorted(members):
            raise ValueError("invalid_sha_sidecar")
        with tempfile.TemporaryDirectory(prefix="rescue-legacy-census-") as temp:
            source = Path(temp) / "input.pub"
            for sha in registered:
                with z.open(members[sha]) as fh:
                    payload = fh.read(128 * 1024 * 1024 + 1)
                if not 0 < len(payload) <= 128 * 1024 * 1024 or digest(payload) != sha:
                    raise ValueError("source_identity_mismatch")
                source.write_bytes(payload)
                try:
                    proc = subprocess.run(
                        [str(binary), "probe-legacy-text", str(source)],
                        stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                        stderr=subprocess.DEVNULL, timeout=45, check=False)
                except (subprocess.TimeoutExpired, OSError) as error:
                    raise ValueError("legacy_probe_execution_failed") from error
                if proc.returncode != 0 or len(proc.stdout) > 16384:
                    raise ValueError("legacy_probe_nonzero_or_oversize")
                try:
                    parsed = json.loads(proc.stdout)
                except ValueError as error:
                    raise ValueError("legacy_probe_invalid_stdout") from error
                row = check_probe(parsed, sha, len(payload))
                if sha_file(source) != sha:
                    raise ValueError("source_mutated_by_probe")
                families[row["family"]] += 1
                statuses[row["status"]] += 1
                matrix[(row["family"], row["status"])] += 1
                ranges += row["confirmed_story_ranges"]
                rows.append({
                    "sha256": sha,
                    "family": row["family"],
                    "status": row["status"],
                    "confirmed_story_ranges": row["confirmed_story_ranges"],
                    "ambiguous_story_ranges": row["ambiguous_story_ranges"],
                    "graph_diagnostic_count": row["graph_diagnostic_count"],
                })
                source.unlink()
    if sum(families.values()) != EXPECTED or len(rows) != EXPECTED:
        raise ValueError("incomplete_corpus_census")
    summary = {
        "schema_version": "chaptera.rescue-legacy-story-census.v1",
        "source_free": True,
        "source_artifact_id": 11038217075,
        "source_archive_sha256": CORPUS_SHA,
        "exact_input_count": EXPECTED,
        "families": dict(sorted(families.items())),
        "statuses": dict(sorted(statuses.items())),
        "family_status_matrix": {f"{k[0]}:{k[1]}": v for k, v in sorted(matrix.items())},
        "confirmed_story_ranges": ranges,
        "legacy_verified_files": sum(
            x["family"].startswith("legacy_") and x["status"].startswith("verified") for x in rows),
        "legacy_verified_story_ranges": sum(
            x["confirmed_story_ranges"] for x in rows if x["family"].startswith("legacy_")),
        "baseline_mature_verified_files": 382,
        "baseline_mature_story_ranges": 5506,
        "no_native_pub_repair": True,
        "source_immutable": True,
        "independent_semantic_oracle": False,
    }
    receipt_dir.mkdir(parents=True, exist_ok=True)
    (receipt_dir / "summary.json").write_text(
        json.dumps(summary, sort_keys=True, indent=2) + "\n", encoding="utf-8")
    (receipt_dir / "sha-only-ledger.json").write_text(
        json.dumps({"schema_version": "chaptera.rescue-legacy-story-ledger.v1",
                    "source_free": True, "entries": rows}, sort_keys=True) + "\n",
        encoding="utf-8")
    print(json.dumps(summary, sort_keys=True))
    return summary


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--archive", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    try:
        census(args.binary, args.archive, args.output)
    except Exception:
        print(json.dumps({"schema_version": "chaptera.rescue-legacy-story-census.v1",
                          "source_free": True, "state": "rejected",
                          "reason": "typed_corpus_or_probe_failure"}))
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
