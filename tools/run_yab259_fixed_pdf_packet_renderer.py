#!/usr/bin/env python3
"""Run the source-neutral fixed-PDF packet adapter inside repaired Yab #259."""

from __future__ import annotations

import argparse
import json
import os
import pathlib
import shutil
import subprocess
import sys
import tempfile

ROOT = pathlib.Path(__file__).resolve().parents[1]
ADAPTER = ROOT / "tools" / "yab259_fixed_pdf_packet_renderer.rs"

sys.path.insert(0, str(ROOT / "tools"))
from run_yab259_fixed_pdf_closure import (  # noqa: E402
    Yab259ClosureError,
    bind_yab_repository,
    prepare_repaired_donor,
)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--yab-checkout", required=True, type=pathlib.Path)
    parser.add_argument("--request", required=True, type=pathlib.Path)
    parser.add_argument("--pdf-output", required=True, type=pathlib.Path)
    parser.add_argument("--result-output", required=True, type=pathlib.Path)
    args = parser.parse_args()

    try:
        repository = bind_yab_repository(args.yab_checkout)
        request = json.loads(args.request.read_text(encoding="utf-8"))
        if request.get("protocol_version") != "chaptera.fixed-pdf-packet-render-request.v1":
            raise Yab259ClosureError("fixed-PDF packet request protocol mismatch")

        with tempfile.TemporaryDirectory(prefix="chaptera-yab259-packet-") as tmp:
            donor = pathlib.Path(tmp) / "donor"
            repair_sha256 = prepare_repaired_donor(repository, donor)
            bin_dir = donor / "crates" / "pub-cli" / "src" / "bin"
            bin_dir.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(ADAPTER, bin_dir / "fixed-pdf-packet.rs")

            env = dict(os.environ)
            args.pdf_output.parent.mkdir(parents=True, exist_ok=True)
            args.result_output.parent.mkdir(parents=True, exist_ok=True)
            env["CHAPTERA_PDF_OUTPUT"] = str(args.pdf_output.resolve())

            completed = subprocess.run(
                [
                    "cargo",
                    "run",
                    "--quiet",
                    "--manifest-path",
                    str(donor / "Cargo.toml"),
                    "-p",
                    "pub-cli",
                    "--bin",
                    "fixed-pdf-packet",
                ],
                cwd=donor,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                input=json.dumps(request, ensure_ascii=False, separators=(",", ":")),
                text=True,
                encoding="utf-8",
                env=env,
                check=False,
            )
            if completed.returncode != 0:
                detail = completed.stderr.strip()
                raise Yab259ClosureError(
                    "source-neutral Yab packet renderer failed"
                    + (f": {detail}" if detail else "")
                )
            result = json.loads(completed.stdout)
            if result.get("protocol_version") != "chaptera.fixed-pdf-packet-render-result.v1":
                raise Yab259ClosureError("fixed-PDF packet result protocol mismatch")
            if result.get("binding") != request.get("binding"):
                raise Yab259ClosureError("fixed-PDF packet binding echo mismatch")
            node_reports = result.get("node_reports")
            if not isinstance(node_reports, list):
                raise Yab259ClosureError("fixed-PDF packet node_reports missing")
            origins = []
            for index, report in enumerate(node_reports):
                if not isinstance(report, dict) or set(report) != {
                    "origin_node_id",
                    "disposition",
                    "code",
                }:
                    raise Yab259ClosureError(
                        f"fixed-PDF packet node_reports[{index}] malformed"
                    )
                origin = report["origin_node_id"]
                disposition = report["disposition"]
                code = report["code"]
                if not isinstance(origin, str) or not origin:
                    raise Yab259ClosureError(
                        f"fixed-PDF packet node_reports[{index}] origin missing"
                    )
                if disposition not in {"painted", "partial", "unsupported"}:
                    raise Yab259ClosureError(
                        f"fixed-PDF packet node_reports[{index}] disposition invalid"
                    )
                if not isinstance(code, str) or not code:
                    raise Yab259ClosureError(
                        f"fixed-PDF packet node_reports[{index}] code missing"
                    )
                origins.append(origin)
            if origins != sorted(set(origins)):
                raise Yab259ClosureError(
                    "fixed-PDF packet node_reports must be canonical unique sorted"
                )
            if not args.pdf_output.is_file() or not args.pdf_output.read_bytes().startswith(b"%PDF-"):
                raise Yab259ClosureError("fixed-PDF packet renderer did not write a PDF")
            args.result_output.write_text(
                json.dumps(
                    {
                        "repair_sha256": repair_sha256,
                        "renderer_result": result,
                    },
                    ensure_ascii=False,
                    indent=2,
                    sort_keys=True,
                )
                + "\n",
                encoding="utf-8",
            )
            return 0
    except (OSError, json.JSONDecodeError, Yab259ClosureError) as error:
        print(str(error), file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
