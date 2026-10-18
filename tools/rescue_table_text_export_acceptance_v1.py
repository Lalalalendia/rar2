#!/usr/bin/env python3
"""SHA-pinned private real-PUB acceptance of explicitly opted-in table export.

Exact files never leave the ephemeral runner. Persist only SHA labels and counts.
The previous mature/legacy text exporter is not invoked or modified here.
"""
from __future__ import annotations

import argparse
from collections import Counter
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
from zipfile import ZipFile

from tools import rescue_legacy_table_cell_proof_census_v1 as baseline
from tools import verify_rescue_table_text_evidence_v1 as verifier

MAX_PAYLOAD = 128 * 1024 * 1024


def worker(argv: list[str]) -> tuple[bool, bytes]:
    try:
        result = subprocess.run(
            argv, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL, timeout=90, check=False,
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        raise ValueError("table_export_worker_unavailable") from error
    if len(result.stdout) > 8192:
        raise ValueError("table_export_stdout_not_source_safe")
    return result.returncode == 0, result.stdout


def run(binary: Path, archive: Path, output: Path) -> dict:
    if not binary.is_file() or not archive.is_file() or baseline.sha_file(archive) != baseline.CORPUS_SHA:
        raise ValueError("source_or_binary_identity_untrusted")
    candidate_count = rejected_count = 0
    total_exported = total_cells = total_separators = total_source_bytes = 0
    positive_files = 0
    tamper_checked = unsupported_checked = False
    rows = []
    with ZipFile(archive) as z:
        names = z.namelist()
        members = {
            m.group(1): name for name in names
            if (m := baseline.MEMBER.fullmatch(name))
        }
        sidecar = z.read("materialized/sha256.txt")
        registered = [line.decode("ascii") for line in sidecar.splitlines()]
        if (len(members) != baseline.EXPECTED
                or len(names) != baseline.EXPECTED + 6
                or baseline.digest(sidecar) != baseline.SIDECAR_SHA
                or registered != sorted(members)
                or len(set(registered)) != baseline.EXPECTED):
            raise ValueError("untrusted_physical_corpus")
        with tempfile.TemporaryDirectory(prefix="chaptera-verified-table-private-") as td:
            source = Path(td) / "input.pub"
            private = Path(td) / "table-output"
            for sha in registered:
                payload = z.read(members[sha])
                if not (0 < len(payload) <= MAX_PAYLOAD and baseline.digest(payload) == sha):
                    raise ValueError("invalid_original_pub_sha")
                source.write_bytes(payload)
                old = baseline.check_probe(
                    baseline.run_probe(binary, source, "probe-legacy-text"),
                    sha, len(payload),
                )
                probe = baseline.check_cell_proof(
                    baseline.run_probe(binary, source, "probe-legacy-table-cells"),
                    old, sha, len(payload),
                )
                candidates = probe["projection_only_candidates"]
                proven = probe["proven_table_stories"]
                candidate_count += candidates
                rejected_count += sum(probe["refused"].values())
                total_source_bytes += probe["proven_source_cell_bytes"]
                if proven:
                    positive_files += 1
                    ok, stdout = worker([
                        str(binary), "extract-legacy-table-text", str(source),
                        "--output", str(private),
                    ])
                    if not ok or stdout.strip() != (
                        f"verified_table_story_ranges={proven}; native_pub_repair=false".encode("ascii")
                    ):
                        raise ValueError("source_exact_table_producer_failed")
                    result = verifier.verify(source, private, binary)
                    if (result["source_sha256"] != sha
                            or result["verified_table_stories"] != proven
                            or result["verified_source_cells"] != probe["proven_cell_count"]
                            or result["virtual_separator_count"] != probe["virtual_separator_count"]
                            or not result["source_byte_verified"]):
                        raise ValueError("table_export_independent_byte_evidence_mismatch")
                    # Re-running the *same* immutable source/output must not mutate evidence.
                    before = (
                        baseline.sha_file(private / "stories.json"),
                        baseline.sha_file(private / "stories.txt"),
                    )
                    again, _ = worker([
                        str(binary), "extract-legacy-table-text", str(source),
                        "--output", str(private),
                    ])
                    after = (
                        baseline.sha_file(private / "stories.json"),
                        baseline.sha_file(private / "stories.txt"),
                    )
                    if not again or before != after:
                        raise ValueError("table_export_repeat_not_idempotent")
                    if not tamper_checked:
                        target = private / "stories.txt"
                        target.write_bytes(b"intentionally-mutated-private-story")
                        if (worker([str(binary), "verify-legacy-table-text", str(source),
                                    "--output", str(private)])[0]
                                or worker([str(binary), "extract-legacy-table-text", str(source),
                                           "--output", str(private)])[0]):
                            raise ValueError("mutated_private_cell_story_accepted")
                        tamper_checked = True
                    total_exported += result["verified_table_stories"]
                    total_cells += result["verified_source_cells"]
                    total_separators += result["virtual_separator_count"]
                    shutil.rmtree(private)
                elif not unsupported_checked and old["family"] == "legacy_0x22_noquill":
                    if worker([
                        str(binary), "extract-legacy-table-text", str(source),
                        "--output", str(private),
                    ])[0] or private.exists():
                        raise ValueError("unproven_legacy_table_promoted")
                    unsupported_checked = True
                if baseline.sha_file(source) != sha:
                    raise ValueError("original_pub_mutated")
                rows.append({
                    "sha256": sha,
                    "source_family": old["family"],
                    "cell_proven_story_count": proven,
                    "cell_proof_refusals": probe["refused"],
                    "private_export_byte_verified": bool(proven),
                })
                source.unlink()
    summary = {
        "schema_version": "chaptera.rescue-table-export-acceptance.v1",
        "source_free": True,
        "source_archive_sha256": baseline.CORPUS_SHA,
        "source_artifact_id": 11038217075,
        "source_files": len(rows),
        "baseline_semantic_story_ranges": 11664,
        "projection_only_candidate_ranges": candidate_count,
        "verified_table_story_ranges": total_exported,
        "verified_table_cell_count": total_cells,
        "verified_table_source_cell_bytes": total_source_bytes,
        "verified_virtual_separator_count": total_separators,
        "refused_projection_count": rejected_count,
        "verified_table_story_files": positive_files,
        "tamper_checked": tamper_checked,
        "unproven_producer_refused": unsupported_checked,
        "all_sources_immutable": True,
        "native_pub_repair": False,
        "page_layout_reconstructed": False,
    }
    if (len(rows) != baseline.EXPECTED or candidate_count != 228
            or total_exported != 166 or rejected_count != 62
            or total_cells != 4334 or total_separators != 4168
            or total_source_bytes != 23955
            or not tamper_checked or not unsupported_checked):
        raise ValueError("table_export_real_corpus_acceptance_regressed")
    output.mkdir(parents=True, exist_ok=True)
    (output / "summary.json").write_text(
        json.dumps(summary, sort_keys=True, indent=2) + "\n", encoding="utf-8",
    )
    (output / "sha-only-ledger.json").write_text(
        json.dumps({
            "schema_version": "chaptera.rescue-table-export-ledger.v1",
            "source_free": True, "entries": rows,
        }, sort_keys=True) + "\n", encoding="utf-8",
    )
    print(json.dumps(summary, sort_keys=True))
    return summary


def main() -> int:
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument("--binary", type=Path, required=True)
    cli.add_argument("--archive", type=Path, required=True)
    cli.add_argument("--output", type=Path, required=True)
    args = cli.parse_args()
    try:
        run(args.binary, args.archive, args.output)
    except Exception:
        # Keep source paths and private Contents/parser errors out of public CI.
        print(json.dumps({
            "schema_version": "chaptera.rescue-table-export-acceptance.v1",
            "source_free": True, "status": "rejected",
            "reason": "private_cell_export_or_byte_check_failed",
        }))
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
