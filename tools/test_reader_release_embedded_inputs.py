#!/usr/bin/env python3
"""Keep immutable Reader release in sync with Rust-embedded guest UI inputs."""

from __future__ import annotations

from fnmatch import fnmatchcase
from pathlib import Path
import re
import unittest

ROOT = Path(__file__).resolve().parents[1]
RELEASE = ROOT / ".github/workflows/cloud-reader-linux-release.yml"
EMBEDDED = ROOT / "apps/chaptera-server/src/product_editor_assets.rs"
LEGACY_PRODUCTION_DEPLOY = ROOT / ".github/workflows/cloud-reader-deploy.yml"
PROD_READER_CONFIG = ROOT / "deploy/config/chaptera.reader.prod.example.toml"

# All exposed Temporary Editor modules are include_bytes! in the Chaptera Rust
# release binary. Source-only UI edits must trigger a *new* immutable packet.
GUEST_RUNTIME_ASSETS = (
    "temporary-editor-preview.html",
    "temporary-editor-preview.mjs",
    "temporary-editor-preview.css",
    "temporary-edit-browser-session-v0.mjs",
    "temporary-edit-move-client-v1.mjs",
    "reader-scene-editor-interaction-v1.mjs",
    "interaction-v1.mjs",
)


class ReaderReleaseEmbeddedAssetTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.workflow = RELEASE.read_text(encoding="utf-8")
        cls.rust = EMBEDDED.read_text(encoding="utf-8")
        match = re.search(r"(?ms)^  push:\n(.*?)^  workflow_dispatch:", cls.workflow)
        if not match:
            raise AssertionError("Reader Linux packet must rebuild on main push")
        cls.triggers = tuple(
            re.findall(r'^\s+- "([^"]+)"\s*$', match.group(1), re.M)
        )

    def test_every_embedded_guest_asset_rebuilds_immutable_packet(self):
        self.assertIn('include_bytes!("../../web/temporary-edit-move-client-v1.mjs")',
                      self.rust.replace("=\n    include_bytes!", "= include_bytes!"))
        for name in GUEST_RUNTIME_ASSETS:
            source = "apps/web/" + name
            with self.subTest(source=source):
                self.assertIn('../../web/' + name, self.rust)
                self.assertTrue(any(fnmatchcase(source, rule) for rule in self.triggers),
                                "editing a Rust-embedded guest asset must trigger new release")

    def test_release_is_main_only_and_publishes_exact_sha(self):
        self.assertIn("branches: [main]", self.workflow)
        self.assertIn("cloud-reader-${{ github.sha }}", self.workflow)
        self.assertIn('printf \'%s\\n\' "${GITHUB_SHA}" > dist/COMMIT_SHA',
                      self.workflow)
        for input_path in (
            "apps/chaptera-server/src/product_editor_assets.rs",
            "apps/cloud-reader/render-v1.mjs",
            "deploy/nginx/chaptera-reader.conf.example",
            ".github/workflows/cloud-reader-linux-release.yml",
        ):
            with self.subTest(path=input_path):
                self.assertTrue(any(fnmatchcase(input_path, rule) for rule in self.triggers))


    def test_existing_sha_pinned_release_refuses_asset_overwrite(self):
        publish = self.workflow.split(
            "      - name: Publish immutable public release asset", 1
        )[1]
        self.assertIn(
            "if: github.event_name == 'push' && github.ref == 'refs/heads/main'",
            publish,
        )
        self.assertIn("gh release create", publish)
        self.assertNotIn("gh release upload", publish)
        self.assertNotIn("--clobber", publish)
        # A re-run of the same push SHA cannot rewrite bytes behind a
        # previously published tag. A new release must use a new commit SHA.
        self.assertRegex(
            publish,
            r'if gh release view [^\n]+; then\n'
            r'\s+echo "::error::[^\n]+\n'
            r'\s+exit 1\n'
            r'\s+fi\n',
        )


    def test_legacy_rar2_auto_production_deploy_is_removed(self):
        # Production deployment authority lives in chaptera-site and requires an
        # operator-pinned immutable release SHA. rar2 must only build releases;
        # release completion must never mutate the real host automatically.
        self.assertFalse(
            LEGACY_PRODUCTION_DEPLOY.exists(),
            "rar2 must not auto-deploy production; use chaptera-site pinned deploy",
        )


    def test_public_temporary_editor_uses_one_hour_sliding_ttl(self):
        config = PROD_READER_CONFIG.read_text(encoding="utf-8")
        self.assertRegex(
            config,
            r"(?ms)^\[cloud_reader_guest\]\n(?:.*\n)*?session_ttl_seconds = 3600$",
            "public Temporary Editor must retain accepted sessions for one hour",
        )


if __name__ == "__main__":
    unittest.main()
