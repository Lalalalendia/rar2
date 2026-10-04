#!/usr/bin/env python3
from __future__ import annotations

import pathlib
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[1]
RUNNER = ROOT / "tools" / "run_yab259_current_viewer_supported_subset_alpha_pdf.py"


class CarltonAlphaSmaskAbSourceTests(unittest.TestCase):
    def test_alpha_runner_reuses_same_mapper_and_renderer(self) -> None:
        text = RUNNER.read_text(encoding="utf-8")
        self.assertIn("yab259_current_viewer_supported_subset_request.rs", text)
        self.assertIn("yab259_fixed_pdf_packet_renderer.rs", text)
        self.assertIn("prepare_alpha_smask_pdf_donor", text)
        self.assertIn('"alpha_repair_sha256": alpha_digest', text)

    def test_alpha_runner_remains_source_neutral(self) -> None:
        text = RUNNER.read_text(encoding="utf-8")
        self.assertNotIn("pub_reader", text)
        self.assertNotIn("pub_viewer", text)
        self.assertNotIn("open_mature", text)
        self.assertNotIn("convert_pdf", text)
        self.assertNotIn("build_pdf_artifact", text)


if __name__ == "__main__":
    unittest.main()
