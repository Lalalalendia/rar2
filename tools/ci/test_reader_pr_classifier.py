#!/usr/bin/env python3
from __future__ import annotations

import importlib.util
from pathlib import Path
import unittest

MODULE_PATH = Path("tools/ci/reader_pr_classifier.py")
SPEC = importlib.util.spec_from_file_location("reader_pr_classifier", MODULE_PATH)
assert SPEC and SPEC.loader
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class ReaderPrClassifierTests(unittest.TestCase):
    def test_pr7_shared_reader_change_avoids_unrelated_products(self) -> None:
        fanout = MODULE.classify(
            [
                "vendor/producer-a/crates/pub-contents/src/lib.rs",
                "vendor/producer-a/crates/pub-reader/src/lib.rs",
                "vendor/producer-a/crates/pub-viewer/src/lib.rs",
            ]
        )
        self.assertTrue(fanout["tier_a_required"])
        self.assertTrue(fanout["reader_windows"])
        self.assertTrue(fanout["desktop_windows"])
        self.assertTrue(fanout["visual_oracle"])
        self.assertTrue(fanout["typography"])
        self.assertTrue(fanout["android_core"])
        self.assertFalse(fanout["android_local_open"])
        self.assertFalse(fanout["android_render"])
        self.assertFalse(fanout["portable"])
        self.assertFalse(fanout["web_chromium"])
        self.assertFalse(fanout["installer"])
        self.assertFalse(fanout["updater"])

    def test_pr15_render_change_does_not_admit_installer_updater_or_web(self) -> None:
        fanout = MODULE.classify(
            [
                "apps/chaptera-desktop/src/render_backend.rs",
                "crates/chaptera-viewer-render-plan/src/lib.rs",
                "tools/ci/reader_consumer_preflight.py",
                "vendor/producer-a/crates/pub-viewer/src/lib.rs",
            ]
        )
        self.assertTrue(fanout["tier_a_required"])
        self.assertTrue(fanout["reader_windows"])
        self.assertTrue(fanout["desktop_windows"])
        self.assertTrue(fanout["visual_oracle"])
        self.assertTrue(fanout["typography"])
        self.assertTrue(fanout["android_core"])
        self.assertFalse(fanout["android_local_open"])
        self.assertFalse(fanout["android_render"])
        self.assertFalse(fanout["portable"])
        self.assertFalse(fanout["web_chromium"])
        self.assertFalse(fanout["installer"])
        self.assertFalse(fanout["updater"])

    def test_mobile_specific_change_admits_android_emulator_workflows(self) -> None:
        fanout = MODULE.classify(
            ["apps/chaptera-mobile-android/app/src/main/AndroidManifest.xml"]
        )
        self.assertTrue(fanout["android_local_open"])
        self.assertTrue(fanout["android_render"])
        self.assertFalse(fanout["web_chromium"])

    def test_web_specific_change_admits_chromium_without_reader_windows(self) -> None:
        fanout = MODULE.classify(["apps/web/editor-shell-http-harness.html"])
        self.assertTrue(fanout["web_chromium"])
        self.assertTrue(fanout["portable"])
        self.assertFalse(fanout["reader_windows"])

    def test_installer_and_updater_are_owned_seams(self) -> None:
        package = MODULE.classify(["packages/product/reader-portable/v1/README.md"])
        self.assertTrue(package["installer"])
        self.assertFalse(package["updater"])

        installer = MODULE.classify(["installer/windows/chaptera-reader.iss"])
        self.assertTrue(installer["installer"])
        self.assertTrue(installer["updater"])

        updater = MODULE.classify(["crates/chaptera-update-engine/src/lib.rs"])
        self.assertFalse(updater["installer"])
        self.assertTrue(updater["updater"])

    def test_force_all_is_explicit_manual_full_fanout(self) -> None:
        fanout = MODULE.classify([], force_all=True)
        self.assertTrue(all(fanout.values()))


if __name__ == "__main__":
    unittest.main()
