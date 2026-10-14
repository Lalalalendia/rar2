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
    ".github/workflows/cloud-s3-provider-physical-probe.yml",
)

DESKTOP_PR_ADMISSION_RULES = {
    ".github/workflows/editor-build-once-pr.yml": {
        "forbidden_paths": ("apps/chaptera-desktop/src/main.rs",),
        "required_paths": (
            "tools/acquire_carlton_march_pair.py",
            "apps/chaptera-desktop/src/acceptance.rs",
            "apps/chaptera-desktop/src/acceptance_cli.rs",
            "vendor/producer-a/crates/pub-editor/**",
        ),
    },
    ".github/workflows/editor-desktop-textbox-restore-v1.yml": {
        "forbidden_paths": ("apps/chaptera-desktop/src/main.rs",),
        "required_paths": (
            "apps/chaptera-desktop/src/text_box_creation.rs",
            "apps/chaptera-desktop/src/text_box_creation_shell.rs",
            "apps/chaptera-desktop/src/text_box_creation_gui_tests.rs",
            "apps/chaptera-desktop/src/text_session.rs",
            "apps/chaptera-desktop/src/text_session_shell.rs",
            "apps/chaptera-desktop/src/selection_keyboard_shell.rs",
        ),
    },
    ".github/workflows/editor-insert-shape-ui-v1.yml": {
        "forbidden_paths": ("apps/chaptera-desktop/src/main.rs",),
        "required_paths": (
            "apps/chaptera-desktop/src/rectangle_creation.rs",
            "apps/chaptera-desktop/src/rectangle_creation_shell.rs",
            "apps/chaptera-desktop/src/selection_keyboard_shell.rs",
        ),
    },
    ".github/workflows/chaptera-win-dll-search-harden.yml": {
        "forbidden_paths": ("apps/chaptera-desktop/src/main.rs",),
        "required_paths": (
            "apps/chaptera-desktop/src/windows_dll_search.rs",
            "apps/chaptera-desktop/src/bin/chaptera-dll-search-probe.rs",
        ),
    },
    ".github/workflows/chaptera-desktop-fallback-font-resource-v1.yml": {
        "forbidden_paths": ("apps/chaptera-desktop/src/main.rs",),
        "required_paths": (
            "crates/chaptera-desktop-fallback-font-resource/**",
            "apps/chaptera-desktop/src/font_binding.rs",
            "apps/chaptera-desktop/src/fallback_font.rs",
            "apps/chaptera-desktop/src/source_font.rs",
            "apps/chaptera-desktop/src/render_backend.rs",
            "crates/chaptera-viewer-render-plan/**",
            "packages/product/editor-live-trial/**",
        ),
    },
    ".github/workflows/editor-duplicate-rectangle-v1.yml": {
        "forbidden_paths": ("apps/chaptera-desktop/src/main.rs",),
        "required_paths": (
            "apps/chaptera-desktop/src/duplicate_rectangle.rs",
            "apps/chaptera-desktop/src/duplicate_rectangle_gui_tests.rs",
        ),
    },
    ".github/workflows/chaptera-vtpe-locale.yml": {
        "forbidden_paths": ("apps/chaptera-desktop/src/main.rs",),
        "required_paths": (
            "apps/chaptera-desktop/src/locale.rs",
            "apps/chaptera-desktop/src/supporter.rs",
            "apps/chaptera-desktop/src/supporter_attribution.rs",
        ),
    },
    ".github/workflows/scene-authored-stack-v1.yml": {
        "forbidden_paths": ("apps/chaptera-desktop/src/main.rs",),
        "required_paths": (
            "apps/chaptera-desktop/src/authored_stack.rs",
        ),
    },
    ".github/workflows/editor-desktop-history-shortcuts-v1.yml": {
        "forbidden_paths": ("apps/chaptera-desktop/src/main.rs",),
        "required_paths": (
            "apps/chaptera-desktop/src/history.rs",
            "apps/chaptera-desktop/src/history_gui_tests.rs",
        ),
    },
    ".github/workflows/editor-desktop-page-nav-shortcuts-v1.yml": {
        "forbidden_paths": ("apps/chaptera-desktop/src/main.rs",),
        "required_paths": (
            "apps/chaptera-desktop/src/page_navigation.rs",
            "apps/chaptera-desktop/src/page_navigation_gui_tests.rs",
        ),
    },
    ".github/workflows/editor-desktop-text-session-restore-v1.yml": {
        "forbidden_paths": ("apps/chaptera-desktop/src/main.rs",),
        "required_paths": (
            "apps/chaptera-desktop/src/text_session.rs",
            "apps/chaptera-desktop/src/text_session_shell.rs",
            "apps/chaptera-desktop/src/text_session_gui_tests.rs",
        ),
    },
    ".github/workflows/editor-desktop-selection-keyboard-restore-v1.yml": {
        "forbidden_paths": ("apps/chaptera-desktop/src/main.rs",),
        "required_paths": (
            "apps/chaptera-desktop/src/selection_keyboard.rs",
            "apps/chaptera-desktop/src/selection_keyboard_shell.rs",
            "apps/chaptera-desktop/src/selection_keyboard_gui_tests.rs",
        ),
    },
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



S3_PROBE_WORKFLOW = ".github/workflows/cloud-s3-provider-physical-probe.yml"
S3_PROBE_JOB_IF = (
    "    if: github.event_name == 'workflow_dispatch' && "
    "github.ref == 'refs/heads/main' && "
    "github.repository == 'Lalalalendia/rar2'"
)
S3_PROBE_RUN_STEP = "      - name: Run owner-pinned physical provider and versioning probe"
S3_PROBE_UPLOAD_STEP = "      - name: Upload sanitized receipt only"


def audit_s3_probe_trust_text(rel: str, text: str) -> list[str]:
    """Guard reviewed source wiring, NOT live environment protection settings."""
    lines = active_lines(text)
    active = "\n".join(lines)
    errors: list[str] = []

    if "on:\n  workflow_dispatch:" not in active or re.search(
        r"(?m)^  (pull_request|pull_request_target|push|schedule|workflow_run|workflow_call|repository_dispatch):",
        active,
    ):
        errors.append(f"{rel}: S3 credentials require workflow_dispatch only")
    if S3_PROBE_JOB_IF not in lines:
        errors.append(f"{rel}: S3 probe requires canonical main-only dispatch condition")
    if "    environment: chaptera-s3-physical" not in lines:
        errors.append(f"{rel}: S3 probe must bind protected chaptera-s3-physical environment")
    if "          ref: ${{ github.sha }}" not in lines:
        errors.append(f"{rel}: checkout must pin immutable trigger commit SHA")
    if any(line == "env:" or line.startswith("    env:") for line in lines):
        errors.append(f"{rel}: S3 credentials cannot live in workflow/job-wide env")
    if re.search(r"(?m)^\s+id-token:\s+write\s*$", active):
        errors.append(f"{rel}: OIDC write authority requires separate reviewed role policy")

    run_start = active.find(S3_PROBE_RUN_STEP)
    upload_start = active.find(S3_PROBE_UPLOAD_STEP, run_start + 1)
    if run_start == -1 or upload_start == -1:
        errors.append(f"{rel}: bounded S3 probe and receipt upload steps required")
    else:
        probe_step = active[run_start:upload_start]
        secret_occurrences = list(
            re.finditer(r"secrets\.CHAPTERA_S3_PROBE_[A-Z_]+", active)
        )
        if len(secret_occurrences) < 2 or any(
            not (run_start <= match.start() < upload_start)
            for match in secret_occurrences
        ):
            errors.append(f"{rel}: S3 secrets must appear only in physical probe step")
        if "        env:" not in probe_step:
            errors.append(f"{rel}: no step-local S3 probe authority detected")

    checkout_start = active.find("      - uses: actions/checkout@")
    if checkout_start == -1:
        errors.append(f"{rel}: checkout action missing")
    else:
        next_step = active.find("\n      - uses:", checkout_start + 1)
        checkout_step = active[checkout_start:next_step if next_step > 0 else None]
        if "          ref: ${{ github.sha }}" not in checkout_step:
            errors.append(f"{rel}: checkout must bind ref to exact triggering SHA")
    return errors


def self_test_s3_probe_trust() -> None:
    path = pathlib.Path(S3_PROBE_WORKFLOW)
    valid = path.read_text(encoding="utf-8")
    if errors := audit_s3_probe_trust_text(S3_PROBE_WORKFLOW, valid):
        raise AssertionError("real S3 probe violates canonical trust: " + "; ".join(errors))

    mutations = [
        valid.replace("    environment: chaptera-s3-physical\n", ""),
        valid.replace(S3_PROBE_JOB_IF + "\n", ""),
        valid.replace("          ref: ${{ github.sha }}", "          ref: refs/heads/unreviewed"),
        valid.replace("  workflow_dispatch:", "  pull_request:"),
        valid.replace("    timeout-minutes: 15\n", "    timeout-minutes: 15\n    env:\n      AWS_ACCESS_KEY_ID: risky\n"),
        valid.replace(
            S3_PROBE_UPLOAD_STEP,
            S3_PROBE_UPLOAD_STEP + "\n        env:\n          AWS_ACCESS_KEY_ID: ${{ secrets.CHAPTERA_S3_PROBE_ACCESS_KEY_ID }}",
        ),
    ]
    for index, mutated in enumerate(mutations):
        if mutated == valid:
            raise AssertionError(f"S3 trust mutation {index} did not alter fixture")
        if not audit_s3_probe_trust_text(S3_PROBE_WORKFLOW, mutated):
            raise AssertionError(f"S3 trust mutation {index} was not rejected")


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
        self_test_s3_probe_trust()
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

    s3_path = root / S3_PROBE_WORKFLOW
    if s3_path.is_file():
        errors.extend(
            audit_s3_probe_trust_text(
                S3_PROBE_WORKFLOW, s3_path.read_text(encoding="utf-8")
            )
        )

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

