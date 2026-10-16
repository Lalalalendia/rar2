"""Source-free contracts for the browser-downloaded Story IDML/ODG consumer."""
from __future__ import annotations

import pathlib
import tempfile
import unittest
import zipfile

from verify_temporary_guest_story_export import safe_failure_code, verify


class StoryConsumerContracts(unittest.TestCase):
    def _zip(self, members: dict[str, bytes]) -> tuple[tempfile.TemporaryDirectory, pathlib.Path]:
        folder = tempfile.TemporaryDirectory()
        archive = pathlib.Path(folder.name) / "proof.zip"
        with zipfile.ZipFile(archive, "w", compression=zipfile.ZIP_DEFLATED) as z:
            for path, body in members.items():
                z.writestr(path, body)
        return folder, archive

    def test_valid_idml_receipt_has_exact_pass_status_expected_by_chromium(self):
        folder, archive = self._zip({
            "designmap.xml": b"<Document/>",
            "Stories/Story_u01.xml": (
                b"<idPkg:Story xmlns:idPkg='urn:adobe:ns:packaging'>"
                b"<Story><ParagraphStyleRange>"
                b"<CharacterStyleRange><Content>AuthoredProof42</Content>"
                b"</CharacterStyleRange></ParagraphStyleRange></Story></idPkg:Story>"
            ),
        })
        try:
            result = verify(archive, "idml", "AuthoredProof42")
            self.assertEqual(result["status"], "PASS")
            self.assertEqual(result["target"], "idml")
            self.assertTrue(result["edited_text_recovered"])
        finally:
            folder.cleanup()

    def test_valid_odg_receipt_has_same_pass_status(self):
        folder, archive = self._zip({
            "mimetype": b"application/vnd.oasis.opendocument.graphics",
            "content.xml": (
                b"<office:document-content "
                b"xmlns:office='urn:oasis:names:tc:opendocument:xmlns:office:1.0' "
                b"xmlns:text='urn:oasis:names:tc:opendocument:xmlns:text:1.0'>"
                b"<text:p>AuthoredProof42</text:p></office:document-content>"
            ),
        })
        try:
            result = verify(archive, "odg", "AuthoredProof42")
            self.assertEqual(result["status"], "PASS")
            self.assertTrue(result["target_story_xml_parsed"])
        finally:
            folder.cleanup()

    def test_no_authored_text_is_fail_closed(self):
        folder, archive = self._zip({
            "designmap.xml": b"<Document/>",
            "Stories/Story_u02.xml": b"<Story><Content>Different</Content></Story>",
        })
        try:
            with self.assertRaisesRegex(AssertionError, "marker absent"):
                verify(archive, "idml", "AuthoredProof42")
        finally:
            folder.cleanup()

    def test_failure_code_is_closed_enum_with_no_user_text(self):
        self.assertEqual(
            safe_failure_code(AssertionError("Scribus SLA reimport lost the edited canonical Story")),
            "scribus_story_missing",
        )
        self.assertEqual(
            safe_failure_code(AssertionError("PRIVATE_USER_TEXT and secret bearer")),
            "consumer_assertion_other",
        )
        self.assertEqual(
            safe_failure_code(ValueError("PRIVATE_USER_TEXT")),
            "consumer_unexpected_error",
        )


if __name__ == "__main__":
    unittest.main()
