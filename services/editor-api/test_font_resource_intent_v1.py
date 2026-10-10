#!/usr/bin/env python3
"""Real RevisionKernel transaction tests for bounded physical-font authoring."""
from __future__ import annotations

import copy
import unittest

from font_resource_intent_v1 import (
    AUTHORITY, CANDIDATE_PROTOCOL, PROTOCOL, FontRevisionKernel,
    validate_font_intent_v1, validate_font_operation_v1,
    validate_server_font_scope_v1,
)
from security.authz_v1 import AuthzKernel
from security.authorized_revision_gateway import AuthorizedRevisionGateway

DOC = "9c2f8d5e-3f1b-4a57-9d40-6a7e2c11b001"
STORY = "f27a8036-8492-480f-8fa6-d2e775cc9f12"
FONT = "82222222-2222-4222-8222-222222222222"
SOURCE = "a" * 64
FINGERPRINT = "sha256:" + "b" * 64
FONT_HASH = "c" * 64
SCENE_ID = "sha256:" + "d" * 64
LAYOUT_ID = "sha256:" + "e" * 64
FONT_SET = "sha256:" + "f" * 64
BEFORE = "sha256:" + "1" * 64
AFTER = "sha256:" + "2" * 64


class FontCommitTests(unittest.TestCase):
    def setUp(self):
        self.kernel = FontRevisionKernel()
        self.baseline = {"schema_version": "pub-editor-v0.11", "source_hash": SOURCE,
                         "identity": {"project_id": "b" * 36, "document_id": DOC,
                                      "history_id": "c" * 36, "genesis_revision_id": "d" * 36},
                         "operations": []}
        self.record = self.kernel.register_baseline(
            document_id=DOC, source_hash=SOURCE, project=copy.deepcopy(self.baseline)
        )
        self.authz = AuthzKernel()
        for principal, role in (("editor", "editor"), ("viewer", "viewer")):
            self.authz.set_role(
                tenant_id="font-test", document_id=DOC, principal_id=principal, role=role
            )
        self.gateway = AuthorizedRevisionGateway(
            kernel=self.kernel, authz=self.authz, tenant_id="font-test"
        )
        self.candidate = {
            "protocol_version": CANDIDATE_PROTOCOL,
            "document_id": DOC,
            "expected_revision_id": self.record.revision_id,
            "scene_snapshot_id": SCENE_ID,
            "layout_environment_id": LAYOUT_ID,
            "font_set_fingerprint": FONT_SET,
            "resource_id": FONT,
            "font_fingerprint": FINGERPRINT,
            "content_hash": FONT_HASH,
            "face_index": 0,
            "authority": AUTHORITY,
        }
        self.request = {
            "protocol_version": PROTOCOL,
            "document_id": DOC,
            "source_hash": SOURCE,
            "base_revision_id": self.record.revision_id,
            "client_operation_id": "font-test-operation-00001",
            "command": {"kind": "set_admitted_font_resource", "story_id": STORY,
                        "start_scalar": 1, "end_scalar": 4,
                        "candidate": self.candidate},
        }
        self.operation = {
            "kind": "set_text_format_property", "story_id": STORY,
            "start_scalar": 1, "end_scalar": 4,
            "property": "font_resource",
            "value": {k: self.candidate[k] for k in
                      ("resource_id", "font_fingerprint", "content_hash", "face_index")},
            "before_state_hash": BEFORE, "after_state_hash": AFTER,
        }
        self.executor_calls = 0

    def executor(self, project, command):
        self.executor_calls += 1
        copied = copy.deepcopy(project)
        copied["operations"].append(copy.deepcopy(self.operation))
        return copy.deepcopy(self.operation), copied, [
            {"key": "font_resource", "state": "partial",
             "note": "authoritative_relayout_not_proven"}
        ]

    def test_edit_is_authorized_canonical_idempotent_and_immutable(self):
        accepted = self.gateway.commit(
            self.request, principal_id="editor", executor=self.executor
        )
        self.assertEqual(accepted["protocol_version"], "chaptera.commit-accepted.v1")
        self.assertEqual(accepted["canonical_operation"], self.operation)
        self.assertEqual(self.executor_calls, 1)
        self.assertEqual(self.kernel.read_revision(
            document_id=DOC, revision_id=accepted["revision_id"]
        ).project["operations"], [self.operation])
        self.assertEqual(self.baseline["operations"], [])
        self.assertEqual(
            self.gateway.commit(self.request, principal_id="editor", executor=self.executor),
            accepted,
        )
        self.assertEqual(self.executor_calls, 1)
        changed = copy.deepcopy(self.request)
        changed["command"]["end_scalar"] += 1
        rejected = self.gateway.commit(changed, principal_id="editor", executor=self.executor)
        self.assertEqual(rejected["code"], "idempotency_conflict")
        self.assertEqual(self.executor_calls, 1)

    def test_denied_viewer_stale_revision_and_forged_intent_cannot_commit(self):
        with self.assertRaises(Exception):
            self.gateway.commit(self.request, principal_id="viewer", executor=self.executor)
        self.assertEqual(self.executor_calls, 0)
        for field, bad in (
            ("authority", "client_is_authoritative"),
            ("expected_revision_id", "sha256:" + "0" * 64),
            ("content_hash", "not-sha256"),
            ("face_index", True),
        ):
            with self.subTest(field=field):
                r = copy.deepcopy(self.request)
                r["command"]["candidate"][field] = bad
                with self.assertRaises(ValueError):
                    validate_font_intent_v1(r)
        for key, value in (
            ("before_state_hash", BEFORE), ("font_path", "/usr/share/fonts/Abel.ttf"),
            ("font_bytes", FONT_HASH),
        ):
            with self.subTest(extra=key):
                r = copy.deepcopy(self.request)
                r["command"][key] = value
                with self.assertRaises(ValueError):
                    validate_font_intent_v1(r)
        initial = self.kernel.current_revision(DOC).revision_id
        self.assertEqual(initial, self.record.revision_id)
        result = self.gateway.commit(self.request, principal_id="editor", executor=self.executor)
        stale = copy.deepcopy(self.request)
        stale["client_operation_id"] = "font-test-operation-00002"
        self.assertEqual(self.gateway.commit(
            stale, principal_id="editor", executor=self.executor
        )["code"], "stale_revision")
        self.assertEqual(self.executor_calls, 1)
        self.assertEqual(self.kernel.current_revision(DOC).revision_id, result["revision_id"])

    def test_rust_canonical_identity_and_nonempty_change_required(self):
        for label, corrupt in (
            ("property", {"property": "font_family"}),
            ("font", {"value": {**self.operation["value"], "content_hash": "0" * 64}}),
            ("range", {"start_scalar": 9}),
            ("noop", {"after_state_hash": BEFORE}),
        ):
            with self.subTest(label=label):
                with self.assertRaises(ValueError):
                    validate_font_operation_v1(
                        self.request["command"], {**self.operation, **corrupt}
                    )
        r = copy.deepcopy(self.request)
        r["command"]["start_scalar"] = 4
        r["command"]["end_scalar"] = 4
        with self.assertRaises(ValueError):
            validate_font_intent_v1(r)

    def test_independent_current_scene_and_authoring_grant(self):
        scene = {
            "document_id": DOC, "revision_id": self.record.revision_id,
            "snapshot_id": SCENE_ID,
            "layout_environment": {
                "environment_id": LAYOUT_ID, "font_set_fingerprint": FONT_SET
            },
        }
        grant = {
            "protocol_version": "chaptera.font-authoring-admission.v1",
            "document_id": DOC, "revision_id": self.record.revision_id,
            "scene_snapshot_id": SCENE_ID,
            "layout_environment_id": LAYOUT_ID,
            "font_set_fingerprint": FONT_SET,
            "resources": [{k: self.candidate[k] for k in
                          ("resource_id", "font_fingerprint", "content_hash", "face_index")}],
        }
        validate_server_font_scope_v1(self.request["command"], scene, grant)
        for label, mutation in (
            ("missing_grant", lambda c, s, a: a.update(resources=[])),
            ("wrong_sha", lambda c, s, a: a["resources"][0].update(content_hash="0" * 64)),
            ("stale_scene", lambda c, s, a: s.update(snapshot_id="sha256:" + "0" * 64)),
            ("stale_policy", lambda c, s, a: s["layout_environment"].update(
                font_set_fingerprint="sha256:" + "0" * 64)),
        ):
            with self.subTest(label=label):
                c, s, a = copy.deepcopy((self.request["command"], scene, grant))
                mutation(c, s, a)
                with self.assertRaises(ValueError):
                    validate_server_font_scope_v1(c, s, a)


if __name__ == "__main__":
    unittest.main()
