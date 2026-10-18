#!/usr/bin/env python3
"""Fail-closed source-free per-Story gap reason receipts."""
from __future__ import annotations

import unittest

from tools.rescue_legacy_story_census_v1 import GAP_REASONS, SCHEMA, check_probe


class LegacyCensusReceiptTests(unittest.TestCase):
    def setUp(self):
        self.sha = "a" * 64
        self.reasons = {key: 0 for key in sorted(GAP_REASONS)}
        self.value = {
            "schema_version": SCHEMA,
            "source_sha256": self.sha,
            "source_byte_len": 1024,
            "family": "legacy_0x22_noquill",
            "status": "verified",
            "confirmed_story_ranges": 2,
            "ambiguous_story_ranges": 0,
            "graph_diagnostic_count": 0,
            "source_graph_story_count": 2,
            "empty_graph_story_count": 0,
            "unadmitted_reasons": self.reasons,
            "source_immutable": True,
            "native_pub_repair_attempted": False,
            "semantic_text_exposed": False,
        }

    def check(self, **changes):
        return check_probe(dict(self.value, **changes), self.sha, 1024)

    def test_valid_strict_existing_proven_story(self):
        self.assertEqual(self.check()["confirmed_story_ranges"], 2)

    def test_accepted_projection_gap_is_not_story_promotion(self):
        gap = dict(self.reasons, projection_only_exact=1)
        evidence = self.check(
            status="verified_partial", source_graph_story_count=3,
            ambiguous_story_ranges=1, unadmitted_reasons=gap)
        self.assertEqual(evidence["confirmed_story_ranges"], 2)
        self.assertEqual(evidence["unadmitted_reasons"]["projection_only_exact"], 1)

    def test_other_reasons_are_bounded_and_mutually_exclusive(self):
        for reason in GAP_REASONS:
            with self.subTest(reason=reason):
                self.check(
                    status="verified_partial", source_graph_story_count=3,
                    ambiguous_story_ranges=1,
                    unadmitted_reasons={key: int(key == reason) for key in GAP_REASONS})
        with self.assertRaisesRegex(ValueError, "unadmitted_reason_partition"):
            self.check(ambiguous_story_ranges=1, status="verified_partial",
                       source_graph_story_count=3)

    def test_empty_graph_without_text_is_valid(self):
        self.check(status="no_proven_story", confirmed_story_ranges=0,
                   source_graph_story_count=0)
        self.check(status="no_proven_story", confirmed_story_ranges=0,
                   source_graph_story_count=2, empty_graph_story_count=2)

    def test_unclassified_never_claims_a_story(self):
        with self.assertRaises(ValueError):
            self.check(family="unclassified")
        self.check(family="unclassified", status="ineligible",
                   confirmed_story_ranges=0, source_graph_story_count=0)
        with self.assertRaises(ValueError):
            self.check(family="unclassified", status="ineligible",
                       confirmed_story_ranges=0, source_graph_story_count=2,
                       empty_graph_story_count=2)

    def test_ambiguous_only_has_no_positive_text(self):
        self.check(status="ambiguous", confirmed_story_ranges=0,
                   ambiguous_story_ranges=1, source_graph_story_count=1,
                   unadmitted_reasons=dict(self.reasons, missing_exact_semantic=1))
        with self.assertRaises(ValueError):
            self.check(status="ambiguous", confirmed_story_ranges=0,
                       source_graph_story_count=0)

    def test_reject_identity_and_immutable_source_claim(self):
        with self.assertRaises(ValueError):
            self.check(source_sha256="b" * 64)
        with self.assertRaises(ValueError):
            self.check(source_byte_len=1025)
        for key, value in (("source_immutable", False),
                           ("native_pub_repair_attempted", True),
                           ("semantic_text_exposed", True)):
            with self.subTest(key=key), self.assertRaises(ValueError):
                self.check(**{key: value})

    def test_reject_private_fields_and_arbitrary_categories(self):
        for key, value in (("text", "PRIVATE STORY"), ("path", "PRIVATE/PUB"),
                           ("source_byte_offset", 42)):
            with self.subTest(key=key):
                with self.assertRaisesRegex(ValueError, "probe_unexpected_output_fields"):
                    self.check(**{key: value})
        with self.assertRaises(ValueError):
            self.check(family="invented_family")
        with self.assertRaises(ValueError):
            self.check(status="repaired_native_pub")

    def test_reject_invalid_reason_protocol(self):
        for reasons in (
            None,
            {},
            dict(self.reasons, arbitrary_reason=1),
            {k: v for k, v in self.reasons.items() if k != "projection_only_exact"},
            dict(self.reasons, projection_only_exact=True),
            dict(self.reasons, projection_only_exact=-1),
            dict(self.reasons, projection_only_exact=20000),
            dict(self.reasons, projection_only_exact="1"),
        ):
            with self.subTest(reasons=reasons):
                with self.assertRaisesRegex(ValueError, "unadmitted_reason_protocol"):
                    self.check(unadmitted_reasons=reasons)

    def test_graph_and_reason_counts_must_reconcile(self):
        for key, value in (("source_graph_story_count", 0),
                           ("empty_graph_story_count", 1),
                           ("source_graph_story_count", True),
                           ("empty_graph_story_count", True)):
            with self.subTest(key=key), self.assertRaises(ValueError):
                self.check(**{key: value})
        self.check(empty_graph_story_count=1, source_graph_story_count=3)

    def test_boolean_is_not_valid_count(self):
        for key in ("confirmed_story_ranges", "ambiguous_story_ranges",
                    "graph_diagnostic_count"):
            with self.subTest(key=key), self.assertRaises(ValueError):
                self.check(**{key: True})

    def test_nonpositive_status_is_not_misreported(self):
        with self.assertRaises(ValueError):
            self.check(status="verified_partial")
        with self.assertRaises(ValueError):
            self.check(status="no_proven_story")
        with self.assertRaises(ValueError):
            self.check(status="verified", ambiguous_story_ranges=1,
                       source_graph_story_count=3,
                       unadmitted_reasons=dict(self.reasons, projection_only_exact=1))


if __name__ == "__main__":
    unittest.main()
