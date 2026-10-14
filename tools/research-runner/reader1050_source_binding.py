#!/usr/bin/env python3
"""Fail-closed binding between a Reader 1050 baseline receipt and frontier source."""
from __future__ import annotations

import argparse
import json
import re
import subprocess
from pathlib import Path


def validate_source_binding(
    reader_root: Path, expected_sha: str, checkout_sha: str
) -> dict[str, str]:
    if not re.fullmatch(r"[0-9a-f]{40}", expected_sha):
        raise ValueError("expected Reader baseline SHA must be 40 lowercase hex characters")
    if checkout_sha != expected_sha:
        raise ValueError(
            f"Reader baseline source checkout mismatch: expected={expected_sha}, actual={checkout_sha}"
        )

    receipts = sorted(reader_root.rglob("acceptance.json"))
    if len(receipts) != 1:
        raise ValueError(f"expected one Reader 1050 acceptance.json, got {len(receipts)}")
    receipt = json.loads(receipts[0].read_text(encoding="utf-8"))
    if receipt.get("schema") != "chaptera.reader-1050-corpus-baseline.v1":
        raise ValueError("Reader 1050 baseline receipt schema mismatch")
    if receipt.get("rar_ref") != f"rar2:{expected_sha}":
        raise ValueError("Reader 1050 baseline receipt SHA does not match checked-out code")

    return {"source_sha":expected_sha, "rar_ref":receipt["rar_ref"]}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--reader-root", required=True, type=Path)
    parser.add_argument("--source-sha", required=True)
    args = parser.parse_args()
    checkout_sha = subprocess.check_output(
        ["git", "rev-parse", "HEAD"], text=True
    ).strip()
    result = validate_source_binding(args.reader_root, args.source_sha, checkout_sha)
    print(f"Reader 1050 baseline/code exact SHA matched: {result['source_sha']}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
