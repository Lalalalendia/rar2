"""Offline immutable-identity / admitted-contract checks for the native experiment."""
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

BASE = Path(__file__).resolve().parents[1]
PACKET = BASE / "experiments" / "auth-wrap-01.packet.json"
SCRIPT = BASE / "operations" / "auth_wrap_01.ps1"
LAUNCHER = BASE / "auth_wrap_direct_1188.ps1"
CHILD = BASE / "auth_wrap_arm_1188.ps1"
REPO = BASE.parents[1]
OPERATION = BASE / "operations" / "auth_wrap_01.ps1"


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
            "auth_wrap_arm_1188.ps1",
            "PubRuntime.psm1", "native-receipt.schema.json",
            "pub-reader/Cargo.toml", "pub-reader/src/bin/auth_wrap_oracle_tool.rs",
        ):
            self.assertIn(filename, direct)

    def test_native_environment_handoff_is_same_process_and_revalidated(self):
        direct = LAUNCHER.read_text(encoding="utf-8")
        child = CHILD.read_text(encoding="utf-8")
        self.assertIn("PUB_RESEARCH_AUTH_WRAP_CHILD_SCRIPT", direct)
        self.assertIn("Start-Process -FilePath (Get-Command pwsh).Source", direct)
        for name in ("prepare_native_run.ps1", "operations/auth_wrap_01.ps1",
                     "finalize_native_run.ps1"):
            self.assertIn(name, child)
        for name in ("PUB_RESEARCH_FIXTURE", "PUB_RESEARCH_PUBLISHER_EXE"):
            self.assertIn(f"Remove-Item Env:{name}", child)
            self.assertIn(f"$env:{name}", child)
        self.assertIn("Prepared fixture binding missing or changed after native preflight", child)
        self.assertIn("Prepared Publisher executable binding missing or changed after native preflight", child)

    def test_native_watchdog_is_bounded_and_owner_only(self):
        direct = LAUNCHER.read_text(encoding="utf-8")
        self.assertIn("Global\\Chaptera_AUTH_WRAP_01", direct)
        self.assertIn("$lease.WaitOne(0)", direct)
        self.assertIn("$child.WaitForExit(600000)", direct)
        self.assertIn("$child.Kill()", direct)
        self.assertIn("timeout_requires_manual_snapshot_restore", direct)
        self.assertIn("auth_wrap_arm_1188.ps1", direct)
        self.assertNotIn("Stop-Process -Name MSPUB", direct)
        self.assertNotIn("Stop-Process -Id $publisherProcess.Id", direct)

    def test_safe_native_admission_guards_are_present(self):
        launcher = LAUNCHER.read_text(encoding="utf-8")
        child = CHILD.read_text(encoding="utf-8")
        operation = OPERATION.read_text(encoding="utf-8")
        self.assertIn("[switch]$AcknowledgeIsolatedLab", launcher)
        self.assertIn("requires explicit -AcknowledgeIsolatedLab", launcher)
        self.assertIn("cargo metadata --offline --no-deps", launcher)
        self.assertIn("Publisher active after reset provider", child)
        self.assertEqual(operation.count("    RequirePublisherNoAmbientInstance\n    $app = New-PubPublisherApplication"), 2)

    @unittest.skipUnless(shutil.which("pwsh"), "PowerShell 7 is unavailable")
    def test_missing_native_acknowledgement_fails_without_output_or_publisher(self):
        head = subprocess.check_output(["git", "-C", str(REPO), "rev-parse", "HEAD"],
                                       text=True).strip()
        with tempfile.TemporaryDirectory() as tmp:
            out = Path(tmp) / "must-not-be-created"
            result = subprocess.run(
                ["pwsh", "-NoProfile", "-File", str(LAUNCHER),
                 "-TrustedCommitSha", head, "-OutputRoot", str(out)],
                cwd=REPO, capture_output=True, text=True, check=False, timeout=90,
            )
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("requires explicit -AcknowledgeIsolatedLab",
                          result.stdout + result.stderr)
            self.assertFalse(out.exists())

    def test_no_ambient_source_downloads(self):
        for path in (SCRIPT, LAUNCHER, CHILD):
            script = path.read_text(encoding="utf-8").lower()
            for forbidden in ("invoke-webrequest", "start-bitstransfer", "downloadfile", "curl.exe"):
                self.assertNotIn(forbidden, script)


if __name__ == "__main__":
    unittest.main()
