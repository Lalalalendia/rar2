#!/usr/bin/env python3
"""Run the exact recovered Yab #259 fixed-PDF engine through the rar2 closure runner."""

from __future__ import annotations

import argparse
import pathlib
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parents[1]
YAB259_HEAD = "4f5d8a57abc089d1ed8eb1326cf9bed602281f7e"


class Yab259ClosureError(RuntimeError):
    pass


def bind_yab_checkout(path: pathlib.Path) -> pathlib.Path:
    checkout = path.expanduser().resolve()
    manifest = checkout / "Cargo.toml"
    if not manifest.is_file():
        raise Yab259ClosureError(f"Yab checkout has no Cargo.toml: {checkout}")
    completed = subprocess.run(
        ["git", "-C", str(checkout), "rev-parse", "HEAD"],
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
        text=True,
    )
    if completed.returncode != 0:
        detail = completed.stderr.strip()
        raise Yab259ClosureError(
            "cannot resolve Yab checkout HEAD" + (f": {detail}" if detail else "")
        )
    head = completed.stdout.strip()
    if head != YAB259_HEAD:
        raise Yab259ClosureError(
            f"Yab checkout HEAD mismatch: expected={YAB259_HEAD} actual={head}"
        )
    return checkout


def engine_command(checkout: pathlib.Path) -> list[str]:
    return [
        "cargo",
        "run",
        "--quiet",
        "--manifest-path",
        str(checkout / "Cargo.toml"),
        "-p",
        "pub-cli",
        "--bin",
        "pub",
        "--",
        "convert",
        "{fixture}",
        "--to",
        "pdf",
        "--output",
        "{pdf}",
        "--fallback-font",
        "{font}",
        "--json",
    ]


def main() -> int:
    parser = argparse.ArgumentParser(
        description=(
            "Bind the exact Yab #259 head and run it as the fixed-PDF engine "
            "behind rar2 tools/run_local_fixed_pdf_shaped_flow.py"
        )
    )
    parser.add_argument("--yab-checkout", required=True, type=pathlib.Path)
    parser.add_argument("--fixture", required=True, type=pathlib.Path)
    parser.add_argument("--pdf-output", required=True, type=pathlib.Path)
    parser.add_argument("--receipt-output", required=True, type=pathlib.Path)
    parser.add_argument("--fallback-font", type=pathlib.Path)
    args = parser.parse_args()

    try:
        checkout = bind_yab_checkout(args.yab_checkout)
    except Yab259ClosureError as error:
        print(str(error), file=sys.stderr)
        return 2

    command = [
        sys.executable,
        str(ROOT / "tools" / "run_local_fixed_pdf_shaped_flow.py"),
        "--fixture",
        str(args.fixture),
        "--pdf-output",
        str(args.pdf_output),
        "--receipt-output",
        str(args.receipt_output),
    ]
    if args.fallback_font is not None:
        command.extend(["--fallback-font", str(args.fallback_font)])
    command.append("--")
    command.extend(engine_command(checkout))

    completed = subprocess.run(command, cwd=ROOT, check=False)
    return completed.returncode


if __name__ == "__main__":
    raise SystemExit(main())
