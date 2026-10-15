#!/usr/bin/env python3
"""Bootstrap a separate low-privilege Chaptera Windows Actions runner.

The default mode is read-only. --install requires a non-admin Windows account,
an explicit acknowledgement of OS-account separation and a temporary GitHub
runner registration token entered directly on the machine.
"""
from __future__ import annotations

import argparse
import ctypes
import getpass
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import shutil
import socket
import stat
import subprocess
import sys
import tempfile
import urllib.request
import zipfile


REPO_URL = "https://github.com/Lalalalendia/rar2"
RELEASE_API = "https://api.github.com/repos/actions/runner/releases/latest"
ASSET_PREFIX = "https://github.com/actions/runner/releases/download/"
LABEL = "chaptera-dev-unprivileged"
SCHEMA = "chaptera.dev-runner-bootstrap.v1"
ASSET_RE = re.compile(r"^actions-runner-win-x64-[0-9]+\.[0-9]+\.[0-9]+\.zip$")
HEX64_RE = re.compile(r"^[a-f0-9]{64}$")
TOKEN_RE = re.compile(r"^[A-Za-z0-9_-]{12,256}$")
MAX_DOWNLOAD = 600 * 1024 * 1024
MAX_EXPANDED = 2 * 1024 * 1024 * 1024
MAX_MEMBERS = 25000
TOOLS = ("git", "cargo", "rustc", "node", "pwsh")


class BootstrapError(RuntimeError):
    """Only a source-safe machine error code."""


def install_root(environ: dict[str, str]) -> Path:
    localappdata = environ.get("LOCALAPPDATA", "")
    if not localappdata or not Path(localappdata).is_absolute():
        raise BootstrapError("localappdata_unavailable")
    user_home = Path(localappdata).resolve()
    return user_home / "Chaptera" / "DevRunner"


def is_administrator() -> bool:
    try:
        return bool(ctypes.windll.shell32.IsUserAnAdmin())
    except (AttributeError, OSError) as exc:
        raise BootstrapError("admin_check_unavailable") from exc


def readiness(*, os_name: str, architecture: str, admin: bool,
              python_version: tuple[int, int], binaries: set[str],
              installed: bool) -> dict:
    blockers: list[str] = []
    if os_name != "Windows" or architecture.lower() not in ("amd64", "x86_64"):
        blockers.append("windows_x64_required")
    if admin:
        blockers.append("administrator_not_allowed")
    if python_version < (3, 12):
        blockers.append("python_3_12_required")
    blockers.extend("missing_" + executable for executable in TOOLS if executable not in binaries)
    if installed:
        blockers.append("install_root_already_exists")
    return {"schema": SCHEMA, "ready": not blockers,
            "blockers": blockers, "runner_label": LABEL, "mode": "read_only_preflight"}


def inspect_host() -> tuple[dict, Path | None]:
    root: Path | None = None
    try:
        root = install_root(os.environ)
        root_exists = root.exists() or root.is_symlink()
    except BootstrapError:
        root_exists = False
    try:
        admin = is_administrator() if os.name == "nt" else False
    except BootstrapError:
        admin = True
    report = readiness(
        os_name=platform.system(), architecture=platform.machine(), admin=admin,
        python_version=(sys.version_info.major, sys.version_info.minor),
        binaries={tool for tool in TOOLS if shutil.which(tool)},
        installed=root_exists,
    )
    if root is None:
        report["blockers"].append("localappdata_unavailable")
        report["ready"] = False
    return report, root


def select_official_asset(metadata: dict) -> tuple[str, str, str]:
    assets = metadata.get("assets")
    if not isinstance(assets, list):
        raise BootstrapError("invalid_release_metadata")
    found: list[tuple[str, str, str]] = []
    for asset in assets:
        if not isinstance(asset, dict):
            continue
        name = asset.get("name", "")
        if not isinstance(name, str) or ASSET_RE.fullmatch(name) is None:
            continue
        url = asset.get("browser_download_url", "")
        digest = asset.get("digest", "")
        if (not isinstance(url, str)
                or not url.startswith(ASSET_PREFIX)
                or not url.endswith("/" + name)):
            raise BootstrapError("untrusted_release_download_url")
        if not isinstance(digest, str) or not digest.startswith("sha256:"):
            raise BootstrapError("missing_official_sha256")
        hexdigest = digest.removeprefix("sha256:").lower()
        if HEX64_RE.fullmatch(hexdigest) is None:
            raise BootstrapError("invalid_release_sha256")
        found.append((name, url, hexdigest))
    if len(found) != 1:
        raise BootstrapError("missing_or_ambiguous_windows_release_asset")
    return found[0]


def metadata_from_github() -> dict:
    request = urllib.request.Request(
        RELEASE_API,
        headers={"Accept": "application/vnd.github+json",
                 "User-Agent": "Chaptera-Dev-Runner-Bootstrap/1"},
    )
    try:
        with urllib.request.urlopen(request, timeout=20) as response:
            raw = response.read(2_000_001)
    except (OSError, ValueError) as exc:
        raise BootstrapError("release_api_unavailable") from exc
    if len(raw) > 2_000_000:
        raise BootstrapError("release_metadata_too_large")
    try:
        result = json.loads(raw)
    except (ValueError, UnicodeError) as exc:
        raise BootstrapError("release_metadata_invalid_json") from exc
    if not isinstance(result, dict):
        raise BootstrapError("release_metadata_invalid")
    return result


def download_verified(url: str, digest: str, destination: Path) -> None:
    hasher = hashlib.sha256()
    total = 0
    request = urllib.request.Request(url, headers={"User-Agent": "Chaptera-Dev-Runner-Bootstrap/1"})
    try:
        with urllib.request.urlopen(request, timeout=60) as response:
            if not response.geturl().startswith("https://"):
                raise BootstrapError("release_redirect_not_https")
            with destination.open("xb") as output:
                while True:
                    chunk = response.read(1024 * 1024)
                    if not chunk:
                        break
                    total += len(chunk)
                    if total > MAX_DOWNLOAD:
                        raise BootstrapError("runner_archive_too_large")
                    hasher.update(chunk)
                    output.write(chunk)
    except (OSError, ValueError) as exc:
        raise BootstrapError("runner_download_failed") from exc
    if total == 0 or hasher.hexdigest() != digest:
        raise BootstrapError("runner_archive_checksum_mismatch")


def safe_archive_members(archive: zipfile.ZipFile) -> list[zipfile.ZipInfo]:
    members = archive.infolist()
    if not members or len(members) > MAX_MEMBERS:
        raise BootstrapError("runner_archive_member_count")
    seen: set[str] = set()
    total = 0
    for info in members:
        raw = info.filename.replace("\\", "/")
        parts = raw.rstrip("/").split("/")
        if (raw.startswith("/") or raw.startswith("//")
                or any(not p or p in (".", "..") or ":" in p
                       or p.rstrip(" .") != p for p in parts)):
            raise BootstrapError("unsafe_runner_archive_path")
        key = "/".join(parts).casefold()
        if key in seen:
            raise BootstrapError("duplicate_runner_archive_path")
        seen.add(key)
        kind = (info.external_attr >> 16) & 0o170000
        if kind not in (0, stat.S_IFREG, stat.S_IFDIR):
            raise BootstrapError("runner_archive_symlink")
        total += info.file_size
        if total > MAX_EXPANDED or info.file_size > MAX_EXPANDED:
            raise BootstrapError("runner_archive_expansion_limit")
    return members


def extract_runner(archive_path: Path, destination: Path) -> None:
    try:
        with zipfile.ZipFile(archive_path) as archive:
            safe_archive_members(archive)
            archive.extractall(destination)
    except (OSError, zipfile.BadZipFile) as exc:
        raise BootstrapError("runner_archive_invalid") from exc
    if not (destination / "config.cmd").is_file() or not (destination / "run.cmd").is_file():
        raise BootstrapError("runner_archive_missing_entrypoints")


def prompt_registration_token() -> str:
    try:
        value = getpass.getpass("Paste GitHub one-hour runner registration token (hidden): ").strip()
    except (EOFError, KeyboardInterrupt) as exc:
        raise BootstrapError("registration_token_not_supplied") from exc
    if TOKEN_RE.fullmatch(value) is None:
        raise BootstrapError("invalid_runner_registration_token")
    return value


def safe_runner_name(hostname: str) -> str:
    sanitized = re.sub(r"[^a-z0-9-]+", "-", hostname.lower()).strip("-")[:36]
    if not sanitized:
        raise BootstrapError("invalid_host_name")
    return "chaptera-dev-" + sanitized


def configure_runner(root: Path, token: str, name: str) -> None:
    if TOKEN_RE.fullmatch(token) is None:
        raise BootstrapError("invalid_runner_registration_token")
    command = [
        "cmd.exe", "/d", "/c", str(root / "config.cmd"), "--unattended",
        "--url", REPO_URL, "--token", token, "--name", name,
        "--labels", LABEL, "--work", "_work",
    ]
    try:
        result = subprocess.run(command, cwd=root, capture_output=True, timeout=180, check=False)
    except (OSError, subprocess.TimeoutExpired) as exc:
        raise BootstrapError("runner_registration_unavailable") from exc
    if result.returncode != 0:
        raise BootstrapError("runner_registration_failed")


def register_user_autostart(root: Path) -> None:
    if not (root / "run.cmd").is_file():
        raise BootstrapError("runner_run_script_missing")
    import winreg
    command = 'cmd.exe /d /c start "" "' + str(root / "run.cmd") + '"'
    path = r"Software\Microsoft\Windows\CurrentVersion\Run"
    try:
        with winreg.CreateKeyEx(winreg.HKEY_CURRENT_USER, path, 0, winreg.KEY_SET_VALUE) as key:
            winreg.SetValueEx(key, "Chaptera Dev Runner", 0, winreg.REG_SZ, command)
    except OSError as exc:
        raise BootstrapError("per_user_autostart_failed") from exc


def start_runner(root: Path) -> None:
    if not (root / "run.cmd").is_file():
        raise BootstrapError("runner_run_script_missing")
    try:
        subprocess.Popen(
            ["cmd.exe", "/d", "/c", str(root / "run.cmd")],
            cwd=root, creationflags=subprocess.CREATE_NEW_CONSOLE,
            stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
    except OSError as exc:
        raise BootstrapError("runner_start_failed") from exc


def install(root: Path, *, autostart: bool) -> dict:
    if root.exists() or root.is_symlink():
        raise BootstrapError("install_root_already_exists")
    root.parent.mkdir(parents=True, exist_ok=True)
    asset_name, url, digest = select_official_asset(metadata_from_github())
    with tempfile.TemporaryDirectory(prefix="chaptera-runner-stage-", dir=root.parent) as temporary:
        staging = Path(temporary)
        download_verified(url, digest, staging / asset_name)
        extracted = staging / "unpacked"
        extracted.mkdir()
        extract_runner(staging / asset_name, extracted)
        # Obtain the short-lived token last, once the release is fully verified.
        token = prompt_registration_token()
        # The official config.cmd writes runner-owned credential files locally.
        # Do not move it after registration or write this token to source/logs.
        os.replace(extracted, root)
        try:
            configure_runner(root, token, safe_runner_name(socket.gethostname()))
        finally:
            del token
    if autostart:
        register_user_autostart(root)
    start_runner(root)
    return {"schema": SCHEMA, "status": "started_unverified",
            "runner_label": LABEL, "release_asset": asset_name,
            "archive_sha256": digest, "autostart_at_user_logon": autostart,
            "github_online_verified": False, "workflow_activated": False}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--install", action="store_true", help="explicitly install and start official Actions runner")
    parser.add_argument("--confirm-separate-user", action="store_true",
                        help="affirm this non-admin Windows account has no Publisher, files or GitHub admin credentials")
    parser.add_argument("--autostart", action="store_true", help="register HKCU Run startup for this user on login")
    args = parser.parse_args()
    ready, root = inspect_host()
    if not args.install:
        print(json.dumps(ready, indent=2, sort_keys=True))
        return 0 if ready["ready"] else 2
    if not args.confirm_separate_user:
        print("CHAPTERA_BOOTSTRAP_BLOCKED separate_user_confirmation_required", file=sys.stderr)
        return 2
    if not ready["ready"] or root is None:
        print(json.dumps(ready, indent=2, sort_keys=True))
        return 2
    try:
        report = install(root, autostart=args.autostart)
    except (BootstrapError, OSError, ValueError) as exc:
        code = str(exc) if isinstance(exc, BootstrapError) else "installation_io_error"
        print("CHAPTERA_BOOTSTRAP_BLOCKED " + code, file=sys.stderr)
        return 2
    print(json.dumps(report, indent=2, sort_keys=True))
    print("NEXT: verify runner Online in GitHub, then enable CHAPTERA_LOCAL_DEV_RUNNER_ENABLED.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
