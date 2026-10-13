#!/usr/bin/env python3
"""Real Chaptera OIDC+HTTPS browser PUB ingress with SQLite and filesystem BlobStore.

The service, validator, baseline producer and signed-in browser are live;
filesystem storage stands in for S3, and export remains out of scope.
"""

from __future__ import annotations

import argparse
import json
import os
import pathlib
import hashlib
import sys
import socket
import ssl
import subprocess
import tempfile
import time
import urllib.error
import urllib.request

from chaptera_local_oidc import OidcFixture

ROOT = pathlib.Path(__file__).resolve().parents[1]
SOURCE_CADDY = ROOT / "deploy" / "caddy" / "Caddyfile.example"
SOURCE_CONFIG = ROOT / "deploy" / "config" / "chaptera.prod.example.toml"
TARGET = ROOT / "target" / "cloud-real-pub-ingress"

EDGE_HOST = "edge.test"
HTTP_PORT = 18080
HTTPS_PORT = 18443
UPSTREAM_PORT = 18082
CONTAINER_NAME = "chaptera-secure-origin-oidc"


def run_checked(
    args: list[str],
    *,
    env: dict[str, str] | None = None,
) -> subprocess.CompletedProcess[str]:
    completed = subprocess.run(
        args,
        cwd=ROOT,
        env=env,
        capture_output=True,
        text=True,
        check=False,
    )
    if completed.returncode != 0:
        raise RuntimeError(
            f"command failed rc={completed.returncode}: {' '.join(args)}\n"
            f"stdout:\n{completed.stdout}\nstderr:\n{completed.stderr}"
        )
    return completed


def render_caddy() -> pathlib.Path:
    source = SOURCE_CADDY.read_text(encoding="utf-8")
    rendered = source.replace("cloud.example.invalid", EDGE_HOST)
    rendered = rendered.replace(
        f"{EDGE_HOST} {{",
        f"{EDGE_HOST} {{\n    tls internal",
        1,
    )
    rendered = rendered.replace(
        "127.0.0.1:8080",
        f"127.0.0.1:{UPSTREAM_PORT}",
        1,
    )
    rendered = rendered.replace(
        "header_up X-Forwarded-Host {host}",
        f"header_up X-Forwarded-Host {EDGE_HOST}:{HTTPS_PORT}",
        1,
    )
    rendered = (
        "{\n"
        f"    http_port {HTTP_PORT}\n"
        f"    https_port {HTTPS_PORT}\n"
        "}\n\n"
        + rendered
    )
    path = TARGET / "Caddyfile"
    path.write_text(rendered, encoding="utf-8")
    return path


def render_config(
    example: str,
    *,
    database: pathlib.Path,
    issuer: str,
    binary: pathlib.Path,
    temp: pathlib.Path,
) -> str:
    public_origin = f"https://{EDGE_HOST}:{HTTPS_PORT}"
    rendered = example
    rendered = rendered.replace(
        'environment = "prod"',
        'environment = "test"',
        1,
    )
    rendered = rendered.replace(
        'listen = "127.0.0.1:8080"',
        f'listen = "127.0.0.1:{UPSTREAM_PORT}"',
        1,
    )
    rendered = rendered.replace(
        'public_origin = "https://cloud.example.invalid"',
        f'public_origin = "{public_origin}"',
        1,
    )
    rendered = rendered.replace(
        'path = "/var/lib/chaptera/chaptera.sqlite"',
        f'path = "{database.as_posix()}"',
        1,
    )
    rendered = rendered.replace(
        'issuer = "https://id.example.invalid"',
        f'issuer = "{issuer}"',
        1,
    )
    def checked_replace(before: str, after: str, *, count: int = 1) -> None:
        nonlocal rendered
        if rendered.count(before) != count:
            raise RuntimeError(f"expected {count} config markers: {before!r}")
        rendered = rendered.replace(before, after, count)

    checked_replace('provider = "s3-compatible"', 'provider = "filesystem"')
    checked_replace('min_free_disk_bytes = 1073741824', 'min_free_disk_bytes = 1')
    checked_replace('isolation_python = "/usr/bin/python3"',
                    f'isolation_python = "{pathlib.Path(sys.executable).as_posix()}"', count=2)
    isolation = (ROOT / "tools" / "migration_pdf_worker_isolation.py").resolve()
    checked_replace('isolation_harness = "/opt/chaptera/current/tools/migration_pdf_worker_isolation.py"',
                    f'isolation_harness = "{isolation.as_posix()}"', count=2)
    checked_replace('worker_binary = "/opt/chaptera/current/chaptera"',
                    f'worker_binary = "{binary.as_posix()}"', count=2)
    checked_replace('temp_root = "/var/lib/chaptera/source-scan-tmp"',
                    f'temp_root = "{(temp / "scan").as_posix()}"')
    checked_replace('temp_root = "/var/lib/chaptera/source-baseline-tmp"',
                    f'temp_root = "{(temp / "baseline").as_posix()}"')
    return rendered


def wait_live(server: subprocess.Popen[str]) -> None:
    deadline = time.monotonic() + 20
    last: object = None
    while time.monotonic() < deadline:
        if server.poll() is not None:
            raise RuntimeError(
                f"Chaptera server exited before /live became ready: {server.returncode}"
            )
        try:
            with urllib.request.urlopen(
                f"http://127.0.0.1:{UPSTREAM_PORT}/live",
                timeout=1,
            ) as response:
                if response.status == 200:
                    return
                last = response.status
        except (OSError, urllib.error.HTTPError) as error:
            last = error
        time.sleep(0.2)
    raise RuntimeError(f"Chaptera /live did not become ready: {last}")


def wait_tls(caddy: subprocess.Popen[str]) -> None:
    context = ssl.create_default_context()
    context.check_hostname = False
    context.verify_mode = ssl.CERT_NONE
    deadline = time.monotonic() + 30
    last: object = None
    while time.monotonic() < deadline:
        if caddy.poll() is not None:
            raise RuntimeError(
                f"Caddy exited before TLS became ready: {caddy.returncode}"
            )
        try:
            with socket.create_connection(
                ("127.0.0.1", HTTPS_PORT),
                timeout=1,
            ) as raw:
                with context.wrap_socket(raw, server_hostname=EDGE_HOST):
                    return
        except OSError as error:
            last = error
        time.sleep(0.25)
    raise RuntimeError(f"Caddy TLS did not become ready: {last}")


def terminate(process: subprocess.Popen[str] | None) -> None:
    if process is None or process.poll() is not None:
        return
    process.terminate()
    try:
        process.wait(timeout=5)
    except subprocess.TimeoutExpired:
        process.kill()
        process.wait(timeout=5)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", type=pathlib.Path, required=True)
    parser.add_argument(
        "--browser-script",
        type=pathlib.Path,
        default=ROOT / "tools" / "run_secure_origin_real_pub_browser.mjs",
    )
    parser.add_argument(
        "--restart-browser-script",
        type=pathlib.Path,
        default=ROOT / "tools" / "run_secure_origin_real_pub_restart_browser.mjs",
    )
    parser.add_argument(
        "--cross-principal-browser-script",
        type=pathlib.Path,
        default=ROOT / "tools" / "run_secure_origin_cross_principal_browser.mjs",
    )
    parser.add_argument("--caddy-image", default="caddy:2.10.2-alpine")
    parser.add_argument("--fixture", type=pathlib.Path, required=True)
    parser.add_argument("--fixture-sha256", required=True)
    parser.add_argument("--story-browser-script", type=pathlib.Path)
    parser.add_argument("--story-fixture", type=pathlib.Path)
    parser.add_argument("--story-fixture-sha256")
    args = parser.parse_args()
    story_flags = (
        args.story_browser_script is not None,
        args.story_fixture is not None,
        args.story_fixture_sha256 is not None,
    )
    if any(story_flags) and not all(story_flags):
        raise SystemExit("Story acceptance requires script, exact fixture and SHA together")

    fixture = args.fixture.resolve()
    if not fixture.is_file() or hashlib.sha256(fixture.read_bytes()).hexdigest() != args.fixture_sha256:
        raise SystemExit("real pinned PUB SHA does not match expected fixture")
    binary = args.binary.resolve()
    browser_script = args.browser_script.resolve()
    story_browser_script = args.story_browser_script.resolve() if args.story_browser_script else None
    story_fixture = args.story_fixture.resolve() if args.story_fixture else None
    if story_fixture is not None:
        if not story_fixture.is_file() or hashlib.sha256(story_fixture.read_bytes()).hexdigest() != args.story_fixture_sha256:
            raise SystemExit("pinned real Story PUB SHA mismatch")
        if not story_browser_script.is_file():
            raise SystemExit("Story browser script not found")
    restart_browser_script = args.restart_browser_script.resolve()
    cross_principal_browser_script = args.cross_principal_browser_script.resolve()
    if not binary.is_file():
        raise SystemExit(f"Chaptera binary not found: {binary}")
    if not browser_script.is_file():
        raise SystemExit(f"browser script not found: {browser_script}")
    if not restart_browser_script.is_file():
        raise SystemExit(f"restart browser script not found: {restart_browser_script}")
    if not cross_principal_browser_script.is_file():
        raise SystemExit(
            f"cross-principal browser script not found: {cross_principal_browser_script}"
        )

    TARGET.mkdir(parents=True, exist_ok=True)
    caddy_config = render_caddy()
    subprocess.run(
        ["docker", "rm", "-f", CONTAINER_NAME],
        cwd=ROOT,
        capture_output=True,
        text=True,
        check=False,
    )

    server: subprocess.Popen[str] | None = None
    worker: subprocess.Popen[str] | None = None
    caddy: subprocess.Popen[str] | None = None
    server_log = TARGET / "server.log"
    caddy_log = TARGET / "caddy.log"
    worker_log = TARGET / "worker.log"
    browser_log = TARGET / "browser.log"
    story_browser_log = TARGET / "browser-story.log"
    cross_principal_browser_log = TARGET / "browser-cross-principal.log"
    restart_server_log = TARGET / "server-restart.log"
    restart_worker_log = TARGET / "worker-restart.log"
    restart_browser_log = TARGET / "browser-restart.log"
    server_handle = None
    caddy_handle = None
    worker_handle = None

    try:
        with tempfile.TemporaryDirectory(prefix="chaptera-secure-origin-") as temp_raw:
            temp = pathlib.Path(temp_raw)
            database = temp / "chaptera.sqlite"
            (temp / "scan").mkdir()
            (temp / "baseline").mkdir()
            credentials = temp / "credentials"
            credentials.mkdir()
            secret = credentials / "oidc_client_secret"
            secret.write_text("chaptera-local-only-secret\n", encoding="utf-8")
            if os.name == "posix":
                secret.chmod(0o600)

            redirect_uri = (
                f"https://{EDGE_HOST}:{HTTPS_PORT}/v1/auth/callback"
            )
            with OidcFixture(redirect_uri=redirect_uri) as provider:
                config = temp / "chaptera.toml"
                config.write_text(
                    render_config(
                        SOURCE_CONFIG.read_text(encoding="utf-8"),
                        database=database,
                        issuer=provider.issuer,
                        binary=binary,
                        temp=temp,
                    ),
                    encoding="utf-8",
                )

                env = os.environ.copy()
                env.update(
                    {
                        "CREDENTIALS_DIRECTORY": str(credentials),
                        "AWS_ACCESS_KEY_ID": "chaptera-secure-origin",
                        "AWS_SECRET_ACCESS_KEY": "chaptera-secure-origin",
                        "AWS_REGION": "us-east-1",
                        "AWS_EC2_METADATA_DISABLED": "true",
                    }
                )

                run_checked(
                    [str(binary), "--config", str(config), "migrate", "up"],
                    env=env,
                )

                server_handle = server_log.open("w", encoding="utf-8")
                server = subprocess.Popen(
                    [str(binary), "--config", str(config), "serve"],
                    cwd=ROOT,
                    env=env,
                    stdout=server_handle,
                    stderr=subprocess.STDOUT,
                    text=True,
                )
                wait_live(server)

                # Serialize SQLite WAL startup: the server must open before the
                # real parse worker begins consuming SourceValidation jobs.
                worker_handle = worker_log.open("w", encoding="utf-8")
                worker = subprocess.Popen(
                    [str(binary), "--config", str(config), "worker"],
                    cwd=ROOT, env=env, stdout=worker_handle,
                    stderr=subprocess.STDOUT, text=True,
                )

                caddy_handle = caddy_log.open("w", encoding="utf-8")
                caddy = subprocess.Popen(
                    [
                        "docker",
                        "run",
                        "--rm",
                        "--name",
                        CONTAINER_NAME,
                        "--network",
                        "host",
                        "-v",
                        f"{caddy_config.resolve()}:/etc/caddy/Caddyfile:ro",
                        "--entrypoint",
                        "caddy",
                        args.caddy_image,
                        "run",
                        "--config",
                        "/etc/caddy/Caddyfile",
                        "--adapter",
                        "caddyfile",
                    ],
                    cwd=ROOT,
                    stdout=caddy_handle,
                    stderr=subprocess.STDOUT,
                    text=True,
                )
                wait_tls(caddy)

                browser_env = env.copy()
                browser_env.update(
                    {
                        "CHAPTERA_SOURCE_PUB_FIXTURE": str(fixture),
                        "CHAPTERA_SOURCE_PUB_SHA256": args.fixture_sha256,
                        "CHAPTERA_SECURE_ORIGIN": (
                            f"https://{EDGE_HOST}:{HTTPS_PORT}"
                        ),
                        "CHAPTERA_OIDC_PROVIDER_ORIGIN": provider.issuer,
                        "CHAPTERA_SECURE_RECEIPT": str(
                            (TARGET / "receipt.json").resolve()
                        ),
                        "CHAPTERA_SECURE_SCREENSHOT": str(
                            (TARGET / "session.png").resolve()
                        ),
                        "CHAPTERA_BROWSER_STORAGE_STATE": str(
                            (TARGET / "browser-storage-state.json").resolve()
                        ),
                    }
                )
                browser = subprocess.run(
                    ["node", str(browser_script)],
                    cwd=ROOT,
                    env=browser_env,
                    capture_output=True,
                    text=True,
                    check=False,
                )
                browser_log.write_text(
                    browser.stdout + "\n--- stderr ---\n" + browser.stderr,
                    encoding="utf-8",
                )
                if worker.poll() is not None:
                    raise RuntimeError(f"real SourceValidation worker exited: {worker.returncode}")
                if browser.returncode != 0:
                    raise RuntimeError(
                        "secure-origin Chromium acceptance failed; "
                        f"see {browser_log}"
                    )
                if not (TARGET / "receipt.json").is_file():
                    raise RuntimeError(
                        "secure-origin Chromium acceptance emitted no receipt"
                    )

                # A separate genuine source-backed Story edit uses the same
                # existing OIDC session and live Product runtime, not a
                # second authentication, storage or revision implementation.
                if story_browser_script is not None:
                    story_engines = [
                        item.strip()
                        for item in os.environ.get(
                            "CHAPTERA_STORY_BROWSER_ENGINES", "chromium"
                        ).split(",")
                        if item.strip()
                    ]
                    if (
                        not story_engines
                        or len(story_engines) != len(set(story_engines))
                        or any(item not in {"chromium", "webkit"} for item in story_engines)
                    ):
                        raise RuntimeError(
                            "Story browser engines must be unique chromium/webkit values"
                        )
                    story_receipts = []
                    for story_engine in story_engines:
                        story_receipt_path = (
                            TARGET / f"story-receipt-{story_engine}.json"
                        ).resolve()
                        engine_story_log = TARGET / f"browser-story-{story_engine}.log"
                        story_env = browser_env.copy()
                        story_env.update(
                            {
                                "CHAPTERA_STORY_PUB_FIXTURE": str(story_fixture),
                                "CHAPTERA_STORY_PUB_SHA256": args.story_fixture_sha256,
                                "CHAPTERA_STORY_RECEIPT": str(story_receipt_path),
                                "CHAPTERA_STORY_BROWSER": story_engine,
                            }
                        )
                        story_browser = subprocess.run(
                            ["node", str(story_browser_script)],
                            cwd=ROOT,
                            env=story_env,
                            capture_output=True,
                            text=True,
                            check=False,
                        )
                        engine_story_log.write_text(
                            story_browser.stdout
                            + "\n--- stderr ---\n"
                            + story_browser.stderr,
                            encoding="utf-8",
                        )
                        if worker.poll() is not None:
                            raise RuntimeError(
                                "real SourceValidation worker exited during Story "
                                f"acceptance: {worker.returncode}"
                            )
                        if story_browser.returncode != 0:
                            raise RuntimeError(
                                "real Product Story browser acceptance failed for "
                                f"{story_engine}; see {engine_story_log}"
                            )
                        if not story_receipt_path.is_file():
                            raise RuntimeError(
                                "real Product Story browser emitted no bounded receipt "
                                f"for {story_engine}"
                            )
                        story_receipt = json.loads(
                            story_receipt_path.read_text(encoding="utf-8")
                        )
                        if (
                            story_receipt.get("schema")
                            != "chaptera.real-product-story-browser-acceptance.v1"
                            or story_receipt.get("browser_engine") != story_engine
                            or not story_receipt.get("browser_version")
                            or story_receipt.get("source_immutable_sha256")
                            != args.story_fixture_sha256
                            or not story_receipt.get("one_authorized_story_commit")
                            or not story_receipt.get("browser_reload_preserves_story")
                            or not story_receipt.get("anonymous_read_denied")
                        ):
                            raise RuntimeError(
                                f"invalid exact {story_engine} Story acceptance receipt"
                            )
                        story_receipts.append(story_receipt)

                    aggregate_story_receipt = {
                        "schema": (
                            "chaptera.real-product-story-two-engine-acceptance.v1"
                        ),
                        "head_sha": os.environ.get("CHAPTERA_HEAD_SHA", "unknown"),
                        "source_immutable_sha256": args.story_fixture_sha256,
                        "browser_engines": [
                            {
                                "engine": receipt["browser_engine"],
                                "version": receipt["browser_version"],
                                "document_id": receipt["document_id"],
                                "original_revision_id": receipt[
                                    "original_revision_id"
                                ],
                                "committed_revision_id": receipt[
                                    "committed_revision_id"
                                ],
                            }
                            for receipt in story_receipts
                        ],
                        "two_engine_claim": set(story_engines)
                        == {"chromium", "webkit"},
                        "browser_layout_is_canonical": False,
                        "authoritative_relayout_overset_verified": False,
                        "story_undo_redo_claim": False,
                        "export_job_claim": False,
                        "source_bytes_in_receipt": False,
                    }
                    (TARGET / "story-receipt.json").write_text(
                        json.dumps(aggregate_story_receipt, indent=2) + "\n",
                        encoding="utf-8",
                    )

                # Same issuer/JWKS, different OIDC subject, fresh browser context.
                # This proves guessed durable DocumentId does not grant a second
                # principal any current/revision/Reader payload.
                primary_subject = provider.subject
                primary_email = provider.email
                provider.subject = "chaptera-cross-principal-user"
                provider.email = "cross-principal@chaptera.invalid"
                try:
                    cross_principal_browser = subprocess.run(
                        ["node", str(cross_principal_browser_script)],
                        cwd=ROOT,
                        env=browser_env,
                        capture_output=True,
                        text=True,
                        check=False,
                    )
                finally:
                    provider.subject = primary_subject
                    provider.email = primary_email
                cross_principal_browser_log.write_text(
                    cross_principal_browser.stdout
                    + "\n--- stderr ---\n"
                    + cross_principal_browser.stderr,
                    encoding="utf-8",
                )
                if cross_principal_browser.returncode != 0:
                    raise RuntimeError(
                        "cross-principal Product authorization acceptance failed; "
                        f"see {cross_principal_browser_log}"
                    )
                if worker.poll() is not None:
                    raise RuntimeError(
                        f"real SourceValidation worker exited during cross-principal proof: "
                        f"{worker.returncode}"
                    )

                # Prove persistence rather than page-local continuity: stop the
                # canonical service and worker, then reopen the same SQLite DB
                # and filesystem BlobStore with fresh processes.
                terminate(worker)
                worker = None
                terminate(server)
                server = None
                if worker_handle is not None:
                    worker_handle.close()
                    worker_handle = None
                if server_handle is not None:
                    server_handle.close()
                    server_handle = None

                server_handle = restart_server_log.open("w", encoding="utf-8")
                server = subprocess.Popen(
                    [str(binary), "--config", str(config), "serve"],
                    cwd=ROOT,
                    env=env,
                    stdout=server_handle,
                    stderr=subprocess.STDOUT,
                    text=True,
                )
                wait_live(server)
                worker_handle = restart_worker_log.open("w", encoding="utf-8")
                worker = subprocess.Popen(
                    [str(binary), "--config", str(config), "worker"],
                    cwd=ROOT,
                    env=env,
                    stdout=worker_handle,
                    stderr=subprocess.STDOUT,
                    text=True,
                )

                restart_browser = subprocess.run(
                    ["node", str(restart_browser_script)],
                    cwd=ROOT,
                    env=browser_env,
                    capture_output=True,
                    text=True,
                    check=False,
                )
                restart_browser_log.write_text(
                    restart_browser.stdout + "\n--- stderr ---\n" + restart_browser.stderr,
                    encoding="utf-8",
                )
                if worker.poll() is not None:
                    raise RuntimeError(
                        f"restarted SourceValidation worker exited: {worker.returncode}"
                    )
                if restart_browser.returncode != 0:
                    raise RuntimeError(
                        "post-restart Chromium reopen failed; "
                        f"see {restart_browser_log}"
                    )
    finally:
        terminate(worker)
        terminate(server)
        subprocess.run(
            ["docker", "rm", "-f", CONTAINER_NAME],
            cwd=ROOT,
            capture_output=True,
            text=True,
            check=False,
        )
        terminate(caddy)
        if server_handle is not None:
            server_handle.close()
        if caddy_handle is not None:
            caddy_handle.close()
        if worker_handle is not None:
            worker_handle.close()

    print((TARGET / "receipt.json").read_text(encoding="utf-8"))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
