#!/usr/bin/env python3
"""Physical no-login temporary EditorSession CURRENT + MoveNode: exact PUB, HTTPS, TTL."""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import pathlib
import re
from decimal import Decimal
import secrets
import shutil
import sqlite3
import subprocess
import tempfile
import threading
import time
from urllib.parse import urlencode

import run_cloud_reader_live_acceptance as live
import verify_editable_export_geometry as geometry


def checked(status: int, expected: int, action: str) -> None:
    if status != expected:
        raise AssertionError(f"{action} HTTP {status}, expected {expected}")


def verify_physical_edited_pdf(data: bytes, expected: geometry.RectEmu) -> dict:
    """Independent physical PDF cross-reference + painted MoveNode geometry consumer."""
    if not data.startswith(b"%PDF-") or not data.rstrip().endswith(b"%%EOF"):
        raise AssertionError("edited PDF has invalid header or trailer")
    marker = re.search(rb"startxref\s+(\d+)\s+%%EOF\s*$", data)
    if marker is None:
        raise AssertionError("edited PDF lacks a cross-reference pointer")
    xref = int(marker.group(1))
    if xref <= 0 or xref >= len(data) or not data[xref:].startswith(b"xref\n"):
        raise AssertionError("edited PDF cross-reference pointer does not reopen")
    page_count = data.count(b"/Type /Page /Parent")
    if page_count <= 0 or page_count > 1000:
        raise AssertionError("edited PDF has no admissible physical pages")
    rects = re.findall(
        rb"(-?\d+(?:\.\d+)?)\s+(-?\d+(?:\.\d+)?)\s+"
        rb"(-?\d+(?:\.\d+)?)\s+(-?\d+(?:\.\d+)?)\s+re\b",
        data,
    )
    # Convert back independently from PDF points. A moved shape must have
    # physical changed coordinates; a valid source-only PDF is insufficient.
    physical = tuple(Decimal(value) for value in (
        expected.x, expected.y, expected.width, expected.height
    ))
    for rect in rects:
        measured = tuple(Decimal(token.decode("ascii")) * 12700 for token in rect)
        if all(abs(a - b) < Decimal("0.01") for a, b in zip(measured, physical)):
            return {"geometry_form": "pdf_rectangle_stream", "pages": page_count}
    raise AssertionError("edited PDF stream does not paint the accepted MoveNode rectangle")


def main() -> int:
    ap = argparse.ArgumentParser()
    for key in ("chaptera", "caddy", "fixture", "out"):
        ap.add_argument("--" + key, type=pathlib.Path, required=True)
    ap.add_argument("--fixture-sha256", required=True)
    args = ap.parse_args()

    fixture = args.fixture.resolve()
    expected_hash = live.sha256_file(fixture)
    if expected_hash != args.fixture_sha256 or not 0 < fixture.stat().st_size < live.FIXTURE_MAX_BYTES:
        raise AssertionError("pinned real PUB source identity/size mismatch")
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    work = pathlib.Path(tempfile.mkdtemp(prefix="chaptera-temp-current-"))
    db, config, wrapper = work / "chaptera.sqlite", work / "config.toml", work / "wrapper.py"
    caddyfile, response = work / "Caddyfile", work / "response.json"
    server_log, caddy_log = out / "chaptera.log", out / "caddy.log"
    appport, httpport, httpsport = live.free_port(), live.free_port(), live.free_port()
    oidc = live.OidcServer(("127.0.0.1", 0))
    oidc_thread = threading.Thread(target=oidc.serve_forever, daemon=True)
    oidc_thread.start()

    live.write_isolation_wrapper(wrapper)
    live.write_config(
        config,
        sqlite_path=db,
        app_port=appport,
        oidc_port=int(oidc.server_address[1]),
        isolation_wrapper=wrapper,
        structural_worker=args.chaptera.resolve(),
        scan_root=work / "scan",
    )
    original_config = config.read_text(encoding="utf-8")
    if original_config.count('provider = "s3-compatible"') != 1:
        raise AssertionError("expected exactly one fixture provider declaration")
    config.write_text(
        original_config.replace('provider = "s3-compatible"', 'provider = "filesystem"'),
        encoding="utf-8",
    )
    live.write_caddy(caddyfile, app_port=appport, http_port=httpport, https_port=httpsport)
    site = caddyfile.read_text(encoding="utf-8")
    needle = "@reader_surface path / /index.html"
    if site.count(needle) != 1:
        raise AssertionError("Caddy Reader surface unexpectedly changed")
    site = site.replace(
        needle,
        "@reader_surface path /v1/editor/temporary-sessions "
        "/v1/editor/temporary-sessions/* / /index.html",
    )
    caddyfile.write_text(site, encoding="utf-8")

    env = os.environ.copy()
    env.update(
        {
            "CHAPTERA_GUEST_RATE_SECRET": secrets.token_hex(32),
            "CHAPTERA_OIDC_CLIENT_SECRET": secrets.token_hex(32),
            "CHAPTERA_ACCEPTANCE_REAL_ISOLATION_HARNESS": str(
                live.ROOT / "tools" / "migration_pdf_worker_isolation.py"
            ),
            "CHAPTERA_ACCEPTANCE_ISOLATION_TRACE": str(work / "isolation.txt"),
        }
    )
    caddy_env = os.environ.copy()
    caddy_env.update(
        {"XDG_DATA_HOME": str(work / "caddy-data"), "XDG_CONFIG_HOME": str(work / "caddy-config")}
    )
    s_log = server_log.open("w", encoding="utf-8")
    c_log = caddy_log.open("w", encoding="utf-8")
    server: subprocess.Popen[str] | None = None
    proxy: subprocess.Popen[str] | None = None
    token: str | None = None

    rate_retries = 0

    def call(method: str, path: str, *, headers: list[str] | None = None, **other):
        nonlocal rate_retries
        for attempt in range(8):
            status, raw = live.curl_request(
                https_port=httpsport,
                method=method,
                path=path,
                output=response,
                headers=headers,
                **other,
            )
            if status != 429 or attempt == 7:
                return status, raw
            # Keep adversarial cases inside the real 30/minute burst-six
            # ReaderSessionOpen admission, rather than weakening live policy.
            rate_retries += 1
            time.sleep(2.2)
        raise AssertionError("temporary private reader admission did not converge")

    try:
        live.run([str(args.chaptera.resolve()), "--config", str(config), "migrate", "up"], env=env)
        server = subprocess.Popen(
            [str(args.chaptera.resolve()), "--config", str(config), "serve"],
            cwd=live.ROOT, env=env, text=True, stdout=s_log, stderr=subprocess.STDOUT,
        )
        live.wait_http(appport, server, server_log)
        live.run([str(args.caddy.resolve()), "validate", "--config", str(caddyfile),
                  "--adapter", "caddyfile"], env=caddy_env, cwd=work)
        proxy = subprocess.Popen(
            [str(args.caddy.resolve()), "run", "--config", str(caddyfile),
             "--adapter", "caddyfile"], cwd=work, env=caddy_env,
            text=True, stdout=c_log, stderr=subprocess.STDOUT,
        )
        live.wait_tls(httpsport, proxy, caddy_log)

        status, raw = call(
            "POST", "/v1/editor/temporary-sessions",
            body_json={"expected_byte_len": fixture.stat().st_size},
        )
        checked(status, 200, "temporary capability issuance")
        issued = json.loads(raw)
        session_id, token = issued["session_id"], issued["access_token"]
        if not session_id.startswith("tempedit:") or len(token) != 64:
            raise AssertionError("temporary capability protocol mismatch")
        path = f"/v1/editor/temporary-sessions/{session_id}/current"
        scene_path = f"/v1/editor/temporary-sessions/{session_id}/scene"
        private = [f"x-chaptera-reader-session: {token}"]
        checked(call("GET", path)[0], 401, "no-capability current")
        checked(call("GET", path, headers=[
            "x-chaptera-reader-session: " + secrets.token_hex(32)
        ])[0], 404, "foreign-capability current")
        checked(call("PUT", issued["upload_path"], upload=fixture, headers=private)[0],
                200, "exact temporary PUB upload")
        opened_status, opened_raw = call("POST", issued["open_path"], headers=private)
        checked(opened_status, 200, "real isolated Reader open")
        opened = json.loads(opened_raw)
        if opened.get("classification") not in {"supported", "partial"}:
            raise AssertionError("pinned Publisher source is not Reader-eligible")
        if opened.get("source_sha256") != expected_hash:
            raise AssertionError("Reader source hash differs from immutable upload")
        physical_source_files = [
            p for p in (work / "blob-store").rglob("*")
            if p.is_file() and p.stat().st_size == fixture.stat().st_size
            and live.sha256_file(p) == expected_hash
        ]
        if len(physical_source_files) != 1:
            raise AssertionError("the exact quarantined original PUB is not physically present")
        physical_source = physical_source_files[0]
        expected_expiry = int(opened["expires_at_ms"])
        if not 3_000_000 < expected_expiry - int(time.time() * 1000) <= 3_600_000:
            raise AssertionError("accepted upload did not start one-hour TTL")

        current_status, current_raw = call("GET", path, headers=private)
        checked(current_status, 200, "isolated canonical EditorSession current")
        current = json.loads(current_raw)
        if current.get("protocol_version") != "chaptera.temporary-editor-current.v1":
            raise AssertionError("canonical current protocol invalid")
        if current.get("document_id") != session_id or current.get("source_hash") != expected_hash:
            raise AssertionError("canonical current source/session binding mismatch")
        if current.get("revision_cursor") != 0:
            raise AssertionError("read-only current illegally created a revision")
        if not isinstance(current.get("project"), dict) or not isinstance(
            current.get("authoring_graph"), dict
        ):
            raise AssertionError("isolated EditorSession returned no project or geometry")
        revision = current.get("revision_id", "")
        authoring_revision = current.get("canonical_authoring_revision_id", "")
        if (
            not isinstance(revision, str)
            or not revision.startswith("sha256:")
            or len(revision) != 71
            or not isinstance(authoring_revision, str)
            or len(authoring_revision) != 64
        ):
            raise AssertionError("canonical source-derived revision identity invalid")
        if current.get("expires_at_ms") != expected_expiry:
            raise AssertionError("GET current illegally renewed temporary TTL")

        second_status, second_raw = call("GET", path, headers=private)
        checked(second_status, 200, "idempotent current")
        second = json.loads(second_raw)
        for field in ("source_hash", "project", "revision_id", "expires_at_ms"):
            if second.get(field) != current.get(field):
                raise AssertionError("second current read changed " + field)


        # Physical P0 mutation: use a bounded real-PUB direct geometric node.
        # Rejected unsupported nodes are skipped; the proof is positive only
        # if a canonical isolated MoveNode event is actually committed.
        # The guest editing surface must use the exact source-neutral isolated
        # Reader projection, never the original Guest upload scene.
        checked(call("GET", scene_path)[0], 401, "guest revised scene requires capability")
        checked(call("GET", scene_path, headers=[
            "x-chaptera-reader-session: " + secrets.token_hex(32)
        ])[0], 404, "foreign capability cannot see revised scene")
        scene_status, scene_raw = call("GET", scene_path, headers=private)
        checked(scene_status, 200, "isolated guest exact baseline Reader Scene")
        scene_head = json.loads(scene_raw)
        if (
            scene_head.get("protocol_version") != "chaptera.temporary-editor-scene.v1"
            or scene_head.get("document_id") != session_id
            or scene_head.get("source_hash") != expected_hash
            or scene_head.get("revision_id") != revision
            or scene_head.get("expires_at_ms") != expected_expiry
        ):
            raise AssertionError("private Guest Reader Scene baseline identity/TTL mismatch")
        rich_scene = scene_head.get("scene")
        if (
            not isinstance(rich_scene, dict)
            or rich_scene.get("protocol_version") != "chaptera.reader-scene.v1"
            or rich_scene.get("document_id") != session_id
            or rich_scene.get("source_hash") != expected_hash
            or rich_scene.get("revision_id") != revision
            or not isinstance(rich_scene.get("nodes"), list)
        ):
            raise AssertionError("isolated baseline Reader Scene protocol or revision invalid")
        visible_direct_ids = {
            node["node_id"]
            for node in rich_scene["nodes"]
            if isinstance(node, dict) and isinstance(node.get("node_id"), str)
            and node.get("parent_node_id") is None
            and node.get("origin_node_id") is None
        }
        nodes = current["authoring_graph"].get("nodes")
        if not isinstance(nodes, dict):
            raise AssertionError("canonical real PUB has no node map for editing")
        painted_direct_ids = {
            node["node_id"] for node in rich_scene["nodes"]
            if isinstance(node, dict) and isinstance(node.get("node_id"), str)
            and isinstance(node.get("paint"), dict)
            and (node["paint"].get("fill_rgb") is not None
                 or (isinstance(node["paint"].get("line"), dict)
                     and node["paint"]["line"].get("width_emu", 0) > 0))
        }
        candidates = []
        for node_id, node in sorted(nodes.items()):
            if not isinstance(node_id, str) or not isinstance(node, dict):
                continue
            if node_id not in visible_direct_ids:
                continue
            bounds = node.get("header", {}).get("bounds", {})
            if not isinstance(bounds, dict):
                continue
            x, y = bounds.get("x"), bounds.get("y")
            if type(x) is not int or type(y) is not int:
                continue
            candidates.append((node_id, x, y))
        if not candidates:
            raise AssertionError("pinned real PUB has no canonical geometric nodes")
        # Prefer a true PDF-rectangle-capable MoveNode for the physical PDF oracle.
        candidates.sort(key=lambda item: (item[0] not in painted_direct_ids, item[0]))

        move_path = f"/v1/editor/temporary-sessions/{session_id}/move-node"
        missing_request = {
            "protocol_version": "chaptera.commit-request.v1",
            "document_id": session_id,
            "source_hash": expected_hash,
            "base_revision_id": revision,
            "client_operation_id": "missing-capability",
            "command": {
                "kind": "move_node_to",
                "node_id": candidates[0][0],
                "x_emu": candidates[0][1] + 6350,
                "y_emu": candidates[0][2],
            },
        }
        checked(call("POST", move_path, body_json=missing_request)[0],
                401, "MoveNode requires private token")
        checked(call(
            "POST", move_path,
            headers=["x-chaptera-reader-session: " + secrets.token_hex(32)],
            body_json=missing_request,
        )[0], 404, "MoveNode rejects foreign private token")

        accepted = None
        chosen = None
        unsupported = 0
        for idx, (node_id, x, y) in enumerate(candidates[:24]):
            intent = {
                "protocol_version": "chaptera.commit-request.v1",
                "document_id": session_id,
                "source_hash": expected_hash,
                "base_revision_id": revision,
                "client_operation_id": f"physical-move-{idx}",
                "command": {
                    "kind": "move_node_to",
                    "node_id": node_id,
                    "x_emu": x + 6350,
                    "y_emu": y + 6350,
                },
            }
            status, raw = call("POST", move_path, headers=private, body_json=intent)
            if status == 422:
                unsupported += 1
                continue
            checked(status, 200, "real isolated canonical MoveNode")
            accepted = json.loads(raw)
            chosen = intent
            break
        if accepted is None:
            raise AssertionError(
                f"no supported visible MoveNode in first 24 direct scene nodes; rejected={unsupported}"
            )
        if (
            accepted.get("protocol_version") != "chaptera.temporary-editor-move-node.v1"
            or accepted.get("document_id") != session_id
            or accepted.get("source_hash") != expected_hash
            or accepted.get("base_revision_id") != revision
            or accepted.get("revision_id") == revision
            or accepted.get("replayed") is not False
            or accepted.get("scene_refresh") != "current"
            or int(accepted.get("expires_at_ms", 0)) <= expected_expiry
        ):
            raise AssertionError("accepted MoveNode does not prove a fresh source-bound revision")
        move_expiry = int(accepted["expires_at_ms"])
        if not 3_000_000 < move_expiry - int(time.time() * 1000) <= 3_600_000:
            raise AssertionError("accepted MoveNode did not atomically refresh one-hour TTL")

        reread_status, reread_raw = call("GET", path, headers=private)
        checked(reread_status, 200, "reopen edited canonical revision")
        reopened = json.loads(reread_raw)
        if (
            reopened.get("revision_id") != accepted["revision_id"]
            or reopened.get("revision_cursor") != 1
            or reopened.get("expires_at_ms") != move_expiry
            or reopened.get("source_hash") != expected_hash
            or reopened.get("project") == current["project"]
            or live.sha256_file(physical_source) != expected_hash
        ):
            raise AssertionError("edited canonical state was not reopened or original PUB mutated")

        chosen_id = chosen["command"]["node_id"]
        before_bounds = current["authoring_graph"]["nodes"][chosen_id]["header"]["bounds"]
        after_bounds = reopened["authoring_graph"]["nodes"][chosen_id]["header"]["bounds"]
        if (
            after_bounds.get("x") != chosen["command"]["x_emu"]
            or after_bounds.get("y") != chosen["command"]["y_emu"]
            or after_bounds == before_bounds
        ):
            raise AssertionError("canonical reopened Scene did not reflect the accepted MoveNode geometry")


        edited_scene_status, edited_scene_raw = call("GET", scene_path, headers=private)
        checked(edited_scene_status, 200, "private exact edited Reader Scene after MoveNode")
        edited_scene_reply = json.loads(edited_scene_raw)
        if (
            edited_scene_reply.get("protocol_version") != "chaptera.temporary-editor-scene.v1"
            or edited_scene_reply.get("document_id") != session_id
            or edited_scene_reply.get("source_hash") != expected_hash
            or edited_scene_reply.get("revision_id") != accepted["revision_id"]
            or edited_scene_reply.get("expires_at_ms") != move_expiry
        ):
            raise AssertionError("edited Guest Scene is not the exact accepted revision")
        edited_rich_scene = edited_scene_reply.get("scene")
        if not isinstance(edited_rich_scene, dict) or not isinstance(edited_rich_scene.get("nodes"), list):
            raise AssertionError("edited Guest Scene lost source-neutral page geometry")
        matches = [
            node for node in edited_rich_scene["nodes"]
            if isinstance(node, dict) and node.get("node_id") == chosen_id
        ]
        if len(matches) != 1 or (
            matches[0].get("bounds", {}).get("x") != after_bounds["x"]
            or matches[0].get("bounds", {}).get("y") != after_bounds["y"]
        ):
            raise AssertionError("real edited Reader Scene did not paint moved canonical node")
        if live.sha256_file(physical_source) != expected_hash:
            raise AssertionError("revised Scene changed the immutable original PUB")

        # The very same pinned moved revision must produce an actual modified
        # editable package, not a converter re-reading immutable PUB bytes.
        expected_rect = geometry.RectEmu(
            x=after_bounds["x"], y=after_bounds["y"],
            width=after_bounds["width"], height=after_bounds["height"],
        )
        query = urlencode({
            "revision_id": accepted["revision_id"],
            "canonical_authoring_revision_id": accepted["canonical_authoring_revision_id"],
        })
        edited_formats = []
        unavailable_formats = []
        export_receipts = []
        for target in ("odg", "idml", "pdf"):
            export_path = f"/v1/editor/temporary-sessions/{session_id}/exports/{target}?{query}"
            checked(call("GET", export_path)[0], 401, "private edited export capability")
            checked(call(
                "GET", export_path,
                headers=["x-chaptera-reader-session: " + secrets.token_hex(32)],
            )[0], 404, "foreign token cannot download edited output")
            pinned_stale = urlencode({
                "revision_id": revision,
                "canonical_authoring_revision_id": authoring_revision,
            })
            checked(call(
                "GET", export_path.split("?")[0] + "?" + pinned_stale,
                headers=private,
            )[0], 409, "original revision cannot be substituted for edited artifact")
            head_status, head_raw = call(
                "GET", export_path + "&report_only=true", headers=private,
            )
            if head_status == 422:
                if target == "pdf":
                    raise AssertionError("current edited PDF is blocked for the pinned real PUB")
                unavailable_formats.append(target)
                continue
            checked(head_status, 200, "private current edit LossReport")
            preflight = json.loads(head_raw)
            if (
                preflight.get("protocol_version") != "chaptera.temporary-edited-export-report.v1"
                or preflight.get("source_hash") != expected_hash
                or preflight.get("revision_id") != accepted["revision_id"]
                or preflight.get("canonical_authoring_revision_id") != accepted["canonical_authoring_revision_id"]
                or preflight.get("target") != target
                or preflight.get("expires_at_ms") != move_expiry
                or not isinstance(preflight.get("loss_report"), dict)
                or preflight["loss_report"].get("can_serialize") is not True
                or preflight["loss_report"].get("counts", {}).get("blocking") != 0
            ):
                raise AssertionError("preflight did not prove exact amended canonical output")
            headers_file = work / ("output-" + target + "-headers.txt")
            status, artifact = call(
                "GET", export_path, headers=private, dump_headers=headers_file,
            )
            checked(status, 200, "physical edited " + target + " download")
            headers_out = live.parse_header_file(headers_file)
            actual_sha = hashlib.sha256(artifact).hexdigest()
            if (
                not artifact.startswith(b"%PDF-" if target == "pdf" else b"PK")
                or actual_sha != preflight["artifact_sha256"]
                or headers_out.get("x-chaptera-export-sha256") != actual_sha
                or headers_out.get("x-chaptera-loss-report-sha256") != preflight["loss_report_sha256"]
                or headers_out.get("x-chaptera-edit-revision") != accepted["revision_id"]
                or headers_out.get("cache-control") != "no-store"
                or headers_out.get("x-content-type-options") != "nosniff"
                or not headers_out.get("content-disposition", "").startswith("attachment; filename=")
                or target not in headers_out.get("content-disposition", "")
            ):
                raise AssertionError("edited package response MIME/identity/private download mismatch")
            expected_mime = {
                "odg": "application/vnd.oasis.opendocument.graphics",
                "idml": "application/vnd.adobe.indesign-idml-package",
                "pdf": "application/pdf",
            }[target]
            if headers_out.get("content-type") != expected_mime:
                raise AssertionError("physical edited output MIME was substituted")
            local_artifact = work / ("edited-" + target + "." + target)
            local_artifact.write_bytes(artifact)
            try:
                observed = (
                    verify_physical_edited_pdf(artifact, expected_rect)
                    if target == "pdf" else
                    geometry.verify_export(local_artifact, target, chosen_id, expected_rect)
                )
            except (AssertionError, ValueError) as exc:
                raise AssertionError(
                    f"edited {target} did not preserve the accepted physical geometry"
                ) from exc
            export_receipts.append({
                "target": target, "sha256": actual_sha,
                "artifact_length": len(artifact),
                "geometry_form": observed.get("geometry_form"),
            })
            edited_formats.append(target)
        if not edited_formats:
            raise AssertionError(
                "both editable formats blocked or failed the physical edited-geometry oracle: "
                + repr(unavailable_formats)
            )
        if list(work.rglob("edited-export.bin")):
            raise AssertionError("isolated worker retained private edited-export scratch bytes")
        if live.sha256_file(physical_source) != expected_hash:
            raise AssertionError("physical edit export modified immutable source PUB")
        if "pdf" not in edited_formats:
            raise AssertionError("temporary guest output never proved physical changed PDF")

        replay_status, replay_raw = call(
            "POST", move_path, headers=private, body_json=chosen
        )
        checked(replay_status, 200, "exact-idempotency MoveNode retry")
        replay = json.loads(replay_raw)
        if (
            replay.get("replayed") is not True
            or replay.get("revision_id") != accepted["revision_id"]
            or replay.get("expires_at_ms") != move_expiry
        ):
            raise AssertionError("replayed operation illegally inserted a new revision or moved TTL")
        conflicting = json.loads(json.dumps(chosen))
        conflicting["command"]["x_emu"] += 1
        checked(call("POST", move_path, headers=private, body_json=conflicting)[0],
                409, "same operation ID with changed intent conflicts")
        stale = json.loads(json.dumps(chosen))
        stale["client_operation_id"] = "physical-stale-after-move"
        checked(call("POST", move_path, headers=private, body_json=stale)[0],
                409, "stale base cannot commit another MoveNode")
        final_status, final_raw = call("GET", path, headers=private)
        checked(final_status, 200, "current state after rejected replays")
        final = json.loads(final_raw)
        if final["revision_cursor"] != 1 or final["expires_at_ms"] != move_expiry:
            raise AssertionError("failed/replayed operations modified revision count or TTL")

        status, raw = call(
            "POST", "/v1/reader/guest-sessions",
            body_json={"expected_byte_len": fixture.stat().st_size},
        )
        checked(status, 200, "ordinary Guest Reader session issuance")
        guest = json.loads(raw)
        checked(call(
            "GET", f"/v1/editor/temporary-sessions/{guest['session_id']}/current",
            headers=[f"x-chaptera-reader-session: {guest['access_token']}"],
        )[0], 404, "ordinary guest forbidden from authoring current")

        with sqlite3.connect(db) as conn:
            conn.execute(
                "UPDATE reader_guest_sessions SET expires_at_ms=? WHERE session_id=?",
                (int(time.time() * 1000) - 1, session_id.encode()),
            )
        expired_status = call("GET", path, headers=private)[0]
        if expired_status not in {404, 410}:
            raise AssertionError("expired capability retained document access")
        checked(call("GET", scene_path, headers=private)[0], expired_status,
                "expired bearer may not read private edited Scene")
        checked(call(
            "GET", f"/v1/editor/temporary-sessions/{session_id}/exports/{edited_formats[0]}?{query}",
            headers=private,
        )[0], expired_status, "expired bearer may not download private edited artifact")
        with sqlite3.connect(db) as conn:
            remaining = conn.execute(
                "SELECT COUNT(*) FROM reader_guest_sessions WHERE session_id=?",
                (session_id.encode(),),
            ).fetchone()[0]
        if remaining:
            raise AssertionError("expired temporary session kept its private metadata")
        if physical_source.exists():
            raise AssertionError("expired temporary session retained original PUB bytes")
        with sqlite3.connect(db) as conn:
            expired_revisions = conn.execute(
                "SELECT COUNT(*) FROM revision_edges WHERE document_id=?",
                (session_id.encode(),),
            ).fetchone()[0]
            expired_bindings = conn.execute(
                "SELECT COUNT(*) FROM revision_identity_bindings WHERE document_id=?",
                (session_id.encode(),),
            ).fetchone()[0]
        if expired_revisions or expired_bindings:
            raise AssertionError("TTL deletion retained private canonical revision bytes")

        # A separate real-PUB source proves user-triggered cleanup while the
        # session is still live; TTL expiration above remains independently tested.
        delete_issue_status, delete_raw = call(
            "POST", "/v1/editor/temporary-sessions",
            body_json={"expected_byte_len": fixture.stat().st_size},
        )
        checked(delete_issue_status, 200, "early-delete capability issuance")
        delete_issue = json.loads(delete_raw)
        delete_id = delete_issue["session_id"]
        delete_token = delete_issue["access_token"]
        if not delete_id.startswith("tempedit:") or len(delete_token) != 64:
            raise AssertionError("early-delete issued invalid guest identity")
        delete_path = f"/v1/editor/temporary-sessions/{delete_id}"
        delete_private = [f"x-chaptera-reader-session: {delete_token}"]
        checked(call("DELETE", delete_path)[0], 401, "delete requires own private capability")
        checked(call("DELETE", delete_path, headers=[
            "x-chaptera-reader-session: " + secrets.token_hex(32)
        ])[0], 404, "foreign bearer cannot delete source")
        checked(call("PUT", delete_issue["upload_path"], upload=fixture,
                     headers=delete_private)[0], 200, "early-delete source upload")
        new_open_status, new_open_raw = call(
            "POST", delete_issue["open_path"], headers=delete_private,
        )
        checked(new_open_status, 200, "early-delete real PUB open")
        new_open = json.loads(new_open_raw)
        if new_open.get("classification") not in {"supported", "partial"}:
            raise AssertionError("early-delete source unexpectedly not Reader eligible")
        if new_open.get("source_sha256") != expected_hash:
            raise AssertionError("early-delete original source identity mismatch")
        new_source_files = [
            candidate for candidate in (work / "blob-store").rglob("*")
            if candidate.is_file() and candidate.stat().st_size == fixture.stat().st_size
            and live.sha256_file(candidate) == expected_hash
        ]
        if len(new_source_files) != 1:
            raise AssertionError("early-delete original source not physically present")
        new_physical_source = new_source_files[0]
        # Destroy an actual edited RevisionStream, not only a read-only upload.
        delete_current_status, delete_current_raw = call(
            "GET", delete_path + "/current", headers=delete_private,
        )
        checked(delete_current_status, 200, "early-delete canonical baseline")
        delete_current = json.loads(delete_current_raw)
        source_node = delete_current.get("authoring_graph", {}).get("nodes", {}).get(chosen_id)
        source_bounds = source_node.get("header", {}).get("bounds") if isinstance(source_node, dict) else None
        if not isinstance(source_bounds, dict):
            raise AssertionError("same real PUB lost deterministic moveable NodeId")
        delete_move = {
            "protocol_version": "chaptera.commit-request.v1",
            "document_id": delete_id,
            "source_hash": expected_hash,
            "base_revision_id": delete_current["revision_id"],
            "client_operation_id": "physical-delete-move",
            "command": {
                "kind": "move_node_to", "node_id": chosen_id,
                "x_emu": source_bounds["x"] + 6350,
                "y_emu": source_bounds["y"] + 6350,
            },
        }
        delete_move_status, delete_move_raw = call(
            "POST", delete_path + "/move-node",
            headers=delete_private, body_json=delete_move,
        )
        checked(delete_move_status, 200, "real canonical edit before explicit delete")
        delete_edit = json.loads(delete_move_raw)
        if delete_edit.get("revision_id") == delete_current["revision_id"]:
            raise AssertionError("delete test did not first create an edited revision")
        with sqlite3.connect(db) as conn:
            early_edges_before = conn.execute(
                "SELECT COUNT(*) FROM revision_edges WHERE document_id=?",
                (delete_id.encode(),),
            ).fetchone()[0]
        if early_edges_before != 1:
            raise AssertionError("delete test did not create one private revision edge")
        with sqlite3.connect(db) as conn:
            active_before_delete = conn.execute(
                "SELECT COUNT(*) FROM reader_guest_sessions "
                "WHERE session_id=? AND state='opened' AND expires_at_ms>?",
                (delete_id.encode(), int(time.time() * 1000)),
            ).fetchone()[0]
        if active_before_delete != 1:
            raise AssertionError("early-delete target was not a live opened session")
        delete_headers = work / "early-delete-response-headers.txt"
        delete_status, _ = call(
            "DELETE", delete_path, headers=delete_private,
            dump_headers=delete_headers,
        )
        checked(delete_status, 204, "explicit physical early-delete acknowledgement")
        if live.parse_header_file(delete_headers).get("cache-control") != "no-store":
            raise AssertionError("early-delete response was cacheable")
        for action in (
            f"/v1/editor/temporary-sessions/{delete_id}/current",
            f"/v1/editor/temporary-sessions/{delete_id}/scene",
            delete_issue["scene_path"],
        ):
            checked(call("GET", action, headers=delete_private)[0],
                    404, "deleted capability retained a private Reader endpoint")
        checked(call("DELETE", delete_path, headers=delete_private)[0],
                404, "deleted private capability resurrected on duplicate request")
        after_delete_query = urlencode({
            "revision_id": delete_edit["revision_id"],
            "canonical_authoring_revision_id": delete_edit["canonical_authoring_revision_id"],
        })
        checked(call(
            "GET", delete_path + "/exports/idml?" + after_delete_query,
            headers=delete_private,
        )[0], 404, "deleted bearer could download edited canonical bytes")
        if new_physical_source.exists():
            raise AssertionError("user deletion retained quarantined original PUB bytes")
        with sqlite3.connect(db) as conn:
            early_remaining = conn.execute(
                "SELECT COUNT(*) FROM reader_guest_sessions WHERE session_id=?",
                (delete_id.encode(),),
            ).fetchone()[0]
            early_edges = conn.execute(
                "SELECT COUNT(*) FROM revision_edges WHERE document_id=?",
                (delete_id.encode(),),
            ).fetchone()[0]
            early_bindings = conn.execute(
                "SELECT COUNT(*) FROM revision_identity_bindings WHERE document_id=?",
                (delete_id.encode(),),
            ).fetchone()[0]
        if early_remaining or early_edges or early_bindings:
            raise AssertionError("user delete did not purge private session/revision rows")

        receipt = {
            "schema": "chaptera.tempedit-canonical-current-https.v1",
            "source_sha256": expected_hash, "source_byte_len": fixture.stat().st_size,
            "canonical_current_status": current_status, "revision_cursor": 0,
            "private_capability_required": True, "foreign_token_rejected": True,
            "ordinary_guest_rejected": True, "no_read_ttl_extension": True,
            "expired_access_status": expired_status, "expired_rows": remaining, "physical_original_deleted": True,
            "isolated_real_pub_read": True, "backend_filesystem": True,
            "raw_source_or_bearer_in_receipt": False,
            "mutation_proven": True, "changed_revision_reopened": True,
            "exact_replay_ttl_unchanged": True,
            "foreign_mutation_capability_rejected": True,
            "failed_and_stale_ops_rejected": True,
            "bounded_rate_retries": rate_retries,
            "private_revision_rows_after_ttl": expired_revisions,
            "private_canonical_bindings_after_ttl": expired_bindings,
            "edited_download_proven": True, "edited_fixed_pdf_proven": "pdf" in edited_formats,
            "edited_target_profiles": edited_formats,
            "revised_reader_scene_proven": True, "revised_node_id": chosen_id,
            "editable_unavailable_targets": unavailable_formats,
            "edited_artifact_receipts": export_receipts,
            "early_delete_proven": True, "early_delete_status": delete_status,
            "early_delete_rows_after_purge": early_remaining,
            "early_delete_edited_revisions_before": early_edges_before,
            "early_delete_physical_original_removed": True,
            "early_delete_foreign_capability_rejected": True,
            "persistent_export_artifacts": False, "v0_go": False,
        }
        (out / "receipt.json").write_text(
            json.dumps(receipt, indent=2, sort_keys=True) + "\n", encoding="utf-8"
        )
        print(json.dumps({"canonical_current_status": current_status, "mutation_proven": True,
                          "edited_download_proven": True, "targets": edited_formats, "v0_go": False}))
    finally:
        live.terminate(proxy)
        live.terminate(server)
        c_log.close()
        s_log.close()
        oidc.shutdown()
        oidc.server_close()
        oidc_thread.join(timeout=2)
        if token is not None:
            for log in (server_log, caddy_log):
                if token in log.read_text(encoding="utf-8", errors="replace"):
                    raise AssertionError("private capability appeared in diagnostic logs")
        shutil.rmtree(work, ignore_errors=True)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
