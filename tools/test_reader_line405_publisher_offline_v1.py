#!/usr/bin/env python3
"""Source-free controls for consuming two exact LINE405 Chromium artifact ZIPs."""
from __future__ import annotations

import hashlib
import json
import stat
import tempfile
import unittest
import zipfile
from pathlib import Path
from unittest.mock import patch

import reader_line405_publisher_offline_v1 as offline


def make_capture_zip(
    path: Path, *, extra: str | None = None,
    missing: str | None = None, traversal: bool = False, symlink: bool = False,
) -> str:
    with zipfile.ZipFile(path, "w", compression=zipfile.ZIP_DEFLATED) as archive:
        for name in sorted(offline.FILES):
            if name == missing:
                continue
            actual_name = "../SampleNewsletter-page-2.png" if traversal and name.endswith("-2.png") else name
            payload = b'{"protocol":"synthetic-only"}' if name == "receipt.json" else b"\x89PNG\r\n\x1a\nsynthetic-png"
            if symlink and name.endswith("-2.png"):
                member = zipfile.ZipInfo(name)
                member.external_attr = (stat.S_IFLNK | 0o777) << 16
                archive.writestr(member, b"../secret")
            else:
                archive.writestr(actual_name, payload)
        if extra:
            archive.writestr(extra, b"unexpected")
    return offline.sha256_file(path)


class Line405OfflinePublisherTests(unittest.TestCase):
    def test_hash_pinned_original_five_member_capture(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            archive = root / "before.zip"
            digest = make_capture_zip(archive)
            receipt = offline.extract_capture_artifact(archive, root / "isolated", digest)
            self.assertTrue(receipt.is_file())
            self.assertEqual(
                sorted(path.name for path in receipt.parent.iterdir()),
                sorted(offline.FILES),
            )
            self.assertTrue((receipt.parent / "SampleNewsletter-page-4.png").read_bytes().startswith(b"\x89PNG"))

    def test_missing_or_wrong_artifact_digest_fails_before_extraction(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            archive = root / "before.zip"
            make_capture_zip(archive)
            with self.assertRaisesRegex(ValueError, "SHA256 mismatch"):
                offline.extract_capture_artifact(archive, root / "isolated", "0" * 64)
            self.assertFalse((root / "isolated").exists())

    def test_extra_missing_and_traversal_members_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            archive = root / "before.zip"
            for kwargs in ({"extra": "PAIRS.csv"}, {"missing": "receipt.json"}, {"traversal": True}):
                digest = make_capture_zip(archive, **kwargs)
                with self.assertRaisesRegex(ValueError, "precisely|unsafe|unexpected"):
                    offline.extract_capture_artifact(archive, root / "isolated", digest)

    def test_symlink_disallowed_even_with_correct_sha(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            archive = root / "before.zip"
            digest = make_capture_zip(archive, symlink=True)
            with self.assertRaisesRegex(ValueError, "unsafe"):
                offline.extract_capture_artifact(archive, root / "isolated", digest)

    def test_numeric_verdict_uses_exact_source_pins_and_one_added_scene_node(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            before, after, native, output = (root / x for x in (
                "before.zip", "after.zip", "private-publisher.zip", "derived.json"))
            base_sha, after_sha = make_capture_zip(before), make_capture_zip(after)
            native.write_bytes(b"private-synthetic-publisher-content-never-emitted")
            verdict = {
                "claims": {"native_pixel_exact_parity_proven": False},
                "pages": [{"customer_page": i} for i in range(1, 5)],
                "page4_disposition": "closer_to_native_publisher",
            }
            with (
                patch.object(offline, "BASE_ARTIFACT_SHA", base_sha),
                patch.object(offline, "CANDIDATE_ARTIFACT_SHA", after_sha),
                patch.object(offline, "require_exact_publisher_bundle") as oracle,
                patch.object(offline, "evaluate", return_value=verdict) as evaluate,
            ):
                actual = offline.run_verdict(before, after, native, output)
            oracle.assert_called_once_with(native)
            self.assertEqual(evaluate.call_args.kwargs, {
                "baseline_commit": offline.BASE_COMMIT,
                "candidate_commit": offline.CANDIDATE_COMMIT,
                "expected_added_nodes": 1,
            })
            self.assertEqual(evaluate.call_args.args[2], native)
            self.assertEqual(actual["source_capture_artifacts"]["artifact_zip_hashes_verified"], True)
            report = json.loads(output.read_text(encoding="utf-8"))
            self.assertEqual(report["offline_runner_schema"], offline.SCHEMA)
            self.assertFalse(report["claims"]["native_pixel_exact_parity_proven"])
            self.assertNotIn("private-synthetic-publisher", output.read_text(encoding="utf-8"))

    def test_fail_closed_publisher_mismatch_leaves_previous_report_intact(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            before, after, native, output = (root / x for x in (
                "before.zip", "after.zip", "oracle.zip", "derived.json"))
            make_capture_zip(before)
            make_capture_zip(after)
            native.write_bytes(b"bad")
            output.write_text("pre-existing-evidence", encoding="utf-8")
            with patch.object(
                offline, "require_exact_publisher_bundle",
                side_effect=ValueError("wrong Publisher ZIP SHA"),
            ):
                with self.assertRaisesRegex(ValueError, "wrong Publisher"):
                    offline.run_verdict(before, after, native, output)
            self.assertEqual(output.read_text(encoding="utf-8"), "pre-existing-evidence")

    def test_output_cannot_overwrite_any_input(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            a = root / "artifact.zip"
            b = root / "candidate.zip"
            c = root / "publisher.zip"
            for path in (a, b, c):
                path.write_bytes(b"synthetic")
            with self.assertRaisesRegex(ValueError, "must never overwrite"):
                offline.run_verdict(a, b, c, c)


if __name__ == "__main__":
    unittest.main()
