"""Offline contracts for allowlisted frozen Rescue engine dispatch."""
import contextlib
from io import StringIO
import json
import sys
import unittest
from unittest.mock import patch
from types import SimpleNamespace

from tools import rescue_engine_entrypoint as engine


class PortableEntrypointTests(unittest.TestCase):
    def test_module_allowlist_rejects_arbitrary_python_code(self):
        output = StringIO()
        with contextlib.redirect_stdout(output):
            self.assertEqual(engine.dispatch(["-m", "os", "--help"]), 2)
        self.assertEqual(json.loads(output.getvalue())["error"], "unsupported_child_module")

    def test_missing_child_module_rejected(self):
        with contextlib.redirect_stdout(StringIO()):
            self.assertEqual(engine.dispatch(["-m"]), 2)

    def test_batch_command_is_default(self):
        seen = []
        module = SimpleNamespace(__file__="frozen/batch.py", main=lambda: seen.append(sys.argv[:]) or 0)
        with patch.object(engine.importlib, "import_module", return_value=module) as loader:
            self.assertEqual(engine.dispatch(["scan", "--output", "C:/out"]), 0)
        loader.assert_called_once_with(engine.BATCH)
        self.assertEqual(seen[0][1:], ["scan", "--output", "C:/out"])

    def test_child_modules_use_same_argument_contract(self):
        for name in sorted(engine.ALLOWED_MODULES):
            seen = []
            module = SimpleNamespace(__file__="frozen/module.py", main=lambda: seen.append(sys.argv[:]) or 0)
            with self.subTest(module=name), patch.object(engine.importlib, "import_module", return_value=module):
                self.assertEqual(engine.dispatch(["-m", name, "input.pub", "--output", "C:/out"]), 0)
                self.assertEqual(seen[0][1:], ["input.pub", "--output", "C:/out"])

    def test_process_argv_restored_after_module_crash(self):
        old = sys.argv
        def raise_error():
            raise RuntimeError("simulated")
        module = SimpleNamespace(__file__="frozen/batch.py", main=raise_error)
        with patch.object(engine.importlib, "import_module", return_value=module):
            with self.assertRaisesRegex(RuntimeError, "simulated"):
                engine.dispatch(["run"])
        self.assertIs(sys.argv, old)

    def test_self_check_uses_import_list(self):
        output = StringIO()
        module = SimpleNamespace(__file__="module.py")
        with patch.object(engine.importlib, "import_module", return_value=module) as loader:
            with contextlib.redirect_stdout(output):
                self.assertEqual(engine.dispatch(["--self-check"]), 0)
        self.assertEqual(loader.call_count, len(engine.ALLOWED_MODULES))
        result = json.loads(output.getvalue())
        self.assertTrue(result["ok"])
        self.assertEqual(result["module_count"], 4)

    def test_self_check_fails_if_dependency_missing(self):
        with patch.object(engine.importlib, "import_module", side_effect=ImportError("missing")):
            with contextlib.redirect_stdout(StringIO()) as output:
                self.assertEqual(engine.dispatch(["--self-check"]), 2)
        self.assertFalse(json.loads(output.getvalue())["ok"])


if __name__ == "__main__":
    unittest.main()
