#!/usr/bin/env python3
from __future__ import annotations

import pathlib
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[1]
ADAPTER = ROOT / "tools" / "yab259_fixed_pdf_packet_renderer.rs"
RUNNER = ROOT / "tools" / "run_yab259_fixed_pdf_packet_renderer.py"


class Yab259PacketRendererSourceTests(unittest.TestCase):
    def test_adapter_is_source_neutral_and_calls_pub_pdf_directly(self) -> None:
        text = ADAPTER.read_text(encoding="utf-8")
        self.assertIn("render_bounded_pdf", text)
        self.assertIn("BoundedResolvedScene", text)
        self.assertIn("FixedPdfResources", text)
        self.assertNotIn("pub_reader", text)
        self.assertNotIn("pub_viewer", text)
        self.assertNotIn("convert_pdf", text)
        self.assertNotIn("build_pdf_artifact", text)

    def test_adapter_echoes_opaque_binding(self) -> None:
        text = ADAPTER.read_text(encoding="utf-8")
        self.assertIn("binding: Value", text)
        self.assertIn("binding: request.binding", text)

    def test_adapter_emits_canonical_per_node_report(self) -> None:
        text = ADAPTER.read_text(encoding="utf-8")
        self.assertIn("struct RenderNodeReport", text)
        self.assertIn("origin_node_id", text)
        self.assertIn('"painted"', text)
        self.assertIn('"partial"', text)
        self.assertIn('"unsupported"', text)
        self.assertIn("node_reports.sort_by", text)


    def test_python_runner_uses_input_without_explicit_stdin_pipe(self) -> None:
        text = RUNNER.read_text(encoding="utf-8")
        self.assertIn("input=json.dumps(", text)
        self.assertNotIn("stdin=subprocess.PIPE", text)
        self.assertIn('encoding="utf-8"', text)


if __name__ == "__main__":
    unittest.main()
