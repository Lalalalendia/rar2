import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

TOOL = Path(__file__).with_name("security_leak_scan_v1.py")


class SecurityLeakScanV1Tests(unittest.TestCase):
    def run_tool(self, *args):
        return subprocess.run(
            [sys.executable, str(TOOL), *map(str, args)],
            check=False,
            text=True,
            capture_output=True,
        )

    def test_clean_receipt_passes_and_writes_summary(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            log = root / "chaptera.log"
            out = root / "receipt.json"
            log.write_text(
                "document=document:opaque result=accepted code=ok\n",
                encoding="utf-8",
            )
            result = self.run_tool(log, "--forbid-literal", "synthetic-secret", "--json-out", out)
            self.assertEqual(result.returncode, 0, result.stderr)
            receipt = json.loads(out.read_text(encoding="utf-8"))
            self.assertTrue(receipt["passed"])
            self.assertEqual(receipt["finding_count"], 0)
            self.assertEqual(receipt["files_scanned"], 1)

    def test_presigned_and_bearer_material_fail_closed(self):
        with tempfile.TemporaryDirectory() as temp:
            log = Path(temp) / "server.log"
            log.write_text(
                "GET https://bucket.example/object?X-Amz-Credential=ASIA1234567890123456&X-Amz-Signature=deadbeef\n"
                "Authorization: Bearer abcdefghijklmnopqrstuvwxyz.0123456789\n",
                encoding="utf-8",
            )
            result = self.run_tool(log)
            self.assertEqual(result.returncode, 1)
            receipt = json.loads(result.stdout)
            kinds = {row["kind"] for row in receipt["findings"]}
            self.assertIn("aws_presigned_signature", kinds)
            self.assertIn("bearer_token", kinds)

    def test_forbidden_literal_catches_synthetic_content_or_path(self):
        with tempfile.TemporaryDirectory() as temp:
            log = Path(temp) / "browser.log"
            log.write_text("error: synthetic-customer-canary\n", encoding="utf-8")
            result = self.run_tool(log, "--forbid-literal", "synthetic-customer-canary")
            self.assertEqual(result.returncode, 1)
            receipt = json.loads(result.stdout)
            self.assertEqual(receipt["findings"][0]["kind"], "forbidden_literal_0")

    def test_symlink_root_is_rejected(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            target = root / "real.log"
            target.write_text("clean\n", encoding="utf-8")
            link = root / "link.log"
            link.symlink_to(target)
            result = self.run_tool(link)
            self.assertEqual(result.returncode, 2)
            self.assertIn("refusing symlink input", result.stderr)


if __name__ == "__main__":
    unittest.main()
