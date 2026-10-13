#!/usr/bin/env python3
"""Source-only public Nginx guest edge proof; no production access or secrets."""

from __future__ import annotations

import re
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
CONFIG = ROOT / "deploy/nginx/chaptera-reader.conf.example"

GUEST_STATIC = [
    "/editor/temporary-preview",
    "/editor/temporary-editor-preview.mjs",
    "/editor/temporary-editor-preview.css",
    "/editor/temporary-edit-browser-session-v0.mjs",
    "/editor/temporary-edit-move-client-v1.mjs",
    "/editor/interaction-v1.mjs",
    "/editor/reader-scene-editor-interaction-v1.mjs",
    "/cloud-reader/render-v1.mjs",
]
SID = "tempedit:" + "a" * 32
BASE = "/v1/editor/temporary-sessions"
GUEST_API = [
    BASE, BASE + "/" + SID,
    *(BASE + "/" + SID + "/" + part for part in (
        "current", "scene", "move-node",
        "exports/idml", "exports/odg", "exports/pdf",
    )),
]
BLOCKED = [
    "/editor/product-editor-entry-v1.mjs",
    "/editor/chaptera-product-editor-service-v1.mjs",
    "/editor/temporary-preview-other",
    "/editor/temporary-edit-move-client-v1.mjs/other",
    "/editor/temporary-editor-preview.html",
    "/v1/editor/projects",
    "/v1/editor/project/123",
    "/v1/auth/session",
    "/v1/editor/temporary-sessions/" + "b" * 32,
    BASE + "/" + SID + "/other",
    BASE + "/" + SID + "/exports/unknown",
    BASE + "/" + SID + "/exports/idml/extra",
    BASE + "/" + SID + "/../../projects",
]


class ReaderNginxGuestEdgeTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.template = CONFIG.read_text(encoding="utf-8")
        # Test exactly the new two restricted guest locations, not the
        # independent /v1/reader/ or index/static Reader locations.
        cls.patterns = []
        cls.blocks = []
        for match in re.finditer(r"^    location ~ (\S+) \{\n", cls.template, re.M):
            pattern = match.group(1).strip('"')
            if not (pattern.startswith("^/(?:editor/") or
                    pattern.startswith("^/v1/editor/temporary-sessions")):
                continue
            end = cls.template.find("\n    }", match.end())
            if end < 0:
                raise AssertionError("Nginx guest location is not closed")
            cls.patterns.append(re.compile(pattern))
            cls.blocks.append(cls.template[match.end():end])

    def admitted(self, path: str) -> bool:
        return any(pattern.fullmatch(path) is not None for pattern in self.patterns)

    def test_real_nginx_configuration_grammar(self):
        nginx = shutil.which("nginx")
        if not nginx:
            self.skipTest("nginx binary not installed")
        with tempfile.TemporaryDirectory(prefix="chaptera-nginx-edge-") as tmp:
            config = Path(tmp) / "nginx.conf"
            config.write_text(
                "pid " + str(Path(tmp) / "nginx.pid") + ";\n"
                "error_log stderr;\n"
                "events { worker_connections 32; }\n"
                "http { access_log off; include " + str(CONFIG) + "; }\n",
                encoding="utf-8",
            )
            result = subprocess.run(
                [nginx, "-t", "-p", tmp + "/", "-c", str(config)],
                capture_output=True, text=True, check=False,
            )
            self.assertEqual(result.returncode, 0,
                             "actual Nginx parser rejected production guest template: " +
                             (result.stderr or result.stdout)[-1200:])

    def test_exact_guest_rule_count_and_positive_paths(self):
        self.assertEqual(2, len(self.patterns))
        for path in GUEST_STATIC + GUEST_API:
            with self.subTest(path=path):
                self.assertTrue(self.admitted(path))

    def test_authenticated_and_unlisted_routes_denied(self):
        self.assertIn("location / {\n        return 404;", self.template)
        self.assertIn("location ^~ /v1/reader/", self.template)
        for path in BLOCKED:
            with self.subTest(path=path):
                self.assertFalse(self.admitted(path))

    def test_guest_routes_keep_loopback_and_trusted_proxy_contract(self):
        self.assertIn("client_max_body_size 8m;", self.template)
        self.assertIn('Content-Security-Policy "default-src', self.template)
        for body in self.blocks:
            self.assertIn("proxy_pass http://127.0.0.1:8080;", body)
            self.assertIn("proxy_set_header X-Forwarded-Host $host;", body)
            self.assertIn("proxy_set_header X-Forwarded-Proto $scheme;", body)
            self.assertIn("proxy_set_header X-Forwarded-For $remote_addr;", body)
        self.assertNotIn("location /editor/", self.template)
        self.assertNotIn("location ^~ /v1/editor/", self.template)


if __name__ == "__main__":
    unittest.main()
