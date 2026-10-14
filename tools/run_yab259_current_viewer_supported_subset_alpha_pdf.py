#!/usr/bin/env python3
"""Map the current Viewer packet to the supported subset and render through exact alpha soft masks."""

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
MAPPER = ROOT / "tools" / "yab259_current_viewer_supported_subset_request.rs"
RENDERER = ROOT / "tools" / "yab259_fixed_pdf_packet_renderer.rs"

sys.path.insert(0, str(ROOT / "tools"))
from run_yab259_fixed_pdf_closure import Yab259ClosureError  # noqa: E402
from yab259_alpha_smask_donor import (  # noqa: E402
    bind_yab_repository,
    prepare_alpha_smask_pdf_donor,
)

INPUT_VERSION = "chaptera.current-viewer-fixed-pdf-input.v1"
MAPPER_VERSION = "chaptera.current-viewer-yab-supported-request.v1"
REQUEST_VERSION = "chaptera.fixed-pdf-packet-render-request.v1"
RESULT_VERSION = "chaptera.fixed-pdf-packet-render-result.v1"


def run_binary(
    donor: pathlib.Path,
    binary: str,
    payload: dict,
    *,
    env: dict[str, str] | None = None,
) -> dict:
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
            binary,
        ],
        cwd=donor,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        input=json.dumps(payload, ensure_ascii=False, separators=(",", ":")),
        text=True,
        encoding="utf-8",
        env=env,
        check=False,
    )
    if completed.returncode != 0:
        detail = completed.stderr.strip() or completed.stdout.strip()
        raise Yab259ClosureError(
            f"{binary} failed" + (f": {detail}" if detail else "")
        )
    try:
        value = json.loads(completed.stdout)
    except json.JSONDecodeError as error:
        raise Yab259ClosureError(f"{binary} emitted invalid JSON") from error
    if not isinstance(value, dict):
        raise Yab259ClosureError(f"{binary} output must be an object")
    return value


def write_json(path: pathlib.Path, value: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(
        json.dumps(value, ensure_ascii=False, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--yab-checkout", required=True, type=pathlib.Path)
    parser.add_argument("--input", required=True, type=pathlib.Path)
    parser.add_argument("--request-output", required=True, type=pathlib.Path)
    parser.add_argument("--mapping-output", required=True, type=pathlib.Path)
    parser.add_argument("--pdf-output", required=True, type=pathlib.Path)
    parser.add_argument("--renderer-result-output", required=True, type=pathlib.Path)
    args = parser.parse_args()

    try:
        repository = bind_yab_repository(args.yab_checkout)
        packet = json.loads(args.input.read_text(encoding="utf-8"))
        if not isinstance(packet, dict) or packet.get("protocol_version") != INPUT_VERSION:
            raise Yab259ClosureError("current Viewer packet protocol mismatch")
        binding = packet.get("binding")

        with tempfile.TemporaryDirectory(prefix="chaptera-yab259-carlton-alpha-") as tmp:
            donor = pathlib.Path(tmp) / "donor"
            base_digest, ordered_digest, alpha_digest = prepare_alpha_smask_pdf_donor(
                repository,
                donor,
            )
            bin_dir = donor / "crates" / "pub-cli" / "src" / "bin"
            bin_dir.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(MAPPER, bin_dir / "current-viewer-supported-request.rs")
            shutil.copyfile(RENDERER, bin_dir / "fixed-pdf-packet.rs")

            mapped = run_binary(donor, "current-viewer-supported-request", packet)
            if mapped.get("protocol_version") != MAPPER_VERSION:
                raise Yab259ClosureError("current Viewer supported-subset mapper protocol mismatch")
            if mapped.get("binding") != binding:
                raise Yab259ClosureError("current Viewer supported-subset binding mismatch")
            request = mapped.get("request")
            mapping = mapped.get("mapping")
            if not isinstance(request, dict) or request.get("protocol_version") != REQUEST_VERSION:
                raise Yab259ClosureError("materialized Yab render request is malformed")
            if request.get("binding") != binding:
                raise Yab259ClosureError("materialized Yab render request binding mismatch")
            if not isinstance(mapping, dict):
                raise Yab259ClosureError("supported-subset mapping summary missing")

            write_json(args.request_output, request)
            write_json(
                args.mapping_output,
                {
                    "schema": "chaptera.current-viewer-yab-supported-mapping.v1",
                    "base_repair_sha256": base_digest,
                    "ordered_repair_sha256": ordered_digest,
                    "alpha_repair_sha256": alpha_digest,
                    "binding": binding,
                    "mapping": mapping,
                },
            )

            env = dict(os.environ)
            args.pdf_output.parent.mkdir(parents=True, exist_ok=True)
            env["CHAPTERA_PDF_OUTPUT"] = str(args.pdf_output.resolve())
            renderer = run_binary(donor, "fixed-pdf-packet", request, env=env)
            if renderer.get("protocol_version") != RESULT_VERSION:
                raise Yab259ClosureError("fixed-PDF renderer result protocol mismatch")
            if renderer.get("binding") != binding:
                raise Yab259ClosureError("fixed-PDF renderer binding mismatch")
            if not args.pdf_output.is_file() or not args.pdf_output.read_bytes().startswith(b"%PDF-"):
                raise Yab259ClosureError("fixed-PDF renderer did not write a PDF")

            write_json(
                args.renderer_result_output,
                {
                    "schema": "chaptera.current-viewer-yab-supported-render.v1",
                    "base_repair_sha256": base_digest,
                    "ordered_repair_sha256": ordered_digest,
                    "alpha_repair_sha256": alpha_digest,
                    "renderer_result": renderer,
                },
            )
            return 0
    except (OSError, json.JSONDecodeError, RuntimeError, Yab259ClosureError) as error:
        print(str(error), file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
