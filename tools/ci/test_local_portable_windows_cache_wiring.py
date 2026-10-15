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
    require(header.count("  workflow_call:\n") == 1, "workflow_call removed or duplicated")
    require(header.count("  workflow_dispatch:\n") == 1, "manual main seed removed or duplicated")
    require(header.count("  push:\n") == 1, "unexpected push trigger count")
    require("  pull_request:\n" not in header and "  pull_request_target:\n" not in header,
            "untrusted PR must not become a cache seed trigger")
    require("  schedule:\n" not in header, "unexpected recurring expensive Windows seed")
    push_body = header.split("  push:\n", 1)[1].strip()
    allowed_main_paths = (
        ".github/workflows/chaptera-local-portable-windows.yml",
        "Cargo.toml",
        "Cargo.lock",
        "**/Cargo.toml",
        "**/Cargo.lock",
        ".cargo/**",
        "rust-toolchain*",
    )
    expected_push = "branches: [main]\n    paths:\n" + "\n".join(
        f'      - "{path}"' for path in allowed_main_paths
    )
    require(
        push_body == expected_push,
        "trusted main seed must use only exact Cargo manifest/lock/toolchain paths, not source-wide",
    )

    require("RUSTUP_HOME: ${{ runner.temp }}\\chaptera-pinned-rustup" in source, "rustup isolation missing")
    require(source.count("RUSTUP_HOME:") == 1, "rustup isolation must be job-local")
    require("name: Verify isolated pinned Rust toolchain" in source, "toolchain check missing")
    require("$toolchains.Count -ne 1" in source and "$toolchains[0] -ne $expected" in source, "inventory proof weakened")
    require("active rustc is not pinned 1.94.1" in source, "active rustc proof missing")
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
        source.replace("RUSTUP_HOME: ${{ runner.temp }}\\chaptera-pinned-rustup", "RUSTUP_HOME: ${{ runner.temp }}\\untrusted", 1),
        source.replace("name: Verify isolated pinned Rust toolchain", "name: Skip pinned Rust toolchain", 1),
        source.replace("$toolchains.Count -ne 1", "$toolchains.Count -lt 1", 1),
        source.replace("$toolchains[0] -ne $expected", "$toolchains[0] -eq $expected", 1),
        source.replace("active rustc is not pinned 1.94.1", "active rustc bypassed", 1),

        source.replace(SAVE_IF, "save-if: true", 1),
        source.replace("vendor/producer-a -> target", "vendor/producer-a -> skipped", 1),
        source.replace("run: cargo build -p chaptera-server --release", "run: echo no-server", 1),
        source.replace("run: cargo build --manifest-path vendor/producer-a/Cargo.toml -p chaptera-producer-a --release", "run: echo no-producer", 1),
        source.replace("RUSTFLAGS: -C target-feature=+crt-static", 'RUSTFLAGS: ""', 1),
        source.replace('cache-workspace-crates: "false"', 'cache-workspace-crates: "true"', 1),
        source.replace('      - ".github/workflows/chaptera-local-portable-windows.yml"', '      - "apps/**"', 1),
        source.replace("Prove no-checkout zero-install startup", "skip clean smoke", 1),
        source.replace('      - "**/Cargo.toml"', '      - "**/*.rs"', 1),
        source.replace('      - "**/Cargo.lock"', '      - "crates/**"', 1),
        source.replace('      - "Cargo.toml"', '      - "apps/**"', 1),
        source.replace('      - "Cargo.lock"', '      - "vendor/**"', 1),
        source.replace('      - ".cargo/**"', '      - "**/*.py"', 1),
        source.replace('      - "rust-toolchain*"', '      - "**/*.rs"', 1),
        source.replace("  push:\n", "  pull_request:\n  push:\n", 1),
        source.replace("  push:\n", '  schedule:\n    - cron: "0 * * * *"\n  push:\n', 1),
        source.replace('      - "**/Cargo.toml"', '      - "**/Cargo.toml"\n      - "apps/**"', 1),
        source.replace("branches: [main]", "branches: ['**']", 1),
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
