#!/usr/bin/env python3
"""Physical no-login temporary EditorSession CURRENT + MoveNode: exact PUB, HTTPS, TTL."""
from __future__ import annotations

import argparse
import json
import os
import pathlib
import secrets
import shutil
import sqlite3
import subprocess
import tempfile
import threading
import time

import run_cloud_reader_live_acceptance as live


def checked(status: int, expected: int, action: str) -> None:
    if status != expected:
        raise AssertionError(f"{action} HTTP {status}, expected {expected}")


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

    def call(method: str, path: str, *, headers: list[str] | None = None, **other):
        return live.curl_request(
            https_port=httpsport,
            method=method,
            path=path,
            output=response,
            headers=headers,
            **other,
        )

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
        nodes = current["authoring_graph"].get("nodes")
        if not isinstance(nodes, dict):
            raise AssertionError("canonical real PUB has no node map for editing")
        candidates = []
        for node_id, node in sorted(nodes.items()):
            if not isinstance(node_id, str) or not isinstance(node, dict):
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
        for idx, (node_id, x, y) in enumerate(candidates[:16]):
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
                f"no supported canonical MoveNode in first 16 real nodes; rejected={unsupported}"
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
            "private_revision_rows_after_ttl": expired_revisions,
            "private_canonical_bindings_after_ttl": expired_bindings,
            "edited_download_proven": False, "v0_go": False,
        }
        (out / "receipt.json").write_text(
            json.dumps(receipt, indent=2, sort_keys=True) + "\n", encoding="utf-8"
        )
        print(json.dumps({"canonical_current_status": current_status, "mutation_proven": True, "v0_go": False}))
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
