#!/usr/bin/env python3
"""Negative checks for the Chaptera local development transport trust boundary."""
import pathlib
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

    def test_workflow_isolated(self):
        workflow = pathlib.Path(".github/workflows/ci-local-dev-transport-01.yml").read_text(encoding="utf-8")
        self.assertIn("chaptera-dev-unprivileged", workflow)
        self.assertNotIn("chaptera-publisher-oracle", workflow)
        self.assertNotIn("pull_request:", workflow)
        self.assertNotIn("issue_comment:", workflow)
        self.assertIn("persist-credentials: false", workflow)
        self.assertIn("github.ref == 'refs/heads/main'", workflow)
        self.assertIn("github.actor == 'Lalalalendia'", workflow)
        self.assertIn("private-logs", pathlib.Path("tools/ci/local_dev_transport.py").read_text(encoding="utf-8"))
        self.assertNotIn("actions: write", workflow)
        self.assertNotIn("contents: write", workflow)


if __name__ == "__main__":
    unittest.main()
