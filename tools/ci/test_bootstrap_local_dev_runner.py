#!/usr/bin/env python3
"""Offline adversarial tests for isolated Windows Actions runner bootstrap."""
import contextlib
import io
import json
from pathlib import Path
import stat
import sys
import tempfile
import unittest
from unittest import mock
import zipfile

sys.path.insert(0, str(Path(__file__).resolve().parent))
import bootstrap_local_dev_runner as setup


def release_asset(name="actions-runner-win-x64-2.400.0.zip",
                  digest="sha256:" + "a" * 64):
    return {"assets": [{
        "name": name,
        "digest": digest,
        "browser_download_url": (
            "https://github.com/actions/runner/releases/download/v2.400.0/" + name
        ),
    }]}


def make_zip(files):
    output = io.BytesIO()
    with zipfile.ZipFile(output, "w") as archive:
        for filename, value in files.items():
            archive.writestr(filename, value)
    output.seek(0)
    return output


class BootstrapTest(unittest.TestCase):
    def test_readonly_preflight(self):
        good = setup.readiness(
            os_name="Windows", architecture="AMD64", admin=False,
            python_version=(3, 12), binaries=set(setup.TOOLS), installed=False,
        )
        self.assertTrue(good["ready"])
        bad = setup.readiness(
            os_name="Linux", architecture="aarch64", admin=True,
            python_version=(3, 11), binaries=set(), installed=True,
        )
        self.assertFalse(bad["ready"])
        self.assertIn("administrator_not_allowed", bad["blockers"])
        self.assertIn("windows_x64_required", bad["blockers"])
        self.assertIn("missing_pwsh", bad["blockers"])
        self.assertIn("install_root_already_exists", bad["blockers"])
        self.assertEqual(good["runner_label"], "chaptera-dev-unprivileged")

    def test_no_credentials_or_downloads_in_default_main(self):
        with mock.patch.object(setup, "inspect_host", return_value=(
                {"schema": setup.SCHEMA, "ready": True, "blockers": []},
                Path("C:" + "/nonexistent"))):
            with mock.patch.object(setup, "install", side_effect=AssertionError("install executed")):
                with mock.patch.object(sys, "argv", ["bootstrap_local_dev_runner.py"]):
                    with contextlib.redirect_stdout(io.StringIO()) as stdout:
                        self.assertEqual(setup.main(), 0)
        data = json.loads(stdout.getvalue())
        self.assertTrue(data["ready"])

    def test_explicit_install_needs_isolation_acknowledgment(self):
        with mock.patch.object(setup, "inspect_host", return_value=(
                {"schema": setup.SCHEMA, "ready": True, "blockers": []},
                Path("C:" + "/nonexistent"))):
            with mock.patch.object(setup, "install", side_effect=AssertionError("install executed")):
                with mock.patch.object(sys, "argv", ["bootstrap_local_dev_runner.py", "--install"]):
                    with contextlib.redirect_stderr(io.StringIO()) as stderr:
                        self.assertEqual(setup.main(), 2)
        self.assertIn("separate_user_confirmation_required", stderr.getvalue())

    def test_release_requires_verified_official_digest_and_url(self):
        name, url, digest = setup.select_official_asset(release_asset())
        self.assertEqual(name, "actions-runner-win-x64-2.400.0.zip")
        self.assertEqual(digest, "a" * 64)
        self.assertTrue(url.startswith(setup.ASSET_PREFIX))
        for mutation in [
            {"assets": []},
            release_asset(digest="not-a-digest"),
            release_asset(digest="sha256:" + "z" * 64),
            release_asset(name="actions-runner-linux-x64-2.400.0.tar.gz"),
            release_asset(name="actions-runner-win-x64-2.400.0.zip?download=yes"),
        ]:
            with self.assertRaises(setup.BootstrapError):
                setup.select_official_asset(mutation)
        wrong = release_asset()
        wrong["assets"][0]["browser_download_url"] = "https://example.invalid/runner.zip"
        with self.assertRaises(setup.BootstrapError):
            setup.select_official_asset(wrong)

    def test_zip_traversal_symlink_and_duplicates(self):
        for path in ("../secret.txt", "/absolute.txt", "C:" + "/drive.txt",
                     "bin/../../escape", "bin/./dot.txt", "bin//double.txt",
                     "bin/trailing. "):
            with self.subTest(path=path):
                stream = make_zip({path: "payload"})
                with zipfile.ZipFile(stream) as archive:
                    with self.assertRaises(setup.BootstrapError):
                        setup.safe_archive_members(archive)
        stream = make_zip({"run.cmd": "ok", "RUN.CMD": "bad"})
        with zipfile.ZipFile(stream) as archive:
            with self.assertRaises(setup.BootstrapError):
                setup.safe_archive_members(archive)

        stream = io.BytesIO()
        with zipfile.ZipFile(stream, "w") as archive:
            link = zipfile.ZipInfo("private-link")
            link.create_system = 3
            link.external_attr = (stat.S_IFLNK | 0o777) << 16
            archive.writestr(link, "other")
        stream.seek(0)
        with zipfile.ZipFile(stream) as archive:
            with self.assertRaises(setup.BootstrapError):
                setup.safe_archive_members(archive)

    def test_extract_requires_both_official_entrypoints(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            zip_path = root / "package.zip"
            with zipfile.ZipFile(zip_path, "w") as archive:
                archive.writestr("config.cmd", "test")
            with self.assertRaises(setup.BootstrapError):
                setup.extract_runner(zip_path, root / "out")
            with zipfile.ZipFile(zip_path, "w") as archive:
                archive.writestr("config.cmd", "test")
                archive.writestr("run.cmd", "test")
            setup.extract_runner(zip_path, root / "out")
            self.assertTrue((root / "out" / "config.cmd").exists())

    def test_token_validation_masked_prompt(self):
        with mock.patch.object(setup.getpass, "getpass", return_value="valid_TOKEN_1234567890"):
            self.assertEqual(setup.prompt_registration_token(), "valid_TOKEN_1234567890")
        with mock.patch.object(setup.getpass, "getpass", return_value="bad token with space"):
            with self.assertRaises(setup.BootstrapError):
                setup.prompt_registration_token()
        with mock.patch.object(setup.getpass, "getpass", side_effect=EOFError):
            with self.assertRaises(setup.BootstrapError):
                setup.prompt_registration_token()

    def test_registration_never_outputs_ephemeral_token(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            completed = mock.Mock(returncode=1, stdout=b"secret", stderr=b"secret")
            with mock.patch.object(setup.subprocess, "run", return_value=completed) as call:
                with self.assertRaises(setup.BootstrapError) as exc:
                    setup.configure_runner(root, "valid_TOKEN_1234567890", "chaptera-dev-test")
            self.assertEqual(str(exc.exception), "runner_registration_failed")
            self.assertIn("--token", call.call_args.args[0])
            self.assertEqual(call.call_args.kwargs["cwd"], root)

    def test_install_does_not_enable_repository_variable(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary) / "Chaptera" / "DevRunner"

            def fake_extract(source, destination):
                (destination / "config.cmd").write_text("placeholder", encoding="utf-8")
                (destination / "run.cmd").write_text("placeholder", encoding="utf-8")

            with mock.patch.object(setup, "metadata_from_github", return_value=release_asset()), \
                 mock.patch.object(setup, "download_verified") as download, \
                 mock.patch.object(setup, "extract_runner", side_effect=fake_extract), \
                 mock.patch.object(setup, "prompt_registration_token", return_value="valid_TOKEN_1234567890"), \
                 mock.patch.object(setup, "configure_runner") as configured, \
                 mock.patch.object(setup, "register_user_autostart") as autostart, \
                 mock.patch.object(setup, "start_runner") as started:
                result = setup.install(root, autostart=True)
            self.assertTrue(root.exists())
            download.assert_called_once()
            configured.assert_called_once()
            autostart.assert_called_once()
            started.assert_called_once()
            self.assertFalse(result["github_online_verified"])
            self.assertFalse(result["workflow_activated"])
            self.assertNotIn("valid_TOKEN_1234567890", json.dumps(result))
            with self.assertRaises(setup.BootstrapError):
                setup.install(root, autostart=True)

    def test_distinct_runner_contract_and_hosted_tests(self):
        main_workflow = Path(".github/workflows/ci-local-dev-transport-01.yml").read_text(encoding="utf-8")
        hosted = Path(".github/workflows/dev-fast-loop-selftest.yml").read_text(encoding="utf-8")
        self.assertIn("chaptera-dev-unprivileged", main_workflow)
        self.assertNotIn("chaptera-publisher-oracle", main_workflow)
        self.assertIn("vars.CHAPTERA_LOCAL_DEV_RUNNER_ENABLED == 'true'", main_workflow)
        self.assertIn("tools/ci/test_bootstrap_local_dev_runner.py", hosted)
        self.assertNotIn("pull_request:", main_workflow)


if __name__ == "__main__":
    unittest.main()
