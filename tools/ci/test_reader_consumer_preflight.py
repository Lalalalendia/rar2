#!/usr/bin/env python3
from __future__ import annotations

import importlib.util
from pathlib import Path
import unittest

MODULE_PATH = Path("tools/ci/reader_consumer_preflight.py")
SPEC = importlib.util.spec_from_file_location("reader_consumer_preflight", MODULE_PATH)
assert SPEC and SPEC.loader
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class ReaderConsumerPreflightTests(unittest.TestCase):
    def test_workspace_editions_resolve_to_concrete_rustfmt_value(self) -> None:
        self.assertEqual(
            MODULE.edition_for("vendor/producer-a/crates/pub-reader/src/lib.rs"),
            "2024",
        )
        self.assertEqual(
            MODULE.edition_for("crates/chaptera-viewer-render-plan/src/lib.rs"),
            "2024",
        )
        self.assertEqual(
            MODULE.edition_for("apps/chaptera-desktop/src/main.rs"),
            "2024",
        )

    def test_pub_contents_change_expands_to_reader_consumer_closure(self) -> None:
        plan = MODULE.build_plan(
            ["vendor/producer-a/crates/pub-contents/src/lib.rs"]
        )
        self.assertEqual(plan["direct_vendor_packages"], ["pub-contents"])
        self.assertEqual(
            plan["affected_vendor_packages"],
            ["pub-contents", "pub-layout", "pub-reader", "pub-viewer"],
        )
        rustfmt = next(command for command in plan["commands"] if command["id"].startswith("rustfmt:"))
        self.assertIn("2024", rustfmt["argv"])
        ids = {command["id"] for command in plan["commands"]}
        self.assertIn("desktop-reader-check", ids)
        self.assertIn("desktop-editor-check", ids)
        self.assertIn("mobile-reader-core-check", ids)
        self.assertTrue(plan["desktop_reader_integration"])
        self.assertTrue(plan["mobile_reader_integration"])


if __name__ == "__main__":
    unittest.main()
