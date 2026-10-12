#!/usr/bin/env python3
"""Bounded, resumable PUB->PDF batch orchestration over Chaptera's current pub CLI.

No Publisher parser/renderer lives here. Input files and loss reports are private.
"""
from __future__ import annotations

import argparse
from dataclasses import dataclass
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import shutil
import stat
import subprocess
import sys
import tempfile
import zipfile

SCHEMA = "chaptera.rescue-batch-pdf.v1"
MAX_FILES = 2_000
MAX_SOURCE_BYTES = 128 * 1024 * 1024
MAX_TOTAL_BYTES = 1024 * 1024 * 1024
PDF_PARTS = ("", ".loss.json", ".loss.txt")


class InputRejected(ValueError):
    pass


@dataclass(frozen=True)
class Source:
    relative_path: str
    local_path: Path | None = None
    archive_member: str | None = None


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def is_pub(name: str) -> bool:
    return name.lower().endswith(".pub")


def safe_relative(raw: str) -> str:
    if (not raw or "\\" in raw or "\x00" in raw or raw.startswith("/")
            or any(part in ("", ".", "..") for part in raw.split("/"))
            or any(":" in part or any(ord(char) < 32 for char in part)
                   for part in raw.split("/"))):
        raise InputRejected("unsafe_archive_member")
    return str(PurePosixPath(raw))


def inventory_folder(root: Path) -> list[Source]:
    found: list[Source] = []

    def walk(directory: Path) -> None:
        for item in sorted(directory.iterdir(), key=lambda p: p.name):
            if item.is_symlink():
                raise InputRejected("symlink_in_source")
            if item.is_dir():
                walk(item)
            elif item.is_file() and is_pub(item.name):
                if item.stat().st_size > MAX_SOURCE_BYTES:
                    raise InputRejected("source_too_large")
                found.append(Source(item.relative_to(root).as_posix(), local_path=item))
                if len(found) > MAX_FILES:
                    raise InputRejected("too_many_sources")

    walk(root)
    return sorted(found, key=lambda entry: entry.relative_path)


def inventory_zip(archive: zipfile.ZipFile) -> list[Source]:
    found: list[Source] = []
    names: set[str] = set()
    size_total = 0
    for info in archive.infolist():
        raw = info.filename.rstrip("/") if info.is_dir() else info.filename
        name = safe_relative(raw)
        canonical = name.casefold()
        if canonical in names:
            raise InputRejected("duplicate_archive_member")
        names.add(canonical)
        mode_type = stat.S_IFMT(info.external_attr >> 16)
        if mode_type not in (0, stat.S_IFREG, stat.S_IFDIR):
            raise InputRejected("unsafe_archive_file_type")
        if info.is_dir():
            continue
        if info.file_size > MAX_SOURCE_BYTES:
            raise InputRejected("source_too_large")
        size_total += info.file_size
        if size_total > MAX_TOTAL_BYTES or len(names) > MAX_FILES * 10:
            raise InputRejected("archive_limits_exceeded")
        if is_pub(name):
            found.append(Source(name, archive_member=info.filename))
            if len(found) > MAX_FILES:
                raise InputRejected("too_many_sources")
    return sorted(found, key=lambda entry: entry.relative_path)


def output_name(relative: str, source_hash: str) -> str:
    filename = PurePosixPath(relative).name
    stem = filename[:-4]
    slug = re.sub(r"[^A-Za-z0-9_-]+", "-", stem).strip("-_")[:48] or "publication"
    location_hash = sha256(relative.encode("utf-8"))[:12]
    return f"{slug}--{location_hash}--{source_hash[:16]}.pdf"


def output_paths(folder: Path, pdf_filename: str) -> tuple[Path, Path, Path]:
    return tuple(folder / (pdf_filename + suffix) for suffix in PDF_PARTS)  # type: ignore[return-value]


def check_regular(path: Path) -> bool:
    return path.is_file() and not path.is_symlink()


def previous_index(folder: Path) -> dict[str, dict]:
    path = folder / "manifest.json"
    if not path.exists():
        return {}
    if not check_regular(path):
        raise InputRejected("manifest_not_regular")
    try:
        manifest = json.loads(path.read_text(encoding="utf-8"))
        if manifest.get("schema") != SCHEMA or not isinstance(manifest.get("entries"), list):
            raise ValueError("manifest schema")
        rows = manifest["entries"]
        if any(not isinstance(row, dict) or not isinstance(row.get("source_relative_path"), str)
               for row in rows):
            raise ValueError("manifest rows")
        if len({row["source_relative_path"] for row in rows}) != len(rows):
            raise ValueError("manifest duplicates")
        return {row["source_relative_path"]: row for row in rows}
    except (OSError, ValueError, TypeError) as error:
        raise InputRejected("invalid_prior_manifest") from error


def reusable(prior: dict | None, source_hash: str, font_hash: str,
             filename: str, folder: Path) -> bool:
    if not prior or prior.get("status") not in ("converted", "reused"):
        return False
    if (prior.get("source_sha256") != source_hash
            or prior.get("fallback_font_sha256") != font_hash
            or prior.get("output_pdf") != filename):
        return False
    for path, key in zip(output_paths(folder, filename),
                         ("pdf_sha256", "loss_json_sha256", "loss_text_sha256")):
        expected = prior.get(key)
        if not isinstance(expected, str) or len(expected) != 64 or not check_regular(path):
            return False
        if sha256_file(path) != expected:
            return False
    with (folder / filename).open("rb") as stream:
        return stream.read(5) == b"%PDF-"


def write_manifest(folder: Path, payload: dict) -> None:
    fd, name = tempfile.mkstemp(prefix=".rescue-manifest-", suffix=".tmp", dir=folder)
    try:
        with os.fdopen(fd, "w", encoding="utf-8") as stream:
            json.dump(payload, stream, sort_keys=True, indent=2, ensure_ascii=True)
            stream.write("\n")
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(name, folder / "manifest.json")
    finally:
        if os.path.exists(name):
            os.unlink(name)


def source_bytes(source: Source, archive: zipfile.ZipFile | None) -> bytes:
    if source.local_path is not None:
        if source.local_path.is_symlink():
            raise InputRejected("symlink_in_source")
        data = source.local_path.read_bytes()
    else:
        assert archive is not None and source.archive_member is not None
        with archive.open(source.archive_member) as stream:
            data = stream.read(MAX_SOURCE_BYTES + 1)
    if len(data) > MAX_SOURCE_BYTES:
        raise InputRejected("source_too_large")
    return data


def convert_one(source: Source, data: bytes, target: tuple[Path, Path, Path],
                pub_cli: str, fallback_font: Path, font_hash: str,
                timeout: int, output_dir: Path) -> dict:
    source_hash = sha256(data)
    filename = target[0].name
    row = {"source_relative_path": source.relative_path,
           "source_sha256": source_hash,
           "fallback_font_sha256": font_hash,
           "output_pdf": filename}
    with tempfile.TemporaryDirectory(prefix=".rescue-stage-", dir=output_dir) as temporary:
        stage = Path(temporary)
        if source.local_path is None:
            source_path = stage / "source.pub"
            with source_path.open("xb") as handle:
                handle.write(data)
            os.chmod(source_path, 0o600)
        else:
            source_path = source.local_path
        stage_pdf = stage / "candidate.pdf"
        command = [pub_cli, "convert", str(source_path), "--to", "pdf",
                   "--output", str(stage_pdf), "--fallback-font", str(fallback_font), "--json"]
        try:
            completed = subprocess.run(command, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                                       timeout=timeout, check=False)
        except (subprocess.TimeoutExpired, OSError) as error:
            row.update(status="failed", reason=("converter_timeout" if isinstance(error, subprocess.TimeoutExpired)
                                                else "converter_unavailable"))
            return row
        if completed.returncode != 0:
            row.update(status="failed", reason="converter_failed")
            return row
        stages = output_paths(stage, "candidate.pdf")
        if not all(check_regular(path) and path.stat().st_size for path in stages):
            row.update(status="failed", reason="incomplete_conversion")
            return row
        if stages[0].read_bytes()[:5] != b"%PDF-":
            row.update(status="failed", reason="invalid_pdf_output")
            return row
        try:
            json.loads(stages[1].read_text(encoding="utf-8"))
        except (UnicodeError, ValueError):
            row.update(status="failed", reason="invalid_loss_report")
            return row
        if source.local_path is not None and sha256_file(source.local_path) != source_hash:
            row.update(status="failed", reason="source_changed_during_conversion")
            return row
        if sha256_file(fallback_font) != font_hash:
            row.update(status="failed", reason="fallback_font_changed_during_conversion")
            return row
        hashes = [sha256_file(path) for path in stages]
        if any(path.is_symlink() or (path.exists() and not path.is_file()) for path in target):
            row.update(status="failed", reason="unsafe_output_target")
            return row
        try:
            for staging, final in zip(stages, target):
                os.replace(staging, final)
        except OSError:
            row.update(status="failed", reason="output_install_failed")
            return row
    row.update(status="converted", pdf_sha256=hashes[0],
               loss_json_sha256=hashes[1], loss_text_sha256=hashes[2])
    return row


def run(args: argparse.Namespace) -> tuple[int, dict]:
    root = args.root
    output = args.output
    if root.is_symlink() or not root.exists():
        raise InputRejected("missing_or_symlink_input")
    if not root.is_dir() and not (root.is_file() and root.suffix.lower() == ".zip"):
        raise InputRejected("input_must_be_directory_or_zip")
    if not check_regular(args.fallback_font):
        raise InputRejected("missing_or_symlink_fallback_font")
    if root.is_dir():
        source_root = root.resolve()
        destination = output.resolve()
        if source_root == destination or source_root in destination.parents or destination in source_root.parents:
            raise InputRejected("source_and_output_trees_overlap")
    if output.is_symlink():
        raise InputRejected("output_is_symlink")
    output.mkdir(parents=True, exist_ok=True)
    output = output.resolve()
    if not output.is_dir():
        raise InputRejected("output_not_directory")
    converter = shutil.which(args.pub_cli)
    if converter is None or not Path(converter).is_file():
        raise InputRejected("pub_cli_unavailable")
    font_hash = sha256_file(args.fallback_font)
    prior = previous_index(output)
    archive: zipfile.ZipFile | None = None
    try:
        if root.is_dir():
            sources = inventory_folder(root)
            input_kind = "directory"
        else:
            archive = zipfile.ZipFile(root)
            sources = inventory_zip(archive)
            input_kind = "zip"
        if not sources:
            raise InputRejected("no_pub_sources")
        receipt = {"schema": SCHEMA, "input_kind": input_kind,
                   "fallback_font_sha256": font_hash,
                   "total_sources": len(sources), "entries": []}
        lock = output / ".rescue-batch.lock"
        lockfd = None
        try:
            lockfd = os.open(lock, os.O_CREAT | os.O_EXCL | os.O_WRONLY, 0o600)
        except FileExistsError as error:
            raise InputRejected("output_locked") from error
        try:
            os.close(lockfd)
            used_names: set[str] = set()
            for source in sources:
                try:
                    data = source_bytes(source, archive)
                    source_hash = sha256(data)
                    name = output_name(source.relative_path, source_hash)
                    if name.casefold() in used_names:
                        raise InputRejected("output_name_collision")
                    used_names.add(name.casefold())
                    target = output_paths(output, name)
                    prev = prior.get(source.relative_path)
                    if reusable(prev, source_hash, font_hash, name, output):
                        row = dict(prev)
                        row["status"] = "reused"
                        row.pop("reason", None)
                    else:
                        row = convert_one(source, data, target, converter, args.fallback_font,
                                          font_hash, args.timeout, output)
                except (OSError, InputRejected, zipfile.BadZipFile):
                    row = {"source_relative_path": source.relative_path,
                           "status": "failed", "reason": "source_read_failed"}
                receipt["entries"].append(row)
                # Checkpoint after each file; survives process interruption.
                write_manifest(output, receipt)
        finally:
            lock.unlink(missing_ok=True)
    finally:
        if archive is not None:
            archive.close()
    failed = sum(row["status"] == "failed" for row in receipt["entries"])
    return (1 if failed else 0), receipt


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("root", type=Path, help="source directory or ZIP archive")
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--fallback-font", type=Path, required=True)
    parser.add_argument("--pub-cli", default="pub", help="path to existing Chaptera `pub` executable")
    parser.add_argument("--timeout", type=int, default=300, help="seconds per file (default: 300)")
    parser.add_argument("--json", action="store_true", help="print source-relative receipt JSON to stdout")
    args = parser.parse_args(argv)
    if args.timeout < 1 or args.timeout > 3600:
        parser.error("timeout must be within 1..3600")
    try:
        status, receipt = run(args)
    except (InputRejected, zipfile.BadZipFile, OSError) as error:
        # Never log source bytes, document text, or full absolute file paths.
        print(f"rescue batch rejected: {type(error).__name__ if not isinstance(error, InputRejected) else str(error)}",
              file=sys.stderr)
        return 2
    if args.json:
        print(json.dumps(receipt, sort_keys=True, indent=2))
    else:
        counts = {key: sum(row["status"] == key for row in receipt["entries"])
                  for key in ("converted", "reused", "failed")}
        print("rescue batch " + " ".join(f"{key}={value}" for key, value in counts.items()))
    return status


if __name__ == "__main__":
    sys.exit(main())
