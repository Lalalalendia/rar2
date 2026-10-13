#!/usr/bin/env python3
"""Prevent Cloud Reader Linux releases from omitting embedded web changes."""
from __future__ import annotations

import fnmatch
import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
RELEASE_WORKFLOW = ROOT / ".github/workflows/cloud-reader-linux-release.yml"
ASSETS_RUST = ROOT / "apps/chaptera-server/src/product_editor_assets.rs"


def push_filters() -> set[str]:
    workflow = RELEASE_WORKFLOW.read_text(encoding="utf-8")
    try:
        push = workflow.split("\n  push:\n", 1)[1].split("\n  workflow_dispatch:", 1)[0]
    except IndexError as error:
        raise AssertionError("release workflow lost its main push trigger") from error
    if "branches: [main]" not in push or "    paths:" not in push:
        raise AssertionError("release workflow must watch source changes on main")
    patterns = set(re.findall(r'^\s+- "([^"]+)"\s*$', push, flags=re.M))
    if not patterns:
        raise AssertionError("empty release workflow path watch")
    return patterns


def watched(path: str, patterns: set[str]) -> bool:
    return any(fnmatch.fnmatchcase(path, pattern) for pattern in patterns)


class CloudReaderReleaseTriggerTests(unittest.TestCase):
    def test_every_compiled_in_browser_asset_triggers_a_release(self) -> None:
        patterns = push_filters()
        rust = ASSETS_RUST.read_text(encoding="utf-8")
        embedded = re.findall(r'include_bytes!\s*\(\s*"([^"]+)"\s*\)', rust)
        self.assertGreaterEqual(len(embedded), 20, "embedded assets not discovered")
        for include in embedded:
            embedded_file = (ASSETS_RUST.parent / include).resolve()
            relative = embedded_file.relative_to(ROOT).as_posix()
            with self.subTest(asset=relative):
                self.assertTrue(embedded_file.is_file(), "missing embedded asset")
                self.assertTrue(watched(relative, patterns),
                                f"compiled asset omitted by release trigger: {relative}")

    def test_rust_workspace_vendor_and_lock_changes_trigger_a_release(self) -> None:
        patterns = push_filters()
        required = (
            "apps/chaptera-server/src/main.rs",
            "crates/pub-model/src/lib.rs",
            "vendor/producer-a/crates/pub-editor/src/lib.rs",
            "vendor/producer-a/crates/pub-reader/src/lib.rs",
            "Cargo.toml",
            "Cargo.lock",
            "deploy/nginx/chaptera-reader.conf.example",
        )
        for relative in required:
            with self.subTest(path=relative):
                # This repository currently omits Cargo.lock. Keep its
                # future-change trigger, but do not require the file to exist.
                if relative != "Cargo.lock":
                    self.assertTrue((ROOT / relative).is_file(), "tracked input missing")
                self.assertTrue(watched(relative, patterns),
                                f"source dependency omitted by release trigger: {relative}")

    def test_release_packet_is_sha_bound_and_does_not_deploy_hosts(self) -> None:
        workflow = RELEASE_WORKFLOW.read_text(encoding="utf-8")
        self.assertIn("TAG: cloud-reader-${{ github.sha }}", workflow)
        self.assertIn("GITHUB_SHA", workflow)
        self.assertIn("dist/COMMIT_SHA", workflow)
        self.assertIn("sha256sum", workflow)
        self.assertNotIn("chaptera-site", workflow)
        self.assertNotIn("DEPLOY_SSH_KEY", workflow)


if __name__ == "__main__":
    unittest.main()
