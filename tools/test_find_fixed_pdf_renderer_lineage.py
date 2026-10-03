#!/usr/bin/env python3
import json
import pathlib
import tempfile
import unittest
import zipfile
import sys

ROOT = pathlib.Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tools"))

from find_fixed_pdf_renderer_lineage import (
    SCHEMA_SUMMARY,
    ScanConfig,
    encode_json,
    make_private_receipt,
    make_summary,
    scan_roots,
)


class FixedPdfRendererRecoveryPreflightTests(unittest.TestCase):
    def test_worktree_source_candidate_is_detected(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            repo = root / "old-yab"
            (repo / "crates" / "pub-pdf").mkdir(parents=True)
            (repo / "crates" / "pub-cli").mkdir(parents=True)
            (repo / "crates" / "pub-pdf" / "Cargo.toml").write_text(
                "[package]\nname='pub-pdf'\n", encoding="utf-8"
            )
            (repo / "crates" / "pub-cli" / "Cargo.toml").write_text(
                "[package]\nname='pub-cli'\n", encoding="utf-8"
            )
            candidates = scan_roots([root], ScanConfig(max_depth=3))
        worktrees = [c for c in candidates if c["kind"] == "worktree_source_candidate"]
        self.assertEqual(len(worktrees), 1)
        self.assertEqual(
            sorted(worktrees[0]["sentinels"]),
            ["crates/pub-cli/Cargo.toml", "crates/pub-pdf/Cargo.toml"],
        )

    def test_zip_source_candidate_is_detected_without_extracting(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            archive = root / "yab-backup.zip"
            with zipfile.ZipFile(archive, "w") as handle:
                handle.writestr("yab/crates/pub-pdf/Cargo.toml", "[package]\n")
                handle.writestr("yab/crates/pub-cli/Cargo.toml", "[package]\n")
            candidates = scan_roots([root], ScanConfig(max_depth=2))
        archives = [c for c in candidates if c["kind"] == "source_archive_candidate"]
        self.assertEqual(len(archives), 1)
        self.assertEqual(
            sorted(archives[0]["sentinels"]),
            ["crates/pub-cli/Cargo.toml", "crates/pub-pdf/Cargo.toml"],
        )

    def test_known_target_release_binary_is_hashed_without_execution(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            repo = root / "old-yab"
            binary = repo / "target" / "release" / "pub.exe"
            binary.parent.mkdir(parents=True)
            binary.write_bytes(b"MZ-not-executed-renderer-candidate")
            candidates = scan_roots([root], ScanConfig(max_depth=3))
        binaries = [c for c in candidates if c["kind"] == "unbound_binary_candidate"]
        self.assertEqual(len(binaries), 1)
        self.assertFalse(binaries[0]["executed"])
        self.assertEqual(binaries[0]["file_name"], "pub.exe")

    def test_authoritative_empty_roots_emit_terminal_typed_blocker(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            private = make_private_receipt(
                [root],
                [],
                authoritative_roots=True,
                config=ScanConfig(),
            )
        self.assertEqual(private["status"], "renderer_lineage_bytes_unavailable")

    def test_summary_contains_no_local_paths(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            candidate = {
                "kind": "worktree_source_candidate",
                "local_path": str(root / "secret" / "old-yab"),
                "sentinels": ["crates/pub-pdf/Cargo.toml"],
                "manifest_fingerprints": {},
            }
            private = make_private_receipt(
                [root],
                [candidate],
                authoritative_roots=False,
                config=ScanConfig(),
            )
            private_bytes = encode_json(private)
            summary = make_summary(private, private_bytes)
            encoded = json.dumps(summary, sort_keys=True)
        self.assertEqual(summary["schema_version"], SCHEMA_SUMMARY)
        self.assertNotIn(str(root), encoded)
        self.assertFalse(summary["local_paths_emitted"])
        self.assertEqual(summary["candidate_count"], 1)


if __name__ == "__main__":
    unittest.main()
