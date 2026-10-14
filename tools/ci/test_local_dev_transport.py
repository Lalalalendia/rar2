#!/usr/bin/env python3
"""Negative checks for the Chaptera local development transport trust boundary."""
import argparse
import json
import pathlib
import subprocess
import sys
import tempfile
import unittest

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import local_dev_transport as transport


class LocalTransportTest(unittest.TestCase):
    def test_admission(self):
        for event in ("schedule", "push", "workflow_dispatch"):
            transport.admit(repository=transport.REPOSITORY, ref="refs/heads/main",
                            event=event, actor=transport.OWNER)
        for options in (
            dict(repository="someone/rar2", ref="refs/heads/main", event="push", actor=transport.OWNER),
            dict(repository=transport.REPOSITORY, ref="refs/heads/topic", event="push", actor=transport.OWNER),
            dict(repository=transport.REPOSITORY, ref="refs/heads/main", event="pull_request", actor=transport.OWNER),
            dict(repository=transport.REPOSITORY, ref="refs/heads/main", event="issue_comment", actor=transport.OWNER),
            dict(repository=transport.REPOSITORY, ref="refs/heads/main", event="workflow_dispatch", actor="attacker"),
        ):
            with self.assertRaises(transport.TransportError):
                transport.admit(**options)

    def test_state_fail_closed(self):
        with tempfile.TemporaryDirectory() as temp:
            state = pathlib.Path(temp) / "state.json"
            initial = transport.load_state(state)
            self.assertIsNone(initial["last_success"])
            state.write_text('{"schema":"wrong","last_success":null}')
            with self.assertRaises(transport.TransportError):
                transport.load_state(state)
            state.write_text('{"schema":"chaptera.local-dev-transport.v1","last_seen":"oops","last_success":null}')
            with self.assertRaises(transport.TransportError):
                transport.load_state(state)

    def test_sha(self):
        transport.require_sha("a" * 40)
        for bad in ("main", "A" * 40, "a" * 39, "../../x", ""):
            with self.assertRaises(transport.TransportError):
                transport.require_sha(bad)


    def test_unauthorized_event_never_reads_checkout(self):
        args = argparse.Namespace(
            sha="a" * 40, run_id="123", profile="edit",
            repo=pathlib.Path("/some-private-source-folder"),
            state_root=pathlib.Path("/another-private-location"),
            logs_root=pathlib.Path("/more-private-logs"),
            receipt=pathlib.Path("/private-receipt.json"), force=False,
        )
        exit_code, report = transport.execute(args, {
            "GITHUB_REPOSITORY": transport.REPOSITORY,
            "GITHUB_REF": "refs/heads/main",
            "GITHUB_EVENT_NAME": "pull_request",
            "GITHUB_ACTOR": transport.OWNER,
            "GITHUB_SHA": "a" * 40,
        })
        self.assertEqual(exit_code, 2)
        self.assertEqual(report["error_code"], "untrusted_event")
        self.assertTrue(report["source_safe"])
        self.assertNotIn("/some-private-source-folder", json.dumps(report))
        self.assertNotIn("/another-private-location", json.dumps(report))

    def test_baseline_detects_deleted_sources_and_history(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = pathlib.Path(temporary)
            def call(*args):
                return subprocess.check_output(["git", "-C", str(root), *args], text=True).strip()
            call("init", "-q")
            call("config", "user.name", "CI test")
            call("config", "user.email", "ci-test@example.invalid")
            (root / "removed.rs").write_text("pub fn value() -> i32 { 1 }\n", encoding="utf-8")
            call("add", "--all")
            call("commit", "-qm", "initial")
            base = call("rev-parse", "HEAD")
            (root / "removed.rs").unlink()
            (root / "created.rs").write_text("pub fn value() -> i32 { 2 }\n", encoding="utf-8")
            call("add", "--all")
            call("commit", "-qm", "next")
            head = call("rev-parse", "HEAD")
            self.assertEqual(transport.baseline(root, head, base),
                             (base, "since_last_success"))
            self.assertEqual(transport.baseline(root, head, None),
                             (base, "bootstrap_last_commit_only"))
            self.assertEqual(transport.changed_paths(root, base, head),
                             ["created.rs", "removed.rs"])

    def test_atomic_checkpoint_roundtrip(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = pathlib.Path(temporary) / "checkpoint.json"
            payload = {"schema": transport.SCHEMA,
                       "last_seen": "1" * 40, "last_success": "1" * 40}
            transport.atomic_json(path, payload)
            self.assertEqual(transport.load_state(path), payload)

    def test_workflow_isolated(self):
        workflow = pathlib.Path(".github/workflows/ci-local-dev-transport-01.yml").read_text(encoding="utf-8")
        self.assertIn("chaptera-dev-unprivileged", workflow)
        self.assertNotIn("chaptera-publisher-oracle", workflow)
        self.assertNotIn("  pull_request:", workflow)
        self.assertIn("cancel-in-progress: false", workflow)
        hosted = pathlib.Path(".github/workflows/dev-fast-loop-selftest.yml").read_text(encoding="utf-8")
        self.assertIn("tools/ci/test_local_dev_transport.py", hosted)
        self.assertIn("python -m unittest discover -s tools/ci -p test_local_dev_transport.py", hosted)
        self.assertIn("fetch-depth: 0", workflow)
        self.assertIn("--logs-root", workflow)
        self.assertIn("        env:\n          CHAPTERA_LOCAL_DEV_STATE:", workflow)
        self.assertNotIn("    env:\n      CHAPTERA_LOCAL_DEV_STATE:", workflow)
        self.assertNotIn("issue_comment:", workflow)
        self.assertIn("persist-credentials: false", workflow)
        self.assertIn("github.ref == 'refs/heads/main'", workflow)
        self.assertIn("github.actor == 'Lalalalendia'", workflow)
        self.assertIn("private-logs", pathlib.Path("tools/ci/local_dev_transport.py").read_text(encoding="utf-8"))
        self.assertIn("github.event_name == 'schedule'", workflow)
        self.assertIn("github.event_name == 'push'", workflow)
        self.assertNotIn("actions: write", workflow)
        self.assertNotIn("contents: write", workflow)


if __name__ == "__main__":
    unittest.main()
