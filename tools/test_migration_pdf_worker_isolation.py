#!/usr/bin/env python3
import errno
import json
import os
import pathlib
import sys
import tempfile
import textwrap
import time
import unittest
from unittest import mock

ROOT = pathlib.Path(__file__).resolve().parents[1]
TOOLS = ROOT / "tools"
sys.path.insert(0, str(TOOLS))

from migration_pdf_worker_isolation import (
    WorkerLimits,
    run_isolated_batch,
    run_isolated_worker,
)


VALID_WORKER = r"""#!/usr/bin/env python3
import json
import os
import pathlib
import resource

out = pathlib.Path(os.environ["CHAPTERA_WORKER_OUTPUT_DIR"])
out.mkdir(parents=True, exist_ok=True)
(out / "document.pdf").write_bytes(b"%PDF-1.4\n% bounded test\n")
(out / "loss.json").write_text(json.dumps({"loss": []}), encoding="utf-8")
(out / "limits.json").write_text(json.dumps({
    "as": list(resource.getrlimit(resource.RLIMIT_AS)),
    "cpu": list(resource.getrlimit(resource.RLIMIT_CPU)),
    "nofile": list(resource.getrlimit(resource.RLIMIT_NOFILE)),
    "fsize": list(resource.getrlimit(resource.RLIMIT_FSIZE)),
    "network_policy": os.environ.get("CHAPTERA_NETWORK_POLICY"),
}), encoding="utf-8")
"""

NETWORK_PROBE = r"""#!/usr/bin/env python3
import errno
import json
import os
import pathlib
import socket

out = pathlib.Path(os.environ["CHAPTERA_WORKER_OUTPUT_DIR"])
out.mkdir(parents=True, exist_ok=True)
attempts = {}
# IPv4 loopback, IPv4 private destinations, IPv6 loopback, and local Unix
# sockets must all be blocked before any connection can be attempted.
for label, family in [
    ("ipv4_loopback", socket.AF_INET),
    ("ipv4_private", socket.AF_INET),
    ("ipv6_loopback", socket.AF_INET6),
    ("unix_stream", socket.AF_UNIX),
]:
    try:
        client = socket.socket(family, socket.SOCK_STREAM)
    except OSError as exc:
        attempts[label] = exc.errno
    else:
        attempts[label] = 0
        client.close()
try:
    pair = socket.socketpair(socket.AF_UNIX, socket.SOCK_STREAM)
except OSError as exc:
    attempts["unix_socketpair"] = exc.errno
else:
    attempts["unix_socketpair"] = 0
    for client in pair:
        client.close()

blocked = all(number == errno.EPERM for number in attempts.values())
(out / "network.json").write_text(
    json.dumps({"blocked": blocked, "attempts": attempts}), encoding="utf-8"
)
raise SystemExit(0 if blocked else 9)
"""

HARDLINK_WORKER = r"""#!/usr/bin/env python3
import os
import pathlib

out = pathlib.Path(os.environ["CHAPTERA_WORKER_OUTPUT_DIR"])
out.mkdir(parents=True, exist_ok=True)
os.link(os.environ["CHAPTERA_WORKER_INPUT"], out / "private-bytes.txt")
"""


PROCESS_GROUP_ESCAPE_WORKER = r"""#!/usr/bin/env python3
import errno
import json
import os
import pathlib

out = pathlib.Path(os.environ["CHAPTERA_WORKER_OUTPUT_DIR"])
out.mkdir(parents=True, exist_ok=True)
child = os.fork()
if child == 0:
    observations = {}
    for name, call in [
        ("setsid", os.setsid),
        ("setpgid", lambda: os.setpgid(0, 0)),
    ]:
        try:
            call()
        except OSError as exc:
            observations[name] = exc.errno
        else:
            observations[name] = 0
    (out / "escape.json").write_text(json.dumps(observations), encoding="utf-8")
    os._exit(0)
os.waitpid(child, 0)
"""


SURVIVING_CHILD_WORKER = r"""#!/usr/bin/env python3
import os
import pathlib
import time

out = pathlib.Path(os.environ["CHAPTERA_WORKER_OUTPUT_DIR"])
out.mkdir(parents=True, exist_ok=True)
(out / "approved.txt").write_bytes(b"approved")
directory_fd = os.open(out, os.O_RDONLY | os.O_DIRECTORY)
if os.fork() == 0:
    # This directory FD remains valid after staging is renamed to published.
    time.sleep(0.7)
    late_fd = os.open(
        "late.txt", os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600, dir_fd=directory_fd
    )
    os.write(late_fd, b"modified-after-publish")
    os.close(late_fd)
    os._exit(0)
os.close(directory_fd)
"""


FAIL_AFTER_PARTIAL = r"""#!/usr/bin/env python3
import os
import pathlib

out = pathlib.Path(os.environ["CHAPTERA_WORKER_OUTPUT_DIR"])
out.mkdir(parents=True, exist_ok=True)
(out / "partial.pdf").write_bytes(b"partial")
raise SystemExit(23)
"""

SLEEP_WORKER = r"""#!/usr/bin/env python3
import os
import pathlib
import time

time.sleep(10)
out = pathlib.Path(os.environ["CHAPTERA_WORKER_OUTPUT_DIR"])
out.mkdir(parents=True, exist_ok=True)
(out / "late.pdf").write_bytes(b"late")
"""

ENV_PROBE = r"""#!/usr/bin/env python3
import json
import os
import pathlib

out = pathlib.Path(os.environ["CHAPTERA_WORKER_OUTPUT_DIR"])
out.mkdir(parents=True, exist_ok=True)
(out / "env.json").write_text(json.dumps({
    "secret_present": "CHAPTERA_TEST_SECRET" in os.environ,
    "explicit_value": os.environ.get("CHAPTERA_SCAN_MODE"),
    "network_policy": os.environ.get("CHAPTERA_NETWORK_POLICY"),
    "input_present": "CHAPTERA_WORKER_INPUT" in os.environ,
}), encoding="utf-8")
"""


@unittest.skipUnless(sys.platform == "linux", "Linux security slice")
class MigrationPdfWorkerIsolationTests(unittest.TestCase):
    def write_worker(self, root, name, content):
        path = pathlib.Path(root) / name
        path.write_text(content, encoding="utf-8")
        return path

    def limits(self):
        return WorkerLimits(
            address_space_bytes=384 * 1024 * 1024,
            cpu_seconds=5,
            open_files=48,
            output_file_bytes=8 * 1024 * 1024,
        )

    def test_valid_worker_publishes_only_after_success_and_inherits_limits(self):
        with tempfile.TemporaryDirectory() as tmp:
            tmp = pathlib.Path(tmp)
            worker = self.write_worker(tmp, "valid.py", VALID_WORKER)
            final = tmp / "published"
            result = run_isolated_worker(
                [sys.executable, str(worker)],
                final_output_dir=final,
                timeout_seconds=5,
                limits=self.limits(),
            )

            self.assertTrue(result.succeeded)
            self.assertFalse(result.timed_out)
            self.assertEqual("seccomp_default_deny", result.network_policy)
            self.assertEqual(
                ("document.pdf", "limits.json", "loss.json"),
                result.outputs,
            )
            self.assertTrue((final / "document.pdf").is_file())
            limits = json.loads((final / "limits.json").read_text(encoding="utf-8"))
            self.assertEqual([384 * 1024 * 1024, 384 * 1024 * 1024], limits["as"])
            self.assertEqual([5, 6], limits["cpu"])
            self.assertEqual([48, 48], limits["nofile"])
            self.assertEqual([8 * 1024 * 1024, 8 * 1024 * 1024], limits["fsize"])
            self.assertEqual("seccomp_default_deny", limits["network_policy"])

    def test_network_socket_creation_is_denied_before_worker_executes_payload(self):
        with tempfile.TemporaryDirectory() as tmp:
            tmp = pathlib.Path(tmp)
            worker = self.write_worker(tmp, "network_probe.py", NETWORK_PROBE)
            final = tmp / "network-result"
            result = run_isolated_worker(
                [sys.executable, str(worker)],
                final_output_dir=final,
                timeout_seconds=5,
                limits=self.limits(),
            )

            self.assertTrue(result.succeeded, result.stderr_tail)
            receipt = json.loads((final / "network.json").read_text(encoding="utf-8"))
            self.assertTrue(receipt["blocked"])
            self.assertEqual(
                {
                    "ipv4_loopback": errno.EPERM,
                    "ipv4_private": errno.EPERM,
                    "ipv6_loopback": errno.EPERM,
                    "unix_stream": errno.EPERM,
                    "unix_socketpair": errno.EPERM,
                },
                receipt["attempts"],
            )

    def test_clear_environment_drops_parent_secrets_but_keeps_explicit_worker_contract(self):
        with tempfile.TemporaryDirectory() as tmp:
            tmp = pathlib.Path(tmp)
            worker = self.write_worker(tmp, "env_probe.py", ENV_PROBE)
            source = tmp / "source.pub"
            source.write_bytes(b"pub")
            final = tmp / "env-result"

            with mock.patch.dict(
                os.environ,
                {"CHAPTERA_TEST_SECRET": "must-not-leak"},
                clear=False,
            ):
                result = run_isolated_worker(
                    [sys.executable, str(worker)],
                    final_output_dir=final,
                    timeout_seconds=5,
                    limits=self.limits(),
                    input_path=source,
                    extra_env={"CHAPTERA_SCAN_MODE": "cloud-source"},
                    inherit_environment=False,
                )

            self.assertTrue(result.succeeded, result.stderr_tail)
            receipt = json.loads((final / "env.json").read_text(encoding="utf-8"))
            self.assertFalse(receipt["secret_present"])
            self.assertEqual("cloud-source", receipt["explicit_value"])
            self.assertEqual("seccomp_default_deny", receipt["network_policy"])
            self.assertTrue(receipt["input_present"])

    def test_inherited_worker_authority_keys_are_reset_before_child_launch(self):
        with tempfile.TemporaryDirectory() as tmp:
            tmp = pathlib.Path(tmp)
            worker = self.write_worker(tmp, "env_probe.py", ENV_PROBE)
            final = tmp / "published"
            foreign_output = tmp / "outside-staging"
            with mock.patch.dict(
                os.environ,
                {
                    "CHAPTERA_WORKER_INPUT": str(tmp / "unapproved-private-input.pub"),
                    "CHAPTERA_WORKER_OUTPUT_DIR": str(foreign_output),
                    "CHAPTERA_NETWORK_POLICY": "allow",
                },
                clear=False,
            ):
                result = run_isolated_worker(
                    [sys.executable, str(worker)],
                    final_output_dir=final,
                    timeout_seconds=5,
                    limits=self.limits(),
                    inherit_environment=True,
                )
            self.assertTrue(result.succeeded, result.stderr_tail)
            receipt = json.loads((final / "env.json").read_text(encoding="utf-8"))
            self.assertFalse(receipt["input_present"])
            self.assertEqual("seccomp_default_deny", receipt["network_policy"])
            self.assertFalse(foreign_output.exists())

    def test_extra_env_cannot_replace_harness_owned_source_output_or_network_contract(self):
        with tempfile.TemporaryDirectory() as tmp:
            tmp = pathlib.Path(tmp)
            worker = self.write_worker(tmp, "env_probe.py", ENV_PROBE)
            final = tmp / "published"
            for key in [
                "CHAPTERA_WORKER_INPUT",
                "CHAPTERA_WORKER_OUTPUT_DIR",
                "CHAPTERA_NETWORK_POLICY",
            ]:
                with self.subTest(key=key):
                    with self.assertRaisesRegex(ValueError, "harness-owned"):
                        run_isolated_worker(
                            [sys.executable, str(worker)],
                            final_output_dir=final,
                            timeout_seconds=5,
                            limits=self.limits(),
                            extra_env={key: str(tmp / "unapproved")},
                            inherit_environment=False,
                        )
                    self.assertFalse(final.exists())
                    self.assertEqual(list(tmp.glob(".published.stage-*")), [])
                    self.assertEqual(list(tmp.glob(".published.control-*")), [])

    def test_forced_zero_timeout_never_publishes_or_runs_worker(self):
        with tempfile.TemporaryDirectory() as tmp:
            tmp = pathlib.Path(tmp)
            marker = tmp / "should-not-exist"
            worker = self.write_worker(
                tmp,
                "marker.py",
                textwrap.dedent(
                    f"""\
                    import pathlib
                    pathlib.Path({str(marker)!r}).write_text("ran")
                    """
                ),
            )
            final = tmp / "zero-timeout"
            result = run_isolated_worker(
                [sys.executable, str(worker)],
                final_output_dir=final,
                timeout_seconds=0,
                limits=self.limits(),
            )

            self.assertEqual("timeout", result.status)
            self.assertTrue(result.timed_out)
            self.assertTrue(result.staging_cleaned)
            self.assertFalse(final.exists())
            self.assertFalse(marker.exists())

    def test_wall_timeout_kills_worker_group_and_removes_partial_staging(self):
        with tempfile.TemporaryDirectory() as tmp:
            tmp = pathlib.Path(tmp)
            worker = self.write_worker(tmp, "sleep.py", SLEEP_WORKER)
            final = tmp / "timeout-result"
            result = run_isolated_worker(
                [sys.executable, str(worker)],
                final_output_dir=final,
                timeout_seconds=0.2,
                limits=self.limits(),
            )

            self.assertEqual("timeout", result.status)
            self.assertTrue(result.timed_out)
            self.assertTrue(result.staging_cleaned)
            self.assertFalse(final.exists())

    def test_forked_worker_cannot_detach_from_parent_process_group(self):
        with tempfile.TemporaryDirectory() as tmp:
            tmp = pathlib.Path(tmp)
            worker = self.write_worker(tmp, "detach.py", PROCESS_GROUP_ESCAPE_WORKER)
            final = tmp / "escape-result"
            result = run_isolated_worker(
                [sys.executable, str(worker)],
                final_output_dir=final,
                timeout_seconds=5,
                limits=self.limits(),
                inherit_environment=False,
            )
            self.assertTrue(result.succeeded, result.stderr_tail)
            observations = json.loads((final / "escape.json").read_text(encoding="utf-8"))
            self.assertEqual(
                {"setsid": errno.EPERM, "setpgid": errno.EPERM}, observations
            )

    def test_successful_leader_cannot_leave_child_to_mutate_published_tree(self):
        with tempfile.TemporaryDirectory() as tmp:
            tmp = pathlib.Path(tmp)
            worker = self.write_worker(tmp, "forked.py", SURVIVING_CHILD_WORKER)
            final = tmp / "published"
            result = run_isolated_worker(
                [sys.executable, str(worker)],
                final_output_dir=final,
                timeout_seconds=5,
                limits=self.limits(),
                inherit_environment=False,
            )

            self.assertTrue(result.succeeded, result.stderr_tail)
            self.assertEqual(("approved.txt",), result.outputs)
            self.assertEqual(b"approved", (final / "approved.txt").read_bytes())
            # The child had an open directory descriptor to the renamed
            # staging tree. It must be killed BEFORE promotion, not only
            # after a timeout or while the original leader is still alive.
            time.sleep(1.0)
            self.assertEqual(["approved.txt"], [p.name for p in final.iterdir()])
            self.assertEqual(list(tmp.glob(".published.stage-*")), [])

    def test_hard_link_to_outside_input_is_never_published(self):
        with tempfile.TemporaryDirectory() as tmp:
            tmp = pathlib.Path(tmp)
            private_source = tmp / "private-source.txt"
            private_source.write_bytes(b"private source content - not a browser resource")
            worker = self.write_worker(tmp, "hardlink.py", HARDLINK_WORKER)
            final = tmp / "published"
            with self.assertRaisesRegex(RuntimeError, "hard link"):
                run_isolated_worker(
                    [sys.executable, str(worker)],
                    final_output_dir=final,
                    input_path=private_source,
                    timeout_seconds=5,
                    limits=self.limits(),
                    inherit_environment=False,
                )

            self.assertFalse(final.exists())
            self.assertEqual(
                private_source.read_bytes(),
                b"private source content - not a browser resource",
            )
            self.assertEqual(private_source.stat().st_nlink, 1)
            self.assertEqual(list(tmp.glob(".published.stage-*")), [])

    def test_failed_file_does_not_publish_partial_output_and_later_job_still_runs(self):
        with tempfile.TemporaryDirectory() as tmp:
            tmp = pathlib.Path(tmp)
            bad = self.write_worker(tmp, "bad.py", FAIL_AFTER_PARTIAL)
            good = self.write_worker(tmp, "good.py", VALID_WORKER)
            bad_final = tmp / "bad-output"
            good_final = tmp / "good-output"

            rows = run_isolated_batch(
                [
                    {
                        "name": "malformed",
                        "command": [sys.executable, str(bad)],
                        "final_output_dir": bad_final,
                    },
                    {
                        "name": "valid-after-malformed",
                        "command": [sys.executable, str(good)],
                        "final_output_dir": good_final,
                    },
                ],
                timeout_seconds=5,
                limits=self.limits(),
            )

            self.assertEqual("failed", rows[0]["status"])
            self.assertEqual(23, rows[0]["exit_code"])
            self.assertTrue(rows[0]["staging_cleaned"])
            self.assertFalse(bad_final.exists())
            self.assertEqual("success", rows[1]["status"])
            self.assertTrue((good_final / "document.pdf").is_file())

    def test_output_file_limit_fails_closed_without_publication(self):
        oversized = r"""#!/usr/bin/env python3
import os
import pathlib
out = pathlib.Path(os.environ["CHAPTERA_WORKER_OUTPUT_DIR"])
out.mkdir(parents=True, exist_ok=True)
with (out / "too-large.bin").open("wb") as fh:
    fh.write(b"x" * (2 * 1024 * 1024))
"""
        with tempfile.TemporaryDirectory() as tmp:
            tmp = pathlib.Path(tmp)
            worker = self.write_worker(tmp, "oversized.py", oversized)
            final = tmp / "oversized-output"
            limits = WorkerLimits(
                address_space_bytes=384 * 1024 * 1024,
                cpu_seconds=5,
                open_files=48,
                output_file_bytes=1024 * 1024,
            )
            result = run_isolated_worker(
                [sys.executable, str(worker)],
                final_output_dir=final,
                timeout_seconds=5,
                limits=limits,
            )
            self.assertEqual("failed", result.status)
            self.assertTrue(result.staging_cleaned)
            self.assertFalse(final.exists())


if __name__ == "__main__":
    unittest.main()
