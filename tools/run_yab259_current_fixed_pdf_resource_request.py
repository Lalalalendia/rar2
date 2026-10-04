#!/usr/bin/env python3
"""Materialize a low-level fixed-PDF render request inside repaired Yab #259."""

from __future__ import annotations

import argparse
import json
import pathlib
import shutil
import subprocess
import sys
import tempfile

ROOT = pathlib.Path(__file__).resolve().parents[1]
ADAPTER = ROOT / "tools" / "yab259_current_fixed_pdf_resource_request.rs"

sys.path.insert(0, str(ROOT / "tools"))
from run_yab259_fixed_pdf_closure import (  # noqa: E402
    Yab259ClosureError,
    bind_yab_repository,
    prepare_repaired_donor,
)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--yab-checkout", required=True, type=pathlib.Path)
    parser.add_argument("--input", required=True, type=pathlib.Path)
    parser.add_argument("--output", required=True, type=pathlib.Path)
    args = parser.parse_args()

    try:
        repository = bind_yab_repository(args.yab_checkout)
        request = json.loads(args.input.read_text(encoding="utf-8"))
        if request.get("protocol_version") != "chaptera.current-fixed-pdf-resource-input.v1":
            raise Yab259ClosureError("current fixed-PDF resource input protocol mismatch")
        binding = request.get("binding")

        with tempfile.TemporaryDirectory(prefix="chaptera-yab259-resources-") as tmp:
            donor = pathlib.Path(tmp) / "donor"
            repair_sha256 = prepare_repaired_donor(repository, donor)
            bin_dir = donor / "crates" / "pub-cli" / "src" / "bin"
            bin_dir.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(ADAPTER, bin_dir / "current-fixed-pdf-request.rs")

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
                    "current-fixed-pdf-request",
                ],
                cwd=donor,
                stdin=subprocess.PIPE,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                input=json.dumps(request, ensure_ascii=False, separators=(",", ":")),
                text=True,
                check=False,
            )
            if completed.returncode != 0:
                detail = completed.stderr.strip()
                raise Yab259ClosureError(
                    "current fixed-PDF resource materializer failed"
                    + (f": {detail}" if detail else "")
                )
            output = json.loads(completed.stdout)
            if output.get("protocol_version") != "chaptera.fixed-pdf-packet-render-request.v1":
                raise Yab259ClosureError("materialized fixed-PDF request protocol mismatch")
            if output.get("binding") != binding:
                raise Yab259ClosureError("materialized fixed-PDF request binding mismatch")
            if not isinstance(output.get("scene"), dict):
                raise Yab259ClosureError("materialized fixed-PDF scene missing")
            resources = output.get("resources")
            if not isinstance(resources, dict):
                raise Yab259ClosureError("materialized FixedPdfResources missing")
            if not isinstance(resources.get("text_runs", []), list):
                raise Yab259ClosureError("materialized fixed text runs malformed")

            args.output.parent.mkdir(parents=True, exist_ok=True)
            args.output.write_text(
                json.dumps(output, ensure_ascii=False, indent=2, sort_keys=True) + "\n",
                encoding="utf-8",
            )
            print(f"prepared_yab259_repair_sha256={repair_sha256}", file=sys.stderr)
            return 0
    except (OSError, json.JSONDecodeError, Yab259ClosureError) as error:
        print(str(error), file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
