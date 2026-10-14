import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from print_composer_nup_output_v1 import NupOutputPublishError, publish_nup_outputs_v1


class OutputPublicationTests(unittest.TestCase):
    def setUp(self):
        self.t = tempfile.TemporaryDirectory()
        self.addCleanup(self.t.cleanup)
        self.root = Path(self.t.name)
        self.src = self.root / "source.pub"
        self.src.write_bytes(b"original-never-clobber")
        self.paths = [self.root / "sheet.pdf", self.root / "sheet.png", self.root / "receipt.json"]
        self.outputs = list(zip(self.paths, [b"%PDF-1.7", b"\x89PNG\r\n\x1a\n", b"{}\n"]))

    def publish(self, outputs=None):
        publish_nup_outputs_v1(inputs=(self.src,), outputs=self.outputs if outputs is None else outputs)

    def test_writes_all_three_outputs_without_mutating_source(self):
        self.publish()
        for path, payload in self.outputs:
            self.assertEqual(payload, path.read_bytes())
        self.assertEqual(b"original-never-clobber", self.src.read_bytes())

    def test_existing_destination_refused_without_writing_any_file(self):
        self.paths[1].write_bytes(b"preexisting")
        with self.assertRaisesRegex(NupOutputPublishError, "output_exists"):
            self.publish()
        self.assertFalse(self.paths[0].exists())
        self.assertEqual(b"preexisting", self.paths[1].read_bytes())
        self.assertFalse(self.paths[2].exists())

    def test_existing_symlink_is_never_followed(self):
        self.paths[0].symlink_to(self.src)
        with self.assertRaisesRegex(NupOutputPublishError, "(collision|exists)"):
            self.publish()
        self.assertEqual(b"original-never-clobber", self.src.read_bytes())

    def test_output_equal_to_source_is_rejected(self):
        with self.assertRaisesRegex(NupOutputPublishError, "path_collision"):
            self.publish([(self.src, b"MUTATION"), *self.outputs[1:]])
        self.assertEqual(b"original-never-clobber", self.src.read_bytes())

    def test_duplicate_outputs_are_rejected(self):
        with self.assertRaisesRegex(NupOutputPublishError, "path_collision"):
            self.publish([(self.paths[0], b"1"), (self.paths[0], b"2")])
        self.assertFalse(self.paths[0].exists())

    def test_missing_parent_no_partial_file(self):
        missing = self.root / "missing" / "sheet.png"
        with self.assertRaisesRegex(NupOutputPublishError, "parent_unavailable"):
            self.publish([(self.paths[0], b"1"), (missing, b"2")])
        self.assertFalse(self.paths[0].exists())

    def test_payload_must_be_bytes(self):
        with self.assertRaisesRegex(NupOutputPublishError, "payload_invalid"):
            self.publish([(self.paths[0], "private-text")])
        self.assertFalse(self.paths[0].exists())

    def test_unwritable_second_output_rolls_back_the_first(self):
        old_open = Path.open
        second = self.paths[1]
        def reject_second(path, *args, **kwargs):
            if path == second and args and args[0] == "xb":
                raise OSError("injected failure")
            return old_open(path, *args, **kwargs)
        with patch.object(Path, "open", new=reject_second):
            with self.assertRaises(OSError):
                self.publish()
        self.assertFalse(self.paths[0].exists())
        self.assertFalse(self.paths[1].exists())
        self.assertFalse(self.paths[2].exists())
        self.assertEqual(b"original-never-clobber", self.src.read_bytes())

    def test_partial_write_failure_rolls_back_current_file_too(self):
        old_fsync = os.fsync
        count = [0]
        def fail_second(fd):
            count[0] += 1
            if count[0] == 2:
                raise OSError("injected fsync failure")
            return old_fsync(fd)
        with patch("print_composer_nup_output_v1.os.fsync", side_effect=fail_second):
            with self.assertRaises(OSError):
                self.publish()
        self.assertFalse(any(p.exists() for p in self.paths))


if __name__ == "__main__":
    unittest.main()
