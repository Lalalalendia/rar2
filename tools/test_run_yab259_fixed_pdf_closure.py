#!/usr/bin/env python3
from __future__ import annotations

import importlib.util
import pathlib
import subprocess
import tempfile
import unittest
from unittest import mock

ROOT = pathlib.Path(__file__).resolve().parents[1]
MODULE_PATH = ROOT / "tools" / "run_yab259_fixed_pdf_closure.py"
SPEC = importlib.util.spec_from_file_location("run_yab259_fixed_pdf_closure", MODULE_PATH)
assert SPEC and SPEC.loader
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class Yab259ClosureTests(unittest.TestCase):
    def test_engine_command_is_exact_and_placeholder_safe(self) -> None:
        checkout = pathlib.Path("/tmp/yab")
        command = MODULE.engine_command(checkout)
        self.assertEqual(command.count("{fixture}"), 1)
        self.assertEqual(command.count("{pdf}"), 1)
        self.assertEqual(command.count("{font}"), 1)
        self.assertIn(str(checkout / "Cargo.toml"), command)
        self.assertEqual(command[-1], "--json")

    def test_bind_checkout_requires_exact_pr259_head(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            checkout = pathlib.Path(tmp)
            (checkout / "Cargo.toml").write_text("[workspace]\n", encoding="utf-8")
            completed = subprocess.CompletedProcess(
                args=[],
                returncode=0,
                stdout=MODULE.YAB259_HEAD + "\n",
                stderr="",
            )
            with mock.patch.object(MODULE.subprocess, "run", return_value=completed):
                self.assertEqual(MODULE.bind_yab_checkout(checkout), checkout.resolve())

    def test_bind_checkout_rejects_drift(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            checkout = pathlib.Path(tmp)
            (checkout / "Cargo.toml").write_text("[workspace]\n", encoding="utf-8")
            completed = subprocess.CompletedProcess(
                args=[],
                returncode=0,
                stdout="0" * 40 + "\n",
                stderr="",
            )
            with mock.patch.object(MODULE.subprocess, "run", return_value=completed):
                with self.assertRaisesRegex(MODULE.Yab259ClosureError, "HEAD mismatch"):
                    MODULE.bind_yab_checkout(checkout)


if __name__ == "__main__":
    unittest.main()
