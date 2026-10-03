#!/usr/bin/env python3
"""Whole-document consumer matrix for the bounded Montserrat typography class.

This is evidence-only. It never changes target capability claims and records no
Story text.
"""

from __future__ import annotations

import argparse
import collections
import json
import os
import subprocess
from pathlib import Path


def run_command(
    command: list[str],
    *,
    timeout: int,
    env: dict[str, str] | None = None,
) -> dict:
    try:
        completed = subprocess.run(
            command,
            env=env,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            timeout=timeout,
            check=False,
        )
        return {
            "status": "success" if completed.returncode == 0 else "failure",
            "returncode": completed.returncode,
        }
    except subprocess.TimeoutExpired:
        return {"status": "timeout", "returncode": None}


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--corpus-root", type=Path, required=True)
    parser.add_argument("--typography-probe", type=Path, required=True)
    parser.add_argument("--route-probe", type=Path, required=True)
    parser.add_argument("--verifier", type=Path, required=True)
    parser.add_argument("--scribus-helper", type=Path, required=True)
    parser.add_argument("--evidence-root", type=Path, required=True)
    parser.add_argument("--source-sha", action="append", required=True)
    args = parser.parse_args()

    args.evidence_root.mkdir(parents=True, exist_ok=True)
    results: list[dict] = []

    for sha in args.source_sha:
        source = args.corpus_root / f"{sha}.pub"
        case_root = args.evidence_root / sha
        case_root.mkdir(parents=True, exist_ok=True)

        row: dict = {"source_sha256": sha}
        if not source.is_file():
            row["fatal"] = "missing_source"
            results.append(row)
            continue

        typography_run = subprocess.run(
            [str(args.typography_probe), str(source)],
            check=True,
            capture_output=True,
            text=True,
            timeout=30,
        )
        typography = json.loads(typography_run.stdout)
        items = [
            item
            for item in typography.get("items", [])
            if item.get("font_family", "").strip().casefold() == "montserrat"
        ]
        size_counts = collections.Counter(int(item["font_size_emu"]) for item in items)
        row["bounded_montserrat_story_count"] = len(items)
        row["size_emu_story_counts"] = {
            str(size): count for size, count in sorted(size_counts.items())
        }

        expected = {
            **typography,
            "eligible_count_in_source": typography.get("eligible_count", 0),
            "eligible_count": len(items),
            "items": items,
            "selection_basis": "exact_1091_montserrat_source_matrix_v1",
        }
        expected_path = case_root / "expected.json"
        expected_path.write_text(json.dumps(expected, indent=2, sort_keys=True) + "\n")

        materialized = case_root / "materialized"
        materialized.mkdir(parents=True, exist_ok=True)
        route = run_command(
            [
                str(args.route_probe),
                str(source),
                "--label",
                f"{sha}.pub",
                "--materialize-dir",
                str(materialized),
            ],
            timeout=90,
        )
        row["materialize"] = route
        if route["status"] != "success":
            results.append(row)
            continue

        wire = run_command(
            [
                "python3",
                str(args.verifier),
                "wire",
                "--expected",
                str(expected_path),
                "--root",
                str(materialized),
                "--receipt",
                str(case_root / "wire.json"),
            ],
            timeout=60,
        )
        row["wire"] = wire

        scribus_root = case_root / "scribus"
        scribus_root.mkdir(parents=True, exist_ok=True)
        round1 = scribus_root / "round1.sla"
        round2 = scribus_root / "round2.sla"

        env1 = os.environ.copy()
        env1["CHAPTERA_SCRIBUS_SLA_OUT"] = str(round1)
        scribus1 = run_command(
            [
                "xvfb-run",
                "-a",
                "scribus",
                "-g",
                "-ns",
                "-py",
                str(args.scribus_helper),
                "--",
                str(materialized / "output.idml"),
            ],
            timeout=180,
            env=env1,
        )
        row["scribus_import_save"] = scribus1

        if scribus1["status"] == "success" and round1.is_file():
            env2 = os.environ.copy()
            env2["CHAPTERA_SCRIBUS_SLA_OUT"] = str(round2)
            scribus2 = run_command(
                [
                    "xvfb-run",
                    "-a",
                    "scribus",
                    "-g",
                    "-ns",
                    "-py",
                    str(args.scribus_helper),
                    "--",
                    str(round1),
                ],
                timeout=180,
                env=env2,
            )
        else:
            scribus2 = {"status": "not_run", "returncode": None}
        row["scribus_fresh_reopen_save"] = scribus2

        if scribus2["status"] == "success" and round2.is_file():
            scribus_verify = run_command(
                [
                    "python3",
                    str(args.verifier),
                    "scribus",
                    "--expected",
                    str(expected_path),
                    "--input",
                    str(round2),
                    "--receipt",
                    str(case_root / "scribus.json"),
                ],
                timeout=60,
            )
        else:
            scribus_verify = {"status": "not_run", "returncode": None}
        row["scribus_typography_verify"] = scribus_verify

        lo1 = case_root / "lo1"
        lo2 = case_root / "lo2"
        lo3 = case_root / "lo3"
        lo1.mkdir(exist_ok=True)
        lo2.mkdir(exist_ok=True)
        lo3.mkdir(exist_ok=True)

        profile1 = (case_root / "lo-profile-1").resolve()
        profile2 = (case_root / "lo-profile-2").resolve()
        profile3 = (case_root / "lo-profile-3").resolve()
        lo_first = run_command(
            [
                "libreoffice",
                "--headless",
                "--nologo",
                "--nodefault",
                "--norestore",
                f"-env:UserInstallation=file://{profile1}",
                "--convert-to",
                "fodg",
                "--outdir",
                str(lo1),
                str(materialized / "output.odg"),
            ],
            timeout=120,
        )
        row["libreoffice_import_save"] = lo_first

        first_fodg = lo1 / "output.fodg"
        if lo_first["status"] == "success" and first_fodg.is_file():
            lo_second = run_command(
                [
                    "libreoffice",
                    "--headless",
                    "--nologo",
                    "--nodefault",
                    "--norestore",
                    f"-env:UserInstallation=file://{profile2}",
                    "--convert-to",
                    "odg",
                    "--outdir",
                    str(lo2),
                    str(first_fodg),
                ],
                timeout=120,
            )
        else:
            lo_second = {"status": "not_run", "returncode": None}
        row["libreoffice_fresh_reopen_save"] = lo_second

        second_odg = lo2 / "output.odg"
        if lo_second["status"] == "success" and second_odg.is_file():
            lo_third = run_command(
                [
                    "libreoffice",
                    "--headless",
                    "--nologo",
                    "--nodefault",
                    "--norestore",
                    f"-env:UserInstallation=file://{profile3}",
                    "--convert-to",
                    "fodg",
                    "--outdir",
                    str(lo3),
                    str(second_odg),
                ],
                timeout=120,
            )
        else:
            lo_third = {"status": "not_run", "returncode": None}
        row["libreoffice_second_reopen_export"] = lo_third

        final_fodg = lo3 / "output.fodg"
        if lo_third["status"] == "success" and final_fodg.is_file():
            lo_verify = run_command(
                [
                    "python3",
                    str(args.verifier),
                    "libreoffice",
                    "--expected",
                    str(expected_path),
                    "--input",
                    str(final_fodg),
                    "--receipt",
                    str(case_root / "libreoffice.json"),
                ],
                timeout=60,
            )
        else:
            lo_verify = {"status": "not_run", "returncode": None}
        row["libreoffice_typography_verify"] = lo_verify
        results.append(row)

    total_stories = sum(int(row.get("bounded_montserrat_story_count", 0)) for row in results)
    receipt = {
        "schema": "chaptera.editable-typography-montserrat-consumer-matrix.v1",
        "source_count": len(results),
        "bounded_montserrat_story_count": total_stories,
        "sources": results,
        "summary": {
            "all_materialized": all(row.get("materialize", {}).get("status") == "success" for row in results),
            "all_wire_verified": all(row.get("wire", {}).get("status") == "success" for row in results),
            "all_scribus_save_reopen_verified": all(
                row.get("scribus_import_save", {}).get("status") == "success"
                and row.get("scribus_fresh_reopen_save", {}).get("status") == "success"
                and row.get("scribus_typography_verify", {}).get("status") == "success"
                for row in results
            ),
            "all_libreoffice_save_reopen_verified": all(
                row.get("libreoffice_import_save", {}).get("status") == "success"
                and row.get("libreoffice_fresh_reopen_save", {}).get("status") == "success"
                and row.get("libreoffice_second_reopen_export", {}).get("status") == "success"
                and row.get("libreoffice_typography_verify", {}).get("status") == "success"
                for row in results
            ),
        },
        "claims": {
            "measurement_only": True,
            "per_story_preserved_disposition_proven": False,
            "loss_report_or_manifest_changed": False,
            "story_text_recorded": False,
        },
    }

    out = args.evidence_root / "receipt.json"
    out.write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n")
    print(json.dumps({
        "source_count": receipt["source_count"],
        "bounded_montserrat_story_count": receipt["bounded_montserrat_story_count"],
        "summary": receipt["summary"],
    }, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
