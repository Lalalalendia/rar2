#!/usr/bin/env python3
"""Offline, source-safe physical Rescue census against a pinned SHA authority.

This invokes existing CFB export/verify tools, not a new parser. Extracted
streams and their descriptive names stay in local bundles; public aggregate
receipts contain only SHA identities, counts, classifications and error codes.
"""
from __future__ import annotations

import argparse
import csv
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile

SCHEMA = "chaptera.rescue-corpus-replay.v1"
DEFAULT_AUTHORITY = Path(__file__).resolve().parent / "corpus/receipts/current-rar-1556.sha256.txt"
REPO_ROOT = Path(__file__).resolve().parents[1]
SHA = re.compile(r"[0-9a-f]{64}\Z")
MAX_FILES = 10000
FIELDS = ("source_sha256", "status", "reason", "source_bytes", "verified_streams")


class Rejected(ValueError):
    pass


def sha_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def load_authority(path: Path) -> list[str]:
    values = [v.strip().lower() for v in path.read_text(encoding="ascii").splitlines() if v.strip()]
    if not values or any(not SHA.fullmatch(v) for v in values):
        raise Rejected("invalid_authority_sha_list")
    if values != sorted(set(values)):
        raise Rejected("authority_must_be_sorted_unique")
    return values


def reject_unsafe_path(path: Path) -> None:
    """Refuse any pre-existing symlink component, including output parents."""
    for part in (path, *path.parents):
        if part.is_symlink():
            raise Rejected("symlink_path")


def check_paths(roots: list[Path], output: Path, authority: Path) -> None:
    if not roots or len(roots) > 32:
        raise Rejected("invalid_root_count")
    reject_unsafe_path(output)
    reject_unsafe_path(authority)
    for root in roots:
        reject_unsafe_path(root)
        if not root.exists() or not (root.is_dir() or root.is_file() and root.suffix.lower() == ".pub"):
            raise Rejected("missing_or_invalid_root")
        src = root.resolve()
        dest = output.resolve()
        if root.is_dir():
            if src == dest or src in dest.parents or dest in src.parents:
                raise Rejected("source_output_overlap")
        elif dest == src or dest in src.parents:
            raise Rejected("source_output_overlap")


def inventory(roots: list[Path], allowed: set[str], max_bytes: int) -> tuple[dict[str, Path], dict[str, int]]:
    found: dict[str, Path] = {}
    stats = dict(discovered_pub=0, outside_authority=0, duplicate_source=0,
                 hash_named_mismatch=0, unreadable=0, unsafe_symlink=0, oversized=0)

    def visit(path: Path) -> None:
        stats["discovered_pub"] += 1
        if stats["discovered_pub"] > MAX_FILES:
            raise Rejected("too_many_source_files")
        if path.is_symlink():
            stats["unsafe_symlink"] += 1
            return
        try:
            if not path.is_file():
                stats["unreadable"] += 1
                return
            if path.stat().st_size > max_bytes:
                stats["oversized"] += 1
                return
            actual = sha_file(path)
            stem = path.stem.lower()
            if SHA.fullmatch(stem) and stem != actual:
                stats["hash_named_mismatch"] += 1
                return
        except OSError:
            stats["unreadable"] += 1
            return
        if actual not in allowed:
            stats["outside_authority"] += 1
        elif actual in found:
            stats["duplicate_source"] += 1
        else:
            found[actual] = path

    for root in roots:
        if root.is_file():
            visit(root)
            continue

        def failed(_error: OSError) -> None:
            stats["unreadable"] += 1

        for folder, subdirs, files in os.walk(root, followlinks=False, onerror=failed):
            folder_path = Path(folder)
            safe_dirs = []
            for subdir in sorted(subdirs):
                if (folder_path / subdir).is_symlink():
                    stats["unsafe_symlink"] += 1
                else:
                    safe_dirs.append(subdir)
            subdirs[:] = safe_dirs
            for name in sorted(files):
                if name.lower().endswith(".pub"):
                    visit(folder_path / name)
    return found, stats


def atomic_json(path: Path, payload: dict) -> None:
    fd, name = tempfile.mkstemp(prefix=".receipt-", suffix=".tmp", dir=path.parent)
    try:
        with os.fdopen(fd, "w", encoding="utf-8") as stream:
            json.dump(payload, stream, indent=2, sort_keys=True, ensure_ascii=True)
            stream.write("\n")
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(name, path)
    finally:
        if os.path.exists(name):
            os.unlink(name)


def invoke(module: str, args: list[str], timeout: int) -> str | None:
    try:
        p = subprocess.run([sys.executable, "-m", module, *args], cwd=REPO_ROOT,
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                           check=False, timeout=timeout)
    except subprocess.TimeoutExpired:
        return "worker_timeout"
    except OSError:
        return "worker_unavailable"
    return None if p.returncode == 0 else "worker_failed"


def verified_bundle(source: Path, bundle: Path, sha: str, timeout: int) -> int | None:
    manifest = bundle / "manifest.json"
    stream_dir = bundle / "streams"
    if not manifest.is_file() or manifest.is_symlink() or not stream_dir.is_dir() or stream_dir.is_symlink():
        return None
    if any(path.is_symlink() for path in stream_dir.iterdir()):
        return None
    receipt = bundle / "verify.json"
    if receipt.is_symlink():
        return None
    status = invoke("tools.verify_rescue_cfb_stream_export", [str(source),
        "--artifact-dir", str(stream_dir), "--manifest", str(manifest),
        "--receipt", str(receipt)], timeout)
    if status:
        return None
    try:
        data = json.loads(receipt.read_text(encoding="utf-8"))
        if (data["pass"] is True and data["source_sha256"] == sha
                and isinstance(data["verified_stream_count"], int)
                and data["verified_stream_count"] >= 0):
            return data["verified_stream_count"]
    except (ValueError, KeyError, OSError, TypeError):
        pass
    return None


def extract(source: Path, bundles: Path, sha: str, timeout: int) -> dict:
    row = {"source_sha256": sha, "status": "failed", "reason": "unknown_failure",
           "source_bytes": source.stat().st_size, "verified_streams": 0}
    target = bundles / sha
    if target.is_symlink():
        row["reason"] = "unsafe_existing_bundle"
        return row
    if target.exists():
        verified = verified_bundle(source, target, sha, timeout)
        row.update(status="reused" if verified is not None else "failed",
                   reason="" if verified is not None else "invalid_existing_bundle",
                   verified_streams=verified or 0)
        return row
    with tempfile.TemporaryDirectory(prefix=".stage-", dir=bundles) as temp:
        stage = Path(temp)
        streams = stage / "streams"
        status = invoke("tools.rescue_cfb_stream_export", [str(source),
            "--output-dir", str(streams), "--manifest", str(stage / "manifest.json")], timeout)
        if status:
            row["reason"] = "export_" + status
            return row
        verified = verified_bundle(source, stage, sha, timeout)
        if verified is None:
            row["reason"] = "independent_verification_failed"
            return row
        if sha_file(source) != sha:
            row["reason"] = "source_changed"
            return row
        try:
            stage.rename(target)
        except OSError:
            row["reason"] = "output_install_failed"
            return row
        row.update(status="extracted", reason="", verified_streams=verified)
    return row


def run(roots: list[Path], authority: Path, output: Path, *, scan_only: bool,
        max_bytes: int, timeout: int) -> dict:
    check_paths(roots, output, authority)
    sha_list = load_authority(authority)
    output.mkdir(parents=True, exist_ok=True)
    if (output / "summary.json").is_symlink() or (output / "entries.csv").is_symlink():
        raise Rejected("unsafe_receipt_target")
    bundles = output / "bundles"
    if bundles.is_symlink():
        raise Rejected("unsafe_bundles_dir")
    bundles.mkdir(exist_ok=True)
    if not bundles.is_dir():
        raise Rejected("invalid_bundles_dir")
    lock_path = output / ".rescue-replay.lock"
    try:
        fd = os.open(lock_path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    except FileExistsError as exc:
        raise Rejected("output_locked") from exc
    try:
        os.close(fd)
        available, scan = inventory(roots, set(sha_list), max_bytes)
        digest = hashlib.sha256(("\n".join(sha_list) + "\n").encode("ascii")).hexdigest()
        receipt = {"schema": SCHEMA, "authority_count": len(sha_list),
                   "authority_digest_sha256": digest, "scan_only": scan_only,
                   "scan": scan, "entries": [], "counts": {}}
        for sha in sha_list:
            path = available.get(sha)
            if path is None:
                row = {"source_sha256": sha, "status": "missing", "reason": "source_not_materialized",
                       "source_bytes": 0, "verified_streams": 0}
            elif scan_only:
                row = {"source_sha256": sha, "status": "present_unprocessed", "reason": "scan_only",
                       "source_bytes": path.stat().st_size, "verified_streams": 0}
            else:
                try:
                    if sha_file(path) != sha:
                        raise ValueError("drift")
                    row = extract(path, bundles, sha, timeout)
                except (OSError, ValueError):
                    row = {"source_sha256": sha, "status": "failed", "reason": "source_drift_or_io",
                           "source_bytes": 0, "verified_streams": 0}
            receipt["entries"].append(row)
            counts = receipt["counts"]
            counts[row["status"]] = counts.get(row["status"], 0) + 1
            atomic_json(output / "summary.json", receipt)  # Durable checkpoint per source.
        with (output / "entries.csv").open("w", newline="", encoding="utf-8") as stream:
            writer = csv.DictWriter(stream, fieldnames=FIELDS)
            writer.writeheader()
            writer.writerows(receipt["entries"])
        return receipt
    finally:
        lock_path.unlink(missing_ok=True)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, action="append", required=True,
                        help="Source PUB file or directory; repeat for multiple roots")
    parser.add_argument("--authority", type=Path, default=DEFAULT_AUTHORITY)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--scan-only", action="store_true")
    parser.add_argument("--max-source-mib", type=int, default=128)
    parser.add_argument("--timeout", type=int, default=45, help="Seconds per export/verify")
    args = parser.parse_args(argv)
    if not 1 <= args.max_source_mib <= 512 or not 1 <= args.timeout <= 3600:
        parser.error("invalid memory or timeout limit")
    try:
        receipt = run(args.root, args.authority, args.output, scan_only=args.scan_only,
                      max_bytes=args.max_source_mib * 1024 * 1024, timeout=args.timeout)
    except (Rejected, OSError, UnicodeError) as exc:
        reason = str(exc) if isinstance(exc, Rejected) else type(exc).__name__
        print(json.dumps({"schema": SCHEMA, "status": "rejected", "reason": reason}))
        return 2
    result = {k: receipt[k] for k in ("schema", "authority_count", "authority_digest_sha256", "scan", "counts")}
    print(json.dumps(result, sort_keys=True))
    complete = (receipt["counts"].get("extracted", 0) + receipt["counts"].get("reused", 0)
                == receipt["authority_count"])
    if args.scan_only:
        return 0 if receipt["counts"].get("present_unprocessed", 0) == receipt["authority_count"] else 1
    return 0 if complete else 1


if __name__ == "__main__":
    raise SystemExit(main())
