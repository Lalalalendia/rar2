#!/usr/bin/env python3
"""Run the source-neutral current-Editor packet renderer inside repaired Yab #259."""

from __future__ import annotations

import argparse
import pathlib
import shutil
import subprocess
import sys
import tempfile

ROOT = pathlib.Path(__file__).resolve().parents[1]
ADAPTER_SOURCE = ROOT / "tools" / "yab259_editor_packet_renderer.rs"

from run_yab259_fixed_pdf_closure import (  # noqa: E402
    Yab259ClosureError,
    bind_yab_repository,
    prepare_repaired_donor,
)


def install_packet_renderer(donor: pathlib.Path) -> pathlib.Path:
    target = donor / "crates" / "pub-cli" / "src" / "bin" / "editor_packet_renderer.rs"
    target.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(ADAPTER_SOURCE, target)
    return target


def renderer_command(donor: pathlib.Path) -> list[str]:
    return [
        "cargo",
        "run",
        "--quiet",
        "--manifest-path",
        str(donor / "Cargo.toml"),
        "-p",
        "pub-cli",
        "--bin",
        "editor_packet_renderer",
    ]


def check_command(donor: pathlib.Path) -> list[str]:
    return [
        "cargo",
        "check",
        "--manifest-path",
        str(donor / "Cargo.toml"),
        "-p",
        "pub-cli",
        "--bin",
        "editor_packet_renderer",
    ]


def main() -> int:
    parser = argparse.ArgumentParser(
        description=(
            "Prepare the exact repaired Yab #259 donor and run the thin "
            "source-neutral current-Editor packet renderer."
        )
    )
    parser.add_argument("--yab-checkout", required=True, type=pathlib.Path)
    parser.add_argument("--check-only", action="store_true")
    args = parser.parse_args()

    try:
        repository = bind_yab_repository(args.yab_checkout)
        with tempfile.TemporaryDirectory(prefix="chaptera-yab259-editor-renderer-") as temporary:
            donor = pathlib.Path(temporary) / "donor"
            repair_sha256 = prepare_repaired_donor(repository, donor)
            installed = install_packet_renderer(donor)
            print(f"prepared_yab259_repair_sha256={repair_sha256}", file=sys.stderr)
            print(f"installed_editor_packet_renderer={installed}", file=sys.stderr)

            if args.check_only:
                completed = subprocess.run(
                    check_command(donor),
                    cwd=ROOT,
                    stdin=subprocess.DEVNULL,
                    check=False,
                )
                return completed.returncode

            request = sys.stdin.buffer.read()
            completed = subprocess.run(
                renderer_command(donor),
                cwd=ROOT,
                input=request,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                check=False,
            )
            sys.stdout.buffer.write(completed.stdout)
            sys.stderr.buffer.write(completed.stderr)
            return completed.returncode
    except (Yab259ClosureError, OSError) as error:
        print(str(error), file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
