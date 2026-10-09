#!/usr/bin/env python3
"""Fail-closed guard for W2 consuming the trusted main Editor Windows Cargo cache."""
from pathlib import Path

W2 = Path(".github/workflows/w2-longform-fixed-pdf-current-revision.yml")
DONOR = Path(".github/workflows/chaptera-desktop-windows.yml")
TARGET = "CARGO_TARGET_DIR: ${{ github.workspace }}/target"
KEY = "key: ${{ hashFiles('Cargo.toml', 'vendor/producer-a/Cargo.toml') }}"
SAVE = 'save-if: "false"'
DONOR_SAVE = "save-if: ${{ github.ref == 'refs/heads/main' }}"
OWNER = "Restore trusted W2 Editor Cargo dependencies (read-only)"
DESKTOP = "cargo build -p chaptera-desktop --release --bin chaptera-editor"
W2_BUILD = "cargo build --manifest-path crates/chaptera-desktop-shaped-flow-runtime/Cargo.toml --release --bin current_fixed_pdf_input --bin w2_same_document_kernel --target-dir target"


def require(ok: bool, why: str) -> None:
    if not ok:
        raise AssertionError(why)


def enforce(source: str, donor: str) -> None:
    header = source.split("\npermissions:", 1)[0]
    require("  pull_request:\n" in header, "W2 PR admission removed")
    require("  workflow_dispatch:\n" in header, "W2 diagnostic dispatch removed")
    require("  push:\n" not in header, "W2 must not write a main cache")
    require('      - ".github/workflows/w2-longform-fixed-pdf-current-revision.yml"' in header, "W2 self-trigger removed")
    require(source.count("  validate:\n") == 1, "W2 product validator duplicated or removed")
    val = source.split("  validate:\n", 1)[1]
    require("needs: classify" in val, "W2 base-classifier dependency removed")
    require("needs.classify.result != 'success'" in val, "W2 fail-closed classifier law removed")
    require("runs-on: windows-latest" in val, "W2 Windows product proof removed")
    require("timeout-minutes: 45" in val, "W2 budget changed")
    require(TARGET in val, "W2 target dir differs from trusted Editor donor")
    require(val.count("name: " + OWNER) == 1, "W2 restore step duplicated/missing")
    cache_marks = (
        "Swatinem/rust-cache@6323deb102c322ba6fcbdcafc7e3dddab59af2b6",
        "shared-key: chaptera-windows-desktop-deps-v1",
        'add-job-id-key: "false"',
        KEY,
        'workspaces: ". -> target"',
        'cache-bin: "false"',
        'cache-workspace-crates: "false"',
        'cache-on-failure: "false"',
        SAVE,
    )
    require(all(x in val for x in cache_marks), "W2 trusted cache mismatch or writes enabled")
    require(val.index(OWNER) < val.index("Preflight acceptance scripts") < val.index("Build Stage 0.1 producer"),
            "Cache must restore before W2 release compilation")
    required_products = (
        DESKTOP, W2_BUILD,
        "Acquire exact May 2023 recurring newsletter",
        "May 2023 newsletter SHA-256 mismatch",
        "Reproduce accepted Stage 0.1 fresh-reopen project",
        "Materialize pinned fixed-output font",
        "Prove same-document previous issue to independent next issue",
        "Render and validate real current-revision PDF",
        "Verify retained Stage 0.2 evidence is source-safe",
        "Build final same-document W2 compound receipt",
        "actions/upload-artifact@v4",
        "if-no-files-found: error",
    )
    require(all(x in val for x in required_products), "W2 product, PDF, receipts or artifacts weakened")
    donor_marks = (
        TARGET,
        "Swatinem/rust-cache@6323deb102c322ba6fcbdcafc7e3dddab59af2b6",
        "shared-key: chaptera-windows-desktop-deps-v1",
        'add-job-id-key: "false"',
        KEY,
        'workspaces: ". -> target"',
        DONOR_SAVE,
    )
    require(all(x in donor for x in donor_marks), "Trusted main Editor cache donor contract changed")


def main() -> int:
    source = W2.read_text(encoding="utf-8")
    donor = DONOR.read_text(encoding="utf-8")
    enforce(source, donor)
    mutations = [
        (source.replace(SAVE, 'save-if: "true"', 1), donor),
        (source.replace("shared-key: chaptera-windows-desktop-deps-v1", "shared-key: no-match", 1), donor),
        (source.replace(TARGET, "CARGO_TARGET_DIR: another-target", 1), donor),
        (source.replace(KEY, "key: no-manifest-fence", 1), donor),
        (source.replace('cache-workspace-crates: "false"', 'cache-workspace-crates: "true"', 1), donor),
        (source.replace('cache-on-failure: "false"', 'cache-on-failure: "true"', 1), donor),
        (source.replace(DESKTOP, "echo skip-windows-editor", 1), donor),
        (source.replace(W2_BUILD, "echo skip-w2-kernel", 1), donor),
        (source.replace("Render and validate real current-revision PDF", "skip PDF proof", 1), donor),
        (source.replace("needs.classify.result != 'success'", "false", 1), donor),
        (source.replace("runs-on: windows-latest", "runs-on: ubuntu-latest", 1), donor),
        (source, donor.replace(DONOR_SAVE, 'save-if: "true"', 1)),
    ]
    for index, (mutation, changed_donor) in enumerate(mutations, 1):
        require(mutation != source or changed_donor != donor, f"mutation {index} not applied")
        try:
            enforce(mutation, changed_donor)
        except (AssertionError, ValueError, IndexError):
            continue
        raise AssertionError(f"unsafe mutation {index} bypassed W2 guard")
    print(f"W2 trusted Editor cache restore-only: PASS, {len(mutations)} negative controls")


if __name__ == "__main__":
    main()
