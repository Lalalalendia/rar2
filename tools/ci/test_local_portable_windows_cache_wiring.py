#!/usr/bin/env python3
"""Fail-closed static/negative controls for the trusted Local portable Windows Cargo cache."""
from pathlib import Path

WORKFLOW = Path(".github/workflows/chaptera-local-portable-windows.yml")
SAVE_IF = "save-if: ${{ github.ref == 'refs/heads/main' && (github.event_name == 'push' || github.event_name == 'workflow_dispatch' || github.event_name == 'schedule') }}"


def require(condition: bool, message: str) -> None:
    if not condition:
        raise AssertionError(message)


def enforce(source: str) -> None:
    header = source.split("\npermissions:", 1)[0]
    require("  workflow_call:\n" in header, "workflow_call removed")
    require("  workflow_dispatch:\n" in header, "manual main seed removed")
    require(header.count("  push:\n") == 1, "unexpected push trigger count")
    push_body = header.split("  push:\n", 1)[1].strip()
    require(
        push_body == 'branches: [main]\n    paths:\n      - ".github/workflows/chaptera-local-portable-windows.yml"',
        "trusted push must trigger only on own workflow, not source-wide",
    )

    require("RUSTFLAGS: -C target-feature=+crt-static" in source, "crt-static contract weakened")
    require("CARGO_TARGET_DIR:" not in source, "producer/server binary paths silently changed")
    build = source.split("\n  build-portable:\n", 1)[1].split(
        "\n  clean-extracted-smoke:\n", 1
    )[0]
    require("runs-on: windows-latest" in build, "Windows binary proof removed")
    require(build.count("name: Restore trusted Local portable Cargo dependencies") == 1, "cache owner duplicated")
    required_cache = (
        "Swatinem/rust-cache@6323deb102c322ba6fcbdcafc7e3dddab59af2b6",
        "shared-key: chaptera-local-portable-crtstatic-deps-v1",
        'add-job-id-key: "false"',
        "hashFiles('Cargo.toml', 'Cargo.lock', 'vendor/producer-a/Cargo.toml', 'vendor/producer-a/Cargo.lock')",
        ". -> target",
        "vendor/producer-a -> target",
        'cache-bin: "false"',
        'cache-targets: "true"',
        'cache-workspace-crates: "false"',
        'cache-on-failure: "false"',
        SAVE_IF,
    )
    require(all(mark in build for mark in required_cache), "trusted cache/read-only PR contract missing")
    require(build.index("Restore trusted Local portable Cargo dependencies") < build.index("Build Chaptera release binary"),
            "cache restore must precede both release builds")
    required_product = (
        "run: cargo build -p chaptera-server --release",
        "run: cargo build --manifest-path vendor/producer-a/Cargo.toml -p chaptera-producer-a --release",
        "target/release/chaptera.exe",
        "vendor/producer-a/target/release/chaptera-producer-a.exe",
        "Build deterministic portable ZIP",
        "Inspect packaged executable imports",
        "actions/upload-artifact@",
    )
    require(all(mark in build for mark in required_product), "exact release binaries/package proof weakened")
    smoke = source.split("\n  clean-extracted-smoke:\n", 1)[1]
    require("runs-on: windows-latest" in smoke, "clean Windows smoke disabled")
    require("Prove no-checkout zero-install startup" in smoke, "clean portable smoke disabled")
    require("Verify state isolation and package immutability" in smoke, "package immutability proof disabled")


def main() -> int:
    source = WORKFLOW.read_text(encoding="utf-8")
    enforce(source)
    mutations = [
        source.replace(SAVE_IF, "save-if: true", 1),
        source.replace("vendor/producer-a -> target", "vendor/producer-a -> skipped", 1),
        source.replace("run: cargo build -p chaptera-server --release", "run: echo no-server", 1),
        source.replace("run: cargo build --manifest-path vendor/producer-a/Cargo.toml -p chaptera-producer-a --release", "run: echo no-producer", 1),
        source.replace("RUSTFLAGS: -C target-feature=+crt-static", 'RUSTFLAGS: ""', 1),
        source.replace('cache-workspace-crates: "false"', 'cache-workspace-crates: "true"', 1),
        source.replace('      - ".github/workflows/chaptera-local-portable-windows.yml"', '      - "apps/**"', 1),
        source.replace("Prove no-checkout zero-install startup", "skip clean smoke", 1),
    ]
    for i, mutation in enumerate(mutations, 1):
        require(mutation != source, f"negative control {i} was not applied")
        try:
            enforce(mutation)
        except (AssertionError, IndexError):
            continue
        raise AssertionError(f"unsafe mutation {i} escaped the cache/product guard")
    print(f"Local portable Windows cache wiring: PASS, {len(mutations)} negative controls")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
