#!/usr/bin/env python3
from __future__ import annotations

import copy
import json
import unittest
from pathlib import Path

if __package__:
    from tools.validate_experiment_receipt import (
        ExperimentReceiptError,
        canonical_json,
        canonical_sha256,
        validate_receipt,
    )
else:
    from validate_experiment_receipt import (
        ExperimentReceiptError,
        canonical_json,
        canonical_sha256,
        validate_receipt,
    )


FIXTURES = Path("packages/protocol/experiment-receipt/v1/fixtures")
SPECIALIZED_NAME = "state-transition.synthetic.json"


def load(name: str):
    return json.loads((FIXTURES / name).read_text(encoding="utf-8"))


class ExperimentReceiptTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.specialized = load(SPECIALIZED_NAME)
        cls.evidence = {SPECIALIZED_NAME: cls.specialized}

    def test_complete_envelope_binds_specialized_receipt(self):
        result = validate_receipt(load("complete.synthetic.json"), self.evidence)
        self.assertEqual(result["comparison_status"], "comparable")
        self.assertEqual(result["referenced_evidence_count"], 1)
        self.assertEqual(result["authority_ceiling"], "synthetic_contract")
        self.assertTrue(result["source_free"])

    def test_canonical_json_and_hash_ignore_object_insertion_order(self):
        receipt = load("complete.synthetic.json")
        reordered = {key: receipt[key] for key in reversed(receipt)}
        self.assertEqual(canonical_json(receipt), canonical_json(reordered))
        self.assertEqual(canonical_sha256(receipt), canonical_sha256(reordered))

    def test_partial_envelope_is_explicitly_not_comparable(self):
        result = validate_receipt(load("partial-not-comparable.synthetic.json"))
        self.assertEqual(result["comparison_status"], "not_comparable")
        self.assertEqual(result["referenced_evidence_count"], 0)

    def test_stale_or_mismatched_reference_digest_is_rejected(self):
        with self.assertRaisesRegex(ExperimentReceiptError, "digest mismatch"):
            validate_receipt(load("stale-digest.invalid.json"), self.evidence)

    def test_incompatible_specialized_receipt_version_is_rejected(self):
        with self.assertRaisesRegex(ExperimentReceiptError, "type/version mismatch"):
            validate_receipt(load("incompatible-version.invalid.json"), self.evidence)

    def test_missing_required_environment_identity_is_rejected(self):
        with self.assertRaisesRegex(ExperimentReceiptError, "fingerprint_sha256"):
            validate_receipt(load("missing-environment.invalid.json"))

    def test_checked_in_absolute_path_privacy_leak_is_rejected(self):
        with self.assertRaisesRegex(ExperimentReceiptError, "absolute local path"):
            validate_receipt(load("privacy-leak.invalid.json"))

    def test_missing_referenced_artifact_is_rejected(self):
        with self.assertRaisesRegex(ExperimentReceiptError, "was not supplied"):
            validate_receipt(load("complete.synthetic.json"))

    def test_specialized_receipt_declared_version_is_checked(self):
        artifact = copy.deepcopy(self.specialized)
        artifact["receipt_version"] = "chaptera.state-transition-receipt.v2"
        with self.assertRaisesRegex(ExperimentReceiptError, "incompatible receipt_version"):
            validate_receipt(load("complete.synthetic.json"), {SPECIALIZED_NAME: artifact})

    def test_unknown_specialized_receipt_type_is_rejected(self):
        receipt = load("complete.synthetic.json")
        receipt["evidence_refs"][0]["type"] = "UnregisteredReceiptV1"
        with self.assertRaisesRegex(ExperimentReceiptError, "unsupported specialized receipt type"):
            validate_receipt(receipt, self.evidence)

    def test_relative_artifact_name_cannot_escape_receipt_bundle(self):
        receipt = load("complete.synthetic.json")
        receipt["evidence_refs"][0]["relative_artifact_name"] = "../private.json"
        with self.assertRaisesRegex(ExperimentReceiptError, "normalized relative artifact name"):
            validate_receipt(receipt, {"../private.json": self.specialized})

    def test_completed_lifecycle_requires_all_semantic_phases(self):
        receipt = load("complete.synthetic.json")
        receipt["semantic_observations"]["reopen"] = {
            "status": "unavailable",
            "reason": "synthetic missing reopen",
        }
        with self.assertRaisesRegex(ExperimentReceiptError, "completed experiment receipt"):
            validate_receipt(receipt, self.evidence)

    def test_privacy_guard_rejects_user_and_payload_fields(self):
        for field, value in [
            ("username", "alice"),
            ("source_bytes", "00ff"),
            ("binary_payload", "AAAA"),
            ("api_key", "redacted-but-still-forbidden"),
        ]:
            receipt = load("partial-not-comparable.synthetic.json")
            receipt["operation"]["parameters"][field] = value
            with self.subTest(field=field):
                with self.assertRaisesRegex(ExperimentReceiptError, "forbidden field"):
                    validate_receipt(receipt)

    def test_privacy_guard_rejects_windows_home_and_embedded_data(self):
        slash = chr(92)
        values = [
            "C:" + slash + "Users" + slash + "alice" + slash + "private.pub",
            slash * 2 + "server" + slash + "share" + slash + "private.pub",
            "~" + "/private.pub",
            "data:application/octet-stream;base64,AAAA",
        ]
        for value in values:
            receipt = load("partial-not-comparable.synthetic.json")
            receipt["operation"]["parameters"]["opaque_value"] = value
            with self.subTest(value=value):
                with self.assertRaises(ExperimentReceiptError):
                    validate_receipt(receipt)

    def test_referenced_specialized_receipt_schema_is_still_authoritative(self):
        artifact = copy.deepcopy(self.specialized)
        artifact["unexpected_envelope_field"] = True
        receipt = load("complete.synthetic.json")
        receipt["evidence_refs"][0]["sha256"] = canonical_sha256(artifact)
        with self.assertRaisesRegex(ExperimentReceiptError, "StateTransitionReceiptV1 contains unknown fields"):
            validate_receipt(receipt, {SPECIALIZED_NAME: artifact})


if __name__ == "__main__":
    unittest.main()
