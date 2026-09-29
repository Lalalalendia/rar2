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
)
UPSTREAM = {"pub-core", "pub-cfb", "pub-contents", "pub-escher", "pub-model", "pub-quill"}


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


def build_plan(paths: list[str]) -> dict:
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
    root_workspace_changed = any(p in {"Cargo.toml", "Cargo.lock"} for p in paths)
    if root_workspace_changed:
        render_plan = True

    desktop = bool(packages) or render_plan or desktop_source_changed or root_workspace_changed

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
                    "rustfmt",
                    "--check",
                    "--edition",
                    edition_for(path),
                    "--config",
                    "skip_children=true",
                    path,
                ],
            }
        )

    packages = sorted(affected)
    if packages:
        flags = [item for package in packages for item in ("-p", package)]
        commands.extend(
            [
                {
                    "id": "vendor-check",
                    "argv": [
                        "cargo",
                        "check",
                        "--manifest-path",
                        "vendor/producer-a/Cargo.toml",
                        *flags,
                        "--all-targets",
                    ],
                },
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

    if render_plan:
        commands.extend(
            [
                {
                    "id": "render-plan-check",
                    "argv": [
                        "cargo",
                        "check",
                        "--manifest-path",
                        "crates/chaptera-viewer-render-plan/Cargo.toml",
                        "--all-targets",
                        "--features",
                        "projected-scene-instances",
                    ],
                },
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
        commands.append(
            {
                "id": "desktop-reader-check",
                "argv": [
                    "cargo",
                    "check",
                    "-p",
                    "chaptera-desktop",
                    "--features",
                    "reader-only",
                    "--all-targets",
                ],
            }
        )

    if desktop_source_changed or root_workspace_changed:
        commands.append(
            {
                "id": "desktop-reader-clippy",
                "argv": [
                    "cargo",
                    "clippy",
                    "-p",
                    "chaptera-desktop",
                    "--features",
                    "reader-only",
                    "--all-targets",
                    "--",
                    "-D",
                    "warnings",
                ],
            }
        )

    return {
        "direct_vendor_packages": sorted(direct),
        "affected_vendor_packages": packages,
        "render_plan": render_plan,
        "desktop_reader_integration": desktop,
        "commands": commands,
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--base", required=True)
    parser.add_argument("--head", required=True)
    parser.add_argument("--receipt", required=True)
    args = parser.parse_args()

    paths = changed_paths(args.base, args.head)
    plan = build_plan(paths)
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
