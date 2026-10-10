#!/usr/bin/env python3
"""Fail-closed unshaped font history, Scene fidelity and output gates."""
from __future__ import annotations

import copy
import pathlib
import sys
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "tools"))

from pinned_font_commit_backend_v1 import (
    contains_font_history, unshaped_scene_project, partial_font_scene,
    blocked_native_pub_preview, FONT_PARTIAL_REASON,
)


class FontProjectionTests(unittest.TestCase):
    def setUp(self):
        self.font = {
            "kind": "set_text_format_property",
            "story_id": "9c2f8d5e-3f1b-4a57-9d40-6a7e2c11b001",
            "start_scalar": 0, "end_scalar": 1,
            "property": "font_resource",
            "value": {
                "resource_id": "f27a8036-8492-480f-8fa6-d2e775cc9f12",
                "font_fingerprint": "sha256:" + "a" * 64,
                "content_hash": "b" * 64, "face_index": 0,
            },
            "before_state_hash": "sha256:" + "1" * 64,
            "after_state_hash": "sha256:" + "2" * 64,
        }

    def test_only_font_history_is_withheld_from_current_geometry(self):
        baseline = {"schema_version": "pub-editor-v0.11", "operations": []}
        project = {**baseline, "operations": [copy.deepcopy(self.font)]}
        self.assertTrue(contains_font_history(project))
        self.assertFalse(contains_font_history(baseline))
        displayed = unshaped_scene_project(project)
        self.assertEqual(displayed["operations"], [])
        self.assertEqual(project["operations"], [self.font])
        invalid = copy.deepcopy(project)
        invalid["operations"][0]["before_state_hash"] = "not-a-sha"
        with self.assertRaises(ValueError):
            contains_font_history(invalid)
        invalid = copy.deepcopy(project)
        invalid["operations"][0]["client_font_path"] = "C:/Fonts/Abel.ttf"
        with self.assertRaises(ValueError):
            unshaped_scene_project(invalid)

    def test_partial_disclosure_changes_snapshot_not_source_geometry(self):
        scene = {
            "protocol_version": "chaptera.scene.v1",
            "document_id": "9c2f8d5e-3f1b-4a57-9d40-6a7e2c11b001",
            "revision_id": "sha256:" + "1" * 64,
            "snapshot_id": "sha256:" + "0" * 64,
            "pages": [], "nodes": [], "stories": [], "story_frames": [],
            "paints": [], "resources": [], "diagnostics": [],
            "capabilities": [],
            "fidelity": {"state": "supported", "reasons": []},
        }
        original = copy.deepcopy(scene)
        changed = partial_font_scene(scene)
        self.assertEqual(changed["fidelity"]["state"], "partial")
        self.assertIn(FONT_PARTIAL_REASON, changed["fidelity"]["reasons"])
        self.assertNotEqual(changed["snapshot_id"], scene["snapshot_id"])
        for key in ("pages", "nodes", "stories", "story_frames", "paints"):
            self.assertEqual(changed[key], original[key])
        self.assertEqual(scene, original)

    def test_download_and_native_publisher_approval_are_never_inferred(self):
        preview = blocked_native_pub_preview(
            document_id="9c2f8d5e-3f1b-4a57-9d40-6a7e2c11b001",
            source_hash="a" * 64, revision_id="sha256:" + "1" * 64,
        )
        self.assertFalse(preview["can_serialize"])
        self.assertFalse(preview["can_download"])
        self.assertFalse(preview["native_publisher_authorized"])
        self.assertEqual(preview["blocker_code"],
                         "font_resource_native_pub_output_not_admitted")


if __name__ == "__main__":
    unittest.main()
