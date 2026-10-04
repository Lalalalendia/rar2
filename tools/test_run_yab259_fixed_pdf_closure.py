#!/usr/bin/env python3
from __future__ import annotations

import importlib.util
import pathlib
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

    def test_closure_command_delegates_to_canonical_runner(self) -> None:
        command = MODULE.closure_command(
            pathlib.Path("/tmp/yab"),
            fixture=pathlib.Path("fixture.pub"),
            pdf_output=pathlib.Path("out.pdf"),
            receipt_output=pathlib.Path("receipt.json"),
            fallback_font=None,
        )
        self.assertIn("run_local_fixed_pdf_shaped_flow.py", command[1])
        self.assertEqual(command.count("{fixture}"), 1)
        self.assertEqual(command.count("{pdf}"), 1)
        self.assertEqual(command.count("{font}"), 1)

    def test_bind_repository_requires_reachable_exact_donor(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            repository = pathlib.Path(tmp)
            (repository / "Cargo.toml").write_text("[workspace]\n", encoding="utf-8")
            with mock.patch.object(
                MODULE,
                "run_checked",
                side_effect=["true\n", ""],
            ) as checked:
                self.assertEqual(
                    MODULE.bind_yab_repository(repository),
                    repository.resolve(),
                )
            self.assertEqual(checked.call_count, 2)
            self.assertIn(
                f"{MODULE.YAB259_HEAD}^{{commit}}",
                checked.call_args_list[1].args[0],
            )

    def test_replace_once_is_fail_closed(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            path = pathlib.Path(tmp) / "source.rs"
            path.write_text("alpha beta gamma\n", encoding="utf-8")
            MODULE.replace_once(path, "beta", "delta", label="test repair")
            self.assertEqual(path.read_text(encoding="utf-8"), "alpha delta gamma\n")
            with self.assertRaisesRegex(MODULE.Yab259ClosureError, "anchor mismatch"):
                MODULE.replace_once(path, "beta", "epsilon", label="test repair")

    def test_repair_file_allowlist_is_bounded(self) -> None:
        self.assertEqual(
            MODULE.EXPECTED_REPAIR_FILES,
            {
                "crates/pub-viewer/src/lib.rs",
                "crates/pub-layout/src/shaped_flow.rs",
                "crates/pub-cli/src/fixed_pdf.rs",
                "crates/pub-pdf/src/text.rs",
            },
        )


if __name__ == "__main__":
    unittest.main()
