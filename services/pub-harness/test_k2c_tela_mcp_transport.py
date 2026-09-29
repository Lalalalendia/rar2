#!/usr/bin/env python3
import os
import unittest
from unittest.mock import patch

from k2c_tela_mcp_transport import (
    DEFAULT_PROXY_PACKAGE,
    TelaMcpError,
    TelaMcpStdioTransport,
    _structured_content,
)


class FakeTransport(TelaMcpStdioTransport):
    def __init__(self, responses):
        super().__init__()
        self.responses = list(responses)
        self.calls = []

    def _call_tool(self, name, arguments):
        self.calls.append((name, dict(arguments)))
        if not self.responses:
            raise AssertionError("unexpected tool call")
        return self.responses.pop(0)


class K2CTelaMcpTransportTests(unittest.TestCase):
    def test_proxy_version_is_pinned(self):
        self.assertEqual(DEFAULT_PROXY_PACKAGE, "tela-mcp@0.7.4")

    def test_missing_secret_fails_before_spawn(self):
        with patch.dict(os.environ, {}, clear=True):
            with self.assertRaisesRegex(TelaMcpError, "TELA_K2C_WRITE_PAT"):
                TelaMcpStdioTransport().start()

    def test_structured_content_prefers_object(self):
        value = _structured_content({"structuredContent": {"ok": True}})
        self.assertEqual(value, {"ok": True})

    def test_structured_content_can_parse_text_json(self):
        value = _structured_content(
            {"content": [{"type": "text", "text": '{"ok":true,"errors":0}'}]}
        )
        self.assertEqual(value["ok"], True)
        self.assertEqual(value["errors"], 0)

    def test_get_page_projects_exact_heading_paths(self):
        t = FakeTransport(
            [
                {
                    "page": {
                        "id": 5049,
                        "space_id": 364,
                        "updated_at": "u1",
                        "sections": [
                            {"path": "Probe"},
                            {"path": "Probe > Live acceptance target"},
                        ],
                    }
                }
            ]
        )
        page = t.get_page(5049)
        self.assertEqual(page.page_id, 5049)
        self.assertEqual(page.space_id, 364)
        self.assertEqual(
            page.section_paths,
            ("Probe", "Probe > Live acceptance target"),
        )
        self.assertEqual(
            t.calls,
            [("get_page", {"id": 5049, "format": "map"})],
        )

    def test_patch_page_derives_updated_cursor_without_body(self):
        t = FakeTransport(
            [
                {
                    "page": {
                        "id": 5049,
                        "space_id": 364,
                        "updated_at": "u1",
                        "sections": [{"path": "Probe > Live acceptance target"}],
                    }
                },
                {"page": {"id": 5049}},
                {
                    "page": {
                        "id": 5049,
                        "space_id": 364,
                        "updated_at": "u2",
                        "sections": [{"path": "Probe > Live acceptance target"}],
                    }
                },
            ]
        )
        out = t.patch_page(
            page_id=5049,
            target="Probe > Live acceptance target",
            operation="append",
            content="x",
            idempotency_key="idem",
        )
        self.assertEqual(out.updated_at, "u2")
        self.assertFalse(out.idempotent_replay)
        self.assertEqual([name for name, _ in t.calls], ["get_page", "patch_page", "get_page"])

    def test_patch_page_marks_server_replay_when_cursor_unchanged(self):
        page = {
            "page": {
                "id": 5049,
                "space_id": 364,
                "updated_at": "u1",
                "sections": [{"path": "Probe > Live acceptance target"}],
            }
        }
        t = FakeTransport([page, {"page": {"id": 5049}}, page])
        out = t.patch_page(
            page_id=5049,
            target="Probe > Live acceptance target",
            operation="append",
            content="x",
            idempotency_key="idem",
        )
        self.assertTrue(out.idempotent_replay)

    def test_lint_page_projects_counts(self):
        t = FakeTransport([{"ok": True, "errors": 0, "warnings": 0}])
        out = t.lint_page(5049)
        self.assertEqual((out.errors, out.warnings), (0, 0))


if __name__ == "__main__":
    unittest.main()
