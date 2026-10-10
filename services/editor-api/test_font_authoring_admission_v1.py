#!/usr/bin/env python3
"""Negative-heavy source-neutral server font catalog tests (no external fonts)."""
from __future__ import annotations

import copy
import hashlib
import unittest
from dataclasses import replace

from font_authoring_admission_v1 import (
    FontAdmissionDenied,
    TrustedFontAuthoringResourceV1,
    issue_font_authoring_admission_v1,
)


DOC = "9c2f8d5e-3f1b-4a57-9d40-6a7e2c11b001"
RID_A = "81111111-1111-4111-8111-111111111111"
RID_B = "82222222-2222-4222-8222-222222222222"
FP_A = "sha256:" + "a" * 64
FP_B = "sha256:" + "c" * 64
REV = "sha256:" + "1" * 64
SNAP = "sha256:" + "2" * 64
LAYOUT = "sha256:" + "3" * 64
SET = "sha256:" + "4" * 64
TENANT = "tenant-exact"


def scene():
    return {
        "document_id": DOC,
        "revision_id": REV,
        "snapshot_id": SNAP,
        "layout_environment": {
            "environment_id": LAYOUT,
            "font_set_fingerprint": SET,
        },
    }


def descriptor(rid=RID_A, fp=FP_A, raw=b"synthetic private font", face=0):
    return {
        "resource_id": rid,
        "font_fingerprint": fp,
        "content_hash": hashlib.sha256(raw).hexdigest(),
        "face_index": face,
        "family": "Example Sans",
        "style": "Regular",
        "delivery": "deliver_exact",
        "fetch_handle": "opaque-server-provided-font-handle",
        "reason_code": "delivery.allowed",
        "fallback": None,
    }


def environment(fonts=None):
    return {
        "protocol_version": "chaptera.font-environment.v1",
        "document_id": DOC,
        "revision_id": REV,
        "scene_snapshot_id": SNAP,
        "layout_environment_id": LAYOUT,
        "font_set_fingerprint": SET,
        "preview_authority": "server_frame_geometry_only",
        "fonts": [descriptor()] if fonts is None else fonts,
        "diagnostics": [],
    }


def trusted(desc=None, raw=b"synthetic private font"):
    d = descriptor(raw=raw) if desc is None else desc
    return TrustedFontAuthoringResourceV1(
        tenant_id=TENANT,
        document_id=DOC,
        layout_environment_id=LAYOUT,
        font_set_fingerprint=SET,
        resource_id=d["resource_id"],
        font_fingerprint=d["font_fingerprint"],
        content_hash=d["content_hash"],
        face_index=d["face_index"],
        face_count=1,
        full_font_bytes=raw,
        parser_verified=True,  # stubbed independent parser/rights in this unit test
        is_full_resource=True,
        authoring_admitted=True,
    )


def issue(scene_=None, env=None, resources=None, tenant=TENANT):
    return issue_font_authoring_admission_v1(
        tenant_id=tenant,
        scene=scene() if scene_ is None else scene_,
        font_environment=environment() if env is None else env,
        trusted_resources=[trusted()] if resources is None else resources,
    )


class ServerFontAdmissionTests(unittest.TestCase):
    def assertDenied(self, code, **kwargs):
        with self.assertRaisesRegex(FontAdmissionDenied, code):
            issue(**kwargs)

    def test_one_exact_byte_admitted_resource_is_source_neutral(self):
        original = environment()
        before = copy.deepcopy(original)
        admitted = issue(env=original)
        self.assertEqual(admitted["protocol_version"], "chaptera.font-authoring-admission.v1")
        self.assertEqual(admitted["document_id"], DOC)
        self.assertEqual(admitted["revision_id"], REV)
        self.assertEqual(admitted["scene_snapshot_id"], SNAP)
        self.assertEqual(admitted["layout_environment_id"], LAYOUT)
        self.assertEqual(admitted["font_set_fingerprint"], SET)
        self.assertEqual(admitted["resources"], [
            {key: before["fonts"][0][key] for key in (
                "resource_id", "font_fingerprint", "content_hash", "face_index"
            )}
        ])
        wire = str(admitted)
        self.assertNotIn("synthetic private font", wire)
        self.assertNotIn("fetch_handle", wire)
        self.assertNotIn("license", wire)
        self.assertEqual(original, before)

    def test_no_independent_registry_is_empty_even_with_exact_delivery(self):
        self.assertEqual(issue(resources=[])["resources"], [])
        self.assertEqual(
            issue(resources=[replace(trusted(), authoring_admitted=False)])["resources"], []
        )
        self.assertEqual(issue(resources=[
            replace(trusted(), parser_verified=False)
        ])["resources"], [])
        self.assertEqual(issue(resources=[
            replace(trusted(), is_full_resource=False)
        ])["resources"], [])

    def test_no_delivery_or_subset_or_fallback_cannot_be_promoted(self):
        self.assertEqual(issue(env=environment(fonts=[]))["resources"], [])
        for mode in (
            "deliver_subset", "blocked", "server_render_only", "substitute_explicit"
        ):
            item = descriptor()
            item["delivery"] = mode
            env = environment(fonts=[item])
            self.assertEqual(issue(env=env)["resources"], [], mode)

    def test_revision_snapshot_layout_and_fontset_stale_rejected(self):
        for scope, key, code in [
            ("scene", "revision_id", "font_environment_revision_id_mismatch"),
            ("scene", "snapshot_id", "font_environment_scene_snapshot_id_mismatch"),
            ("scene_layout", "environment_id", "font_environment_layout_environment_id_mismatch"),
            ("scene_layout", "font_set_fingerprint", "font_environment_font_set_fingerprint_mismatch"),
        ]:
            changed = scene()
            target = changed["layout_environment"] if scope == "scene_layout" else changed
            target[key] = "sha256:" + "f" * 64
            self.assertDenied(code, scene_=changed)
        self.assertDenied(
            "font_environment_protocol_mismatch",
            env=dict(environment(), protocol_version="foreign"),
        )

    def test_cross_document_tenant_and_environment_do_not_grant(self):
        self.assertEqual(
            issue(resources=[replace(trusted(), tenant_id="tenant-other")])["resources"], []
        )
        self.assertEqual(
            issue(resources=[replace(trusted(), document_id=RID_B)])["resources"], []
        )
        self.assertDenied(
            "trusted_font_registry_environment_mismatch",
            resources=[replace(trusted(), font_set_fingerprint="sha256:" + "9" * 64)],
        )
        self.assertDenied("tenant_scope_missing", tenant="")

    def test_wrong_physical_hash_face_or_fingerprint_never_degrades_to_family(self):
        self.assertDenied(
            "trusted_font_bytes_hash_mismatch",
            resources=[replace(trusted(), full_font_bytes=b"different bytes")],
        )
        self.assertDenied(
            "trusted_font_resource_identity_mismatch",
            resources=[replace(trusted(), font_fingerprint=FP_B)],
        )
        self.assertDenied(
            "trusted_font_face_not_admitted",
            resources=[replace(trusted(), face_count=0)],
        )
        self.assertDenied(
            "trusted_font_resource_identity_mismatch",
            resources=[replace(trusted(), face_index=1, face_count=2)],
        )
        self.assertDenied(
            "exact_font_delivery_handle_missing",
            env=environment(fonts=[dict(descriptor(), fetch_handle=None)]),
        )

    def test_two_same_family_independent_physical_resources_sorted(self):
        raw_a, raw_b = b"font A", b"font B"
        a = descriptor(rid=RID_A, fp=FP_A, raw=raw_a)
        b = descriptor(rid=RID_B, fp=FP_B, raw=raw_b)
        result = issue(
            env=environment(fonts=[b, a]),
            resources=[trusted(b, raw_b), trusted(a, raw_a)],
        )
        self.assertEqual(
            [item["resource_id"] for item in result["resources"]], [RID_A, RID_B]
        )
        self.assertNotEqual(
            result["resources"][0]["content_hash"],
            result["resources"][1]["content_hash"],
        )

    def test_non_boolean_policy_values_are_never_authority(self):
        self.assertEqual(issue(resources=[
            replace(trusted(), authoring_admitted=1)
        ])["resources"], [])
        self.assertEqual(issue(resources=[
            replace(trusted(), parser_verified=1)
        ])["resources"], [])
        self.assertEqual(issue(resources=[
            replace(trusted(), is_full_resource=1)
        ])["resources"], [])

    def test_http_endpoint_requires_edit_text_capability_and_returns_no_fake_fonts(self):
        """The fixture HTTP surface cannot expose any unprovisioned fonts."""
        import json
        import threading
        from http.server import ThreadingHTTPServer
        from urllib.error import HTTPError
        from urllib.request import Request, urlopen

        import web_real_acceptance_service as service

        class Authz:
            def authorize(self, *, capability, principal_id, **kwargs):
                if capability != service.CAP_EDIT_TEXT or principal_id != "editor":
                    raise service.AuthzDenied("font_edit_forbidden")

        class TestState:
            tenant_id = TENANT
            document_id = DOC
            authz = Authz()

            def font_authoring_admission(self):
                return issue(resources=[])

        original_state = service.STATE
        service.STATE = TestState()
        server = ThreadingHTTPServer(("127.0.0.1", 0), service.Handler)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            url = f"http://127.0.0.1:{server.server_port}/v1/editor/font-authoring-admission"
            for principal in (None, "viewer"):
                headers = {} if principal is None else {"x-chaptera-principal-id": principal}
                with self.assertRaises(HTTPError) as failed:
                    urlopen(Request(url, headers=headers), timeout=5)
                self.assertEqual(failed.exception.code, 403)
            req = Request(url, headers={"x-chaptera-principal-id": "editor"})
            with urlopen(req, timeout=5) as response:
                self.assertEqual(response.status, 200)
                self.assertEqual(
                    json.load(response),
                    issue(resources=[]),
                )
        finally:
            server.shutdown()
            server.server_close()
            thread.join(timeout=5)
            service.STATE = original_state

    def test_duplicates_invalid_shape_and_missing_font_are_fail_closed(self):
        self.assertDenied(
            "duplicate_delivered_font_resource",
            env=environment(fonts=[descriptor(), descriptor()]),
        )
        self.assertDenied(
            "duplicate_trusted_font_resource",
            resources=[trusted(), trusted()],
        )
        self.assertDenied(
            "invalid_font_resource_id",
            env=environment(fonts=[dict(descriptor(), resource_id="Example Sans")]),
        )
        self.assertDenied(
            "invalid_font_face",
            env=environment(fonts=[dict(descriptor(), face_index=True)]),
        )
        self.assertDenied(
            "invalid_font_resource_digest",
            env=environment(fonts=[dict(descriptor(), content_hash="deadbeef")]),
        )
        self.assertDenied(
            "untrusted_font_registry_entry",
            resources=[{"resource_id": RID_A}],
        )


if __name__ == "__main__":
    unittest.main()
