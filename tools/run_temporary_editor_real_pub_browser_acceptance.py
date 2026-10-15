#!/usr/bin/env python3
"""Real Publisher guest editor UI through actual Caddy/TLS and isolated Rust workers.

The Chromium script uses native browser file upload, pointer events, download
and reload. No test substitutes an HTTP response or performs a manual MoveNode.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import pathlib
import secrets
import shutil
import subprocess
import tempfile
import threading

import run_cloud_reader_live_acceptance as live


def _cgroup_memory_events(path: pathlib.Path = pathlib.Path("/sys/fs/cgroup/memory.events")) -> dict[str, int]:
    """Read numeric resource counters only; never dump private process state."""
    try:
        return {
            name: int(value) for name, value in
            (line.split(maxsplit=1) for line in path.read_text().splitlines())
            if name in {"oom", "oom_kill", "oom_group_kill"}
        }
    except (OSError, ValueError):
        return {}


def _redact_caddy_log(path: pathlib.Path) -> None:
    """Emit only allowlisted, source-neutral Caddy diagnostics.

    Recursive field-name redaction is insufficient: a proxy error may contain
    a private URI, cookie, token or filename under an arbitrary *value* key.
    Construct a new bounded record instead of copying the original JSON.
    """
    if not path.exists():
        return

    def sanitize(entry: object) -> dict[str, object]:
        if not isinstance(entry, dict):
            raise ValueError("Caddy diagnostic is not an object")
        result: dict[str, object] = {"level": "warn", "msg": "other"}
        if entry.get("level") in ("debug", "info", "warn", "error"):
            result["level"] = entry["level"]
        raw_msg = entry.get("msg")
        if isinstance(raw_msg, str):
            if "aborting with incomplete response" in raw_msg:
                result["msg"] = "aborting with incomplete response"
            elif "reverse_proxy" in raw_msg:
                result["msg"] = "reverse_proxy"
        raw_error = entry.get("error")
        if isinstance(raw_error, str):
            for needle, category in (
                ("connection reset by peer", "connection reset by peer"),
                ("unexpected EOF", "unexpected EOF"),
                ("i/o timeout", "upstream timeout"),
                ("context canceled", "context canceled"),
            ):
                if needle in raw_error:
                    result["error"] = category
                    break
            else:
                result["error"] = "other"
        status = entry.get("status")
        if type(status) is int and 100 <= status <= 599:
            result["status"] = status
        request = entry.get("request")
        if isinstance(request, dict):
            cleaned_request: dict[str, object] = {
                "uri": "[redacted]",
                "referer": "[redacted]",
            }
            method = request.get("method")
            if method in ("GET", "POST", "PUT", "PATCH", "DELETE", "HEAD"):
                cleaned_request["method"] = method
            headers = request.get("headers")
            if isinstance(headers, dict):
                safe_headers: dict[str, object] = {}
                for key in ("X-Chaptera-Reader-Session", "X-Csrf-Token",
                            "Authorization", "Cookie"):
                    if any(isinstance(h, str) and h.lower() == key.lower()
                           for h in headers):
                        safe_headers[key] = "[redacted]"
                for key, value in headers.items():
                    if isinstance(key, str) and key.lower() == "content-type" \
                            and isinstance(value, list) and len(value) == 1 \
                            and value[0] in ("application/json", "application/octet-stream"):
                        safe_headers["Content-Type"] = value
                cleaned_request["headers"] = safe_headers
            result["request"] = cleaned_request
        return result

    try:
        safe = path.with_name(path.name + ".safe")
        with path.open("r", encoding="utf-8", errors="replace") as source, \
                safe.open("w", encoding="utf-8") as output:
            for line in source:
                try:
                    cleaned = sanitize(json.loads(line))
                except (json.JSONDecodeError, TypeError, ValueError):
                    cleaned = {"level": "warn", "msg": "unparseable Caddy diagnostic suppressed"}
                output.write(json.dumps(cleaned, sort_keys=True) + "\n")
        safe.replace(path)
    except OSError:
        # On I/O failures, do not upload raw Caddy logs or partial output.
        safe = path.with_name(path.name + ".safe")
        safe.unlink(missing_ok=True)
        path.unlink(missing_ok=True)


def _wait_tls_source_safe(port: int, process: subprocess.Popen[str], log_path: pathlib.Path) -> str:
    """Keep raw reverse-proxy startup diagnostics out of public traceback.

    The shared TLS readiness helper includes raw Caddy log tails on failure;
    those tails could contain private headers. The guarded finally block will
    sanitize the file before any CI artifact upload.
    """
    try:
        return live.wait_tls(port, process, log_path)
    except Exception:
        raise RuntimeError("Caddy TLS readiness failed; inspect sanitized diagnostic artifact") from None


def main() -> int:
    ap = argparse.ArgumentParser()
    for key in ("chaptera", "caddy", "fixture", "out"):
        ap.add_argument("--" + key, required=True, type=pathlib.Path)
    ap.add_argument("--fixture-sha256", required=True)
    ap.add_argument("--target", choices=(
        "idml", "pdf", "story-idml", "story-odg",
        "resize-idml", "resize-odg", "keyboard-idml",
        "create-idml", "create-odg"), default="idml")
    args = ap.parse_args()
    binary = args.chaptera.resolve()
    caddy = args.caddy.resolve()
    fixture = args.fixture.resolve()
    out = args.out.resolve()
    if not binary.is_file() or not caddy.is_file():
        raise AssertionError("canonical Chaptera binary and pinned Caddy required")
    story_mode = args.target.startswith("story-")
    create_mode = args.target.startswith("create-")
    resize_mode = args.target.startswith("resize-")
    keyboard_mode = args.target == "keyboard-idml"
    expected_length = 72192 if story_mode else 291840
    if (live.sha256_file(fixture) != args.fixture_sha256 or
            fixture.stat().st_size != expected_length):
        raise AssertionError("public pinned real Publisher fixture invalid")
    out.mkdir(parents=True, exist_ok=True)
    work = pathlib.Path(tempfile.mkdtemp(prefix="chaptera-temp-guest-browser-"))
    db, config = work / "chaptera.sqlite", work / "config.toml"
    wrapper = work / "wrapper.py"
    caddyfile = work / "Caddyfile"
    server_log, caddy_log = out / "chaptera.log", out / "caddy.log"
    appport, httpport, httpsport = (
        live.free_port(), live.free_port(), live.free_port()
    )
    oidc = live.OidcServer(("127.0.0.1", 0))
    oidc_thread = threading.Thread(target=oidc.serve_forever, daemon=True)
    oidc_thread.start()
    server = proxy = None
    server_stream = caddy_stream = None
    oom_before = _cgroup_memory_events()
    try:
        live.write_isolation_wrapper(wrapper)
        live.write_config(
            config, sqlite_path=db, app_port=appport,
            oidc_port=int(oidc.server_address[1]),
            isolation_wrapper=wrapper, structural_worker=binary,
            scan_root=work / "scan",
        )
        source = config.read_text(encoding="utf-8")
        if source.count('provider = "s3-compatible"') != 1:
            raise AssertionError("fixture must override exactly one storage provider")
        origin_setting = 'public_origin = "https://' + live.EDGE_HOST + '"'
        if source.count(origin_setting) != 1:
            raise AssertionError("expected one canonical public browser origin")
        # Chromium sends Origin and Host with the ephemeral TLS port. The
        # real Rust edge enforces an exact scheme/host/port + CSRF equality.
        source = source.replace(
            origin_setting,
            'public_origin = "https://' + live.EDGE_HOST + f':{httpsport}"',
        )
        config.write_text(
            source.replace('provider = "s3-compatible"', 'provider = "filesystem"'),
            encoding="utf-8",
        )
        live.write_caddy(caddyfile, app_port=appport, http_port=httpport,
                         https_port=httpsport)
        caddy_config = caddyfile.read_text(encoding="utf-8")
        required = (
            "header_up X-Forwarded-Host {hostport}",
            "/editor/temporary-preview",
            "/editor/temporary-edit-move-client-v1.mjs",
            "/editor/text-protocol-v1.mjs",
            "/editor/story-line-endings-v1.mjs",
            "/packages/protocol/editor-resize/v1/resize-gesture-v1.mjs",
            "/editor/reader-scene-editor-interaction-v1.mjs",
            "/cloud-reader/render-v1.mjs",
            "/v1/editor/temporary-sessions",
            "/v1/editor/temporary-sessions/*",
        )
        for path in required:
            if path not in caddy_config:
                raise AssertionError("deployed Caddy gate excludes guest path " + path)
        if "/editor/*" in caddy_config.split("@reader_surface path", 1)[-1]:
            raise AssertionError("wildcard Product Editor static exposure forbidden")
        env = os.environ.copy()
        env.update({
            "CHAPTERA_GUEST_RATE_SECRET": secrets.token_hex(32),
            "CHAPTERA_OIDC_CLIENT_SECRET": secrets.token_hex(32),
            "CHAPTERA_ACCEPTANCE_REAL_ISOLATION_HARNESS":
                str(live.ROOT / "tools" / "migration_pdf_worker_isolation.py"),
            "CHAPTERA_ACCEPTANCE_ISOLATION_TRACE":
                str(work / "isolation.txt"),
        })
        caddy_env = os.environ.copy()
        caddy_env.update({
            "XDG_DATA_HOME": str(work / "caddy-data"),
            "XDG_CONFIG_HOME": str(work / "caddy-config"),
        })
        live.run([str(binary), "--config", str(config), "migrate", "up"],
                 env=env)
        server_stream = server_log.open("w", encoding="utf-8")
        server = subprocess.Popen(
            [str(binary), "--config", str(config), "serve"],
            env=env, cwd=live.ROOT, text=True,
            stdout=server_stream, stderr=subprocess.STDOUT,
        )
        live.wait_http(appport, server, server_log)
        live.run([str(caddy), "validate", "--config", str(caddyfile),
                  "--adapter", "caddyfile"], env=caddy_env, cwd=work)
        caddy_stream = caddy_log.open("w", encoding="utf-8")
        proxy = subprocess.Popen(
            [str(caddy), "run", "--config", str(caddyfile),
             "--adapter", "caddyfile"], env=caddy_env, cwd=work,
            text=True, stdout=caddy_stream, stderr=subprocess.STDOUT,
        )
        certificate_hash = _wait_tls_source_safe(httpsport, proxy, caddy_log)
        if not certificate_hash or len(certificate_hash) != 64:
            raise AssertionError("guest public TLS listener not established")
        origin = f"https://{live.EDGE_HOST}:{httpsport}"
        command = [
            "node", str(live.ROOT / "tools" /
                        ("run_temporary_editor_real_pub_story_browser.mjs"
                         if story_mode else
                         "run_temporary_editor_real_pub_create_browser.mjs"
                         if create_mode else
                         "run_temporary_editor_real_pub_browser.mjs")),
            "--origin", origin,
            "--fixture", str(fixture),
            "--fixture-sha256", args.fixture_sha256,
            "--out", str(out / "receipt.json"),
            "--target", args.target.removeprefix("story-").removeprefix("resize-").removeprefix("keyboard-").removeprefix("create-"),
        ]
        if resize_mode:
            command.append("--resize")
        if keyboard_mode:
            command.append("--keyboard-nudge")
        result = subprocess.run(
            command, env=env, cwd=live.ROOT, text=True,
            capture_output=True, timeout=540, check=False,
        )
        if result.returncode != 0:
            # Never echo raw source bytes, private HTTP payloads or bearer
            # material in the public CI progress output.
            raise AssertionError(
                "physical Chromium guest acceptance failed; see sanitized "
                "chaptera/Caddy logs. Process/oom metadata: " +
                json.dumps({
                    "server_exit": server.poll() if server else None,
                    "proxy_exit": proxy.poll() if proxy else None,
                    "new_oom_kills": max(
                        0, _cgroup_memory_events().get("oom_kill", 0)
                        - oom_before.get("oom_kill", 0)),
                }, sort_keys=True) +
                "; browser exit status: " + str(result.returncode)
            )
        receipt = json.loads((out / "receipt.json").read_text(encoding="utf-8"))

        if create_mode:
            target = args.target.removeprefix("create-")
            consumer = "Scribus" if target == "idml" else "LibreOffice Draw"
            required = {
                "schema": "chaptera.tempedit-real-pub-create-shape-chromium-https.v1",
                "status": "PASS",
                "source_sha256": args.fixture_sha256,
                "source_byte_len": expected_length,
                "browser_pointer_create_commits": 1,
                "target_application_consumer": consumer,
                "export_target": target,
                "v0_go": False,
            }
            flags = [
                "real_pub", "real_chromium", "real_isolated_rust_guest",
                "exact_current_reopened", "exact_reader_scene_created_node",
                "partial_stacking_truthful", "physical_target_xml_consumer",
                "target_application_reopened", "same_browser_delete_and_revoke",
                "private_bearer_not_in_urls",
                "read_replay_export_no_ttl_extension", "original_pub_immutable",
                "loss_report_checked",
            ]
            if (any(receipt.get(k) != value for k, value in required.items()) or
                    any(receipt.get(k) is not True for k in flags) or
                    not isinstance(receipt.get("changed_artifact_sha256"), str) or
                    len(receipt["changed_artifact_sha256"]) != 64):
                raise AssertionError("real Publisher Guest CreateShape receipt invalid")
            print(json.dumps({
                "schema": receipt["schema"], "status": "PASS",
                "target": target, "real_pub_browser_https": True,
                "physical_native_app": consumer,
                "artifact_sha256": receipt["changed_artifact_sha256"],
            }, sort_keys=True))
            return 0
        if story_mode:
            target = args.target.removeprefix("story-")
            if (receipt.get("schema") != "chaptera.tempedit-real-pub-story-consumer.v1" or
                    receipt.get("status") != "PASS" or
                    receipt.get("source_sha256") != args.fixture_sha256 or
                    receipt.get("source_byte_len") != expected_length or
                    receipt.get("real_pub") is not True or
                    receipt.get("real_chromium") is not True or
                    receipt.get("browser_story_commits") != 1 or
                    receipt.get("guest_story_ui_commit") is not True or
                    receipt.get("canvas_story_pick_proven") is not True or
                    receipt.get("canvas_pick_no_mutation") is not True or
                    receipt.get("export_target") != target or
                    receipt.get("target_consumer_text_recovered") is not True or
                    receipt.get("physical_target_xml_consumer") is not True or
                    receipt.get("target_application_reopened") is not True or
                    receipt.get("target_application_consumer") !=
                        ("Scribus" if target == "idml" else "LibreOffice Draw") or
                    receipt.get("exact_current_story_reopened") is not True or
                    receipt.get("exact_reader_scene_story") is not True or
                    receipt.get("identical_operation_id_replay_no_second_revision") is not True or
                    receipt.get("stale_story_write_denied") is not True or
                    receipt.get("conflicting_operation_id_denied") is not True or
                    receipt.get("foreign_bearer_mutation_denied") is not True or
                    receipt.get("read_replay_export_does_not_renew_ttl") is not True or
                    receipt.get("same_browser_session_delete_proven") is not True or
                    receipt.get("revoked_bearer_denied") is not True or
                    receipt.get("bearer_absent_from_urls") is not True or
                    receipt.get("original_pub_immutable") is not True):
                raise AssertionError("physical Guest Story export receipt invalid")
            print(json.dumps({
                "schema": receipt["schema"],
                "status": "PASS",
                "real_pub_browser_https": True,
                "target": target,
                "changed_artifact_sha256": receipt["changed_artifact_sha256"],
                "physical_text_consumer": True,
            }, sort_keys=True))
            return 0
        if resize_mode:
            target = args.target.removeprefix("resize-")
            geometry = (
                "idml_path_geometry" if target == "idml"
                else "odg_frame_attributes"
            )
            if (receipt.get("schema") !=
                    "chaptera.tempedit-real-pub-resize-chromium-https.v1" or
                    receipt.get("status") != "PASS" or
                    receipt.get("source_sha256") != args.fixture_sha256 or
                    receipt.get("source_byte_len") != expected_length or
                    receipt.get("real_pub") is not True or
                    receipt.get("real_chromium") is not True or
                    receipt.get("browser_pointer_commits") != 1 or
                    receipt.get("resize_handle_pointer_commit") is not True or
                    receipt.get("resized_bounds_reopened") is not True or
                    receipt.get("resized_target_application_reopened") is not True or
                    receipt.get("export_target") != target or
                    receipt.get(f"changed_{target}_geometry_proven") is not True or
                    receipt.get(f"changed_{target}_identity_proven") is not True or
                    receipt.get(f"changed_{target}_geometry_form") != geometry or
                    receipt.get("same_browser_session_delete_proven") is not True or
                    receipt.get("revoked_old_current_scene_export_denied") is not True or
                    receipt.get("temporary_bearer_cleared_after_delete") is not True or
                    receipt.get("loss_report_checked") is not True or
                    receipt.get("original_pub_immutable") is not True or
                    receipt.get("bearer_absent_from_urls") is not True or
                    receipt.get("v0_go") is not False):
                raise AssertionError("physical Guest ResizeNode receipt invalid")
            print(json.dumps({
                "schema": receipt["schema"], "status": "PASS",
                "real_pub_browser_https": True,
                "resize_node_commits": receipt["browser_pointer_commits"],
                "target": target,
                "physical_geometry_consumer": True,
                "changed_artifact_sha256":
                    receipt[f"changed_{target}_download_sha256"],
            }, sort_keys=True))
            return 0
        if keyboard_mode:
            if (receipt.get("schema") !=
                    "chaptera.tempedit-real-pub-keyboard-nudge-chromium-https.v1" or
                    receipt.get("status") != "PASS" or
                    receipt.get("source_sha256") != args.fixture_sha256 or
                    receipt.get("source_byte_len") != expected_length or
                    receipt.get("real_pub") is not True or
                    receipt.get("real_chromium") is not True or
                    receipt.get("browser_keyboard_commits") != 1 or
                    receipt.get("browser_pointer_commits") != 0 or
                    receipt.get("keyboard_nudge_step_emu") != 12700 or
                    receipt.get("keyboard_canvas_focus_verified") is not True or
                    receipt.get("keyboard_modified_arrow_denied") is not True or
                    receipt.get("keyboard_exact_one_point_reopened") is not True or
                    receipt.get("keyboard_idml_target_application_reopened") is not True or
                    receipt.get("export_target") != "idml" or
                    receipt.get("changed_idml_geometry_proven") is not True or
                    receipt.get("changed_idml_identity_proven") is not True or
                    receipt.get("changed_idml_geometry_form") != "idml_path_geometry" or
                    receipt.get("same_browser_session_delete_proven") is not True or
                    receipt.get("revoked_old_current_scene_export_denied") is not True or
                    receipt.get("temporary_bearer_cleared_after_delete") is not True or
                    receipt.get("loss_report_checked") is not True or
                    receipt.get("original_pub_immutable") is not True or
                    receipt.get("bearer_absent_from_urls") is not True or
                    receipt.get("v0_go") is not False):
                raise AssertionError("physical keyboard Guest MoveNode receipt invalid")
            print(json.dumps({
                "schema": receipt["schema"], "status": "PASS",
                "real_pub_browser_https": True,
                "browser_keyboard_commits": receipt["browser_keyboard_commits"],
                "exact_nudge_emu": receipt["keyboard_nudge_step_emu"],
                "target": "idml", "scribus_reopened": True,
                "changed_artifact_sha256": receipt["changed_idml_download_sha256"],
            }, sort_keys=True))
            return 0
        target = args.target
        expected_geometry = (
            "pdf_rectangle_stream" if target == "pdf" else "idml_path_geometry"
        )
        if (receipt.get("status") != "PASS" or
                receipt.get("real_pub") is not True or
                receipt.get("real_chromium") is not True or
                receipt.get("export_target") != target or
                receipt.get("browser_pointer_commits") != 1 or
                receipt.get("source_sha256") != args.fixture_sha256 or
                receipt.get(f"changed_{target}_geometry_proven") is not True or
                receipt.get(f"changed_{target}_identity_proven") is not True or
                receipt.get(f"changed_{target}_geometry_form") != expected_geometry or
                receipt.get("same_browser_session_delete_proven") is not True or
                receipt.get("revoked_old_current_scene_export_denied") is not True or
                receipt.get("temporary_bearer_cleared_after_delete") is not True or
                receipt.get("loss_report_checked") is not True or
                receipt.get("bearer_absent_from_urls") is not True or
                receipt.get("original_pub_immutable") is not True or
                receipt.get("v0_go") is not False):
            raise AssertionError("full physical browser receipt invalid")
        if target == "pdf" and (
            receipt.get("pristine_pdf_without_edit_proven") is not True
            or receipt.get("pristine_pdf_unchanged_revision_proven") is not True
            or receipt.get("pristine_pdf_private_report_proven") is not True
            or not isinstance(receipt.get("pristine_pdf_sha256"), str)
            or len(receipt["pristine_pdf_sha256"]) != 64
            or not isinstance(receipt.get("pristine_pdf_pages"), int)
            or receipt["pristine_pdf_pages"] < 1
        ):
            raise AssertionError("real-browser untouched PUB baseline PDF proof is missing")
        print(json.dumps({
            "schema": receipt["schema"],
            "status": receipt["status"],
            "real_pub_browser_https": True,
            "browser_pointer_commits": receipt["browser_pointer_commits"],
            "target": target,
            "edited_artifact_sha256":
                receipt[f"changed_{target}_download_sha256"],
            "downloaded_consumer_geometry_proven": True,
            "v0_go": False,
        }, sort_keys=True))
        return 0
    finally:
        live.terminate(proxy)
        live.terminate(server)
        if caddy_stream is not None:
            caddy_stream.close()
        if server_stream is not None:
            server_stream.close()
        _redact_caddy_log(caddy_log)
        oidc.shutdown()
        oidc.server_close()
        oidc_thread.join(timeout=2)
        shutil.rmtree(work, ignore_errors=True)


if __name__ == "__main__":
    raise SystemExit(main())
