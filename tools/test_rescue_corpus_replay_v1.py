#!/usr/bin/env python3
"""Source-safe offline corpus replay acceptance for the existing CFB tool seam."""
from __future__ import annotations

import hashlib
import json
import os
import subprocess
import sys
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from tools import rescue_corpus_replay_v1 as replay


def sha(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


class ReplayTests(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.base = Path(self.tmp.name)
        self.source = self.base / "sources"
        self.source.mkdir()
        self.out = self.base / "results"
        self.authority = self.base / "authority.txt"

    def add(self, name: str, content: bytes) -> str:
        (self.source / name).write_bytes(content)
        return sha(content)

    def auth(self, *shas: str) -> None:
        self.authority.write_text("\n".join(sorted(shas)) + "\n", encoding="ascii")

    def execute(self, *, scan_only: bool = False) -> dict:
        return replay.run([self.source], self.authority, self.out, scan_only=scan_only,
                          max_bytes=1024 * 1024, timeout=5)

    @staticmethod
    def fake_runner(module: str, args: list[str], timeout: int) -> str | None:
        if module == "tools.rescue_cfb_stream_export":
            source = Path(args[0])
            if source.read_bytes().startswith(b"BAD"):
                return "worker_failed"
            dest = Path(args[args.index("--output-dir") + 1])
            dest.mkdir(parents=True)
            (dest / "sid-000001-contents.bin").write_bytes(source.read_bytes())
            manifest = Path(args[args.index("--manifest") + 1])
            manifest.write_text("{}", encoding="utf-8")
            return None
        if module == "tools.verify_rescue_cfb_stream_export":
            source = Path(args[0])
            dest = Path(args[args.index("--artifact-dir") + 1])
            if (dest / "sid-000001-contents.bin").read_bytes() != source.read_bytes():
                return "worker_failed"
            Path(args[args.index("--receipt") + 1]).write_text(json.dumps({
                "pass": True, "source_sha256": sha(source.read_bytes()),
                "verified_stream_count": 1}), encoding="utf-8")
            return None
        raise AssertionError(module)

    def test_authority_and_source_counts_are_distinct(self):
        present = self.add("a.pub", b"source-a")
        absent = sha(b"missing")
        self.add("outside.PUB", b"outside")
        self.add("dupe.pub", b"source-a")
        self.auth(present, absent)
        r = self.execute(scan_only=True)
        self.assertEqual(r["authority_count"], 2)
        self.assertEqual(r["counts"], {"present_unprocessed": 1, "missing": 1})
        self.assertEqual(r["scan"]["discovered_pub"], 3)
        self.assertEqual(r["scan"]["outside_authority"], 1)
        self.assertEqual(r["scan"]["duplicate_source"], 1)
        self.assertNotIn(str(self.source), json.dumps(r))
        self.assertFalse((self.out / "bundles" / present).exists())

    def test_export_reuse_and_tamper_fail_closed(self):
        data = b"source-data"
        h = self.add("corpus.pub", data)
        self.auth(h)
        with patch.object(replay, "invoke", side_effect=self.fake_runner):
            first = self.execute()
            self.assertEqual(first["counts"], {"extracted": 1})
            self.assertEqual(first["entries"][0]["verified_streams"], 1)
            self.assertEqual((self.source / "corpus.pub").read_bytes(), data)
            second = self.execute()
            self.assertEqual(second["counts"], {"reused": 1})
            bundle = self.out / "bundles" / h
            (bundle / "streams" / "sid-000001-contents.bin").write_bytes(b"tampered")
            third = self.execute()
            self.assertEqual(third["counts"], {"failed": 1})
            self.assertEqual(third["entries"][0]["reason"], "invalid_existing_bundle")
            self.assertEqual((bundle / "streams" / "sid-000001-contents.bin").read_bytes(), b"tampered")

    def test_bad_source_cannot_abort_other_sources(self):
        good = self.add("good.pub", b"GOOD")
        bad = self.add("bad.pub", b"BAD contents")
        self.auth(good, bad)
        with patch.object(replay, "invoke", side_effect=self.fake_runner):
            r = self.execute()
        self.assertEqual(r["counts"], {"failed": 1, "extracted": 1})
        rows = {entry["source_sha256"]: entry for entry in r["entries"]}
        self.assertEqual(rows[bad]["reason"], "export_worker_failed")
        self.assertEqual(rows[good]["status"], "extracted")

    def test_wrong_hash_named_file_rejected(self):
        present = sha(b"correct")
        self.add(present + ".pub", b"wrong")
        self.auth(present)
        r = self.execute(scan_only=True)
        self.assertEqual(r["counts"], {"missing": 1})
        self.assertEqual(r["scan"]["hash_named_mismatch"], 1)

    def test_single_file_with_sibling_output_is_safe(self):
        h = self.add("source.pub", b"A")
        self.auth(h)
        r = replay.run([self.source / "source.pub"], self.authority,
                       self.source.parent / "single_result", scan_only=True,
                       max_bytes=500, timeout=5)
        self.assertEqual(r["counts"], {"present_unprocessed": 1})

    def test_existing_symlink_receipt_target_rejected(self):
        h = self.add("safe.pub", b"A")
        self.auth(h)
        self.out.mkdir()
        (self.out / "entries.csv").symlink_to(self.source / "safe.pub")
        with self.assertRaisesRegex(replay.Rejected, "unsafe_receipt_target"):
            self.execute(scan_only=True)
        self.assertEqual((self.source / "safe.pub").read_bytes(), b"A")

    def test_source_output_overlap_rejected(self):
        self.add("a.pub", b"A")
        self.auth(sha(b"A"))
        with self.assertRaisesRegex(replay.Rejected, "source_output_overlap"):
            replay.run([self.source], self.authority, self.source / "out", scan_only=True,
                       max_bytes=500, timeout=5)

    def test_source_symlink_skipped(self):
        self.add("a.pub", b"A")
        (self.source / "alias.pub").symlink_to(self.source / "a.pub")
        self.auth(sha(b"A"))
        r = self.execute(scan_only=True)
        self.assertEqual(r["scan"]["unsafe_symlink"], 1)
        self.assertEqual(r["counts"], {"present_unprocessed": 1})

    def test_output_lock_survives_prior_run_and_rejects_live_contention(self):
        self.out.mkdir()
        lock = self.out / ".rescue-replay.lock"
        with replay.exclusive_output_lock(lock):
            with self.assertRaisesRegex(replay.Rejected, "output_locked"):
                with replay.exclusive_output_lock(lock):
                    pass
        self.assertTrue(lock.is_file())
        with replay.exclusive_output_lock(lock):
            pass

    def test_abrupt_child_exit_does_not_block_resume(self):
        self.out.mkdir()
        lock = self.out / ".rescue-replay.lock"
        code = (
            "import os,sys; "
            "from tools.rescue_corpus_replay_v1 import exclusive_output_lock; "
            "from pathlib import Path; "
            "with_context = exclusive_output_lock(Path(sys.argv[1])); "
            "with_context.__enter__(); os._exit(37)"
        )
        result = subprocess.run([sys.executable, "-c", code, str(lock)],
                                cwd=replay.REPO_ROOT, check=False)
        self.assertEqual(result.returncode, 37)
        with replay.exclusive_output_lock(lock):
            pass

    def test_csv_write_is_atomic_when_serialization_fails(self):
        self.out.mkdir()
        target = self.out / "entries.csv"
        target.write_text("previous\n", encoding="utf-8")
        with self.assertRaises(ValueError):
            replay.atomic_csv(target, [{"invalid_col": "value"}])
        self.assertEqual(target.read_text(encoding="utf-8"), "previous\n")
        self.assertEqual(list(self.out.glob(".rescue-csv-*.tmp")), [])

    def test_sorted_unique_sha_authority_required(self):
        h1 = sha(b"a")
        self.authority.write_text(h1 + "\n" + h1 + "\n")
        with self.assertRaisesRegex(replay.Rejected, "authority_must_be_sorted_unique"):
            replay.load_authority(self.authority)

    def test_default_pinned_replay_authority_when_present(self):
        if not replay.DEFAULT_AUTHORITY.exists():
            self.skipTest("Standalone local test checkout lacks repo authority receipt")
        values = replay.load_authority(replay.DEFAULT_AUTHORITY)
        self.assertEqual(len(values), 1556)
        self.assertEqual(sha(("\n".join(values) + "\n").encode("ascii")),
                         "950af4b1a1346168a55835b80bda062bbe1ebf9d926a4c5dc92a3d8bdb855073")


if __name__ == "__main__":
    unittest.main()
