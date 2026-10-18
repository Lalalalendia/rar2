#!/usr/bin/env python3
"""Pure source-free negative boundaries for exact Publisher legacy census receipts."""
from __future__ import annotations

import unittest
from tools.rescue_legacy_story_census_v1 import check_probe, SCHEMA


class LegacyCensusReceiptTests(unittest.TestCase):
    def setUp(self):
        self.sha = "a" * 64
        self.value = {
            "schema_version": SCHEMA,
            "source_sha256": self.sha,
            "source_byte_len": 1024,
            "family": "legacy_0x22_noquill",
            "status": "verified",
            "confirmed_story_ranges": 2,
            "ambiguous_story_ranges": 0,
            "graph_diagnostic_count": 0,
            "source_immutable": True,
            "native_pub_repair_attempted": False,
            "semantic_text_exposed": False,
        }

    def ok(self, **updates):
        return check_probe(dict(self.value, **updates), self.sha, 1024)

    def test_source_proven_legacy_positive(self):
        self.assertEqual(self.ok()["confirmed_story_ranges"], 2)

    def test_reject_source_sha_drift(self):
        with self.assertRaises(ValueError):
            self.ok(source_sha256="b" * 64)

    def test_reject_source_size_drift(self):
        with self.assertRaises(ValueError):
            self.ok(source_byte_len=1025)

    def test_reject_extra_content_payload(self):
        value = dict(self.value, text="PRIVATE STORY")
        with self.assertRaisesRegex(ValueError, "unexpected_output_fields"):
            check_probe(value, self.sha, 1024)

    def test_ineligible_not_promoted(self):
        with self.assertRaises(ValueError):
            self.ok(family="unclassified")
        self.assertEqual(self.ok(family="unclassified", status="ineligible",
                                 confirmed_story_ranges=0)["status"], "ineligible")

    def test_negative_no_story_is_not_text(self):
        self.assertEqual(self.ok(status="no_proven_story", confirmed_story_ranges=0)[
            "confirmed_story_ranges"], 0)
        with self.assertRaises(ValueError):
            self.ok(status="no_proven_story")

    def test_partial_needs_ambiguous_range(self):
        self.ok(status="verified_partial", ambiguous_story_ranges=1)
        with self.assertRaises(ValueError):
            self.ok(status="verified_partial")
        with self.assertRaises(ValueError):
            self.ok(status="verified", ambiguous_story_ranges=1)

    def test_unproven_nonempty_is_ambiguous(self):
        self.ok(status="ambiguous", confirmed_story_ranges=0, ambiguous_story_ranges=1)
        with self.assertRaises(ValueError):
            self.ok(status="ambiguous", confirmed_story_ranges=0)

    def test_reject_unknown_family_and_status(self):
        for key, value in (("family", "Publisher_fixed"), ("status", "native_repaired")):
            with self.subTest(key=key), self.assertRaises(ValueError):
                self.ok(**{key: value})

    def test_bool_is_not_valid_count(self):
        for key in ("confirmed_story_ranges", "ambiguous_story_ranges",
                    "graph_diagnostic_count"):
            with self.subTest(key=key), self.assertRaises(ValueError):
                self.ok(**{key: True})

    def test_no_native_repair_or_source_mutation_claim(self):
        for key, value in (("native_pub_repair_attempted", True),
                           ("source_immutable", False),
                           ("semantic_text_exposed", True)):
            with self.subTest(key=key), self.assertRaises(ValueError):
                self.ok(**{key: value})


if __name__ == "__main__":
    unittest.main()
