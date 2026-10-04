#!/usr/bin/env python3
from __future__ import annotations

import importlib.util
import pathlib
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[1]
MODULE_PATH = ROOT / "tools" / "run_yab259_editor_packet_renderer.py"
SPEC = importlib.util.spec_from_file_location("run_yab259_editor_packet_renderer", MODULE_PATH)
assert SPEC and SPEC.loader
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class Yab259EditorPacketRendererTests(unittest.TestCase):
    def test_renderer_command_targets_only_injected_packet_binary(self) -> None:
        donor = pathlib.Path("/tmp/yab259")
        command = MODULE.renderer_command(donor)
        self.assertIn(str(donor / "Cargo.toml"), command)
        self.assertEqual(command[-2:], ["--bin", "editor_packet_renderer"])
        self.assertNotIn("convert", command)
        self.assertNotIn("pub", command[-1:])

    def test_check_command_targets_same_packet_binary(self) -> None:
        donor = pathlib.Path("/tmp/yab259")
        command = MODULE.check_command(donor)
        self.assertEqual(command[0:2], ["cargo", "check"])
        self.assertEqual(command[-2:], ["--bin", "editor_packet_renderer"])

    def test_install_packet_renderer_copies_rar_owned_thin_adapter(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            donor = pathlib.Path(tmp) / "donor"
            target = MODULE.install_packet_renderer(donor)
            self.assertEqual(
                target,
                donor
                / "crates"
                / "pub-cli"
                / "src"
                / "bin"
                / "editor_packet_renderer.rs",
            )
            self.assertEqual(target.read_bytes(), MODULE.ADAPTER_SOURCE.read_bytes())


if __name__ == "__main__":
    unittest.main()
