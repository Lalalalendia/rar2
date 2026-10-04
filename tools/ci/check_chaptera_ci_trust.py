#!/usr/bin/env python3
from __future__ import annotations

import argparse
import pathlib
import re
import sys

PROTECTED_WORKFLOWS = (
    ".github/workflows/chaptera-local-portable-windows.yml",
    ".github/workflows/chaptera-reader-installer.yml",
    ".github/workflows/chaptera-win-sign-preflight.yml",
    ".github/workflows/chaptera-server-package-integrity-v1.yml",
    ".github/workflows/editor-live-trial-package.yml",
    ".github/workflows/pub-ksu-publication4-provenance.yml",
    ".github/workflows/cloud-deploy-v0.yml",
)

DESKTOP_PR_ADMISSION_RULES = {
    ".github/workflows/chaptera-suite-handoff-v1.yml": {
        "forbidden_paths": ("apps/chaptera-desktop/src/main.rs",),
        "required_paths": (
            "apps/chaptera-desktop/src/suite_handoff_cli.rs",
            "apps/chaptera-desktop/src/diagnostic_sweep.rs",
        ),
    },
}

USES_RE = re.compile(r"^\s*-\s*uses:\s*([^\s#]+)")
FULL_SHA_RE = re.compile(r"^[0-9a-f]{40}$")
WRITE_PERMISSION_RE = re.compile(
    r"^\s+(actions|attestations|checks|contents|deployments|id-token|issues|packages|pages|pull-requests|security-events|statuses):\s*write\s*(?:#.*)?$"
)


def active_lines(text: str) -> list[str]:
    return [line for line in text.splitlines() if not line.lstrip().startswith("#")]


def has_trigger(lines: list[str], trigger: str) -> bool:
    needle = f"{trigger}:"
    return any(
        line.startswith("  ")
        and not line.startswith("    ")
        and line.strip() == needle
        for line in lines
    )


def audit(path: pathlib.Path) -> list[str]:
    rel = path.as_posix()
    text = path.read_text(encoding="utf-8")
    lines = active_lines(text)
    errors: list[str] = []

    if not any(line.startswith("permissions:") for line in lines):
        errors.append(f"{rel}: missing explicit top-level permissions block")
    if any(line.strip() == "permissions: write-all" for line in lines):
        errors.append(f"{rel}: permissions: write-all is forbidden")
    if has_trigger(lines, "pull_request_target"):
        errors.append(f"{rel}: pull_request_target is forbidden in protected workflows")

    if has_trigger(lines, "pull_request"):
        for lineno, line in enumerate(lines, 1):
            if WRITE_PERMISSION_RE.match(line):
                errors.append(
                    f"{rel}:{lineno}: write/OIDC authority is forbidden when protected workflow runs on pull_request"
                )
            if line.strip() == "secrets: inherit":
                errors.append(
                    f"{rel}:{lineno}: secrets: inherit is forbidden when protected workflow runs on pull_request"
                )

    for lineno, line in enumerate(lines, 1):
        match = USES_RE.match(line)
        if not match:
            continue
        target = match.group(1)
        if target.startswith("./"):
            continue
        if "@" not in target:
            errors.append(f"{rel}:{lineno}: external action/reusable workflow lacks @ref: {target}")
            continue
        owner_path, ref = target.rsplit("@", 1)
        if not FULL_SHA_RE.fullmatch(ref):
            errors.append(
                f"{rel}:{lineno}: external action/reusable workflow must be pinned to full 40-char commit SHA: {owner_path}@{ref}"
            )

    return errors


def audit_desktop_pr_admission_text(
    rel: str, text: str, rule: dict[str, tuple[str, ...]]
) -> list[str]:
    active = "\n".join(active_lines(text))
    errors: list[str] = []

    for forbidden in rule["forbidden_paths"]:
        if forbidden in active:
            errors.append(f"{rel}: forbidden broad PR admission path restored: {forbidden}")

    for required in rule["required_paths"]:
        if required not in active:
            errors.append(f"{rel}: required owned PR admission path missing: {required}")

    return errors


def audit_desktop_pr_admission(
    path: pathlib.Path, rule: dict[str, tuple[str, ...]]
) -> list[str]:
    return audit_desktop_pr_admission_text(
        path.as_posix(), path.read_text(encoding="utf-8"), rule
    )


def self_test_desktop_pr_admission() -> None:
    rel = ".github/workflows/chaptera-suite-handoff-v1.yml"
    rule = DESKTOP_PR_ADMISSION_RULES[rel]
    valid = """
on:
  pull_request:
    paths:
      - "apps/chaptera-desktop/src/suite_handoff_cli.rs"
      - "apps/chaptera-desktop/src/diagnostic_sweep.rs"
"""
    if audit_desktop_pr_admission_text(rel, valid, rule):
        raise AssertionError("valid owned-path fixture was rejected")

    broad = valid + '      - "apps/chaptera-desktop/src/main.rs"\n'
    broad_errors = audit_desktop_pr_admission_text(rel, broad, rule)
    if not any("forbidden broad PR admission path" in error for error in broad_errors):
        raise AssertionError("broad main.rs regression was not rejected")

    missing = """
on:
  pull_request:
    paths:
      - "apps/chaptera-desktop/src/suite_handoff_cli.rs"
"""
    missing_errors = audit_desktop_pr_admission_text(rel, missing, rule)
    if not any("required owned PR admission path missing" in error for error in missing_errors):
        raise AssertionError("missing owned diagnostic path was not rejected")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=pathlib.Path, default=pathlib.Path("."))
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()

    if args.self_test:
        self_test_desktop_pr_admission()
        print("CHAPTERA-CI-TRUST-01 self-test PASS")
        return 0

    root = args.root.resolve()
    errors: list[str] = []
    for rel in PROTECTED_WORKFLOWS:
        path = root / rel
        if not path.is_file():
            errors.append(f"{rel}: protected workflow missing")
            continue
        errors.extend(audit(path))

    for rel, rule in DESKTOP_PR_ADMISSION_RULES.items():
        path = root / rel
        if not path.is_file():
            errors.append(f"{rel}: admission-guarded workflow missing")
            continue
        errors.extend(audit_desktop_pr_admission(path, rule))

    if errors:
        for error in errors:
            print(error, file=sys.stderr)
        return 1

    print(
        "CHAPTERA-CI-TRUST-01 PASS: "
        f"{len(PROTECTED_WORKFLOWS)} protected workflows; "
        f"{len(DESKTOP_PR_ADMISSION_RULES)} desktop admission rule"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
