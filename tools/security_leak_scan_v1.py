#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import re
import sys
from dataclasses import dataclass
from pathlib import Path

MAX_FILE_BYTES = 32 * 1024 * 1024

BUILTIN_PATTERNS = [
    ("aws_presigned_signature", re.compile(r"(?i)(?:X-Amz-Signature|X-Amz-Credential|X-Amz-Security-Token)=")),
    ("bearer_token", re.compile(r"(?i)\bAuthorization\s*:\s*Bearer\s+[A-Za-z0-9._~+/=-]{8,}")),
    ("cookie_secret", re.compile(r"(?i)\b(?:session|session_token|csrf|csrf_token|refresh_token|access_token)\s*[=:]\s*[\"']?[A-Za-z0-9._~+/=-]{12,}")),
    ("private_key", re.compile(r"-----BEGIN (?:RSA |EC |OPENSSH )?PRIVATE KEY-----")),
    ("aws_access_key", re.compile(r"\b(?:AKIA|ASIA)[A-Z0-9]{16}\b")),
    ("raw_pub_marker", re.compile(r"(?i)\braw_pub_bytes\b")),
    ("provider_token_marker", re.compile(r"(?i)\bprovider_token\b")),
    ("private_source_path_marker", re.compile(r"(?i)\bprivate_(?:file_)?path\b")),
]


@dataclass(frozen=True)
class Finding:
    path: str
    kind: str
    line: int


def iter_files(paths: list[Path]):
    for root in paths:
        if root.is_symlink():
            raise ValueError(f"refusing symlink input: {root}")
        if root.is_file():
            yield root
            continue
        if not root.is_dir():
            raise ValueError(f"input does not exist: {root}")
        for path in sorted(root.rglob("*")):
            if path.is_symlink():
                continue
            if path.is_file():
                yield path


def scan_file(path: Path, forbidden_literals: list[str]) -> list[Finding]:
    size = path.stat().st_size
    if size > MAX_FILE_BYTES:
        raise ValueError(f"input file exceeds {MAX_FILE_BYTES} bytes: {path}")
    text = path.read_bytes().decode("utf-8", errors="replace")
    findings: list[Finding] = []
    for line_no, line in enumerate(text.splitlines(), start=1):
        for kind, pattern in BUILTIN_PATTERNS:
            if pattern.search(line):
                findings.append(Finding(str(path), kind, line_no))
        for index, literal in enumerate(forbidden_literals):
            if literal and literal in line:
                findings.append(Finding(str(path), f"forbidden_literal_{index}", line_no))
    return findings


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="Fail closed when security-sensitive material appears in acceptance receipts/logs."
    )
    parser.add_argument("paths", nargs="+", type=Path)
    parser.add_argument(
        "--forbid-literal",
        action="append",
        default=[],
        help="Exact synthetic/customer-sensitive literal that must not appear. Repeatable.",
    )
    parser.add_argument("--json-out", type=Path)
    args = parser.parse_args(argv)

    try:
        files = list(iter_files(args.paths))
        findings: list[Finding] = []
        for path in files:
            findings.extend(scan_file(path, args.forbid_literal))
    except (OSError, ValueError) as error:
        print(f"security-leak-scan: {error}", file=sys.stderr)
        return 2

    receipt = {
        "protocol_version": "chaptera.security-leak-scan.v1",
        "files_scanned": len(files),
        "forbidden_literal_count": len(args.forbid_literal),
        "finding_count": len(findings),
        "findings": [
            {"path": item.path, "kind": item.kind, "line": item.line}
            for item in findings
        ],
        "passed": not findings,
    }
    payload = json.dumps(receipt, indent=2, sort_keys=True) + "\n"
    if args.json_out:
        args.json_out.parent.mkdir(parents=True, exist_ok=True)
        args.json_out.write_text(payload, encoding="utf-8")
    print(payload, end="")
    return 1 if findings else 0


if __name__ == "__main__":
    raise SystemExit(main())
