"""CI-only safety tests: private Guest bearer never appears in public Caddy artifacts."""
import json
import pathlib
import tempfile
import unittest

from run_temporary_editor_real_pub_browser_acceptance import (
    _cgroup_memory_events,
    _redact_caddy_log,
)


class TestBrowserLogSafety(unittest.TestCase):
    def test_scrubs_guest_bearer_and_csrf_while_retaining_reset_evidence(self):
        with tempfile.TemporaryDirectory() as directory:
            path = pathlib.Path(directory) / "caddy.log"
            token, csrf = "bearer-example-private-123", "csrf-example-private-456"
            record = {
                "level": "warn",
                "logger": "http.handlers.reverse_proxy",
                "msg": "aborting with incomplete response",
                "error": "reading: connection reset by peer",
                "request": {
                    "method": "POST",
                    "uri": "/v1/reader/guest-sessions/{session}/open?secret=" + token,
                    "referer": "https://example.invalid/editor?csrf=" + csrf,
                    "headers": {
                        "X-Chaptera-Reader-Session": [token],
                        "X-Csrf-Token": [csrf],
                        "Authorization": ["Bearer hidden"],
                        "Cookie": ["session=hidden"],
                        "Content-Type": ["application/json"],
                    },
                },
            }
            path.write_text(json.dumps(record) + "\n" +
                            json.dumps(token) + "\n" +
                            "unexpected broken token line " + token + "\n")
            _redact_caddy_log(path)
            result = path.read_text()
            self.assertNotIn(token, result)
            self.assertNotIn(csrf, result)
            self.assertNotIn("Bearer hidden", result)
            self.assertNotIn("session=hidden", result)
            self.assertIn("connection reset by peer", result)
            self.assertIn("aborting with incomplete response", result)
            self.assertIn("application/json", result)
            self.assertIn("unparseable Caddy diagnostic suppressed", result)
            clean_request = json.loads(result.splitlines()[0])["request"]
            self.assertEqual(clean_request["uri"], "[redacted]")
            self.assertEqual(clean_request["referer"], "[redacted]")
            self.assertEqual(json.loads(result.splitlines()[0])
                             ["request"]["headers"]["X-Chaptera-Reader-Session"], "[redacted]")

    def test_only_numeric_oom_counters_are_exposed(self):
        with tempfile.TemporaryDirectory() as directory:
            path = pathlib.Path(directory) / "memory.events"
            path.write_text("low 12\nhigh 0\noom 2\noom_kill 1\noom_group_kill 0\n")
            self.assertEqual(_cgroup_memory_events(path),
                             {"oom": 2, "oom_kill": 1, "oom_group_kill": 0})
            path.write_text("oom_kill unsafe-private-message\n")
            self.assertEqual(_cgroup_memory_events(path), {})


if __name__ == "__main__":
    unittest.main()
