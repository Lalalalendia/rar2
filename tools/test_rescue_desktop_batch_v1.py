#!/usr/bin/env python3
"""Offline tests for the private batch queue and source-free public reports."""
from __future__ import annotations

import json
from pathlib import Path
import tempfile
import threading
import time
import unittest
from unittest.mock import patch

from tools import rescue_desktop_batch_v1 as batch
from tools import rescue_salvage_bundle_v1 as salvage


class RescueBatchTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.sources = self.root / "sources"
        self.sources.mkdir()
        self.output = self.root / "result"

    def sample(self, name: str, payload: bytes = b"Not a CFB: synthetic fixture") -> Path:
        path = self.sources / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(payload)
        return path

    def test_recursive_dedup_by_bytes_and_source_free_report(self):
        a = self.sample("one.pub", b"PUB-A")
        b = self.sample("nested/two.PUB", b"PUB-A")
        self.sample("nested/three.pub", b"PUB-B")
        self.sample("ignored.txt", b"PUB-C")
        result = batch.scan([self.sources], self.output)
        self.assertEqual(result["unique_sources"], 2)
        self.assertEqual(result["duplicate_paths"], 1)
        self.assertTrue(result["source_free"])
        self.assertEqual(result["counts"], {"pending": 2})
        private = (self.output / "queue-private.json").read_text()
        public = (self.output / "summary.json").read_text()
        csv = (self.output / "entries.csv").read_text()
        self.assertIn(str(b.absolute()), [path for row in batch.load_queue(self.output)["entries"]
                                          for path in row["paths"]])
        self.assertNotIn(str(self.sources), public)
        self.assertNotIn("one.pub", csv)
        self.assertNotIn("two.PUB", public)
        self.assertEqual(a.read_bytes(), b.read_bytes())

    def test_repeat_scan_requires_append_and_append_is_idempotent(self):
        self.sample("a.pub", b"A")
        batch.scan([self.sources], self.output)
        with self.assertRaisesRegex(batch.BatchError, "queue_exists_use_append"):
            batch.scan([self.sources], self.output)
        self.assertEqual(batch.scan([self.sources], self.output, append=True)["duplicate_paths"], 0)
        self.sample("b.pub", b"B")
        self.assertEqual(batch.scan([self.sources], self.output, append=True)["unique_sources"], 2)
        self.assertEqual(batch.status(self.output)["counts"], {"pending": 2})

    def test_output_must_not_be_inside_input_directory(self):
        self.sample("a.pub", b"A")
        with self.assertRaisesRegex(batch.BatchError, "output_overlaps_input_tree"):
            batch.scan([self.sources], self.sources / "subdir")
        with self.assertRaisesRegex(batch.BatchError, "output_overlaps_input_tree"):
            batch.scan([self.sources], self.root)
        self.assertFalse((self.sources / "subdir").exists())

    def test_input_symlink_rejected_and_directory_symlink_not_followed(self):
        self.sample("a.pub", b"A")
        target = self.root / "ref.pub"
        target.symlink_to(self.sources / "a.pub")
        with self.assertRaises(salvage.UnsafeInput):
            batch.scan([target], self.output)
        (self.sources / "linked-dir").symlink_to(self.sources, target_is_directory=True)
        self.assertEqual(batch.scan([self.sources], self.output)["unique_sources"], 1)

    def test_unreadable_invalid_files_reported_not_promoted(self):
        self.sample("empty.pub", b"")
        self.sample("valid.pub", b"A")
        result = batch.scan([self.sources], self.output)
        self.assertEqual(result["unique_sources"], 1)
        self.assertEqual(result["rejected_paths"], {"unsafe_or_invalid_pub": 1})

    def test_already_running_lock_fails_closed_and_releases(self):
        self.output.mkdir()
        with batch.output_lock(self.output):
            with self.assertRaisesRegex(batch.BatchError, "already_running"):
                with batch.output_lock(self.output):
                    pass
        with batch.output_lock(self.output):
            pass

    def test_status_does_not_reveal_private_paths(self):
        self.sample("client-private-name.pub", b"A")
        batch.scan([self.sources], self.output)
        rendered = json.dumps(batch.status(self.output))
        self.assertNotIn("client-private-name", rendered)
        self.assertNotIn(str(self.root), rendered)

    def test_unsupported_is_processed_not_fabricated_success(self):
        a = self.sample("one.pub", b"garbled-unknown")
        b = self.sample("two.pub", b"other-invalid")
        original = {p: p.read_bytes() for p in (a, b)}
        batch.scan([self.sources], self.output)
        result = batch.run(self.output, workers=2)
        self.assertEqual(result["counts"], {"done_unsupported": 2})
        self.assertEqual({r["attempts"] for r in result["entries"]}, {1})
        for path in (a, b):
            self.assertEqual(path.read_bytes(), original[path])
        for row in result["entries"]:
            bundle = self.output / "bundles" / row["sha256"]
            self.assertTrue((bundle / "bundle.json").exists())
            self.assertEqual(json.loads((bundle / "bundle.json").read_text())["outcome"], "unsupported")
        second = batch.run(self.output, workers=2)
        self.assertEqual(second["counts"], {"done_unsupported": 2})
        self.assertEqual({r["attempts"] for r in second["entries"]}, {1})

    def test_source_drift_fails_before_running_worker(self):
        source = self.sample("source.pub", b"A")
        batch.scan([source], self.output)
        source.write_bytes(b"CHANGED")
        result = batch.run(self.output)
        self.assertEqual(result["counts"], {"failed": 1})
        self.assertEqual(result["entries"][0]["reason"], "source_drift")
        self.assertFalse((self.output / "bundles" / result["entries"][0]["sha256"]).exists())

    def test_missing_source_never_replayed_as_done(self):
        source = self.sample("source.pub", b"A")
        batch.scan([source], self.output)
        batch.run(self.output)
        source.unlink()
        result = batch.run(self.output)
        self.assertEqual(result["counts"], {"failed": 1})
        self.assertEqual(result["entries"][0]["reason"], "source_missing")

    def test_tampered_completed_bundle_cannot_be_reused(self):
        source = self.sample("source.pub", b"A")
        batch.scan([source], self.output)
        batch.run(self.output)
        receipt = self.output / "bundles" / salvage.sha_file(source) / "bundle.json"
        j = json.loads(receipt.read_text())
        j["native_pub_delivery_allowed"] = True
        receipt.write_text(json.dumps(j))
        result = batch.run(self.output)
        self.assertEqual(result["counts"], {"failed": 1})
        self.assertEqual(result["entries"][0]["reason"], "bundle_integrity")
        self.assertIn("failed", batch.status(self.output)["counts"])

    def test_stale_running_state_resumes_after_crash(self):
        source = self.sample("source.pub", b"A")
        batch.scan([source], self.output)
        private = batch.load_queue(self.output)
        private["entries"][0]["status"] = "running"
        private["run_token"] = "dead-process-id"
        batch.persist(self.output, private)
        result = batch.run(self.output, workers=1)
        self.assertEqual(result["counts"], {"done_unsupported": 1})
        self.assertEqual(batch.load_queue(self.output)["run_token"], None)

    def test_retry_is_explicit_for_failure(self):
        source = self.sample("source.pub", b"A")
        batch.scan([source], self.output)
        source.write_bytes(b"B")
        result = batch.run(self.output)
        self.assertEqual(result["counts"], {"failed": 1})
        self.assertEqual(batch.retry(self.output)["counts"], {"pending": 1})
        self.assertEqual(batch.run(self.output)["counts"], {"failed": 1})

    def test_bad_queue_does_not_execute(self):
        self.sample("source.pub", b"A")
        batch.scan([self.sources], self.output)
        queue = batch.load_queue(self.output)
        queue["entries"][0]["sha256"] = "../../bad"
        batch.json_write(self.output / "queue-private.json", queue)
        with self.assertRaisesRegex(batch.BatchError, "queue_row_invalid"):
            batch.run(self.output)

    def test_append_after_stale_crash_token(self):
        self.sample("a.pub", b"A")
        batch.scan([self.sources], self.output)
        state = batch.load_queue(self.output)
        state["run_token"] = "dead-token"
        batch.persist(self.output, state)
        self.sample("b.pub", b"B")
        self.assertEqual(batch.scan([self.sources], self.output, append=True)["unique_sources"], 2)

    def test_pause_drains_current_worker_and_resume_continues(self):
        self.sample("a.pub", b"A")
        self.sample("b.pub", b"B")
        batch.scan([self.sources], self.output)
        started = threading.Event()
        release = threading.Event()
        original = batch.process_one
        counter = [0]

        def fake(*args, **kwargs):
            counter[0] += 1
            started.set()
            release.wait(3)
            return {"status": "failed", "reason": "worker_error", "outcome": None,
                    "stream_count": 0, "asset_count": 0}

        outcome = {}
        with patch.object(batch, "process_one", side_effect=fake):
            thread = threading.Thread(target=lambda: outcome.setdefault("report", batch.run(self.output, workers=1)))
            thread.start()
            self.assertTrue(started.wait(2))
            batch.request_action(self.output, "pause")
            release.set()
            thread.join(5)
        self.assertFalse(thread.is_alive())
        self.assertEqual(counter[0], 1)
        self.assertEqual(outcome["report"]["counts"], {"failed": 1, "pending": 1})
        batch.retry(self.output)
        result = batch.run(self.output, workers=1)
        self.assertEqual(result["counts"], {"done_unsupported": 2})

    def test_cancel_aborts_inflight_and_retries_later(self):
        self.sample("a.pub", b"A")
        batch.scan([self.sources], self.output)
        started = threading.Event()
        outcome = {}

        def blocking(row, root, *, abort, **kwargs):
            started.set()
            abort.wait(3)
            return {"status": "cancelled", "reason": "cancelled", "outcome": None,
                    "stream_count": 0, "asset_count": 0}

        with patch.object(batch, "process_one", side_effect=blocking):
            thread = threading.Thread(target=lambda: outcome.setdefault("report", batch.run(self.output)))
            thread.start()
            self.assertTrue(started.wait(2))
            batch.request_action(self.output, "cancel")
            thread.join(5)
        self.assertFalse(thread.is_alive())
        self.assertEqual(outcome["report"]["counts"], {"cancelled": 1})
        self.assertEqual(batch.retry(self.output)["counts"], {"pending": 1})
        self.assertEqual(batch.run(self.output)["counts"], {"done_unsupported": 1})

    def test_concurrency_is_bounded(self):
        for i in range(5):
            self.sample(f"{i}.pub", f"PUB-{i}".encode())
        batch.scan([self.sources], self.output)
        guard = threading.Lock()
        active, high = [0], [0]

        def fake(*args, **kwargs):
            with guard:
                active[0] += 1
                high[0] = max(high[0], active[0])
            time.sleep(0.1)
            with guard:
                active[0] -= 1
            return {"status": "failed", "reason": "worker_error", "outcome": None,
                    "stream_count": 0, "asset_count": 0}

        with patch.object(batch, "process_one", side_effect=fake):
            result = batch.run(self.output, workers=2)
        self.assertEqual(result["counts"], {"failed": 5})
        self.assertEqual(high[0], 2)

    def test_atomic_csv_keeps_prior_on_write_failure(self):
        self.output.mkdir()
        target = self.output / "entries.csv"
        target.write_text("old", encoding="utf-8")
        with patch.object(batch.os, "replace", side_effect=OSError("disk full")):
            with self.assertRaises(OSError):
                batch.atomic_write(target, "new")
        self.assertEqual(target.read_text(encoding="utf-8"), "old")
        self.assertFalse(list(self.output.glob(".rescue-atomic-*")))

    def test_cli_flags_and_return_codes(self):
        self.sample("a.pub", b"A")
        with patch("builtins.print") as printer:
            self.assertEqual(batch.main(["scan", "--input", str(self.sources), "--output", str(self.output)]), 0)
            self.assertEqual(batch.main(["run", "--output", str(self.output), "--workers", "1"]), 0)
            self.assertEqual(batch.main(["status", "--output", str(self.output)]), 0)
        self.assertTrue(printer.called)


if __name__ == "__main__":
    unittest.main()
