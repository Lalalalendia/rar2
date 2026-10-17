#!/usr/bin/env python3
"""Windows onedir offline smoke: actual frozen Rescue children and exact CFB bytes.

This probe only creates a synthetic, non-customer CFB fixture and must never
upload real PUB source, bundle payloads, or the local private queue.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile

from tools.test_rescue_cfb_stream_identity import collision_fixture


def sha(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def invoke(exe: Path, args: list[str], env: dict, timeout: int = 150) -> tuple[int, str]:
    result = subprocess.run([str(exe), *args], stdin=subprocess.DEVNULL,
                            stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                            text=True, encoding="utf-8", timeout=timeout, env=env,
                            check=False)
    if result.returncode not in (0, 1, 2):
        raise AssertionError(f"unexpected engine exit: {result.returncode}: {result.stderr[-400:]}")
    return result.returncode, result.stdout


def main() -> int:
    arg = argparse.ArgumentParser()
    arg.add_argument("--portable-dir", type=Path, required=True)
    args = arg.parse_args()
    home = args.portable_dir.resolve()
    engine = home / "rescue-engine.exe"
    gui = home / "chaptera-rescue.exe"
    pub_cli = home / "pub.exe"
    for exe in (engine, gui, pub_cli):
        assert exe.is_file(), f"missing portable runtime component: {exe.name}"
    assert (home / "_internal").is_dir(), "PyInstaller onedir runtime absent"

    env = {k: v for k, v in os.environ.items() if not k.upper().startswith("PYTHON")}
    env["PATH"] = str(Path(os.environ["SystemRoot"]) / "System32")
    code, stdout = invoke(engine, ["--self-check"], env)
    assert code == 0, "frozen dependency import failed"
    check = json.loads(stdout)
    assert check == {"schema_version": "chaptera.rescue-portable-engine.v1", "ok": True,
                     "frozen": True, "module_count": 4}, check

    # Real Rust GUI binary must remain identifiable even when launched offline.
    code, stdout = invoke(gui, ["--self-check"], env)
    assert code == 0 and json.loads(stdout)["product_id"] == "chaptera.rescue"
    code, stdout = invoke(pub_cli, ["--help"], env)
    assert code == 0, "image-export CLI unusable offline"

    with tempfile.TemporaryDirectory(prefix="rescue-portable-") as td:
        tmp = Path(td)
        inputs = tmp / "documents"
        nested = inputs / "Пример"
        nested.mkdir(parents=True)
        source = inputs / "alpha.pub"
        duplicate = nested / "duplicate.PUB"
        source_bytes = collision_fixture()
        source.write_bytes(source_bytes)
        duplicate.write_bytes(source_bytes)
        output = tmp / "output"
        origin_digest = sha(source_bytes)

        code, _ = invoke(engine, ["scan", "--input", str(inputs), "--output", str(output)], env)
        assert code == 0
        initial = json.loads((output / "summary.json").read_text(encoding="utf-8"))
        assert initial["source_free"] is True
        assert initial["unique_sources"] == 1 and initial["duplicate_paths"] == 1
        assert "alpha.pub" not in (output / "summary.json").read_text(encoding="utf-8")
        assert "duplicate.PUB" not in (output / "entries.csv").read_text(encoding="utf-8")

        code, _ = invoke(engine, ["run", "--output", str(output), "--workers", "2",
                                  "--pub-cli", str(pub_cli)], env)
        assert code == 0, "physical CFB salvage must succeed despite no image content"
        rows = json.loads((output / "summary.json").read_text(encoding="utf-8"))["entries"]
        assert len(rows) == 1 and rows[0]["sha256"] == origin_digest
        assert rows[0]["status"] == "done_partial" and rows[0]["stream_count"] == 2
        bundle = output / "bundles" / origin_digest
        files = sorted((bundle / "physical" / "streams").glob("sid-*.bin"))
        assert len(files) == 2
        assert source.read_bytes() == source_bytes and duplicate.read_bytes() == source_bytes

        code, _ = invoke(engine, ["run", "--output", str(output)], env)
        assert code == 0 and json.loads((output / "summary.json").read_text(encoding="utf-8"))["entries"][0]["status"] == "done_partial"
        files[0].write_bytes(b"tampered")
        code, _ = invoke(engine, ["run", "--output", str(output)], env)
        assert code == 1, "tampered physical stream must fail closed"
        broken = json.loads((output / "summary.json").read_text(encoding="utf-8"))["entries"][0]
        assert broken["status"] == "failed" and broken["reason"] == "bundle_integrity", broken
        assert source.read_bytes() == source_bytes

    print(json.dumps({"schema_version": "chaptera.rescue-portable-acceptance.v1",
                      "frozen": True, "offline_path": True, "source_unchanged": True,
                      "verified_streams": 2, "sha_dedup": True, "resume": True,
                      "tamper_fail_closed": True}, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
