#!/usr/bin/env python3
from __future__ import annotations

import csv
import hashlib
import json
import sys
import tempfile
import unittest
import zipfile
from pathlib import Path

import fitz

sys.path.insert(0, str(Path(__file__).resolve().parent))

from cloud_reader_manual_oracle_bundle_v1 import (
    CFB_MAGIC,
    MAX_BUNDLE_MEMBERS,
    compare_bundle,
    prepare_bundle,
)


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


class ManualOracleBundleTests(unittest.TestCase):
    def make_pair(self, root: Path, basename: str = "Fixture") -> tuple[Path, Path]:
        pub = root / f"{basename}.pub"
        pub.write_bytes(CFB_MAGIC + b"\0" * 8192)

        pdf = root / f"{basename}.pdf"
        document = fitz.open()
        page = document.new_page(width=612, height=792)
        page.insert_text((72, 72), "Publisher oracle fixture")
        document.set_metadata(
            {
                "creator": "Microsoft Publisher test oracle",
                "producer": "Microsoft Publisher test oracle",
            }
        )
        document.save(pdf)
        document.close()

        with (root / "PAIRS.csv").open("w", newline="", encoding="utf-8") as handle:
            writer = csv.DictWriter(
                handle,
                fieldnames=[
                    "oracle_id",
                    "basename",
                    "pub_filename",
                    "pub_bytes",
                    "pub_sha256",
                    "pdf_filename",
                    "pdf_bytes",
                    "pdf_sha256",
                    "pdf_pages",
                ],
            )
            writer.writeheader()
            writer.writerow(
                {
                    "oracle_id": "TEST-01",
                    "basename": basename,
                    "pub_filename": pub.name,
                    "pub_bytes": pub.stat().st_size,
                    "pub_sha256": sha256(pub),
                    "pdf_filename": pdf.name,
                    "pdf_bytes": pdf.stat().st_size,
                    "pdf_sha256": sha256(pdf),
                    "pdf_pages": 1,
                }
            )
        return pub, pdf

    def test_prepare_validates_exact_pair_and_omits_page_assertion(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary) / "pair"
            root.mkdir()
            pub, _ = self.make_pair(root)
            manifest = Path(temporary) / "reader.json"
            registry = Path(temporary) / "registry.json"

            result = prepare_bundle(root, Path(temporary) / "work", manifest, registry)

            self.assertEqual(result["pair_count"], 1)
            self.assertEqual(result["reference_page_count"], 1)
            reader = json.loads(manifest.read_text(encoding="utf-8"))
            fixture = reader["fixtures"][0]
            self.assertEqual(fixture["name"], "Fixture")
            self.assertEqual(fixture["sha256"], sha256(pub))
            self.assertNotIn(
                "pages",
                fixture,
                "author PDF page count is evidence to compare, not a pre-render assertion",
            )

    def test_prepare_accepts_one_nested_zip_root(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary) / "source"
            root.mkdir()
            self.make_pair(root)
            archive = Path(temporary) / "bundle.zip"
            with zipfile.ZipFile(archive, "w") as zipped:
                for path in root.iterdir():
                    zipped.write(path, f"oracle/{path.name}")

            result = prepare_bundle(
                archive,
                Path(temporary) / "extract",
                Path(temporary) / "reader.json",
                Path(temporary) / "registry.json",
            )
            self.assertEqual(result["pair_count"], 1)

    def test_prepare_rejects_zip_traversal(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            archive = Path(temporary) / "unsafe.zip"
            with zipfile.ZipFile(archive, "w") as zipped:
                zipped.writestr("../PAIRS.csv", "bad")
            with self.assertRaisesRegex(ValueError, "unsafe path"):
                prepare_bundle(
                    archive,
                    Path(temporary) / "extract",
                    Path(temporary) / "reader.json",
                    Path(temporary) / "registry.json",
                )

    def test_prepare_rejects_excessive_zip_member_inventory(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            archive = Path(temporary) / "too-many.zip"
            with zipfile.ZipFile(archive, "w") as zipped:
                for index in range(MAX_BUNDLE_MEMBERS + 1):
                    zipped.writestr(f"member-{index}.txt", b"")
            with self.assertRaisesRegex(ValueError, "too many members"):
                prepare_bundle(
                    archive,
                    Path(temporary) / "extract",
                    Path(temporary) / "reader.json",
                    Path(temporary) / "registry.json",
                )

    def test_compare_builds_ranked_zero_diff_page(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary) / "pair"
            root.mkdir()
            _, pdf = self.make_pair(root)
            registry_path = Path(temporary) / "registry.json"
            prepare_bundle(
                root,
                Path(temporary) / "prepare",
                Path(temporary) / "reader.json",
                registry_path,
            )

            cloud_dir = Path(temporary) / "cloud"
            cloud_dir.mkdir()
            candidate = cloud_dir / "Fixture-page-1.png"
            document = fitz.open(pdf)
            pixmap = document.load_page(0).get_pixmap(
                dpi=144,
                colorspace=fitz.csRGB,
                alpha=False,
            )
            pixmap.save(candidate)
            document.close()

            receipt = {
                "protocol": "chaptera.cloud-reader-real-scene-browser.v1",
                "repository_commit_sha": "a" * 40,
                "browser": "test-browser",
                "results": [
                    {
                        "fixture": "Fixture",
                        "source_sha256": sha256(root / "Fixture.pub"),
                        "source_byte_len": (root / "Fixture.pub").stat().st_size,
                        "classification": "supported",
                        "rendered": True,
                        "fidelity": {"level": "full", "reasons": []},
                        "stacking_fidelity": "source_back_to_front",
                        "fidelity_reasons": [],
                        "diagnostic_codes": [],
                        "browser_preserved_scene_node_order": True,
                        "pages": 1,
                        "reference_raster_dpi": 144,
                        "page_geometry": [
                            {
                                "page_id": "page:1",
                                "order": 0,
                                "width_emu": 612 * 12700,
                                "height_emu": 792 * 12700,
                            }
                        ],
                        "screenshots": [
                            {
                                "page": 1,
                                "filename": candidate.name,
                                "sha256": sha256(candidate),
                            }
                        ],
                    }
                ],
            }
            cloud_receipt = cloud_dir / "receipt.json"
            cloud_receipt.write_text(
                json.dumps(receipt, indent=2, sort_keys=True) + "\n",
                encoding="utf-8",
            )

            summary = compare_bundle(
                cloud_receipt,
                registry_path,
                Path(temporary) / "pairs",
                Path(temporary) / "summary.json",
            )
            self.assertEqual(summary["compared_page_count"], 1)
            self.assertEqual(summary["unsupported_pair_count"], 0)
            self.assertEqual(summary["page_count_mismatch_pair_count"], 0)
            self.assertTrue(
                summary["ranked_pages"][0]["physical_page_size_matches_reference"]
            )
            self.assertEqual(summary["ranked_pages"][0]["significant_fraction"], 0.0)


if __name__ == "__main__":
    unittest.main()
