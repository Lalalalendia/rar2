#!/usr/bin/env python3
import argparse
import json
from pathlib import Path

import jsonschema

START = "<!-- chaptera-windows-support:start -->"
END = "<!-- chaptera-windows-support:end -->"


def read_json(path):
    return json.loads(Path(path).read_text(encoding="utf-8-sig"))


def render_copy(matrix):
    public = matrix["public_copy"]
    scales = " and ".join(str(value) + "%" for value in public["fixed_display_scales_percent"])
    edition = ", ".join(public["required_edition_receipts"])
    return "\n".join([
        START,
        "## Windows system requirements",
        "",
        "Public V0 target: **" + public["baseline"] + "**.",
        "",
        "- Consumer Windows support is not claimed until the corresponding matrix row has its physical/VM product receipts.",
        "- The first required edition receipt is **Windows 11 25H2 " + edition + " x64**; other editions require independent receipts.",
        "- Fixed display-scale acceptance is **" + scales + "**. Live movement across mixed-DPI monitors is not claimed.",
        "- Native ARM64 support is not claimed; hosted ARM64 work is compile-only preflight.",
        "- GitHub windows-latest / Windows Server 2025 is **CI mechanics evidence only**, not consumer-Windows support evidence.",
        "- Windows 10 22H2 is outside the default public support promise.",
        END,
    ])


def semantic_errors(matrix):
    errors = []
    public = matrix["public_copy"]
    if public["baseline"] != "Windows 11 25H2 x64":
        errors.append("baseline must be Windows 11 25H2 x64")
    if public["fixed_display_scales_percent"] != [100, 150]:
        errors.append("fixed display-scale cells must be exactly 100 and 150")
    if public["mixed_dpi_transition_support"]:
        errors.append("mixed-DPI transition support is not admitted")
    if public["arm64_native_support"]:
        errors.append("native ARM64 support cannot come from hosted preflight")
    if matrix["ci_evidence"]["consumer_support_claim"]:
        errors.append("windows-latest must remain CI-only evidence")

    cells = {
        (c["os"], c["version"], c["edition"], c["architecture"]): c
        for c in matrix["cells"]
    }
    required = {
        ("Windows 11", "25H2", "Pro", "x86_64"): "candidate",
        ("Windows 11", "25H2", "Home", "x86_64"): "candidate",
        ("Windows 10", "22H2", "any", "x86_64"): "excluded",
        ("Windows Server", "2025", "GitHub-hosted", "x86_64"): "ci_only",
        ("Windows 11", "25H2", "any", "arm64"): "evaluation_only",
    }
    for key, state in required.items():
        if key not in cells:
            errors.append("missing required cell: " + repr(key))
        elif cells[key]["state"] != state:
            errors.append("wrong state for " + repr(key) + ": expected " + state)
    for key, cell in cells.items():
        if cell["state"] == "supported" and not cell.get("receipt_refs"):
            errors.append("supported cell lacks receipt_refs: " + repr(key))
        if cell["os"] == "Windows Server" and cell["state"] != "ci_only":
            errors.append("Windows Server rows must remain ci_only")
    return errors


def validate_matrix(matrix_path, schema_path, readme_path):
    matrix = read_json(matrix_path)
    schema = read_json(schema_path)
    jsonschema.Draft202012Validator(schema).validate(matrix)
    errors = semantic_errors(matrix)
    if errors:
        raise ValueError("; ".join(errors))

    readme = Path(readme_path).read_text(encoding="utf-8")
    start = readme.find(START)
    end = readme.find(END)
    if start < 0 or end < start:
        raise ValueError("README support marker block is missing")
    actual = readme[start:end + len(END)].strip()
    if actual != render_copy(matrix):
        raise ValueError("README Windows support copy drifted from matrix")


def validate_receipt(receipt_path, matrix_path):
    receipt = read_json(receipt_path)
    matrix = read_json(matrix_path)
    if receipt.get("schema_version") != "chaptera.windows-runner-receipt.v1":
        raise ValueError("runner receipt schema mismatch")
    if receipt.get("runner_label") != matrix["ci_evidence"]["github_runner_label"]:
        raise ValueError("runner label mismatch")
    if receipt.get("consumer_support_claim") is not False:
        raise ValueError("hosted runner cannot claim consumer support")
    expected = matrix["ci_evidence"]["expected_runner_family"]
    if expected not in str(receipt.get("os_caption", "")):
        raise ValueError("unexpected runner family")
    if str(receipt.get("runner_arch", "")).upper() not in {"X64", "AMD64"}:
        raise ValueError("runner is not x64")


def main():
    parser = argparse.ArgumentParser()
    sub = parser.add_subparsers(dest="command", required=True)
    matrix_cmd = sub.add_parser("matrix")
    matrix_cmd.add_argument("matrix")
    matrix_cmd.add_argument("schema")
    matrix_cmd.add_argument("readme")
    receipt_cmd = sub.add_parser("runner-receipt")
    receipt_cmd.add_argument("receipt")
    receipt_cmd.add_argument("matrix")
    args = parser.parse_args()

    if args.command == "matrix":
        validate_matrix(args.matrix, args.schema, args.readme)
    else:
        validate_receipt(args.receipt, args.matrix)


if __name__ == "__main__":
    main()
