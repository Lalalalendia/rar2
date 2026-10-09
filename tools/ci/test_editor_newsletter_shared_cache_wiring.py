#!/usr/bin/env python3
"""Guard read-only newsletter restore from trusted Windows Editor Cargo cache."""
from pathlib import Path

NEWSLETTER = Path(".github/workflows/editor-newsletter-continuity-v1.yml")
EDITOR_DONOR = Path(".github/workflows/chaptera-desktop-windows.yml")
TARGET = "CARGO_TARGET_DIR: ${{ github.workspace }}/target"
KEY = "key: ${{ hashFiles('Cargo.toml', 'vendor/producer-a/Cargo.toml') }}"
DONOR_SAVE = "save-if: ${{ github.ref == 'refs/heads/main' }}"


def require(condition: bool, message: str) -> None:
    if not condition:
        raise AssertionError(message)


def enforce(source: str, donor: str) -> None:
    header = source.split("\npermissions:", 1)[0]
    require("  pull_request:\n" in header, "Newsletter PR admission removed")
    require("  workflow_dispatch:\n" in header, "Newsletter diagnostic dispatch removed")
    require("  push:\n" not in header, "Newsletter must not add its own main cache writer")
    require('      - ".github/workflows/editor-newsletter-continuity-v1.yml"' in header, "Newsletter self-surface removed")
    require("  real-newsletter-kernel:\n" in source, "Newsletter product owner removed")
    job = source.split("  real-newsletter-kernel:\n", 1)[1]
    require("runs-on: windows-latest" in job, "Windows product validation missing")
    require("timeout-minutes: 45" in job, "Product timeout silently modified")
    require(TARGET in job, "Target dir must match trusted Editor donor")
    require(job.count("name: Restore trusted Editor Windows Cargo dependencies (read-only)") == 1, "Newsletter cache owner duplicated")
    required_cache = (
        "Swatinem/rust-cache@6323deb102c322ba6fcbdcafc7e3dddab59af2b6",
        "shared-key: chaptera-windows-desktop-deps-v1",
        'add-job-id-key: "false"',
        KEY,
        'workspaces: ". -> target"',
        'cache-bin: "false"',
        'cache-workspace-crates: "false"',
        'cache-on-failure: "false"',
        'save-if: "false"',
    )
    require(all(mark in job for mark in required_cache), "Newsletter must restore donor deps without writing PR caches")
    require(job.index("Restore trusted Editor Windows Cargo dependencies (read-only)") < job.index("Install receipt validator"),
            "Restore must precede product compiles")
    product_markers = (
        "cargo build -p chaptera-desktop --release --bin chaptera-editor",
        "cargo build --manifest-path crates/chaptera-desktop-shaped-flow-runtime/Cargo.toml --release --bin current_fixed_pdf_input --target-dir target",
        "Acquire pinned Apache POI SampleNewsletter",
        "Run bounded recurring-newsletter kernel",
        "Materialize pinned fixed-output font",
        "Assemble newsletter current-revision fixed-PDF input",
        "Acquire exact public Yab 259 donor",
        "Render the same accepted newsletter revision to fixed PDF",
        "target/release/editor-newsletter-current-revision.real.pdf",
        "actions/upload-artifact@v4",
    )
    require(all(mark in job for mark in product_markers), "Newsletter product/runtime/PDF receipt weakened")
    donor_require = (
        TARGET,
        "Swatinem/rust-cache@6323deb102c322ba6fcbdcafc7e3dddab59af2b6",
        "shared-key: chaptera-windows-desktop-deps-v1",
        'add-job-id-key: "false"',
        KEY,
        'workspaces: ". -> target"',
        DONOR_SAVE,
    )
    require(all(mark in donor for mark in donor_require), "Editor trusted main donor cache contract changed")


def main() -> int:
    src = NEWSLETTER.read_text(encoding="utf-8")
    donor = EDITOR_DONOR.read_text(encoding="utf-8")
    enforce(src, donor)
    mutations = [
        (src.replace('save-if: "false"', 'save-if: "true"', 1), donor),
        (src.replace("shared-key: chaptera-windows-desktop-deps-v1", "shared-key: arbitrary-key", 1), donor),
        (src.replace(TARGET, "CARGO_TARGET_DIR: other/target", 1), donor),
        (src.replace('cache-workspace-crates: "false"', 'cache-workspace-crates: "true"', 1), donor),
        (src.replace("cargo build -p chaptera-desktop --release --bin chaptera-editor", "echo skip-editor-binary", 1), donor),
        (src.replace("cargo build --manifest-path crates/chaptera-desktop-shaped-flow-runtime/Cargo.toml --release --bin current_fixed_pdf_input --target-dir target", "echo skip-fixed-pdf-binary", 1), donor),
        (src.replace("Render the same accepted newsletter revision to fixed PDF", "skip PDF production", 1), donor),
        (src, donor.replace(DONOR_SAVE, "save-if: true", 1)),
    ]
    for index, (mutated, mutated_donor) in enumerate(mutations, 1):
        require(mutated != src or mutated_donor != donor, f"negative mutation {index} was a no-op")
        try:
            enforce(mutated, mutated_donor)
        except (AssertionError, IndexError, ValueError):
            continue
        raise AssertionError(f"negative mutation {index} bypassed newsletter/donor contract")
    print(f"Newsletter trusted Editor cache restore-only wiring: PASS, {len(mutations)} negative controls")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
