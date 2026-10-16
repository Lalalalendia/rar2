#!/usr/bin/env python3
"""Offline unit contracts for the proposed bounded Rescue Salvage Bundle V1."""
from __future__ import annotations

import hashlib
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from tools import rescue_salvage_bundle_v1 as salvage


def sha(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


class SalvageBundleTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.source_dir = self.root / "inputs"
        self.source_dir.mkdir()
        self.source = self.source_dir / "input.pub"
        self.original = b"synthetic-placeholder-not-a-real-PUB"
        self.source.write_bytes(self.original)
        self.out = self.root / "bundles"
        self.fake_cli = self.root / "pub-cli"
        self.fake_cli.write_text("not an executable; used only by test stub")

    @staticmethod
    def fake_runner(args: list[str], timeout: int) -> str | None:
        if "tools.rescue_cfb_stream_export" in args:
            source = Path(args[3])
            streams = Path(args[args.index("--output-dir") + 1])
            streams.mkdir()
            payload = source.read_bytes()
            name = "sid-000001-contents.bin"
            (streams / name).write_bytes(payload)
            manifest = {
                "schema_version": salvage.PHYSICAL_SCHEMA,
                "artifact_count": 1,
                "streams": [{"sid": 1, "artifact_path": name,
                             "payload_sha256": sha(payload),
                             "artifact_sha256": sha(payload)}],
            }
            Path(args[args.index("--manifest") + 1]).write_text(json.dumps(manifest))
            return None
        if "tools.verify_rescue_cfb_stream_export" in args:
            source = Path(args[3])
            manifest = json.loads(Path(args[args.index("--manifest") + 1]).read_text())
            artifact = Path(args[args.index("--artifact-dir") + 1]) / manifest["streams"][0]["artifact_path"]
            if artifact.read_bytes() != source.read_bytes():
                return "failed"
            Path(args[args.index("--receipt") + 1]).write_text(json.dumps({
                "pass": True, "source_sha256": sha(source.read_bytes()), "verified_stream_count": 1
            }))
            return None
        if "extract-assets" in args:
            image_dir = Path(args[args.index("--output") + 1])
            image_dir.mkdir()
            binary = b"\xff\xd8\xff\xd9"
            name = "asset-123.jpg"
            (image_dir / name).write_bytes(binary)
            (image_dir / "manifest.json").write_text(json.dumps({
                "schema_version": salvage.ASSET_SCHEMA,
                "assets": [{"filename": name, "sha256": sha(binary), "byte_len": len(binary)}],
                "diagnostics": [],
            }))
            return None
        raise AssertionError("unexpected command " + repr(args))

    def test_bundle_creates_and_reuses_exact_verified_raw_streams(self):
        with patch.object(salvage, "run_command", side_effect=self.fake_runner):
            status, report = salvage.execute(self.source, self.out)
            self.assertEqual(status, "created")
            self.assertEqual(report["physical"]["stream_count"], 1)
            self.assertEqual(report["images"]["status"], "not_requested")
            self.assertEqual(report["text"], {"status": "not_implemented"})
            self.assertFalse(report["native_pub_delivery_allowed"])
            self.assertEqual(self.source.read_bytes(), self.original)
            status, check = salvage.execute(self.source, self.out)
            self.assertEqual(status, "reused")
            self.assertEqual(report, check)
            self.assertEqual(list(self.out.glob(".stage-*")), [])

    def test_independent_replay_rejects_modified_stream(self):
        with patch.object(salvage, "run_command", side_effect=self.fake_runner):
            salvage.execute(self.source, self.out)
            p = self.out / sha(self.original) / "physical" / "streams" / "sid-000001-contents.bin"
            p.write_bytes(b"not the source")
            with self.assertRaisesRegex(salvage.UnsafeInput, "physical_verifier_failed"):
                salvage.execute(self.source, self.out)
            self.assertEqual(p.read_bytes(), b"not the source")

    def test_images_verified_with_real_manifest_sha_and_count(self):
        with patch.object(salvage, "run_command", side_effect=self.fake_runner):
            status, report = salvage.execute(self.source, self.out, pub_cli=self.fake_cli)
            self.assertEqual(status, "created")
            self.assertEqual(report["images"]["asset_count"], 1)
            self.assertEqual(report["images"]["status"], "verified")
            status, _ = salvage.execute(self.source, self.out, pub_cli=self.fake_cli)
            self.assertEqual(status, "reused")

    def test_image_tamper_never_gets_reused(self):
        with patch.object(salvage, "run_command", side_effect=self.fake_runner):
            salvage.execute(self.source, self.out, pub_cli=self.fake_cli)
            artifact = self.out / sha(self.original) / "images" / "asset-123.jpg"
            artifact.write_bytes(b"wrong")
            with self.assertRaisesRegex(salvage.UnsafeInput, "asset_.*"):
                salvage.execute(self.source, self.out, pub_cli=self.fake_cli)

    def test_extra_unmanifested_asset_is_rejected(self):
        with patch.object(salvage, "run_command", side_effect=self.fake_runner):
            salvage.execute(self.source, self.out, pub_cli=self.fake_cli)
            d = self.out / sha(self.original) / "images"
            (d / "hidden-extra.bin").write_bytes(b"extra")
            with self.assertRaisesRegex(salvage.UnsafeInput, "unexpected_extra_asset"):
                salvage.execute(self.source, self.out)

    def test_no_path_traversal_from_asset_manifest(self):
        folder = self.root / "assets"
        folder.mkdir()
        (folder / "manifest.json").write_text(json.dumps({
            "schema_version": salvage.ASSET_SCHEMA,
            "assets": [{"filename": "../escape.bin", "sha256": sha(b"a"), "byte_len": 1}]
        }))
        with self.assertRaisesRegex(salvage.UnsafeInput, "unsafe_or_duplicate"):
            salvage.check_asset_output(folder)

    def test_retain_original_and_reject_replaced_copy(self):
        with patch.object(salvage, "run_command", side_effect=self.fake_runner):
            _, report = salvage.execute(self.source, self.out, retain_original=True)
            self.assertEqual(report["original"]["status"], "retained")
            saved = self.out / sha(self.original) / "original.pub"
            self.assertEqual(saved.read_bytes(), self.original)
            saved.write_bytes(b"tampered")
            with self.assertRaisesRegex(salvage.UnsafeInput, "bundle_original_mismatch"):
                salvage.execute(self.source, self.out)

    def test_unparseable_pub_still_reports_gap_not_false_success(self):
        with patch.object(salvage, "run_command", return_value="failed"):
            _, report = salvage.execute(self.source, self.out)
        self.assertEqual(report["outcome"], "unsupported")
        self.assertEqual(report["physical"]["reason"], "physical_failed")
        self.assertEqual(report["images"]["status"], "not_requested")
        self.assertIn("physical_streams_unavailable", report["known_gaps"])
        self.assertEqual(self.source.read_bytes(), self.original)

    def test_bad_image_producer_does_not_erase_good_physical_streams(self):
        def broken(args: list[str], timeout: int) -> str | None:
            if "extract-assets" in args:
                return "failed"
            return self.fake_runner(args, timeout)
        with patch.object(salvage, "run_command", side_effect=broken):
            _, report = salvage.execute(self.source, self.out, pub_cli=self.fake_cli)
        self.assertEqual(report["physical"]["status"], "verified")
        self.assertEqual(report["images"]["status"], "unavailable")
        self.assertEqual(report["images"]["reason"], "image_failed")
        self.assertFalse((self.out / sha(self.original) / "images").exists())

    def test_symlink_input_and_output_rejected(self):
        alias = self.root / "link.pub"
        alias.symlink_to(self.source)
        with self.assertRaisesRegex(salvage.UnsafeInput, "symlink_path"):
            salvage.execute(alias, self.out)
        link_out = self.root / "output-link"
        link_out.symlink_to(self.source_dir, target_is_directory=True)
        with self.assertRaisesRegex(salvage.UnsafeInput, "symlink_path"):
            salvage.execute(self.source, link_out)

    def test_manifest_claims_cannot_override_known_gaps_or_native_repair_gate(self):
        with patch.object(salvage, "run_command", side_effect=self.fake_runner):
            salvage.execute(self.source, self.out)
            p = self.out / sha(self.original) / "bundle.json"
            data = json.loads(p.read_text())
            data["outcome"] = "complete"
            data["native_pub_delivery_allowed"] = True
            p.write_text(json.dumps(data))
            with self.assertRaisesRegex(salvage.UnsafeInput, "unsupported_native_or_immutability_claim"):
                salvage.execute(self.source, self.out)

    def test_unmanifested_physical_artifact_fails_closed(self):
        with patch.object(salvage, "run_command", side_effect=self.fake_runner):
            salvage.execute(self.source, self.out)
            physical = self.out / sha(self.original) / "physical" / "streams"
            (physical / "extra.bin").write_bytes(b"hidden")
            with self.assertRaisesRegex(salvage.UnsafeInput, "unmanifested_physical_artifact"):
                salvage.execute(self.source, self.out)

    def test_reuse_rejects_new_requested_capability_instead_of_ignoring_it(self):
        with patch.object(salvage, "run_command", side_effect=self.fake_runner):
            salvage.execute(self.source, self.out)
            with self.assertRaisesRegex(salvage.UnsafeInput, "existing_bundle_missing_requested_original"):
                salvage.execute(self.source, self.out, retain_original=True)
            with self.assertRaisesRegex(salvage.UnsafeInput, "existing_bundle_missing_requested_images"):
                salvage.execute(self.source, self.out, pub_cli=self.fake_cli)

    def test_verified_zero_streams_do_not_claim_salvage(self):
        self.assertEqual(salvage.outcome_for({"status": "verified", "stream_count": 0},
                                             {"status": "not_requested"}), "unsupported")

    def test_source_output_conflicts_fail_closed(self):
        with self.assertRaisesRegex(salvage.UnsafeInput, "output_overlaps_source"):
            salvage.execute(self.source, self.source_dir)
        hash_named = self.source_dir / ("0" * 64 + ".pub")
        hash_named.write_bytes(self.original)
        with self.assertRaisesRegex(salvage.UnsafeInput, "hash_named_source_drift"):
            salvage.execute(hash_named, self.out)


if __name__ == "__main__":
    unittest.main()
