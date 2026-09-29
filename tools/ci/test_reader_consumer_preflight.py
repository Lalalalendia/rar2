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

RUSTFMT_MODULE_PATH = Path("tools/ci/check_rustfmt_delta.py")
RUSTFMT_SPEC = importlib.util.spec_from_file_location("check_rustfmt_delta", RUSTFMT_MODULE_PATH)
assert RUSTFMT_SPEC and RUSTFMT_SPEC.loader
RUSTFMT_MODULE = importlib.util.module_from_spec(RUSTFMT_SPEC)
RUSTFMT_SPEC.loader.exec_module(RUSTFMT_MODULE)


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
            ["vendor/producer-a/crates/pub-contents/src/lib.rs"],
            "BASE",
            "HEAD",
        )
        self.assertEqual(plan["direct_vendor_packages"], ["pub-contents"])
        self.assertEqual(
            plan["affected_vendor_packages"],
            ["pub-contents", "pub-layout", "pub-reader", "pub-viewer"],
        )
        rustfmt = next(command for command in plan["commands"] if command["id"].startswith("rustfmt:"))
        self.assertIn("tools/ci/check_rustfmt_delta.py", rustfmt["argv"])
        self.assertIn("BASE", rustfmt["argv"])
        self.assertIn("HEAD", rustfmt["argv"])
        self.assertIn("2024", rustfmt["argv"])
        ids = {command["id"] for command in plan["commands"]}
        self.assertIn("desktop-reader-check", ids)
        self.assertIn("desktop-editor-check", ids)
        self.assertNotIn("desktop-reader-clippy", ids)
        self.assertIn("mobile-reader-core-check", ids)
        self.assertTrue(plan["desktop_reader_integration"])
        self.assertFalse(plan["desktop_reader_clippy"])
        self.assertTrue(plan["mobile_reader_integration"])
        self.assertNotIn("vendor-check", ids)
        self.assertIn("vendor-clippy", ids)
        vendor_clippy = next(command for command in plan["commands"] if command["id"] == "vendor-clippy")
        self.assertIn("--all-targets", vendor_clippy["argv"])
        self.assertEqual(vendor_clippy["argv"][-2:], ["-D", "warnings"])

    def test_render_plan_uses_clippy_as_compile_gate(self) -> None:
        plan = MODULE.build_plan(
            ["crates/chaptera-viewer-render-plan/src/lib.rs"],
            "BASE",
            "HEAD",
        )
        ids = {command["id"] for command in plan["commands"]}
        self.assertNotIn("render-plan-check", ids)
        self.assertIn("render-plan-clippy", ids)
        self.assertIn("render-plan-tests", ids)
        render_clippy = next(command for command in plan["commands"] if command["id"] == "render-plan-clippy")
        self.assertIn("--all-targets", render_clippy["argv"])
        self.assertIn("projected-scene-instances", render_clippy["argv"])
        self.assertEqual(render_clippy["argv"][-2:], ["-D", "warnings"])

    def test_direct_desktop_change_keeps_desktop_clippy(self) -> None:
        plan = MODULE.build_plan(
            ["apps/chaptera-desktop/src/render_backend.rs"],
            "BASE",
            "HEAD",
        )
        ids = {command["id"] for command in plan["commands"]}
        self.assertIn("desktop-reader-check", ids)
        self.assertIn("desktop-editor-check", ids)
        self.assertIn("desktop-reader-clippy", ids)
        self.assertTrue(plan["desktop_reader_integration"])
        self.assertTrue(plan["desktop_reader_clippy"])

    def test_rustfmt_delta_parses_added_and_deletion_only_hunks(self) -> None:
        diff = """@@ -10,2 +10,3 @@
@@ -25 +26,0 @@
"""
        self.assertEqual(
            RUSTFMT_MODULE.parse_changed_head_ranges(diff),
            [(10, 12), (26, 26)],
        )

    def test_rustfmt_delta_intersection_is_bounded(self) -> None:
        self.assertTrue(
            RUSTFMT_MODULE.ranges_intersect([(50, 52)], [(52, 60)])
        )
        self.assertFalse(
            RUSTFMT_MODULE.ranges_intersect([(10, 12)], [(50, 60)])
        )


if __name__ == "__main__":
    unittest.main()
