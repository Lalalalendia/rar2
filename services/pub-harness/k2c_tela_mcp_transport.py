#!/usr/bin/env python3
"""Live Tela MCP stdio transport for the guarded K2C applier.

This module speaks MCP only through the version-pinned official tela-mcp
stdio<->HTTP proxy. The PAT is inherited from TELA_K2C_WRITE_PAT and is never
placed on argv, printed, persisted, or included in receipts.
"""

from __future__ import annotations

import json
import os
import selectors
import shutil
import subprocess
from collections.abc import Mapping
from typing import Any

from k2c_tela_applier import (
    TelaLintOutcome,
    TelaPageSnapshot,
    TelaPatchOutcome,
)

PROTOCOL_VERSION = "2025-06-18"
DEFAULT_BASE_URL = "https://telawiki.com"
DEFAULT_PROXY_PACKAGE = "tela-mcp@0.7.4"
REQUIRED_TOOLS = frozenset({"get_page", "patch_page", "lint_page"})


class TelaMcpError(RuntimeError):
    pass


def _structured_content(result: Mapping[str, Any]) -> Mapping[str, Any]:
    structured = result.get("structuredContent")
    if isinstance(structured, Mapping):
        return structured

    content = result.get("content")
    if isinstance(content, list):
        for item in content:
            if not isinstance(item, Mapping):
                continue
            text = item.get("text")
            if not isinstance(text, str):
                continue
            try:
                parsed = json.loads(text)
            except json.JSONDecodeError:
                continue
            if isinstance(parsed, Mapping):
                return parsed
    raise TelaMcpError("MCP tool response has no structured object content")


class TelaMcpStdioTransport:
    def __init__(
        self,
        *,
        base_url: str = DEFAULT_BASE_URL,
        token_env: str = "TELA_K2C_WRITE_PAT",
        proxy_package: str = DEFAULT_PROXY_PACKAGE,
        timeout_seconds: float = 30.0,
    ) -> None:
        self.base_url = base_url.rstrip("/")
        self.token_env = token_env
        self.proxy_package = proxy_package
        self.timeout_seconds = timeout_seconds
        self._proc: subprocess.Popen[str] | None = None
        self._selector: selectors.BaseSelector | None = None
        self._next_id = 1

    def __enter__(self) -> "TelaMcpStdioTransport":
        self.start()
        return self

    def __exit__(self, exc_type, exc, tb) -> None:
        self.close()

    def start(self) -> None:
        if self._proc is not None:
            return
        token = os.environ.get(self.token_env, "")
        if not token:
            raise TelaMcpError(f"{self.token_env} is required for live Tela MCP")
        npx = shutil.which("npx")
        if not npx:
            raise TelaMcpError("npx is required for tela-mcp live transport")

        env = os.environ.copy()
        env["TELA_BASE_URL"] = self.base_url
        env["TELA_API_KEY"] = token

        self._proc = subprocess.Popen(
            [npx, "-y", self.proxy_package],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            text=True,
            bufsize=1,
            env=env,
        )
        if self._proc.stdin is None or self._proc.stdout is None:
            self.close()
            raise TelaMcpError("failed to open tela-mcp stdio pipes")

        self._selector = selectors.DefaultSelector()
        self._selector.register(self._proc.stdout, selectors.EVENT_READ)

        initialized = self._request(
            "initialize",
            {
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": {"name": "chaptera-k2c-applier", "version": "1"},
            },
        )
        if not isinstance(initialized, Mapping):
            self.close()
            raise TelaMcpError("initialize returned an invalid result")
        self._notify("notifications/initialized")

        listed = self._request("tools/list", {})
        tools = listed.get("tools", []) if isinstance(listed, Mapping) else []
        names = {
            str(item.get("name"))
            for item in tools
            if isinstance(item, Mapping) and item.get("name")
        }
        missing = sorted(REQUIRED_TOOLS - names)
        if missing:
            self.close()
            raise TelaMcpError("required Tela MCP tools missing: " + ", ".join(missing))

    def close(self) -> None:
        selector = self._selector
        self._selector = None
        if selector is not None:
            try:
                selector.close()
            except Exception:
                pass

        proc = self._proc
        self._proc = None
        if proc is None:
            return
        try:
            if proc.stdin is not None:
                proc.stdin.close()
        except Exception:
            pass
        if proc.poll() is None:
            proc.terminate()
            try:
                proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                proc.kill()
                proc.wait(timeout=5)

    def _send(self, value: Mapping[str, Any]) -> None:
        proc = self._proc
        if proc is None or proc.stdin is None:
            raise TelaMcpError("Tela MCP transport is not started")
        proc.stdin.write(json.dumps(value, separators=(",", ":")) + "\n")
        proc.stdin.flush()

    def _read_message(self) -> Mapping[str, Any]:
        proc = self._proc
        selector = self._selector
        if proc is None or proc.stdout is None or selector is None:
            raise TelaMcpError("Tela MCP transport is not started")

        events = selector.select(self.timeout_seconds)
        if not events:
            raise TelaMcpError("timed out waiting for Tela MCP response")
        line = proc.stdout.readline()
        if line == "":
            code = proc.poll()
            raise TelaMcpError(f"tela-mcp closed stdout unexpectedly (exit={code})")
        try:
            value = json.loads(line)
        except json.JSONDecodeError as exc:
            raise TelaMcpError("tela-mcp emitted non-JSON stdout") from exc
        if not isinstance(value, Mapping):
            raise TelaMcpError("tela-mcp emitted non-object JSON-RPC frame")
        return value

    def _notify(self, method: str, params: Mapping[str, Any] | None = None) -> None:
        payload: dict[str, Any] = {"jsonrpc": "2.0", "method": method}
        if params is not None:
            payload["params"] = dict(params)
        self._send(payload)

    def _request(
        self,
        method: str,
        params: Mapping[str, Any] | None,
    ) -> Mapping[str, Any]:
        request_id = self._next_id
        self._next_id += 1
        payload: dict[str, Any] = {
            "jsonrpc": "2.0",
            "id": request_id,
            "method": method,
        }
        if params is not None:
            payload["params"] = dict(params)
        self._send(payload)

        while True:
            message = self._read_message()
            if message.get("id") != request_id:
                continue
            error = message.get("error")
            if error is not None:
                if isinstance(error, Mapping):
                    code = error.get("code")
                    text = error.get("message", "MCP request failed")
                    raise TelaMcpError(f"{method} failed ({code}): {text}")
                raise TelaMcpError(f"{method} failed")
            result = message.get("result")
            if not isinstance(result, Mapping):
                raise TelaMcpError(f"{method} returned invalid result")
            return result

    def _call_tool(
        self,
        name: str,
        arguments: Mapping[str, Any],
    ) -> Mapping[str, Any]:
        result = self._request(
            "tools/call",
            {"name": name, "arguments": dict(arguments)},
        )
        if result.get("isError") is True:
            raise TelaMcpError(f"Tela MCP tool {name} returned isError")
        return _structured_content(result)

    def get_page(self, page_id: int) -> TelaPageSnapshot:
        out = self._call_tool("get_page", {"id": page_id, "format": "map"})
        page = out.get("page")
        if not isinstance(page, Mapping):
            raise TelaMcpError("get_page response missing page")
        sections = page.get("sections", [])
        paths = tuple(
            str(item["path"])
            for item in sections
            if isinstance(item, Mapping) and item.get("path")
        )
        return TelaPageSnapshot(
            page_id=int(page["id"]),
            space_id=int(page["space_id"]),
            updated_at=str(page["updated_at"]),
            section_paths=paths,
        )

    def patch_page(
        self,
        *,
        page_id: int,
        target: str,
        operation: str,
        content: str,
        idempotency_key: str,
    ) -> TelaPatchOutcome:
        before = self.get_page(page_id)
        self._call_tool(
            "patch_page",
            {
                "id": page_id,
                "target": target,
                "operation": operation,
                "content": content,
                "idempotency_key": idempotency_key,
            },
        )
        after = self.get_page(page_id)
        return TelaPatchOutcome(
            page_id=page_id,
            updated_at=after.updated_at,
            idempotent_replay=(after.updated_at == before.updated_at),
        )

    def lint_page(self, page_id: int) -> TelaLintOutcome:
        out = self._call_tool("lint_page", {"id": page_id})
        return TelaLintOutcome(
            page_id=page_id,
            errors=int(out.get("errors", 0)),
            warnings=int(out.get("warnings", 0)),
        )
