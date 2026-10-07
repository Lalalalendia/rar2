#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import subprocess
import tomllib

VENDOR_CRATES = (
    "pub-core",
    "pub-cfb",
    "pub-contents",
    "pub-escher",
    "pub-model",
    "pub-quill",
    "pub-reader",
    "pub-layout",
    "pub-viewer",
    "pub-editor",
)
UPSTREAM = {"pub-core", "pub-cfb", "pub-contents", "pub-escher", "pub-model", "pub-quill"}

# The active repository also has a root pub-model@0.1.0. The vendored
# producer workspace intentionally keeps its donor model at 0.1.0-donor,
# so plain `-p pub-model` is ambiguous whenever Cargo resolves both graphs.
VENDOR_PACKAGE_SPECS = {
    "pub-model": "pub-model@0.1.0-donor",
}
PAGE_PROJECTION_SOURCE = "vendor/producer-a/crates/pub-reader/src/page_projection.rs"


def changed_paths(base: str, head: str) -> list[str]:
    return sorted(
        dict.fromkeys(
            p
            for p in subprocess.check_output(
                ["git", "diff", "--name-only", f"{base}...{head}"], text=True
            ).splitlines()
            if p
        )
    )


def edition_for(path: str) -> str:
    p = Path(path)
    manifests: list[tuple[Path, Path]] = []
    if path.startswith("vendor/producer-a/crates/"):
        manifests.append((p.parents[1] / "Cargo.toml", Path("vendor/producer-a/Cargo.toml")))
    elif path.startswith("crates/chaptera-viewer-render-plan/"):
        manifests.append((Path("crates/chaptera-viewer-render-plan/Cargo.toml"), Path("Cargo.toml")))
    elif path.startswith("apps/chaptera-desktop/"):
        manifests.append((Path("apps/chaptera-desktop/Cargo.toml"), Path("Cargo.toml")))

    for manifest, workspace_manifest in manifests:
        if not manifest.exists():
            continue
        with manifest.open("rb") as fh:
            package = tomllib.load(fh).get("package", {})
        edition = package.get("edition", "2021")
        if isinstance(edition, str):
            return edition
        if isinstance(edition, dict) and edition.get("workspace") is True:
            with workspace_manifest.open("rb") as fh:
                workspace = tomllib.load(fh).get("workspace", {})
            inherited = workspace.get("package", {}).get("edition", "2021")
            if isinstance(inherited, str):
                return inherited
            raise ValueError(f"workspace edition is not a string in {workspace_manifest}")
        raise ValueError(f"unsupported Cargo edition form in {manifest}: {edition!r}")
    return "2021"


def build_plan(paths: list[str], base: str, head: str) -> dict:
    direct: set[str] = set()
    changed_rust: list[str] = []

    for path in paths:
        for crate in VENDOR_CRATES:
            prefix = f"vendor/producer-a/crates/{crate}/"
            if path.startswith(prefix):
                direct.add(crate)
                if path.endswith(".rs"):
                    changed_rust.append(path)

    if any(p in {"vendor/producer-a/Cargo.toml", "vendor/producer-a/Cargo.lock"} for p in paths):
        direct.update(VENDOR_CRATES)

    affected = set(direct)
    if affected & UPSTREAM:
        affected.update({"pub-reader", "pub-layout", "pub-viewer"})
    if "pub-reader" in affected:
        affected.update({"pub-layout", "pub-viewer"})
    if "pub-layout" in affected:
        affected.add("pub-viewer")

    render_plan = any(p.startswith("crates/chaptera-viewer-render-plan/") for p in paths)
    desktop_source_changed = any(
        p in {
            "apps/chaptera-desktop/src/render_backend.rs",
            "apps/chaptera-desktop/src/main.rs",
        }
        for p in paths
    )
    desktop = desktop_source_changed
    if any(p in {"Cargo.toml", "Cargo.lock"} for p in paths):
        render_plan = True
        desktop = True

    # Shared Reader/Viewer changes must prove product integration cheaply in
    # Tier A so they do not need full Editor Windows / Android acceptance.
    if affected or render_plan:
        desktop = True
    mobile_reader = render_plan or bool(
        affected & {"pub-reader", "pub-layout", "pub-viewer"}
    )

    for path in paths:
        if path.startswith("crates/chaptera-viewer-render-plan/") and path.endswith(".rs"):
            changed_rust.append(path)
        if path in {
            "apps/chaptera-desktop/src/render_backend.rs",
            "apps/chaptera-desktop/src/main.rs",
        }:
            changed_rust.append(path)

    commands: list[dict] = []
    for path in sorted(dict.fromkeys(changed_rust)):
        commands.append(
            {
                "id": f"rustfmt:{path}",
                "argv": [
                    "python",
                    "tools/ci/check_rustfmt_delta.py",
                    "--base",
                    base,
                    "--head",
                    head,
                    "--path",
                    path,
                    "--edition",
                    edition_for(path),
                ],
            }
        )

    packages = sorted(affected)
    if packages:
        flags = [
            item
            for package in packages
            for item in ("-p", VENDOR_PACKAGE_SPECS.get(package, package))
        ]
        commands.extend(
            [
                {
                    "id": "vendor-clippy",
                    "argv": [
                        "cargo",
                        "clippy",
                        "--manifest-path",
                        "vendor/producer-a/Cargo.toml",
                        *flags,
                        "--all-targets",
                        "--",
                        "-D",
                        "warnings",
                    ],
                },
                {
                    "id": "vendor-source-free-tests",
                    "argv": [
                        "cargo",
                        "test",
                        "--manifest-path",
                        "vendor/producer-a/Cargo.toml",
                        *flags,
                        "--lib",
                    ],
                },
            ]
        )
        if "pub-viewer" in affected:
            commands.append(
                {
                    "id": "pub-viewer-cmo-slot-compose",
                    "argv": [
                        "cargo",
                        "test",
                        "--manifest-path",
                        "vendor/producer-a/Cargo.toml",
                        "-p",
                        "pub-viewer",
                        "--features",
                        "cmo-slot-compose",
                        "--lib",
                    ],
                }
            )
        if "pub-editor" in affected:
            commands.append(
                {
                    "id": "pub-editor-integration-tests",
                    "argv": [
                        "cargo",
                        "test",
                        "--manifest-path",
                        "vendor/producer-a/Cargo.toml",
                        "-p",
                        "pub-editor",
                        "--tests",
                    ],
                }
            )

    if PAGE_PROJECTION_SOURCE in paths:
        commands.append(
            {
                "id": "pub-reader-master-bridge-check",
                "argv": [
                    "cargo",
                    "check",
                    "--manifest-path",
                    "vendor/producer-a/Cargo.toml",
                    "-p",
                    "pub-reader",
                    "--bin",
                    "master_projection_bridge_receipt",
                    "--features",
                    "master-authority-bridge",
                ],
            }
        )

    if render_plan:
        commands.extend(
            [
                {
                    "id": "render-plan-clippy",
                    "argv": [
                        "cargo",
                        "clippy",
                        "--manifest-path",
                        "crates/chaptera-viewer-render-plan/Cargo.toml",
                        "--all-targets",
                        "--features",
                        "projected-scene-instances",
                        "--",
                        "-D",
                        "warnings",
                    ],
                },
                {
                    "id": "render-plan-tests",
                    "argv": [
                        "cargo",
                        "test",
                        "--manifest-path",
                        "crates/chaptera-viewer-render-plan/Cargo.toml",
                        "--features",
                        "projected-scene-instances",
                    ],
                },
            ]
        )

    if desktop:
        # Shared Reader/Viewer changes only need to prove the two shipped desktop
        # binary feature surfaces compile. --all-targets pulls desktop
        # dev/test-only dependencies and is reserved for directly owned desktop
        # source or root workspace changes.
        desktop_all_targets = desktop_source_changed or any(
            p in {"Cargo.toml", "Cargo.lock"} for p in paths
        )
        desktop_target_args = (
            ["--all-targets"]
            if desktop_all_targets
            else ["--bin", "chaptera-editor"]
        )
        commands.extend(
            [
                {
                    "id": "desktop-reader-check",
                    "argv": [
                        "cargo",
                        "check",
                        "-p",
                        "chaptera-desktop",
                        "--features",
                        "reader-only",
                        *desktop_target_args,
                    ],
                },
                {
                    "id": "desktop-editor-check",
                    "argv": [
                        "cargo",
                        "check",
                        "-p",
                        "chaptera-desktop",
                        *desktop_target_args,
                    ],
                },
            ]
        )
        if desktop_source_changed:
            changed_desktop = [
                path
                for path in paths
                if path in {
                    "apps/chaptera-desktop/src/render_backend.rs",
                    "apps/chaptera-desktop/src/main.rs",
                }
            ]
            commands.append(
                {
                    "id": "desktop-reader-clippy-delta",
                    "argv": [
                        "python",
                        "tools/ci/check_desktop_clippy_delta.py",
                        "--base",
                        base,
                        "--head",
                        head,
                        *[
                            item
                            for path in changed_desktop
                            for item in ("--path", path)
                        ],
                    ],
                }
            )
    if mobile_reader:
        commands.append(
            {
                "id": "mobile-reader-core-check",
                "argv": [
                    "cargo",
                    "check",
                    "--manifest-path",
                    "crates/chaptera-mobile-reader-core/Cargo.toml",
                ],
            }
        )

    return {
        "direct_vendor_packages": sorted(direct),
        "affected_vendor_packages": packages,
        "render_plan": render_plan,
        "desktop_reader_integration": desktop,
        "desktop_reader_clippy": desktop_source_changed,
        "mobile_reader_integration": mobile_reader,
        "commands": commands,
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--base", required=True)
    parser.add_argument("--head", required=True)
    parser.add_argument("--receipt", required=True)
    args = parser.parse_args()

    paths = changed_paths(args.base, args.head)
    plan = build_plan(paths, args.base, args.head)
    receipt = {
        "schema": "chaptera.reader-consumer-preflight.v1",
        "base_sha": args.base,
        "head_sha": args.head,
        "changed_paths": paths,
        **plan,
    }
    receipt["commands"] = [
        {"id": c["id"], "argv": c["argv"], "status": "planned"} for c in plan["commands"]
    ]

    out = Path(args.receipt)
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n")
    print(json.dumps(receipt, indent=2, sort_keys=True))

    for index, command in enumerate(plan["commands"]):
        print(f"::group::{command['id']}")
        try:
            subprocess.run(command["argv"], check=True)
        except subprocess.CalledProcessError:
            receipt["commands"][index]["status"] = "failure"
            out.write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n")
            raise
        else:
            receipt["commands"][index]["status"] = "success"
            out.write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n")
        finally:
            print("::endgroup::")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
