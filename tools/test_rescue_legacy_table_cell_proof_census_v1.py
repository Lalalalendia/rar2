#!/usr/bin/env python3
"""Negative boundaries for source-free legacy table-cell provenance proofs."""
from __future__ import annotations

import unittest
from tools.rescue_legacy_table_cell_proof_census_v1 import (
    SCHEMA, REFUSALS, check_cell_proof,
)


class TableCellProofProtocolTests(unittest.TestCase):
    def setUp(self):
        self.sha = "a" * 64
        self.length = 4096
        self.old = {
            "family": "legacy_0x22_noquill",
            "confirmed_story_ranges": 3,
            "ambiguous_story_ranges": 2,
            "unadmitted_reasons": {"projection_only_exact": 2},
        }
        self.proof = {
            "schema_version": SCHEMA,
            "source_sha256": self.sha,
            "source_byte_len": self.length,
            "family": "legacy_0x22_noquill",
            "previously_admitted_stories": 3,
            "projection_only_candidates": 2,
            "proven_table_stories": 1,
            "proven_cell_count": 2,
            "proven_source_cell_bytes": 7,
            "virtual_separator_count": 1,
            "refused": {name: int(name == "incomplete_or_merged_grid") for name in REFUSALS},
            "source_immutable": True,
            "semantic_aggregate_exported": False,
            "native_pub_repair_attempted": False,
            "private_text_exposed": False,
        }

    def verified(self, **changes):
        value = dict(self.proof, **changes)
        return check_cell_proof(value, self.old, self.sha, self.length)

    def test_positive_and_partial_are_source_safe(self):
        row = self.verified()
        self.assertEqual(row["proven_table_stories"], 1)
        self.assertFalse(row["semantic_aggregate_exported"])

    def test_reject_source_sha_size_family_or_prior_story_drift(self):
        for name, value in (
            ("source_sha256", "b" * 64),
            ("source_byte_len", self.length + 1),
            ("family", "mature_0x2c"),
            ("previously_admitted_stories", 4),
        ):
            with self.subTest(name=name), self.assertRaises(ValueError):
                self.verified(**{name: value})

    def test_reject_untrusted_schema_and_private_text_path_fields(self):
        with self.assertRaisesRegex(ValueError, "unexpected_cell_proof_fields"):
            self.verified(text="a private cell")
        with self.assertRaisesRegex(ValueError, "unexpected_cell_proof_fields"):
            self.verified(file_path="a/source.pub")
        with self.assertRaises(ValueError):
            self.verified(schema_version="chaptera.fake.v1")

    def test_reject_unbounded_or_boolean_counts(self):
        for name, value in (
            ("projection_only_candidates", True),
            ("proven_table_stories", -1),
            ("proven_cell_count", 130_000_000),
            ("proven_source_cell_bytes", "9"),
            ("virtual_separator_count", 12),
        ):
            with self.subTest(name=name), self.assertRaises(ValueError):
                self.verified(**{name: value})

    def test_reject_wrong_refusal_partition_or_missing_reason(self):
        with self.assertRaisesRegex(ValueError, "cell_proof_partition_mismatch"):
            self.verified(refused=dict(self.proof["refused"], incomplete_or_merged_grid=0))
        with self.assertRaisesRegex(ValueError, "unexpected_cell_proof_refusals"):
            self.verified(refused={"missing_or_ambiguous_owner": 1})
        with self.assertRaisesRegex(ValueError, "invalid_cell_proof_count"):
            self.verified(refused=dict(self.proof["refused"], unverified_cell_carrier=True))

    def test_zero_candidates_are_not_promoted_to_text(self):
        old = dict(self.old, family="mature_0x2c", ambiguous_story_ranges=0,
                   unadmitted_reasons={"projection_only_exact": 0})
        proof = dict(self.proof, family="mature_0x2c",
                     projection_only_candidates=0, proven_table_stories=0,
                     proven_cell_count=0, proven_source_cell_bytes=0,
                     virtual_separator_count=0,
                     refused={name: 0 for name in REFUSALS})
        check_cell_proof(proof, old, self.sha, self.length)
        with self.assertRaises(ValueError):
            check_cell_proof(dict(proof, proven_table_stories=1), old, self.sha, self.length)

    def test_zero_candidate_does_not_mean_no_source_story(self):
        old = dict(self.old, confirmed_story_ranges=0, ambiguous_story_ranges=0,
                   unadmitted_reasons={"projection_only_exact": 0},
                   family="unclassified")
        proof = dict(self.proof, family="unclassified",
                     previously_admitted_stories=0, projection_only_candidates=0,
                     proven_table_stories=0, proven_cell_count=0,
                     proven_source_cell_bytes=0, virtual_separator_count=0,
                     refused={name: 0 for name in REFUSALS})
        check_cell_proof(proof, old, self.sha, self.length)

    def test_reject_virtual_cr_or_byte_span_mismatch(self):
        with self.assertRaisesRegex(ValueError, "cell_proof_partition_mismatch"):
            self.verified(virtual_separator_count=2)
        with self.assertRaisesRegex(ValueError, "cell_proof_partition_mismatch"):
            self.verified(proven_source_cell_bytes=4097)

    def test_refuse_implied_native_repair_or_semantic_promotion(self):
        for key in ("semantic_aggregate_exported", "native_pub_repair_attempted",
                    "private_text_exposed"):
            with self.subTest(key=key), self.assertRaises(ValueError):
                self.verified(**{key: True})
        with self.assertRaises(ValueError):
            self.verified(source_immutable=False)


if __name__ == "__main__":
    unittest.main()
