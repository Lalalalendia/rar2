#!/usr/bin/env python3
"""Bounded regression tests for Chaptera Local process startup ordering."""

from __future__ import annotations

from contextlib import ExitStack, nullcontext
from pathlib import Path
from tempfile import TemporaryDirectory
from types import SimpleNamespace
from unittest import TestCase
from unittest.mock import patch

import run_local_full_stack as local


class FakeProcess:
    def __init__(self, name: str, *, returncode: int | None = None) -> None:
        self.name = name
        self.returncode = returncode

    def poll(self) -> int | None:
        return self.returncode

    def terminate(self) -> None:
        self.returncode = 0

    def kill(self) -> None:
        self.returncode = -9

    def wait(self, timeout: int | None = None) -> int:
        if self.returncode is None:
            self.returncode = 0
        return self.returncode


class LocalStartupOrderTest(TestCase):
    def run_synthetic_smoke(
        self, *, server_exit: int | None = None, worker_exit: int | None = None
    ) -> tuple[int, list[str], list[str]]:
        events: list[str] = []
        failures: list[str] = []
        with TemporaryDirectory(prefix="chaptera-launch-order-") as directory:
            root = Path(directory) / "repo"
            binary = root / "target" / "debug" / "chaptera"
            binary.parent.mkdir(parents=True)
            binary.touch()
            state = root / ".chaptera-local"

            def popen(args: list[str], **_kwargs: object) -> FakeProcess:
                name = args[-1]
                if name not in ("serve", "worker"):
                    name = "editor"
                events.append(f"spawn:{name}")
                code = server_exit if name == "serve" else worker_exit if name == "worker" else None
                return FakeProcess(name, returncode=code)

            def readiness(url: str) -> int:
                self.assertEqual(url, local.URL + "/ready")
                events.append("server-ready")
                return 200

            def failure(message: str, *, launch_browser: bool) -> None:
                self.assertFalse(launch_browser)
                failures.append(message)

            replacements = {
                "ROOT": root,
                "STATE": state,
                "LOGS": state / "logs",
                "CONFIG": state / "chaptera.local.toml",
                "DATABASE": state / "chaptera.sqlite",
                "CREDENTIALS": state / "credentials",
                "SERVER_LOG": state / "logs" / "server.log",
                "WORKER_LOG": state / "logs" / "worker.log",
                "EDITOR_LOG": state / "logs" / "editor-service.log",
                "PACKAGED": False,
                "OidcFixture": lambda: nullcontext(SimpleNamespace(issuer="https://issuer.test")),
                "render_config": lambda _issuer: None,
                "run_checked": lambda _args, _env: None,
                "http_code": readiness,
                "open_failure_page": failure,
            }
            with ExitStack() as stack:
                for name, value in replacements.items():
                    stack.enter_context(patch.object(local, name, value))
                stack.enter_context(patch.object(local.subprocess, "Popen", popen))
                stack.enter_context(
                    patch.object(
                        local.urllib.request,
                        "urlopen",
                        lambda _url, timeout: SimpleNamespace(status=200),
                    )
                )
                stack.enter_context(
                    patch.object(
                        local.sys,
                        "argv",
                        ["chaptera-local", "--skip-build", "--smoke", "--no-browser"],
                    )
                )
                return local.main(), events, failures

    def test_worker_starts_only_after_real_server_ready(self) -> None:
        status, events, failures = self.run_synthetic_smoke()
        self.assertEqual(status, 0)
        self.assertEqual(
            events,
            ["spawn:serve", "server-ready", "spawn:worker", "spawn:editor"],
        )
        self.assertEqual(failures, [])

    def test_failed_server_does_not_start_worker_or_editor(self) -> None:
        status, events, failures = self.run_synthetic_smoke(server_exit=7)
        self.assertEqual(status, 1)
        self.assertEqual(events, ["spawn:serve"])
        self.assertEqual(len(failures), 1)
        self.assertIn("server exited during startup", failures[0])

    def test_failed_worker_does_not_start_editor(self) -> None:
        status, events, failures = self.run_synthetic_smoke(worker_exit=9)
        self.assertEqual(status, 1)
        self.assertEqual(events, ["spawn:serve", "server-ready", "spawn:worker"])
        self.assertEqual(len(failures), 1)
        self.assertIn("worker exited during startup", failures[0])
