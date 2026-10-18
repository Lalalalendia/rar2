#!/usr/bin/env python3
"""Exact SHA-bound real-PUB census of *cell* proofs, without exporting text.

This only consumes Chaptera's canonical Rust reader and pub-cfb proof command.
Public receipts have fixed typed keys, source SHA, and bounded counts.
"""
from __future__ import annotations

import argparse
from collections import Counter
import json
from pathlib import Path
import subprocess
import tempfile
from zipfile import ZipFile

from tools.rescue_legacy_story_census_v1 import (
    CORPUS_SHA, SIDECAR_SHA, EXPECTED, MEMBER, check_probe, digest, sha_file,
)

SCHEMA = "chaptera.rescue-table-cell-proof.v1"
REFUSALS = frozenset({
    "missing_or_ambiguous_owner", "incomplete_or_merged_grid",
    "unverified_cell_carrier", "mismatch_in_cell_projection",
    "source_stream_unavailable",
})
PROBE_KEYS = frozenset({
    "schema_version", "source_sha256", "source_byte_len", "family",
    "previously_admitted_stories", "projection_only_candidates",
    "proven_table_stories", "proven_cell_count", "proven_source_cell_bytes",
    "virtual_separator_count", "refused", "source_immutable",
    "semantic_aggregate_exported", "native_pub_repair_attempted",
    "private_text_exposed",
})
MAX_COUNT = 1_000_000


def check_cell_proof(value: object, original: dict, sha: str, length: int) -> dict:
    if type(value) is not dict or set(value) != PROBE_KEYS:
        raise ValueError("unexpected_cell_proof_fields")
    refused = value.get("refused")
    if type(refused) is not dict or set(refused) != REFUSALS:
        raise ValueError("unexpected_cell_proof_refusals")
    counts = (
        value.get("previously_admitted_stories"),
        value.get("projection_only_candidates"),
        value.get("proven_table_stories"),
        value.get("proven_cell_count"),
        value.get("proven_source_cell_bytes"),
        value.get("virtual_separator_count"),
        *refused.values(),
    )
    if any(type(n) is not int or not 0 <= n <= MAX_COUNT * 128 for n in counts):
        raise ValueError("invalid_cell_proof_count")
    if (value.get("schema_version") != SCHEMA
            or value.get("source_sha256") != sha
            or value.get("source_byte_len") != length
            or value.get("family") != original["family"]
            or value.get("source_immutable") is not True
            or value.get("semantic_aggregate_exported") is not False
            or value.get("native_pub_repair_attempted") is not False
            or value.get("private_text_exposed") is not False):
        raise ValueError("invalid_cell_proof_identity_or_authority")
    candidate = value["projection_only_candidates"]
    proven = value["proven_table_stories"]
    cells = value["proven_cell_count"]
    sep = value["virtual_separator_count"]
    if (value["previously_admitted_stories"] != original["confirmed_story_ranges"]
            or candidate != original["unadmitted_reasons"]["projection_only_exact"]
            or candidate > original["ambiguous_story_ranges"]
            or sum(refused.values()) + proven != candidate
            or proven > cells
            or cells - proven != sep
            or (proven == 0 and (cells or sep or value["proven_source_cell_bytes"]))
            or (proven > 0 and (cells == 0 or value["proven_source_cell_bytes"] > length))
            or (candidate != 0 and value["family"] != "legacy_0x22_noquill")):
        raise ValueError("cell_proof_partition_mismatch")
    return value


def run_probe(binary: Path, source: Path, command: str) -> dict:
    try:
        done = subprocess.run(
            [str(binary), command, str(source)],
            stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL, timeout=45, check=False,
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        raise ValueError("canonical_pub_probe_failed") from error
    if done.returncode != 0 or len(done.stdout) > 16384:
        raise ValueError("canonical_pub_probe_nonzero_or_oversized")
    try:
        return json.loads(done.stdout)
    except ValueError as error:
        raise ValueError("invalid_canonical_pub_probe_json") from error


def census(binary: Path, archive: Path, output: Path) -> dict:
    if not binary.is_file() or not archive.is_file() or sha_file(archive) != CORPUS_SHA:
        raise ValueError("untrusted_binary_or_corpus")
    families, statuses, refusals = Counter(), Counter(), Counter()
    proven_stories = proven_cells = proven_bytes = synthetic_seps = 0
    candidates = admitted = unproven_files = mixed = 0
    rows = []
    with ZipFile(archive) as z:
        names = z.namelist()
        members = {m.group(1): name for name in names if (m := MEMBER.fullmatch(name))}
        sidecar = z.read("materialized/sha256.txt")
        if (len(members) != EXPECTED or len(names) != EXPECTED + 6
                or digest(sidecar) != SIDECAR_SHA):
            raise ValueError("untrusted_corpus_membership")
        registered = [line.decode("ascii") for line in sidecar.splitlines()]
        if (len(registered) != EXPECTED or len(set(registered)) != EXPECTED
                or registered != sorted(members)):
            raise ValueError("untrusted_corpus_sidecar")
        with tempfile.TemporaryDirectory(prefix="chaptera-cell-proof-private-") as td:
            source = Path(td) / "input.pub"
            for sha in registered:
                with z.open(members[sha]) as stream:
                    payload = stream.read(128 * 1024 * 1024 + 1)
                if not (0 < len(payload) <= 128 * 1024 * 1024 and digest(payload) == sha):
                    raise ValueError("untrusted_pub_source")
                source.write_bytes(payload)
                old = check_probe(
                    run_probe(binary, source, "probe-legacy-text"), sha, len(payload),
                )
                result = check_cell_proof(
                    run_probe(binary, source, "probe-legacy-table-cells"),
                    old, sha, len(payload),
                )
                if sha_file(source) != sha:
                    raise ValueError("source_mutated_by_probes")
                families[result["family"]] += 1
                statuses[old["status"]] += 1
                refusals.update(result["refused"])
                candidates += result["projection_only_candidates"]
                admitted += result["previously_admitted_stories"]
                proven_stories += result["proven_table_stories"]
                proven_cells += result["proven_cell_count"]
                proven_bytes += result["proven_source_cell_bytes"]
                synthetic_seps += result["virtual_separator_count"]
                unproven_files += int(old["confirmed_story_ranges"] == 0)
                mixed += int(old["status"] == "verified_partial")
                rows.append({
                    "sha256": sha,
                    "family": result["family"],
                    "previously_admitted_stories": result["previously_admitted_stories"],
                    "projection_only_candidates": result["projection_only_candidates"],
                    "proven_table_stories": result["proven_table_stories"],
                    "proven_cell_count": result["proven_cell_count"],
                    "proven_source_cell_bytes": result["proven_source_cell_bytes"],
                    "virtual_separator_count": result["virtual_separator_count"],
                    "refused": result["refused"],
                })
                source.unlink()
    summary = {
        "schema_version": "chaptera.rescue-table-cell-census.v1",
        "source_free": True,
        "source_artifact_id": 11038217075,
        "source_archive_sha256": CORPUS_SHA,
        "exact_input_count": len(rows),
        "families": dict(sorted(families.items())),
        "statuses": dict(sorted(statuses.items())),
        "previously_admitted_story_ranges": admitted,
        "projection_only_story_candidates": candidates,
        "cell_proven_table_stories": proven_stories,
        "cell_proven_source_cells": proven_cells,
        "cell_proven_source_bytes": proven_bytes,
        "synthetic_virtual_separators": synthetic_seps,
        "refusal_counts": {key: refusals[key] for key in sorted(REFUSALS)},
        "unproven_text_files": unproven_files,
        "mixed_partial_files": mixed,
        "source_immutable": True,
        "semantic_aggregate_exported": False,
        "native_pub_repair_attempted": False,
        "independent_publisher_oracle": False,
    }
    if (len(rows) != EXPECTED or admitted != 11664
            or candidates != 228 or mixed != 79 or unproven_files != 39
            or sum(refusals.values()) + proven_stories != candidates
            or synthetic_seps != proven_cells - proven_stories
            or len({row["sha256"] for row in rows}) != EXPECTED):
        raise ValueError("real_corpus_cell_proof_regression")
    output.mkdir(parents=True, exist_ok=True)
    (output / "summary.json").write_text(
        json.dumps(summary, sort_keys=True, indent=2) + "\n", encoding="utf-8",
    )
    (output / "sha-only-ledger.json").write_text(
        json.dumps({
            "schema_version": "chaptera.rescue-table-cell-ledger.v1",
            "source_free": True, "entries": rows,
        }, sort_keys=True) + "\n", encoding="utf-8",
    )
    print(json.dumps(summary, sort_keys=True))
    return summary


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--binary", type=Path, required=True)
    p.add_argument("--archive", type=Path, required=True)
    p.add_argument("--output", type=Path, required=True)
    args = p.parse_args()
    try:
        census(args.binary, args.archive, args.output)
    except Exception:
        # CI never prints private original paths/text/error content.
        print(json.dumps({
            "schema_version": "chaptera.rescue-table-cell-census.v1",
            "source_free": True, "state": "rejected",
            "reason": "typed_private_census_failure",
        }))
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
