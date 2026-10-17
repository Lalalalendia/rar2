#!/usr/bin/env python3
"""Private Story admission tests: no second Quill parser or customer assets."""
from __future__ import annotations

import hashlib
import json
from pathlib import Path
import tempfile
import threading
import time
import unittest
from unittest.mock import patch

from tools import rescue_batch_text_v1 as text_batch
from tools import rescue_desktop_batch_v1 as batch
from tools import rescue_salvage_bundle_v1 as salvage


class StoryBatchTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.source = self.root / "example.pub"
        self.source.write_bytes(b"test-private-synthetic-source")
        self.digest = salvage.sha_file(self.source)
        self.output = self.root / "rescue"
        (self.output / "recovered" / self.digest).mkdir(parents=True)
        (self.output / "recovered" / self.digest / "recovery.json").write_bytes(b"verified-base-manifest")
        self.pub = self.root / "pub.exe"
        self.pub.write_bytes(b"fake-cli-placeholder")

    def make_story(self, target: Path):
        target.mkdir(parents=True)
        txt = "=== Story test-story [utf16 0..4] ===\nA😀Z\n\n".encode("utf-8")
        (target / "stories.txt").write_bytes(txt)
        record = {
            "schema_version": "chaptera.rescue-story-evidence.v1",
            "source_sha256": self.digest,
            "extraction_class": "eligible_corrupt_source_confirmed_quill",
            "source_immutable": True, "native_pub_repair_attempted": False,
            "native_pub_delivery_allowed": False, "page_geometry_reconstructed": False,
            "story_count": 1, "stories_txt_sha256": hashlib.sha256(txt).hexdigest(),
            "stories": [{
                "story_key": "test-story", "utf16_start": 0, "utf16_end": 4,
                "text": "A😀Z", "utf8_byte_len": len("A😀Z".encode()),
                "text_sha256": hashlib.sha256("A😀Z".encode()).hexdigest(),
                "evidence_class": "salvage_confirmed_quill_text_range",
                "carrier": None, "carrier_byte_offset": None, "carrier_byte_len": None,
            }],
        }
        (target / "stories.json").write_text(json.dumps(record), encoding="utf-8")

    def admit(self, pub_cli=None):
        return text_batch.admit(
            self.source, self.output, self.digest, pub_cli=pub_cli,
            deadline=time.monotonic() + 45, abort=threading.Event(),
            kill_tree=lambda proc: None)

    def test_verified_private_evidence_and_idempotent_index(self):
        def fake(_cli, _src, target, *_args):
            self.make_story(target)
            return True
        with patch.object(text_batch, "run_cli", side_effect=fake):
            self.assertEqual(self.admit(self.pub), (1, "verified"))
        private_page = self.output / "recovered-text-pages" / self.digest / "index.html"
        first = private_page.read_bytes()
        self.assertEqual(self.admit(), (1, "verified"))
        self.assertEqual(private_page.read_bytes(), first)
        assert b"repaired native PUB" in first
        self.assertNotIn("A😀Z".encode(), first)
        self.assertNotIn(self.source.name.encode(), first)
        self.assertEqual(salvage.sha_file(self.source), self.digest)

    def test_missing_producer_is_not_verified(self):
        self.assertEqual(self.admit(), (0, "not_requested"))

    def test_unsupported_producer_is_not_verified(self):
        with patch.object(text_batch, "run_cli", return_value=False):
            self.assertEqual(self.admit(self.pub), (0, "unavailable"))
        self.assertFalse((self.output / "recovered-text-pages" / self.digest).exists())

    def test_inconsistent_worker_output_fails_closed(self):
        def fake(_cli, _src, target, *_args):
            target.mkdir()
            (target / "stories.txt").write_bytes(b"bad")
            return False
        with patch.object(text_batch, "run_cli", side_effect=fake):
            with self.assertRaisesRegex(text_batch.TextRecoveryError, "unverified_text"):
                self.admit(self.pub)

    def test_tampered_story_blocks_restart(self):
        self.make_story(self.output / "recovered-text" / self.digest)
        self.assertEqual(self.admit(), (1, "verified"))
        (self.output / "recovered-text" / self.digest / "stories.txt").write_bytes(b"bad")
        with self.assertRaisesRegex(text_batch.TextRecoveryError, "text_evidence_integrity"):
            self.admit(self.pub)

    def test_tampered_immutable_private_page_rejected(self):
        self.make_story(self.output / "recovered-text" / self.digest)
        self.assertEqual(self.admit(), (1, "verified"))
        path = self.output / "recovered-text-pages" / self.digest / "index.html"
        path.write_text("<script>bad</script>")
        with self.assertRaisesRegex(text_batch.TextRecoveryError, "text_presentation_integrity"):
            self.admit()

    def test_cancel_and_source_drift(self):
        abort = threading.Event()
        abort.set()
        with self.assertRaises(text_batch.TextInterrupted):
            text_batch.admit(self.source, self.output, self.digest, pub_cli=self.pub,
                             deadline=time.monotonic() + 30, abort=abort,
                             kill_tree=lambda _: None)
        self.source.write_bytes(b"drift")
        with self.assertRaisesRegex(text_batch.TextRecoveryError, "text_source_drift"):
            self.admit()

    def test_orphaned_index_rejected(self):
        (self.output / "recovered-text-pages" / self.digest).mkdir(parents=True)
        with self.assertRaisesRegex(text_batch.TextRecoveryError, "orphan_text_presentation"):
            self.admit()

    def test_refuses_text_symlink(self):
        foreign = self.root / "outside"
        foreign.mkdir()
        (self.output / "recovered-text").mkdir()
        (self.output / "recovered-text" / self.digest).symlink_to(foreign, target_is_directory=True)
        with self.assertRaisesRegex(text_batch.TextRecoveryError, "unsafe_text_path"):
            self.admit()
        self.assertEqual(list(foreign.iterdir()), [])


    def test_old_unsupported_bundle_gains_verified_story_without_rewrite(self):
        """Text backfill must not ask immutable legacy CFB bundle for new images."""
        batch_output = self.root / "batch"
        scanned = batch.scan([self.source], batch_output)
        self.assertEqual(scanned["counts"], {"pending": 1})
        previous = batch.run(batch_output, workers=1)
        self.assertEqual(previous["counts"], {"done_unsupported": 1})
        original_bundle = (batch_output / "bundles" / self.digest / "bundle.json").read_bytes()
        original_gallery = (batch_output / "recovered" / self.digest / "index.html").read_bytes()

        def generate(_cli, _src, target, *_args):
            self.make_story(target)
            return True

        with patch.object(text_batch, "run_cli", side_effect=generate) as invoked:
            promoted = batch.run(batch_output, workers=1, pub_cli=self.pub)
        self.assertEqual(invoked.call_count, 1)
        self.assertEqual(promoted["counts"], {"done_partial": 1})
        row = promoted["entries"][0]
        self.assertEqual(row["text_count"], 1)
        self.assertEqual(row["text_status"], "verified")
        self.assertEqual(row["auto_tier"], "verified_text")
        self.assertEqual(row["outcome"], "partial")
        self.assertEqual(row["auto_index"],
                         f"recovered-text-pages/{self.digest}/index.html")
        self.assertEqual((batch_output / "bundles" / self.digest / "bundle.json").read_bytes(),
                         original_bundle)
        self.assertEqual((batch_output / "recovered" / self.digest / "index.html").read_bytes(),
                         original_gallery)
        self.assertEqual(salvage.sha_file(self.source), self.digest)
        public = (batch_output / "summary.json").read_text(encoding="utf-8")
        csv = (batch_output / "entries.csv").read_text(encoding="utf-8")
        self.assertNotIn(self.source.name, public + csv)
        self.assertNotIn("A😀Z", public + csv)
        self.assertNotIn("test-story", public + csv)
        self.assertEqual(batch.run(batch_output, workers=1)["counts"], {"done_partial": 1})
        self.assertEqual(batch.status(batch_output)["entries"][0]["attempts"], 2)

        # Rechecking a previously promoted text result is fail-closed.
        (batch_output / "recovered-text" / self.digest / "stories.txt").write_bytes(b"tampered")
        failed = batch.run(batch_output, workers=1)
        self.assertEqual(failed["counts"], {"failed": 1})
        self.assertEqual(failed["entries"][0]["reason"], "text_integrity")
        self.assertEqual((batch_output / "bundles" / self.digest / "bundle.json").read_bytes(),
                         original_bundle)

    def test_first_batch_run_promotes_source_backed_story(self):
        """A text-only partial must not be mislabeled done_unsupported."""
        batch_output = self.root / "first-batch"
        batch.scan([self.source], batch_output)

        def generate(_cli, _src, target, *_args):
            self.make_story(target)
            return True

        with patch.object(text_batch, "run_cli", side_effect=generate):
            report = batch.run(batch_output, workers=1, pub_cli=self.pub)
        self.assertEqual(report["counts"], {"done_partial": 1})
        row = report["entries"][0]
        self.assertEqual(row["text_count"], 1)
        self.assertEqual(row["auto_tier"], "verified_text")
        self.assertEqual(row["stream_count"], 0)
        self.assertEqual(row["asset_count"], 0)
        self.assertEqual(salvage.sha_file(self.source), self.digest)

    def test_cancelled_text_does_not_publish_positive_result(self):
        """Two concurrent source jobs must obey the shared batch abort event."""
        batch_output = self.root / "cancel-batch"
        other = self.root / "other.pub"
        other.write_bytes(b"other-private-synthetic-source")
        batch.scan([self.source, other], batch_output)
        started = threading.Event()
        stop = threading.Event()
        outcome = {}

        def blocked(*_args, **_kwargs):
            started.set()
            stop.wait(4)
            raise text_batch.TextInterrupted("cancelled")

        with patch.object(text_batch, "run_cli", side_effect=blocked):
            worker = threading.Thread(target=lambda: outcome.setdefault(
                "report", batch.run(batch_output, workers=2, pub_cli=self.pub)))
            worker.start()
            self.assertTrue(started.wait(3))
            batch.request_action(batch_output, "cancel")
            stop.set()
            worker.join(10)
        self.assertFalse(worker.is_alive())
        self.assertIn("cancelled", outcome["report"]["counts"])
        self.assertTrue(all(row["text_status"] is None
                            for row in outcome["report"]["entries"]
                            if row["status"] != "done_partial"))
        self.assertEqual(salvage.sha_file(self.source), self.digest)


    def test_existing_unavailable_row_reprobes_once_with_legacy_profile(self):
        output = self.root / "legacy-reprobe"
        batch.scan([self.source], output)
        with patch.object(text_batch, "run_cli", return_value=False):
            baseline = batch.run(output, workers=1, pub_cli=self.pub)
        self.assertEqual(baseline["entries"][0]["text_status"], "unavailable")
        original_attempts = baseline["entries"][0]["attempts"]

        def supported(_cli, _src, target, *_args):
            self.make_story(target)
            return True

        with patch.object(text_batch, "run_cli", side_effect=supported) as worker:
            upgraded = batch.run(output, workers=1, pub_cli=self.pub,
                                 legacy_text_v1=True)
        self.assertEqual(worker.call_count, 1)
        self.assertTrue(worker.call_args.args[-1])
        self.assertEqual(upgraded["entries"][0]["text_status"], "verified")
        self.assertEqual(upgraded["entries"][0]["text_count"], 1)
        self.assertEqual(upgraded["entries"][0]["auto_tier"], "verified_text")
        self.assertEqual(upgraded["entries"][0]["attempts"], original_attempts + 1)
        self.assertEqual(batch.run(output, workers=1, pub_cli=self.pub,
                                   legacy_text_v1=True)["entries"][0]["attempts"],
                         original_attempts + 1)
        self.assertEqual(salvage.sha_file(self.source), self.digest)

    def test_failed_legacy_reprobe_not_retried_at_same_version(self):
        output = self.root / "legacy-unavailable"
        batch.scan([self.source], output)
        with patch.object(text_batch, "run_cli", return_value=False) as worker:
            batch.run(output, workers=1, pub_cli=self.pub)
            one = batch.run(output, workers=1, pub_cli=self.pub, legacy_text_v1=True)
            self.assertEqual(worker.call_count, 2)
            self.assertEqual(one["entries"][0]["text_status"], "unavailable")
            attempts = one["entries"][0]["attempts"]
            two = batch.run(output, workers=1, pub_cli=self.pub, legacy_text_v1=True)
            self.assertEqual(worker.call_count, 2)
        self.assertEqual(two["entries"][0]["attempts"], attempts)
        self.assertEqual(salvage.sha_file(self.source), self.digest)

    def test_legacy_flag_rejects_missing_pub_cli(self):
        output = self.root / "legacy-invalid"
        batch.scan([self.source], output)
        with self.assertRaisesRegex(batch.BatchError, "legacy_text_requires_pub_cli"):
            batch.run(output, legacy_text_v1=True)


if __name__ == "__main__":
    unittest.main()
