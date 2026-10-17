#!/usr/bin/env python3
"""Keep immutable Reader release in sync with every Rust-embedded editor asset."""

from __future__ import annotations

from fnmatch import fnmatchcase
from pathlib import Path
import re
import subprocess
import sys
import unittest

ROOT = Path(__file__).resolve().parents[1]
RELEASE = ROOT / ".github/workflows/cloud-reader-linux-release.yml"
EMBEDDED = ROOT / "apps/chaptera-server/src/product_editor_assets.rs"
LEGACY_PRODUCTION_DEPLOY = ROOT / ".github/workflows/cloud-reader-deploy.yml"
HOST_INSTALL = ROOT / "deploy/install-reader-host.sh"
HOST_BOOTSTRAP = ROOT / "deploy/bootstrap-reader-deployer.sh"
HOST_HARNESS = ROOT / "tools/run_cloud_reader_host_acceptance.py"

# Rust source owns the inventory of compile-time embedded files.
# Authenticated and temporary Editor assets share the same immutable release packet.


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

    def test_all_rust_embedded_editor_assets_trigger_a_new_release(self):
        references = re.findall(
            r'include_bytes!\s*\(\s*"([^"]+)"\s*\)',
            self.rust,
        )
        self.assertTrue(references, "missing compile-time asset inventory")
        source_paths = set()
        for relative in references:
            physical = (EMBEDDED.parent / relative).resolve()
            try:
                source = physical.relative_to(ROOT.resolve()).as_posix()
            except ValueError:
                self.fail(f"embedded asset escapes repository: {relative}")
            source_paths.add(source)
            with self.subTest(source=source):
                self.assertTrue(physical.is_file(), "embedded asset is missing")
                self.assertTrue(
                    any(fnmatchcase(source, rule) for rule in self.triggers),
                    "Rust-embedded asset changed without a new release trigger",
                )
        for critical in (
            "apps/web/temporary-edit-move-client-v1.mjs",
            "apps/web/rich-reader-editor-shell-v1.mjs",
            "apps/web/reader-scene-editor-adapter-v1.mjs",
            "packages/protocol/editor-resize/v1/resize-gesture-v1.mjs",
        ):
            self.assertIn(critical, source_paths)

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


    def test_current_host_acceptance_harness_is_immutable_packet_input(self):
        harness = HOST_HARNESS.read_text(encoding="utf-8")
        installer = HOST_INSTALL.read_text(encoding="utf-8")
        bootstrap = HOST_BOOTSTRAP.read_text(encoding="utf-8")
        path = "tools/run_cloud_reader_host_acceptance.py"
        self.assertTrue(any(fnmatchcase(path, rule) for rule in self.triggers))
        self.assertIn("dist/" + path, self.workflow)
        self.assertIn('''${RELEASE}/''' + path, installer)
        self.assertIn(path, bootstrap)
        self.assertIn('SERVICES = ("chaptera-reader.service",)', harness)
        self.assertNotIn('"chaptera-web.service"', harness)
        self.assertNotIn('"chaptera-worker.service"', harness)
        self.assertNotIn('stderr[-800:]', harness)
        self.assertIn('"host acceptance subprocess failed rc=', harness)
        self.assertIn("private_cleanup_verified = private_cleanup[\"verified\"]", harness)
        self.assertIn("complete = host_operability_complete and private_cleanup_verified and doctor_verified", harness)
        self.assertIn("doctor_receipt = live_doctor_receipt(current_binary, args.config)", harness)
        self.assertIn('"chaptera.doctor-live.v1"', harness)
        self.assertIn('doctor_verified = doctor_receipt["verified"]', harness)
        self.assertIn("def private_storage_snapshot(", harness)
        self.assertIn("mode=ro", harness)
        self.assertIn('"--verify-private-expiry"', harness)
        self.assertIn("private_expiry_not_requested", harness)
        self.assertIn('not before["reservation_active"]', harness)
        self.assertIn("common = guest_mutation_headers(origin, csrf)", harness)
        self.assertIn('"origin": origin', harness)
        result = subprocess.run(
            [sys.executable, str(HOST_HARNESS), "--self-test"],
            capture_output=True, text=True, check=False,
        )
        self.assertEqual(result.returncode, 0, result.stderr[-1200:])

    def test_reader_host_install_preserves_verified_previous_release(self):
        install = HOST_INSTALL.read_text(encoding="utf-8")
        self.assertIn('PREVIOUS="/opt/chaptera/previous"', install)
        self.assertIn('readlink -f "${CURRENT}"', install)
        self.assertIn('^/opt/chaptera/releases/[0-9a-f]{40}$', install)
        self.assertIn('[[ ! -x "${old_release}/chaptera" ]]', install)
        self.assertIn('if [[ "${old_release}" != "${RELEASE}" ]]; then', install)
        self.assertIn('ln -sfn "${old_release}" "${PREVIOUS}"', install)
        self.assertIn('ln -sfn "${RELEASE}" "${CURRENT}"', install)
        self.assertLess(
            install.index('ln -sfn "${old_release}" "${PREVIOUS}"'),
            install.index('ln -sfn "${RELEASE}" "${CURRENT}"'),
            "rollback target must be recorded before current is switched",
        )


if __name__ == "__main__":
    unittest.main()
