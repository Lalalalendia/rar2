#!/usr/bin/env python3
"""Real OFL font-byte admission witness; no synthetic parser approval."""
from __future__ import annotations

from dataclasses import replace
import hashlib
import struct
import unittest

from pinned_opentype_resource_v1 import (
    ABEL_PATH, ABEL_SHA256, ABEL_RESOURCE_ID,
    PinnedFontDenied, attest_sfnt, load_pinned_abel,
)
from font_authoring_admission_v1 import (
    FontAdmissionDenied, issue_font_authoring_admission_v1,
)

DOC = "9c2f8d5e-3f1b-4a57-9d40-6a7e2c11b001"
TENANT = "test-tenant"
REV = "sha256:" + "1" * 64
SNAP = "sha256:" + "2" * 64
LAYOUT = "sha256:" + "3" * 64
FONTSET = "sha256:" + "4" * 64


def scopes():
    scene = dict(
        document_id=DOC, revision_id=REV, snapshot_id=SNAP,
        layout_environment=dict(
            environment_id=LAYOUT, font_set_fingerprint=FONTSET,
        ),
    )
    font = dict(
        resource_id=ABEL_RESOURCE_ID,
        font_fingerprint="sha256:" + ABEL_SHA256,
        content_hash=ABEL_SHA256,
        face_index=0, family="Abel", style="Regular",
        delivery="deliver_exact",
        fetch_handle="exact-server-handle",
    )
    environment = dict(
        protocol_version="chaptera.font-environment.v1",
        document_id=DOC, revision_id=REV,
        scene_snapshot_id=SNAP, layout_environment_id=LAYOUT,
        font_set_fingerprint=FONTSET, fonts=[font], diagnostics=[],
    )
    return scene, environment


class PinnedOflTests(unittest.TestCase):
    def test_real_complete_font_identity_and_sfnt(self):
        resource = load_pinned_abel()
        self.assertEqual(len(resource.raw), 35220)
        self.assertEqual(hashlib.sha256(resource.raw).hexdigest(), ABEL_SHA256)
        self.assertEqual(resource.face_count, 1)
        self.assertEqual(attest_sfnt(resource.raw), 1)
        self.assertTrue(ABEL_PATH.is_file())

    def test_one_real_admitted_grant_requires_delivery_and_independent_authority(self):
        resource = load_pinned_abel()
        record = resource.trusted(
            tenant_id=TENANT, document_id=DOC,
            layout_environment_id=LAYOUT, font_set_fingerprint=FONTSET,
        )
        scene, env = scopes()

        def issue(records, environment=env):
            return issue_font_authoring_admission_v1(
                tenant_id=TENANT, scene=scene,
                font_environment=environment, trusted_resources=records,
            )

        self.assertEqual(issue([record])["resources"], [{
            "resource_id": ABEL_RESOURCE_ID,
            "font_fingerprint": "sha256:" + ABEL_SHA256,
            "content_hash": ABEL_SHA256,
            "face_index": 0,
        }])
        self.assertEqual(issue([])["resources"], [])
        self.assertEqual(issue([replace(record, authoring_admitted=False)])["resources"], [])
        self.assertEqual(issue([replace(record, parser_verified=False)])["resources"], [])
        self.assertEqual(issue([replace(record, is_full_resource=False)])["resources"], [])
        self.assertEqual(issue([replace(record, tenant_id="other")])["resources"], [])
        self.assertEqual(issue([replace(record, document_id="other")])["resources"], [])
        for mode in ("blocked", "deliver_subset", "server_render_only", "substitute_explicit"):
            changed = dict(env, fonts=[dict(env["fonts"][0], delivery=mode)])
            self.assertEqual(issue([record], changed)["resources"], [])
        with self.assertRaises(FontAdmissionDenied):
            issue([replace(record, full_font_bytes=record.full_font_bytes + b"!")])
        with self.assertRaises(FontAdmissionDenied):
            issue([replace(record, face_index=1, face_count=2)])

    def test_invalid_sfnt_ranges_metrics_and_truncated_payload_fail(self):
        data = load_pinned_abel().raw
        def denied(blob):
            with self.assertRaises(PinnedFontDenied):
                attest_sfnt(bytes(blob))
        denied(data[:14])
        damaged = bytearray(data)
        damaged[0:4] = b"ttcf"
        denied(damaged)
        damaged = bytearray(data)
        damaged[4:6] = struct.pack(">H", 257)
        denied(damaged)
        damaged = bytearray(data)
        damaged[20:24] = struct.pack(">I", len(data) + 1)
        denied(damaged)
        damaged = bytearray(data)
        damaged[12:16] = damaged[28:32]  # two duplicate table tags
        denied(damaged)
        damaged = bytearray(data)
        damaged[20:24] = struct.pack(">I", 12)  # directory self-overlap
        denied(damaged)

    def test_wrong_pinned_bytes_are_rejected_before_parser(self):
        resource = load_pinned_abel()
        from pathlib import Path
        from tempfile import TemporaryDirectory
        with TemporaryDirectory() as tmp:
            path = Path(tmp) / "Abel-Regular.ttf"
            path.write_bytes(resource.raw[:-1])
            with self.assertRaisesRegex(PinnedFontDenied, "pinned_font_sha256_mismatch"):
                load_pinned_abel(path)


if __name__ == "__main__":
    unittest.main()
