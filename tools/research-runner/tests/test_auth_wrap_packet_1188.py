"""Offline immutable-identity / admitted-contract checks for the native experiment."""
import json
from pathlib import Path
import unittest

BASE = Path(__file__).resolve().parents[1]
PACKET = BASE / "experiments" / "auth-wrap-01.packet.json"
SCRIPT = BASE / "operations" / "auth_wrap_01.ps1"
LAUNCHER = BASE / "auth_wrap_direct_1188.ps1"


class PacketContract(unittest.TestCase):
    def test_source_is_exact_pinned_apache_poi_fixture(self):
        packet = json.loads(PACKET.read_text(encoding="utf-8"))
        self.assertEqual(packet["schema"], "pub-research-experiment.v1")
        self.assertEqual(packet["id"], "AUTH-WRAP-01")
        self.assertEqual(packet["fixture"]["sha256"],
                         "6a825ba26ba35d6e885acdc62e859591ed37cb0ff7480b554b9cb362b644dfcf")
        self.assertEqual(packet["fixture"]["source"], "runner-root")
        self.assertEqual(packet["publisher"]["exe_sha256"],
                         "e1ef8811b85b82045f37c4173b92726101be3a25e550b0dcb9f178df834ab20b")
        self.assertTrue(packet["reset"]["required"])
        self.assertTrue((Path(__file__).resolve().parents[3] / packet["operation"]["script"]).is_file())

    def test_arm_results_are_not_declared_layout_authority(self):
        op = SCRIPT.read_text(encoding="utf-8")
        direct = LAUNCHER.read_text(encoding="utf-8")
        self.assertIn("layout_outcome = 'not_observed'", op)
        self.assertIn("closure_candidate = $false", direct)
        self.assertIn("authority_class = 'inconclusive'", direct)
        self.assertIn("Verify_reset_receipt.py".lower(), direct.lower())
        self.assertIn("ExactGit -arguments @('hash-object'", direct)
        self.assertIn("Get-Process -Name MSPUB", direct)

    def test_launcher_pins_executable_shared_scripts(self):
        direct = LAUNCHER.read_text(encoding="utf-8")
        for filename in (
            "validate_packet.py", "prepare_native_run.ps1", "finalize_native_run.ps1",
            "verify_reset_receipt.py", "validate_auth_wrap_native_receipt.py",
            "PubRuntime.psm1", "native-receipt.schema.json",
            "pub-reader/Cargo.toml", "pub-reader/src/bin/auth_wrap_oracle_tool.rs",
        ):
            self.assertIn(filename, direct)

    def test_no_ambient_source_downloads(self):
        for path in (SCRIPT, LAUNCHER):
            script = path.read_text(encoding="utf-8").lower()
            for forbidden in ("invoke-webrequest", "start-bitstransfer", "downloadfile", "curl.exe"):
                self.assertNotIn(forbidden, script)


if __name__ == "__main__":
    unittest.main()
