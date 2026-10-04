#!/usr/bin/env python3
"""Whole-document consumer matrix for bounded full-Story Center alignment.

Evidence-only. This script discovers the exact Center-bearing source set from
the pinned corpus, materializes editable targets, and requires two independent
consumer save/reopen paths. It never changes capability claims and records no
Story text.
"""

from __future__ import annotations

import argparse
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
        result = {
            "status": "success" if completed.returncode == 0 else "failure",
            "returncode": completed.returncode,
        }
        if completed.returncode != 0:
            result["stdout_tail"] = completed.stdout[-4000:]
            result["stderr_tail"] = completed.stderr[-4000:]
        return result
    except subprocess.TimeoutExpired:
        return {"status": "timeout", "returncode": None}


def probe_source(probe: Path, source: Path) -> dict:
    run = subprocess.run(
        [str(probe), str(source)],
        check=True,
        capture_output=True,
        text=True,
        timeout=30,
    )
    return json.loads(run.stdout)


def run_json_command(command: list[str], *, timeout: int) -> dict:
    try:
        completed = subprocess.run(
            command,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            timeout=timeout,
            check=False,
        )
    except subprocess.TimeoutExpired:
        return {"status": "timeout", "returncode": None}

    result = {
        "status": "success" if completed.returncode == 0 else "failure",
        "returncode": completed.returncode,
    }
    if completed.returncode != 0:
        result["stdout_tail"] = completed.stdout[-4000:]
        result["stderr_tail"] = completed.stderr[-4000:]
        return result

    try:
        result["payload"] = json.loads(completed.stdout)
    except json.JSONDecodeError:
        result["status"] = "failure"
        result["reason_code"] = "invalid_json"
        result["stdout_tail"] = completed.stdout[-4000:]
        result["stderr_tail"] = completed.stderr[-4000:]
    return result


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--corpus-root", type=Path, required=True)
    parser.add_argument("--alignment-probe", type=Path, required=True)
    parser.add_argument("--route-probe", type=Path, required=True)
    parser.add_argument("--verifier", type=Path, required=True)
    parser.add_argument("--scribus-helper", type=Path, required=True)
    parser.add_argument("--evidence-root", type=Path, required=True)
    args = parser.parse_args()

    args.evidence_root.mkdir(parents=True, exist_ok=True)

    selected: list[tuple[Path, dict]] = []
    for source in sorted(args.corpus_root.glob("*.pub")):
        probe = probe_source(args.alignment_probe, source)
        if any(item.get("alignment") == "center" for item in probe.get("items", [])):
            selected.append((source, probe))

    center_story_count = sum(
        sum(1 for item in probe.get("items", []) if item.get("alignment") == "center")
        for _, probe in selected
    )
    if len(selected) != 53 or center_story_count != 183:
        raise AssertionError(
            "Center class drifted from exact-1050 census: "
            f"sources={len(selected)} stories={center_story_count}"
        )

    results: list[dict] = []
    for source, alignment_probe in selected:
        sha = source.stem.casefold()
        case_root = args.evidence_root / sha
        case_root.mkdir(parents=True, exist_ok=True)

        all_items = alignment_probe.get("items", [])
        items = [item for item in all_items if item.get("alignment") == "center"]
        expected = {
            "schema": "chaptera.editable-paragraph-alignment-center-expected.v1",
            "source_sha256": alignment_probe["source_sha256"],
            "eligible_count": len(items),
            "center_story_count": len(items),
            "right_story_count": sum(
                1 for item in all_items if item.get("alignment") == "right"
            ),
            "items": items,
            "claims": {
                "story_text_recorded": False,
                "selection_basis": "exact_1232_center_class_v1",
            },
        }
        expected_path = case_root / "expected.json"
        expected_path.write_text(json.dumps(expected, indent=2, sort_keys=True) + "\n")

        row: dict = {
            "source_sha256": sha,
            "eligible_story_count": len(items),
            "center_story_count": expected["center_story_count"],
            "right_story_count": expected["right_story_count"],
        }

        materialized = case_root / "materialized"
        materialized.mkdir(parents=True, exist_ok=True)
        route = run_json_command(
            [
                str(args.route_probe),
                str(source),
                "--label",
                f"{sha}.pub",
                "--materialize-dir",
                str(materialized),
            ],
            timeout=120,
        )
        row["route_probe"] = {
            key: value for key, value in route.items() if key != "payload"
        }
        if route["status"] != "success":
            row["target_route_eligible"] = False
            results.append(row)
            continue

        route_payload = route["payload"]
        row["route"] = route_payload
        targets = route_payload.get("targets", {})
        target_route_eligible = all(
            targets.get(target, {}).get("state") == "available_with_declared_losses"
            and targets.get(target, {}).get("materialized") is True
            for target in ("idml", "odg")
        )
        row["target_route_eligible"] = target_route_eligible
        if not target_route_eligible:
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
        row["scribus_alignment_verify"] = scribus_verify

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
        row["libreoffice_alignment_verify"] = lo_verify
        results.append(row)

    available = [row for row in results if row.get("target_route_eligible") is True]
    excluded = [row for row in results if row.get("target_route_eligible") is False]

    def expected_preview_failed_exclusion(row: dict) -> bool:
        if row.get("route_probe", {}).get("status") != "success":
            return False
        route = row.get("route", {})
        if route.get("open_state") != "admitted":
            return False
        targets = route.get("targets", {})
        return all(
            targets.get(target, {}).get("state") == "not_verified"
            and targets.get(target, {}).get("reason_code") == "preview_failed"
            and targets.get(target, {}).get("materialized") is False
            for target in ("idml", "odg")
        )

    summary = {
        "all_center_source_routes_accounted": (
            len(available) + len(excluded) == len(results)
            and all(expected_preview_failed_exclusion(row) for row in excluded)
        ),
        "all_materializable_wire_verified": all(
            row.get("wire", {}).get("status") == "success"
            for row in available
        ),
        "all_materializable_scribus_save_reopen_verified": all(
            row.get("scribus_import_save", {}).get("status") == "success"
            and row.get("scribus_fresh_reopen_save", {}).get("status") == "success"
            and row.get("scribus_alignment_verify", {}).get("status") == "success"
            for row in available
        ),
        "all_materializable_libreoffice_save_reopen_verified": all(
            row.get("libreoffice_import_save", {}).get("status") == "success"
            and row.get("libreoffice_fresh_reopen_save", {}).get("status") == "success"
            and row.get("libreoffice_second_reopen_export", {}).get("status") == "success"
            and row.get("libreoffice_alignment_verify", {}).get("status") == "success"
            for row in available
        ),
    }
    materializable_center_class_proven = all(summary.values())
    receipt = {
        "schema": "chaptera.editable-paragraph-alignment-center-consumer-matrix.v1",
        "source_count": len(results),
        "center_story_count": sum(row["center_story_count"] for row in results),
        "materializable_source_count": len(available),
        "materializable_center_story_count": sum(
            row["center_story_count"] for row in available
        ),
        "non_materializable_source_count": len(excluded),
        "non_materializable_center_story_count": sum(
            row["center_story_count"] for row in excluded
        ),
        "non_materializable_source_sha256": sorted(
            row["source_sha256"] for row in excluded
        ),
        "right_story_count_in_selected_sources": sum(
            row["right_story_count"] for row in results
        ),
        "sources": results,
        "summary": summary,
        "claims": {
            "measurement_only": True,
            "consumer_survival_proven_for_materializable_center_class": (
                materializable_center_class_proven
            ),
            "consumer_survival_proven_for_full_center_source_class": False,
            "all_center_sources_materializable": len(excluded) == 0,
            "right_class_fully_proven": False,
            "loss_report_or_manifest_changed": False,
            "story_text_recorded": False,
        },
    }
    out = args.evidence_root / "receipt.json"
    out.write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n")
    print(json.dumps({
        "source_count": receipt["source_count"],
        "center_story_count": receipt["center_story_count"],
        "materializable_source_count": receipt["materializable_source_count"],
        "materializable_center_story_count": receipt[
            "materializable_center_story_count"
        ],
        "non_materializable_source_count": receipt[
            "non_materializable_source_count"
        ],
        "non_materializable_center_story_count": receipt[
            "non_materializable_center_story_count"
        ],
        "right_story_count_in_selected_sources": receipt[
            "right_story_count_in_selected_sources"
        ],
        "summary": receipt["summary"],
    }, indent=2, sort_keys=True))

    if not all(receipt["summary"].values()):
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
