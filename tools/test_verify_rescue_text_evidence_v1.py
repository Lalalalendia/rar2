#!/usr/bin/env python3
"""Synthetic integrity contracts for private Chaptera Story evidence."""
from __future__ import annotations

import hashlib
import json
from pathlib import Path
import tempfile
import unittest

from tools import verify_rescue_text_evidence_v1 as evidence


class SourceExactStoryEvidenceTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        root = Path(self.tmp.name)
        self.source = root / "test.pub"
        self.source.write_bytes(b"synthetic-source-not-a-real-pub")
        self.output = root / "text"
        self.output.mkdir()
        self.rows = [{
            "story_key": "quill-syid:00000001",
            "utf16_start": 0,
            "utf16_end": 4,
            "text": "A😀Z",
            "utf8_byte_len": len("A😀Z".encode("utf-8")),
            "text_sha256": evidence.sha256("A😀Z".encode("utf-8")),
            "evidence_class": "salvage_confirmed_quill_text_range",
            "carrier": None,
            "carrier_byte_offset": None,
            "carrier_byte_len": None,
        }]
        self.write()

    def write(self):
        txt = b"".join(
            f"=== Story {x['story_key']} [utf16 {x['utf16_start']}..{x['utf16_end']}] ===\n".encode("ascii")
            + x["text"].encode("utf-8") + b"\n\n"
            for x in self.rows
        )
        manifest = {
            "schema_version": evidence.SCHEMA,
            "source_sha256": evidence.sha256(self.source.read_bytes()),
            "extraction_class": "eligible_corrupt_source_confirmed_quill",
            "source_immutable": True,
            "native_pub_repair_attempted": False,
            "native_pub_delivery_allowed": False,
            "page_geometry_reconstructed": False,
            "story_count": len(self.rows),
            "stories_txt_sha256": evidence.sha256(txt),
            "stories": self.rows,
        }
        (self.output / "stories.txt").write_bytes(txt)
        (self.output / "stories.json").write_text(
            json.dumps(manifest, ensure_ascii=False), encoding="utf-8")

    def test_valid_unicode_story_is_byte_consistent(self):
        verified = evidence.verify(self.source, self.output)
        self.assertEqual(verified["story_count"], 1)
        self.assertTrue(verified["byte_verified"])
        self.assertFalse(verified["native_pub_repair_claim"])
        self.assertFalse(verified["semantic_source_independently_decoded"])

    def test_text_tamper_fails_closed(self):
        (self.output / "stories.txt").write_text("not the story")
        with self.assertRaisesRegex(evidence.StoryEvidenceError, "authority_mismatch"):
            evidence.verify(self.source, self.output)

    def test_source_changed_fails_closed(self):
        self.source.write_bytes(b"new source")
        with self.assertRaisesRegex(evidence.StoryEvidenceError, "authority_mismatch"):
            evidence.verify(self.source, self.output)

    def test_illegal_utf16_range_fails_closed_even_if_manifest_rehashed(self):
        self.rows[0]["utf16_end"] = 3
        self.write()
        with self.assertRaisesRegex(evidence.StoryEvidenceError, "content_mismatch"):
            evidence.verify(self.source, self.output)

    def test_unproved_provenance_fails_closed(self):
        self.rows[0]["evidence_class"] = "source_guessed"
        self.write()
        with self.assertRaisesRegex(evidence.StoryEvidenceError, "unknown_evidence_class"):
            evidence.verify(self.source, self.output)

    def test_native_repair_claim_rejected(self):
        manifest_path = self.output / "stories.json"
        obj = json.loads(manifest_path.read_text())
        obj["native_pub_delivery_allowed"] = True
        manifest_path.write_text(json.dumps(obj))
        with self.assertRaisesRegex(evidence.StoryEvidenceError, "authority_mismatch"):
            evidence.verify(self.source, self.output)

    def test_unexpected_private_output_rejected(self):
        (self.output / "private-other.txt").write_bytes(b"no")
        with self.assertRaisesRegex(evidence.StoryEvidenceError, "unexpected_evidence_entry"):
            evidence.verify(self.source, self.output)

    def test_symlink_story_txt_rejected(self):
        txt = self.output / "stories.txt"
        txt.unlink()
        try:
            txt.symlink_to(self.source)
        except (OSError, NotImplementedError):
            self.skipTest("symlink creation unavailable")
        with self.assertRaisesRegex(evidence.StoryEvidenceError, "symlink_path"):
            evidence.verify(self.source, self.output)

    def test_fake_mature_story_missing_source_carrier_is_rejected(self):
        self.rows[0]["evidence_class"] = "mature_authoritative_exact"
        self.write()
        with self.assertRaisesRegex(evidence.StoryEvidenceError, "unverified_mature_story_provenance"):
            evidence.verify(self.source, self.output)

    def legacy_manifest(self, source_class="legacy_0x22_noquill_authoritative_story",
                        evidence_class="legacy_0x22_noquill_authoritative_exact",
                        unadmitted=2):
        self.rows[0].update(
            evidence_class=evidence_class, carrier="/Contents",
            carrier_byte_offset=4, carrier_byte_len=12)
        self.write()
        target = self.output / "stories.json"
        manifest = json.loads(target.read_text(encoding="utf-8"))
        manifest["extraction_class"] = source_class
        manifest["unadmitted_story_count"] = unadmitted
        target.write_text(json.dumps(manifest), encoding="utf-8")

    def test_legacy_noquill_private_text_with_gap_is_byte_verified(self):
        self.legacy_manifest()
        verified = evidence.verify(self.source, self.output)
        self.assertEqual(verified["story_count"], 1)
        self.assertFalse(verified["semantic_source_independently_decoded"])

    def test_legacy_quill_private_text_is_byte_verified(self):
        self.legacy_manifest("legacy_0x22_quill_authoritative_story",
                             "legacy_0x22_quill_authoritative_exact", 0)
        self.assertEqual(evidence.verify(self.source, self.output)["story_count"], 1)

    def test_cross_family_or_missing_legacy_provenance_fails_closed(self):
        self.legacy_manifest(evidence_class="legacy_0x22_quill_authoritative_exact")
        with self.assertRaisesRegex(evidence.StoryEvidenceError, "legacy_story_provenance"):
            evidence.verify(self.source, self.output)
        self.legacy_manifest(unadmitted=-1)
        with self.assertRaisesRegex(evidence.StoryEvidenceError, "invalid_legacy_omitted_count"):
            evidence.verify(self.source, self.output)
        self.legacy_manifest()
        self.rows[0]["carrier_byte_offset"] = None
        self.write()
        manifest = json.loads((self.output / "stories.json").read_text(encoding="utf-8"))
        manifest["extraction_class"] = "legacy_0x22_noquill_authoritative_story"
        manifest["unadmitted_story_count"] = 0
        (self.output / "stories.json").write_text(json.dumps(manifest), encoding="utf-8")
        with self.assertRaisesRegex(evidence.StoryEvidenceError, "legacy_story_provenance"):
            evidence.verify(self.source, self.output)


if __name__ == "__main__":
    unittest.main()
