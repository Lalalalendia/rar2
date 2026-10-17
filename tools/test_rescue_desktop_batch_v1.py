#!/usr/bin/env python3
"""Offline tests for the private batch queue and source-free public reports."""
from __future__ import annotations

import json
import shutil
from pathlib import Path
import tempfile
import threading
import time
import unittest
from unittest.mock import patch

from tools import rescue_desktop_batch_v1 as batch
from tools import rescue_salvage_bundle_v1 as salvage
from tools import rescue_auto_restore_v1 as autorestore
from tools.test_rescue_cfb_stream_identity import collision_fixture


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

    def test_stale_gui_controls_cannot_cancel_successor_run(self):
        self.sample("a.pub", b"A")
        batch.scan([self.sources], self.output)
        first_token = "gui-run-previous-0001"
        next_token = "gui-run-successor-0002"
        state = batch.load_queue(self.output)
        state["run_token"] = first_token
        batch.persist(self.output, state)
        with self.assertRaisesRegex(batch.BatchError, "run_token_mismatch"):
            batch.request_action(self.output, "cancel", expected_run_token=next_token)
        self.assertFalse((self.output / "control.json").exists())
        batch.request_action(self.output, "pause", expected_run_token=first_token)
        self.assertEqual(batch.control_action(self.output, first_token), "pause")
        state["run_token"] = next_token
        batch.persist(self.output, state)
        self.assertIsNone(batch.control_action(self.output, next_token))
        with self.assertRaisesRegex(batch.BatchError, "run_token_mismatch"):
            batch.request_action(self.output, "cancel", expected_run_token=first_token)
        self.assertIsNone(batch.control_action(self.output, next_token))
        batch.request_action(self.output, "cancel", expected_run_token=next_token)
        self.assertEqual(batch.control_action(self.output, next_token), "cancel")

    def test_gui_cancel_token_stops_two_workers_and_retry_is_safe(self):
        self.sample("a.pub", b"A")
        self.sample("b.pub", b"B")
        batch.scan([self.sources], self.output)
        token = "gui-run-parallel-0001"
        both_active = threading.Event()
        entered = [0]
        mutex = threading.Lock()
        result = {}

        def blocked(_row, _root, *, abort, **_kwargs):
            with mutex:
                entered[0] += 1
                if entered[0] == 2:
                    both_active.set()
            abort.wait(6)
            return {"status": "cancelled", "reason": "cancelled", "outcome": None,
                    "stream_count": 0, "asset_count": 0}

        with patch.object(batch, "process_one", side_effect=blocked):
            worker = threading.Thread(target=lambda: result.setdefault(
                "report", batch.run(self.output, workers=2, run_token=token)))
            worker.start()
            self.assertTrue(both_active.wait(4))
            with self.assertRaisesRegex(batch.BatchError, "run_token_mismatch"):
                batch.request_action(self.output, "cancel", expected_run_token="gui-stale-worker-0001")
            self.assertTrue(worker.is_alive())
            batch.request_action(self.output, "cancel", expected_run_token=token)
            worker.join(8)
        self.assertFalse(worker.is_alive())
        self.assertEqual(result["report"]["counts"], {"cancelled": 2})
        self.assertIsNone(batch.load_queue(self.output)["run_token"])
        self.assertEqual(batch.retry(self.output)["counts"], {"pending": 2})
        self.assertEqual(batch.run(self.output, workers=2)["counts"], {"done_unsupported": 2})

    def test_invalid_explicit_run_token_fails_before_queue_mutation(self):
        self.sample("a.pub", b"A")
        batch.scan([self.sources], self.output)
        initial = (self.output / "queue-private.json").read_bytes()
        for token in ("short", "../outside", " " * 32):
            with self.assertRaisesRegex(batch.BatchError, "invalid_run_token"):
                batch.run(self.output, run_token=token)
        self.assertEqual((self.output / "queue-private.json").read_bytes(), initial)

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


class RescueAutoPresentationTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.source = self.root / "confidential-example.pub"
        self.output = self.root / "result"

    def test_unsupported_real_batch_auto_indexes_without_false_repair(self):
        self.source.write_bytes(b"synthetic-unsupported-input")
        original = self.source.read_bytes()
        scan = batch.scan([self.source], self.output)
        self.assertEqual(scan["counts"], {"pending": 1})
        first = batch.run(self.output, workers=1)
        self.assertEqual(first["counts"], {"done_unsupported": 1})
        row = first["entries"][0]
        self.assertEqual(row["auto_tier"], "unsupported")
        self.assertEqual(row["auto_index"], f"recovered/{row['sha256']}/index.html")
        self.assertIn("auto_tier", (self.output / "entries.csv").read_text())
        target = self.output / "recovered" / row["sha256"]
        public = json.loads((target / "recovery.json").read_text())
        page = (target / "index.html").read_text()
        self.assertEqual(public["tier"], "unsupported")
        self.assertFalse(public["native_pub_repair_attempted"])
        self.assertFalse(public["semantic_text_reconstructed"])
        self.assertFalse(public["native_pub_delivery_allowed"])
        self.assertNotIn(self.source.name, page)
        self.assertNotIn(str(self.root), page)
        self.assertEqual(self.source.read_bytes(), original)
        html = (target / "index.html").read_bytes()
        rerun = batch.run(self.output, workers=1)
        self.assertEqual(rerun["counts"], {"done_unsupported": 1})
        self.assertEqual(rerun["entries"][0]["attempts"], 1)
        self.assertEqual((target / "index.html").read_bytes(), html)

    def test_real_cfb_two_sid_streams_auto_index_without_repaired_pub(self):
        self.source.write_bytes(collision_fixture())
        original = salvage.sha_file(self.source)
        batch.scan([self.source], self.output)
        result = batch.run(self.output, workers=1)
        self.assertEqual(result["counts"], {"done_partial": 1})
        row = result["entries"][0]
        self.assertEqual(row["auto_tier"], "verified_streams")
        folder = self.output / "recovered" / row["sha256"]
        summary = json.loads((folder / "recovery.json").read_text())
        self.assertEqual(summary["physical_stream_count"], 2)
        self.assertEqual(summary["image_asset_count"], 0)
        self.assertFalse(summary["native_pub_delivery_allowed"])
        html = (folder / "index.html").read_text(encoding="utf-8")
        self.assertIn("sid-000001", html)
        self.assertIn("sid-000002", html)
        self.assertIn("../../bundles/", html)
        self.assertNotIn("repaired.pub", html)
        self.assertEqual(salvage.sha_file(self.source), original)

    def test_tampered_presentation_cannot_be_silently_regenerated(self):
        self.source.write_bytes(b"synthetic-unsupported-input")
        batch.scan([self.source], self.output)
        first = batch.run(self.output)
        sha = first["entries"][0]["sha256"]
        target = self.output / "recovered" / sha / "index.html"
        target.write_text("<script>unexpected</script>", encoding="utf-8")
        after = batch.run(self.output)
        self.assertEqual(after["counts"], {"failed": 1})
        self.assertEqual(after["entries"][0]["reason"], "auto_restore_integrity")
        self.assertIn("unexpected", target.read_text())
        batch.retry(self.output)
        again = batch.run(self.output)
        self.assertEqual(again["counts"], {"failed": 1})
        self.assertEqual(again["entries"][0]["reason"], "auto_restore_integrity")

    def test_missing_legacy_presentation_backfills_without_worker_rerun(self):
        self.source.write_bytes(b"synthetic-unsupported-input")
        batch.scan([self.source], self.output)
        first = batch.run(self.output)
        sha = first["entries"][0]["sha256"]
        old_attempts = first["entries"][0]["attempts"]
        shutil.rmtree(self.output / "recovered" / sha)
        next_result = batch.run(self.output)
        self.assertEqual(next_result["entries"][0]["attempts"], old_attempts)
        self.assertEqual(next_result["entries"][0]["auto_tier"], "unsupported")
        self.assertTrue((self.output / "recovered" / sha / "recovery.json").is_file())

    def test_verified_image_gallery_and_private_path_separation(self):
        self.source.write_bytes(b"synthetic-image-only-evidence")
        digest = salvage.sha_file(self.source)
        bundle = self.output / "bundles" / digest
        (bundle / "images").mkdir(parents=True)
        (bundle / "images" / "cover.png").write_bytes(b"\x89PNG\r\n\x1a\nsynthetic")
        (bundle / "images" / "manifest.json").write_text(
            json.dumps({"assets": [{"filename": "cover.png"}]}), encoding="utf-8")
        (bundle / "bundle.json").write_text("synthetic existing independently verified receipt")
        # This unit test passes the same already-verified receipt contract that
        # the batch executor gets only after read_bundle_report succeeds.
        verified = {
            "source": {"sha256": digest},
            "source_immutable": True,
            "native_pub_delivery_allowed": False,
            "text": {"status": "not_implemented"},
            "outcome": "partial",
            "physical": {"status": "unavailable"},
            "images": {"status": "verified", "asset_count": 1},
        }
        data = autorestore.materialize(
            self.source, bundle, self.output / "recovered", verified)
        self.assertEqual(data["tier"], "verified_images")
        page = (self.output / "recovered" / digest / "index.html").read_text()
        self.assertIn("images/cover.png", page)
        self.assertNotIn("confidential-example.pub", page)
        self.assertNotIn(str(self.source), page)

    def test_untrusted_manifest_artifact_path_fails_before_presentation(self):
        self.source.write_bytes(b"synthetic-private-input")
        digest = salvage.sha_file(self.source)
        bundle = self.output / "bundles" / digest
        (bundle / "images").mkdir(parents=True)
        (bundle / "images" / "manifest.json").write_text(
            json.dumps({"assets": [{"filename": "../escape.html"}]}))
        (bundle / "bundle.json").write_text("synthetic")
        receipt = {
            "source": {"sha256": digest}, "source_immutable": True,
            "native_pub_delivery_allowed": False, "text": {"status": "not_implemented"},
            "outcome": "partial", "physical": {"status": "unavailable"},
            "images": {"status": "verified", "asset_count": 1},
        }
        with self.assertRaisesRegex(autorestore.AutoRestoreError, "unsafe_artifact_name"):
            autorestore.materialize(
                self.source, bundle, self.output / "recovered", receipt)
        self.assertFalse((self.output / "recovered" / digest).exists())

    def test_public_tier_field_cannot_echo_private_queue_value(self):
        self.source.write_bytes(b"local-only")
        batch.scan([self.source], self.output)
        private = batch.load_queue(self.output)
        private["entries"][0]["auto_tier"] = str(self.source)
        report = batch.report(private)
        self.assertIsNone(report["entries"][0]["auto_tier"])
        self.assertIsNone(report["entries"][0]["auto_index"])
        self.assertNotIn(self.source.name, json.dumps(report))

    def test_real_salvage_verified_asset_manifest_builds_gallery(self):
        """Exercise the actual salvage output verifier, not a fake receipt."""
        self.source.write_bytes(b"synthetic-image-carrier")
        fake_pub_cli = self.root / "synthetic-pub-cli.exe"
        fake_pub_cli.write_bytes(b"test only")
        payload = b"\xff\xd8\xff\xd9"
        name = "asset-123.jpg"

        def fake_producer(args, _timeout):
            if "extract-assets" not in args:
                return "failed"  # no synthetic physical stream evidence
            asset_dir = Path(args[args.index("--output") + 1])
            asset_dir.mkdir()
            (asset_dir / name).write_bytes(payload)
            (asset_dir / "manifest.json").write_text(json.dumps({
                "schema_version": salvage.ASSET_SCHEMA,
                "assets": [{
                    "filename": name, "sha256": salvage.hashlib.sha256(payload).hexdigest(),
                    "byte_len": len(payload)
                }],
                "diagnostics": []
            }), encoding="utf-8")
            return None

        bundle_root = self.output / "bundles"
        with patch.object(salvage, "run_command", side_effect=fake_producer):
            kind, report = salvage.execute(
                self.source, bundle_root, pub_cli=fake_pub_cli)
            self.assertEqual(kind, "created")
            digest = salvage.sha_file(self.source)
            bundle = bundle_root / digest
            admitted = salvage.read_bundle_report(self.source, bundle, timeout=45)
        self.assertEqual(admitted, report)
        data = autorestore.materialize(
            self.source, bundle, self.output / "recovered", admitted)
        self.assertEqual(data["tier"], "verified_images")
        self.assertEqual(data["physical_stream_count"], 0)
        self.assertEqual(data["image_asset_count"], 1)
        html = (self.output / "recovered" / digest / "index.html").read_text()
        self.assertIn("images/asset-123.jpg", html)
        self.assertNotIn(self.source.name, html)
        self.assertFalse(data["native_pub_delivery_allowed"])
        with patch.object(salvage, "run_command", side_effect=fake_producer):
            again = salvage.read_bundle_report(self.source, bundle, timeout=45)
        self.assertEqual(
            autorestore.materialize(self.source, bundle, self.output / "recovered", again),
            data)
        (bundle / "images" / name).write_bytes(b"tamper")
        with self.assertRaisesRegex(salvage.UnsafeInput, "asset_"):
            salvage.read_bundle_report(self.source, bundle, timeout=45)

    def test_stale_auto_tier_is_never_published_for_nonterminal_status(self):
        self.source.write_bytes(b"private source")
        batch.scan([self.source], self.output)
        state = batch.load_queue(self.output)
        row = state["entries"][0]
        for current in ("pending", "running", "failed", "cancelled"):
            with self.subTest(status=current):
                row["status"] = current
                row["auto_tier"] = "verified_images"
                report = batch.report(state)
                self.assertIsNone(report["entries"][0]["auto_tier"])
                self.assertIsNone(report["entries"][0]["auto_index"])

    def test_presentation_symlink_rejected_without_overwrite(self):
        self.source.write_bytes(b"synthetic")
        batch.scan([self.source], self.output)
        result = batch.run(self.output)
        digest = result["entries"][0]["sha256"]
        victim = self.root / "do-not-touch"
        victim.mkdir()
        target = self.output / "recovered" / digest
        shutil.rmtree(target)
        try:
            target.symlink_to(victim, target_is_directory=True)
        except (NotImplementedError, OSError):
            self.skipTest("symlinks unavailable on this Windows test host")
        rerun = batch.run(self.output)
        self.assertEqual(rerun["counts"], {"failed": 1})
        self.assertEqual(rerun["entries"][0]["reason"], "auto_restore_integrity")
        self.assertEqual(list(victim.iterdir()), [])


if __name__ == "__main__":
    unittest.main()
