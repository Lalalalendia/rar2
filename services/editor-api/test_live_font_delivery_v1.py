#!/usr/bin/env python3
"""Real exact font delivery + independent rights + HTTP negative controls."""
from __future__ import annotations

import copy
import hashlib
import json
import threading
import unittest
from http.server import ThreadingHTTPServer
from urllib.error import HTTPError
from urllib.parse import urlencode
from urllib.request import Request, urlopen

# Imports tools/scene_v1 through the existing real Editor service owner.
import web_real_acceptance_service as service
from scene_v1 import finalize_snapshot
from pinned_opentype_resource_v1 import (
    ABEL_RESOURCE_ID, ABEL_SHA256, load_pinned_abel,
)
from live_font_delivery_v1 import (
    LiveFontDeliveryDenied, bind_font_set_to_scene, build_font_environment,
    issue_current_admission, read_current_exact_font,
)

DOC = "9c2f8d5e-3f1b-4a57-9d40-6a7e2c11b001"
TENANT = "pinned-live-test"


def fixture_scene():
    return finalize_snapshot({
        "protocol_version": "chaptera.scene.v1",
        "document_id": DOC,
        "source_hash": "a" * 64,
        "revision_id": "sha256:" + "1" * 64,
        "snapshot_id": "sha256:" + "0" * 64,
        "layout_environment": {
            "environment_id": "sha256:" + "3" * 64,
            "font_set_fingerprint": "sha256:" + "4" * 64,
            "engine_revision": "viewer-geometry-v0.1",
            "resource_fingerprint": "sha256:" + "5" * 64,
        },
        "stacking_fidelity": "unknown",
        "pages": [], "nodes": [], "stories": [], "story_frames": [],
        "paints": [], "resources": [], "diagnostics": [],
        "capabilities": [], "fidelity": {"state": "partial", "reasons": []},
    })


class LiveFontDeliveryTests(unittest.TestCase):
    def setUp(self):
        self.raw_font = load_pinned_abel()
        self.baseline = fixture_scene()
        self.scene = bind_font_set_to_scene(self.baseline, self.raw_font)
        self.env = build_font_environment(self.scene, self.raw_font)

    def test_opt_in_changes_only_scoped_font_environment_and_snapshot(self):
        self.assertEqual(bind_font_set_to_scene(self.baseline, None), self.baseline)
        self.assertEqual(self.baseline["source_hash"], self.scene["source_hash"])
        self.assertEqual(self.baseline["revision_id"], self.scene["revision_id"])
        self.assertNotEqual(self.baseline["snapshot_id"], self.scene["snapshot_id"])
        self.assertNotEqual(
            self.baseline["layout_environment"]["font_set_fingerprint"],
            self.scene["layout_environment"]["font_set_fingerprint"],
        )
        self.assertEqual(self.scene, bind_font_set_to_scene(self.baseline, self.raw_font))
        self.assertEqual(self.scene["nodes"], self.baseline["nodes"])
        self.assertEqual(self.scene["story_frames"], self.baseline["story_frames"])

    def test_one_real_delivered_face_and_grant_are_current_and_independent(self):
        self.assertEqual(self.env["scene_snapshot_id"], self.scene["snapshot_id"])
        self.assertEqual(self.env["layout_environment_id"],
                         self.scene["layout_environment"]["environment_id"])
        self.assertEqual(self.env["font_set_fingerprint"],
                         self.scene["layout_environment"]["font_set_fingerprint"])
        self.assertEqual(len(self.env["fonts"]), 1)
        self.assertEqual(self.env["fonts"][0]["delivery"], "deliver_exact")
        self.assertEqual(self.env["fonts"][0]["font_fingerprint"], "sha256:" + ABEL_SHA256)
        grant = issue_current_admission(
            scene=self.scene, resource=self.raw_font, tenant_id=TENANT,
        )
        self.assertEqual(grant["resources"], [{
            "resource_id": ABEL_RESOURCE_ID,
            "font_fingerprint": "sha256:" + ABEL_SHA256,
            "content_hash": ABEL_SHA256,
            "face_index": 0,
        }])
        self.assertEqual(
            issue_current_admission(
                scene=self.baseline, resource=None, tenant_id=TENANT,
            )["resources"], [],
        )
        self.assertEqual(build_font_environment(self.baseline, None)["fonts"], [])
        delivered = read_current_exact_font(
            scene=self.scene, tenant_id=TENANT, resource=self.raw_font,
            resource_id=ABEL_RESOURCE_ID,
            revision_id=self.scene["revision_id"], snapshot_id=self.scene["snapshot_id"],
        )
        self.assertEqual(hashlib.sha256(delivered).hexdigest(), ABEL_SHA256)
        self.assertEqual(len(delivered), 35220)

    def test_stale_revision_snapshot_wrong_resource_and_disabled_mode_fail_closed(self):
        arguments = dict(
            scene=self.scene, tenant_id=TENANT, resource=self.raw_font,
            resource_id=ABEL_RESOURCE_ID,
            revision_id=self.scene["revision_id"],
            snapshot_id=self.scene["snapshot_id"],
        )
        for overrides in (
            {"snapshot_id": self.baseline["snapshot_id"]},
            {"revision_id": "sha256:" + "e" * 64},
            {"resource_id": DOC},
            {"resource": None},
        ):
            with self.subTest(overrides=overrides), self.assertRaises(LiveFontDeliveryDenied):
                read_current_exact_font(**dict(arguments, **overrides))

    def test_real_http_all_font_surfaces_are_edit_only_and_stale_binary_denied(self):
        env = self.env
        scene = self.scene
        resource = self.raw_font

        class Authz:
            def authorize(self, *, capability, principal_id, **kwargs):
                if capability != service.CAP_EDIT_TEXT or principal_id != "editor":
                    raise service.AuthzDenied("font_edit_forbidden")

        class TestState:
            authz = Authz()
            document_id = DOC
            tenant_id = TENANT

            def font_environment(self):
                return build_font_environment(scene, resource)

            def font_authoring_admission(self):
                return issue_current_admission(
                    scene=scene, tenant_id=TENANT, resource=resource,
                )

            def font_resource_bytes(self, fetch_handle):
                return read_current_exact_font(
                    scene=scene, tenant_id=TENANT, resource=resource,
                    resource_id=ABEL_RESOURCE_ID, revision_id=scene["revision_id"],
                    snapshot_id=scene["snapshot_id"], fetch_handle=fetch_handle,
                )

        original_state = service.STATE
        service.STATE = TestState()
        server = ThreadingHTTPServer(("127.0.0.1", 0), service.Handler)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        base = f"http://127.0.0.1:{server.server_port}"
        handles = [
            "/v1/editor/font-environment",
            "/v1/editor/font-authoring-admission",
            "/v1/editor/font-resource/" + self.env["fonts"][0]["fetch_handle"],
        ]
        try:
            for handle in handles:
                for principal in (None, "viewer"):
                    headers = {} if principal is None else {
                        "x-chaptera-principal-id": principal,
                    }
                    with self.subTest(handle=handle, principal=principal):
                        with self.assertRaises(HTTPError) as denied:
                            urlopen(Request(base + handle, headers=headers), timeout=5)
                        self.assertEqual(denied.exception.code, 403)
            headers = {"x-chaptera-principal-id": "editor"}
            for handle, key in ((handles[0], "fonts"), (handles[1], "resources")):
                with urlopen(Request(base + handle, headers=headers), timeout=5) as response:
                    self.assertEqual(len(json.load(response)[key]), 1)
            with urlopen(Request(base + handles[2], headers=headers), timeout=5) as response:
                self.assertEqual(response.headers.get("content-type"), "font/ttf")
                self.assertEqual(response.headers.get("x-chaptera-font-content-sha256"),
                                 ABEL_SHA256)
                self.assertEqual(hashlib.sha256(response.read()).hexdigest(), ABEL_SHA256)
            for handle in (
                "/v1/editor/font-resource/" + ABEL_RESOURCE_ID,
                handles[2] + "changed",
                "/v1/editor/font-resource/" + "stale_invalid_handle",
            ):
                with self.assertRaises(HTTPError) as denied:
                    urlopen(Request(base + handle, headers=headers), timeout=5)
                self.assertEqual(denied.exception.code, 409)
        finally:
            server.shutdown()
            server.server_close()
            thread.join(timeout=5)
            service.STATE = original_state


if __name__ == "__main__":
    unittest.main()
