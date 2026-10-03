#!/usr/bin/env python3
from __future__ import annotations

import importlib.util
import json
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("t891", ROOT / "tools" / "t891_evidence_classifier.py")
assert SPEC and SPEC.loader
T891 = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(T891)


def counts(unexplained: int = 0) -> dict[str, int]:
    return {
        "expected_derived": 0,
        "requested_semantic": 0,
        "save_normalization": 0,
        "unavailable": 0,
        "unexplained_collateral": unexplained,
    }


def blast(candidate_id: str, mode: str, changed: bool) -> dict:
    return {
        "schema_version": T891.BLAST_SCHEMA,
        "operation": {
            "kind": f"publisher-tlb-property-{mode}",
            "candidate_id": candidate_id,
        },
        "cfb": {
            "control_mutation_stream_delta": [{"stream": "Contents"}] if changed else [],
            "control_mutation_topology_delta": [],
            "control_mutation_byte_ranges": [{"start": 1, "end": 2}] if changed else [],
        },
        "classification_counts": counts(1 if changed else 0),
        "invariants": {
            "raw_byte_inequality_is_not_semantic_evidence": True,
            "matched_noop_control_used": True,
            "unexplained_collateral_preserved": True,
            "public_receipt_contains_raw_document_bytes": False,
            "native_pub_writer_capability_granted": False,
        },
    }


def candidate(candidate_id: str) -> dict:
    return {
        "candidate": {"id": candidate_id},
        "control": {"status": "ok"},
        "same_value": {"status": "ok"},
        "changed_value": {"status": "ok", "runtime_changed": True},
        "comparison": {
            "same_vs_control": {
                "semantic_equal": True,
                "persistence_equal": True,
                "render_equal": True,
                "changed_streams": [],
            },
            "changed_vs_control": {
                "semantic_equal": False,
                "persistence_equal": False,
                "render_equal": True,
                "changed_streams": ["Contents"],
            },
        },
        "blast_radius": {
            "same_vs_control": {
                "changed_stream_count": 0,
                "topology_delta_count": 0,
                "changed_range_count": 0,
                "classification_counts": counts(),
            },
            "changed_vs_control": {
                "changed_stream_count": 1,
                "topology_delta_count": 0,
                "changed_range_count": 1,
                "classification_counts": counts(1),
            },
        },
        "classification": "persisted-semantic-change",
    }


class T891EvidenceClassifierTests(unittest.TestCase):
    def fixture(self, root: Path) -> tuple[Path, Path]:
        analysis_dir = root / "analysis"
        blast_dir = analysis_dir / "blast-radius"
        blast_dir.mkdir(parents=True)
        doc = {
            "schema": T891.BATCH_SCHEMA,
            "experiment_id": T891.EXPERIMENT_ID,
            "task_id": T891.TASK_ID,
            "batch_id": T891.BATCH_ID,
            "candidates": [candidate(value) for value in T891.EXPECTED_CANDIDATES],
        }
        analysis = analysis_dir / "tlb-shape-effects-batch01.json"
        analysis.write_text(json.dumps(doc), encoding="utf-8")
        for candidate_id in T891.EXPECTED_CANDIDATES:
            slug = T891.slug(candidate_id)
            (blast_dir / f"{slug}-same_value.json").write_text(
                json.dumps(blast(candidate_id, "same_value", False)),
                encoding="utf-8",
            )
            (blast_dir / f"{slug}-changed_value.json").write_text(
                json.dumps(blast(candidate_id, "changed_value", True)),
                encoding="utf-8",
            )
        return analysis, blast_dir

    def test_complete_batch_stays_below_law_promotion(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            analysis, blast_dir = self.fixture(Path(tmp))
            summary = T891.build_summary(T891.load_json(analysis), blast_dir)
            self.assertEqual("evidence-complete-carrier-attribution-required", summary["batch_state"])
            self.assertEqual(9, summary["candidate_count"])
            self.assertEqual(27, summary["arm_count"])
            self.assertEqual(18, summary["blast_receipt_count"])
            self.assertTrue(all(not row["pub_law_promoted"] for row in summary["candidate_summaries"]))
            for row in summary["candidate_summaries"]:
                for side in ("same_vs_control", "changed_vs_control"):
                    receipt_path = row[side]["blast"]["receipt_path"]
                    self.assertTrue(receipt_path.startswith("analysis/blast-radius/"))
                    self.assertNotIn(str(root), receipt_path)

    def test_missing_receipt_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            analysis, blast_dir = self.fixture(Path(tmp))
            (blast_dir / "glowformat-radius-changed_value.json").unlink()
            with self.assertRaisesRegex(T891.EvidenceError, "missing blast receipt"):
                T891.build_summary(T891.load_json(analysis), blast_dir)

    def test_receipt_identity_mismatch_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            analysis, blast_dir = self.fixture(Path(tmp))
            path = blast_dir / "glowformat-radius-same_value.json"
            path.write_text(
                json.dumps(blast("ReflectionFormat.Blur", "same_value", False)),
                encoding="utf-8",
            )
            with self.assertRaisesRegex(T891.EvidenceError, "candidate_id mismatch"):
                T891.build_summary(T891.load_json(analysis), blast_dir)

    def test_same_value_side_effect_routes_to_separation_gate(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            analysis, blast_dir = self.fixture(Path(tmp))
            doc = T891.load_json(analysis)
            first = doc["candidates"][0]
            first["comparison"]["same_vs_control"]["persistence_equal"] = False
            first["comparison"]["same_vs_control"]["changed_streams"] = ["Contents"]
            first["blast_radius"]["same_vs_control"] = {
                "changed_stream_count": 1,
                "topology_delta_count": 0,
                "changed_range_count": 1,
                "classification_counts": counts(1),
            }
            analysis.write_text(json.dumps(doc), encoding="utf-8")
            candidate_id = first["candidate"]["id"]
            (blast_dir / f"{T891.slug(candidate_id)}-same_value.json").write_text(
                json.dumps(blast(candidate_id, "same_value", True)),
                encoding="utf-8",
            )
            summary = T891.build_summary(doc, blast_dir)
            row = summary["candidate_summaries"][0]
            self.assertTrue(row["same_value_side_effect"])
            self.assertEqual(
                "separate-same-value-materialization-from-changed-value-carrier",
                row["next_gate"],
            )


if __name__ == "__main__":
    unittest.main()
