#!/usr/bin/env python3
"""Bounded local provenance preflight for FIXED-PDF-RENDERER-RECOVERY-01.

The preflight never executes recovered renderer binaries and never fetches from
the network. It searches only explicitly supplied local roots for:

- git repositories that still contain exact historical renderer commits;
- checked-out pub-pdf/pub-cli source trees;
- ZIP/TAR source archives containing those crate paths;
- narrowly named historical CLI binary candidates.

It emits a private receipt with local paths plus a sanitized public summary that
contains no local paths or archive member names.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import pathlib
import subprocess
import sys
import tarfile
import zipfile
from dataclasses import dataclass
from typing import Any, Iterable

SCHEMA_PRIVATE = "chaptera.fixed-pdf-renderer-recovery-private.v1"
SCHEMA_SUMMARY = "chaptera.fixed-pdf-renderer-recovery-summary.v1"

HISTORICAL_COMMITS = {
    "fixed_render_geometry": "5dbb6e7bbf0a88d0026309d9c4010cdf994dfcf6",
    "fixed_render_real_pub": "5dbb20f882e0a036e124d38e4550bf33fa2dc1c1",
    "fixed_render_images": "241cafdaa4f2054583aebd1cefad8a2da8082b53",
    "fixed_render_text": "5b91a5fa4fb90e3aa01ad11dda73ac53e7672c21",
    "public_pdf_cli": "a6b2ed6602c649d967ba45fcecdf8a30c3b5ac9c",
}

SOURCE_SENTINELS = (
    "crates/pub-pdf/Cargo.toml",
    "crates/pub-cli/Cargo.toml",
)

BINARY_NAMES = {
    "pub",
    "pub.exe",
    "pub-cli",
    "pub-cli.exe",
}

KNOWN_BUILD_BINARY_PATHS = (
    "target/release/pub",
    "target/release/pub.exe",
    "target/release/pub-cli",
    "target/release/pub-cli.exe",
    "target/debug/pub",
    "target/debug/pub.exe",
    "target/debug/pub-cli",
    "target/debug/pub-cli.exe",
)

ARCHIVE_SUFFIXES = (
    ".zip",
    ".tar",
    ".tar.gz",
    ".tgz",
)


class RecoveryPreflightError(RuntimeError):
    pass


@dataclass(frozen=True)
class ScanConfig:
    max_depth: int = 5
    max_archive_bytes: int = 512 * 1024 * 1024


def sha256_file(path: pathlib.Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def normalize_member(name: str) -> str:
    return name.replace("\\", "/").lstrip("./")


def member_has_source_sentinel(name: str) -> bool:
    normalized = normalize_member(name)
    return any(
        normalized == sentinel or normalized.endswith("/" + sentinel)
        for sentinel in SOURCE_SENTINELS
    )


def archive_source_sentinels(path: pathlib.Path) -> list[str]:
    lower = path.name.casefold()
    found: set[str] = set()
    try:
        if lower.endswith(".zip"):
            with zipfile.ZipFile(path) as archive:
                for name in archive.namelist():
                    normalized = normalize_member(name)
                    for sentinel in SOURCE_SENTINELS:
                        if normalized == sentinel or normalized.endswith("/" + sentinel):
                            found.add(sentinel)
        elif lower.endswith((".tar", ".tar.gz", ".tgz")):
            mode = "r:gz" if lower.endswith((".tar.gz", ".tgz")) else "r:"
            with tarfile.open(path, mode) as archive:
                for member in archive:
                    if not member.isfile():
                        continue
                    normalized = normalize_member(member.name)
                    for sentinel in SOURCE_SENTINELS:
                        if normalized == sentinel or normalized.endswith("/" + sentinel):
                            found.add(sentinel)
    except (OSError, zipfile.BadZipFile, tarfile.TarError):
        return []
    return sorted(found)


def run_git(repo: pathlib.Path, *args: str) -> subprocess.CompletedProcess[bytes]:
    return subprocess.run(
        ["git", "-C", str(repo), *args],
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )


def git_commit_paths(repo: pathlib.Path, commit: str) -> list[str]:
    exists = run_git(repo, "cat-file", "-e", commit + "^{commit}")
    if exists.returncode != 0:
        return []
    tree = run_git(
        repo,
        "ls-tree",
        "-r",
        "--name-only",
        commit,
        "--",
        "crates/pub-pdf",
        "crates/pub-cli",
    )
    if tree.returncode != 0:
        return []
    return [
        line.strip()
        for line in tree.stdout.decode("utf-8", errors="replace").splitlines()
        if line.strip()
    ]


def git_head(repo: pathlib.Path) -> str | None:
    result = run_git(repo, "rev-parse", "HEAD")
    if result.returncode != 0:
        return None
    value = result.stdout.decode("ascii", errors="ignore").strip()
    return value or None


def depth_from(root: pathlib.Path, path: pathlib.Path) -> int:
    return len(path.relative_to(root).parts)


def walk_bounded(root: pathlib.Path, max_depth: int) -> Iterable[tuple[pathlib.Path, list[str], list[str]]]:
    for current, dirs, files in os.walk(root, followlinks=False):
        current_path = pathlib.Path(current)
        depth = depth_from(root, current_path)
        if depth >= max_depth:
            dirs[:] = []
        else:
            dirs[:] = [
                item
                for item in dirs
                if item not in {".git", "node_modules", "target", ".venv", "__pycache__"}
            ]
        yield current_path, dirs, files


def inspect_git_repo(repo: pathlib.Path) -> dict[str, Any] | None:
    matched: dict[str, dict[str, Any]] = {}
    for label, commit in HISTORICAL_COMMITS.items():
        paths = git_commit_paths(repo, commit)
        if paths:
            matched[label] = {
                "commit": commit,
                "renderer_path_count": len(paths),
                "has_pub_pdf": any(path.startswith("crates/pub-pdf/") for path in paths),
                "has_pub_cli": any(path.startswith("crates/pub-cli/") for path in paths),
            }
    if not matched:
        return None
    return {
        "kind": "git_commit_provenance_match",
        "local_path": str(repo),
        "git_head": git_head(repo),
        "historical_matches": matched,
    }


def inspect_worktree(root: pathlib.Path) -> dict[str, Any] | None:
    present = [sentinel for sentinel in SOURCE_SENTINELS if (root / sentinel).is_file()]
    if not present:
        return None
    manifests = {
        sentinel: {
            "sha256": sha256_file(root / sentinel),
            "byte_len": (root / sentinel).stat().st_size,
        }
        for sentinel in present
    }
    return {
        "kind": "worktree_source_candidate",
        "local_path": str(root),
        "sentinels": present,
        "manifest_fingerprints": manifests,
    }


def inspect_archive(path: pathlib.Path, max_archive_bytes: int) -> dict[str, Any] | None:
    size = path.stat().st_size
    if size > max_archive_bytes:
        return None
    sentinels = archive_source_sentinels(path)
    if not sentinels:
        return None
    return {
        "kind": "source_archive_candidate",
        "local_path": str(path),
        "archive_sha256": sha256_file(path),
        "archive_byte_len": size,
        "sentinels": sentinels,
    }


def inspect_binary(path: pathlib.Path) -> dict[str, Any]:
    return {
        "kind": "unbound_binary_candidate",
        "local_path": str(path),
        "file_name": path.name,
        "sha256": sha256_file(path),
        "byte_len": path.stat().st_size,
        "executed": False,
    }


def scan_roots(roots: list[pathlib.Path], config: ScanConfig) -> list[dict[str, Any]]:
    candidates: list[dict[str, Any]] = []
    seen_git: set[pathlib.Path] = set()
    seen_worktree: set[pathlib.Path] = set()
    seen_files: set[pathlib.Path] = set()

    for raw_root in roots:
        root = raw_root.expanduser().resolve(strict=True)
        if not root.is_dir():
            raise RecoveryPreflightError(f"scan root is not a directory: {root}")

        for current, dirs, files in walk_bounded(root, config.max_depth):
            if (current / ".git").exists() and current not in seen_git:
                seen_git.add(current)
                candidate = inspect_git_repo(current)
                if candidate is not None:
                    candidates.append(candidate)

            if current not in seen_worktree:
                seen_worktree.add(current)
                candidate = inspect_worktree(current)
                if candidate is not None:
                    candidates.append(candidate)

            for relative in KNOWN_BUILD_BINARY_PATHS:
                path = current / relative
                if path in seen_files or not path.is_file():
                    continue
                seen_files.add(path)
                candidates.append(inspect_binary(path))

            for name in files:
                path = current / name
                if path in seen_files or not path.is_file():
                    continue
                seen_files.add(path)
                lower = name.casefold()
                if lower in BINARY_NAMES:
                    candidates.append(inspect_binary(path))
                    continue
                if lower.endswith(ARCHIVE_SUFFIXES):
                    candidate = inspect_archive(path, config.max_archive_bytes)
                    if candidate is not None:
                        candidates.append(candidate)

    return candidates


def make_private_receipt(
    roots: list[pathlib.Path],
    candidates: list[dict[str, Any]],
    *,
    authoritative_roots: bool,
    config: ScanConfig,
) -> dict[str, Any]:
    if candidates:
        status = "candidate_recovered"
    elif authoritative_roots:
        status = "renderer_lineage_bytes_unavailable"
    else:
        status = "no_candidate_in_scanned_roots"
    return {
        "schema_version": SCHEMA_PRIVATE,
        "status": status,
        "authoritative_roots": authoritative_roots,
        "scan_roots": [str(root.expanduser().resolve()) for root in roots],
        "max_depth": config.max_depth,
        "max_archive_bytes": config.max_archive_bytes,
        "network_fetch_performed": False,
        "candidate_execution_performed": False,
        "historical_commits": HISTORICAL_COMMITS,
        "candidates": candidates,
    }


def make_summary(private: dict[str, Any], private_bytes: bytes) -> dict[str, Any]:
    counts: dict[str, int] = {}
    matched_commits: set[str] = set()
    for candidate in private["candidates"]:
        kind = candidate["kind"]
        counts[kind] = counts.get(kind, 0) + 1
        for value in (candidate.get("historical_matches") or {}).values():
            commit = value.get("commit")
            if commit:
                matched_commits.add(commit)
    return {
        "schema_version": SCHEMA_SUMMARY,
        "status": private["status"],
        "authoritative_roots": private["authoritative_roots"],
        "root_count": len(private["scan_roots"]),
        "candidate_count": len(private["candidates"]),
        "candidate_kind_counts": dict(sorted(counts.items())),
        "matched_historical_commits": sorted(matched_commits),
        "private_receipt_sha256": hashlib.sha256(private_bytes).hexdigest(),
        "local_paths_emitted": False,
        "archive_member_names_emitted": False,
        "candidate_execution_performed": False,
        "network_fetch_performed": False,
    }


def encode_json(value: dict[str, Any]) -> bytes:
    return (json.dumps(value, indent=2, sort_keys=True) + "\n").encode("utf-8")


def write_bytes(path: pathlib.Path, data: bytes) -> None:
    path = path.expanduser().resolve()
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(data)


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Find authorized local bytes for the historical Chaptera fixed-PDF renderer lineage"
    )
    parser.add_argument("--root", action="append", required=True, type=pathlib.Path)
    parser.add_argument("--private-output", required=True, type=pathlib.Path)
    parser.add_argument("--summary-output", required=True, type=pathlib.Path)
    parser.add_argument("--max-depth", type=int, default=5)
    parser.add_argument(
        "--max-archive-bytes",
        type=int,
        default=512 * 1024 * 1024,
    )
    parser.add_argument(
        "--authoritative-roots",
        action="store_true",
        help="declare supplied roots exhaustive for authorized owner/local provenance surfaces",
    )
    args = parser.parse_args()

    if args.max_depth < 0:
        parser.error("--max-depth must be non-negative")
    if args.max_archive_bytes <= 0:
        parser.error("--max-archive-bytes must be positive")

    config = ScanConfig(
        max_depth=args.max_depth,
        max_archive_bytes=args.max_archive_bytes,
    )

    try:
        candidates = scan_roots(args.root, config)
        private = make_private_receipt(
            args.root,
            candidates,
            authoritative_roots=args.authoritative_roots,
            config=config,
        )
        private_bytes = encode_json(private)
        summary = make_summary(private, private_bytes)
        write_bytes(args.private_output, private_bytes)
        write_bytes(args.summary_output, encode_json(summary))
    except (OSError, RecoveryPreflightError) as error:
        print(str(error), file=sys.stderr)
        return 2

    print(json.dumps(summary, indent=2, sort_keys=True))
    if private["status"] == "renderer_lineage_bytes_unavailable":
        return 3
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
