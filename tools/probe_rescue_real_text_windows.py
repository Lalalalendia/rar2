#!/usr/bin/env python3
"""Accept real public PUB Story extraction through the FROZEN Windows Rescue batch.

The public Apache POI SampleNewsletter PUB is pinned by byte count and SHA.
All Story text and local evidence remain inside the ephemeral runner directory.
Only source-free numeric receipts are printed to CI logs.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile

from tools import verify_rescue_text_evidence_v1 as verifier

EXPECTED_BYTES = 291_840
EXPECTED_SHA = "6a825ba26ba35d6e885acdc62e859591ed37cb0ff7480b554b9cb362b644dfcf"
EXPECTED_STORIES = 44


def invoke(engine: Path, args: list[str], env: dict[str, str], timeout: int = 360) -> tuple[int, dict]:
    process = subprocess.run(
        [str(engine), *args], stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL, encoding="utf-8", errors="replace", env=env,
        timeout=timeout, check=False)
    if process.returncode not in (0, 1, 2):
        raise AssertionError("unexpected_frozen_engine_exit")
    if len(process.stdout) > 2 * 1024 * 1024:
        raise AssertionError("unexpectedly_large_source_free_report")
    return process.returncode, json.loads(process.stdout)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--portable-dir", type=Path, required=True)
    parser.add_argument("--fixture", type=Path, required=True)
    args = parser.parse_args()

    home = args.portable_dir.resolve()
    engine = home / "rescue-engine.exe"
    pub_cli = home / "pub.exe"
    if not engine.is_file() or not pub_cli.is_file():
        raise AssertionError("missing_frozen_engine_or_Rust_pub_cli")
    original = args.fixture.read_bytes()
    if len(original) != EXPECTED_BYTES or hashlib.sha256(original).hexdigest() != EXPECTED_SHA:
        raise AssertionError("pinned_public_fixture_identity_changed")
    env = {k: v for k, v in os.environ.items() if not k.upper().startswith("PYTHON")}
    env["PATH"] = str(Path(os.environ["SystemRoot"]) / "System32")

    with tempfile.TemporaryDirectory(prefix="chaptera-pinned-text-batch-") as temp:
        root = Path(temp)
        incoming = root / "source"
        incoming.mkdir()
        source = incoming / "SampleNewsletter.pub"
        source.write_bytes(original)
        output = root / "output"

        code, scanned = invoke(
            engine, ["scan", "--input", str(source), "--output", str(output)], env)
        if code != 0 or scanned.get("unique_sources") != 1:
            raise AssertionError("frozen_scan_did_not_admit_real_PUB")

        code, processed = invoke(
            engine, ["run", "--output", str(output), "--workers", "2",
                     "--pub-cli", str(pub_cli)], env)
        if code != 0 or processed.get("counts") != {"done_partial": 1}:
            raise AssertionError("frozen_batch_text_not_completed")
        row = processed["entries"][0]
        if not (row["sha256"] == EXPECTED_SHA
                and row["auto_tier"] == "verified_text"
                and row["text_status"] == "verified"
                and row["text_count"] == EXPECTED_STORIES
                and row["auto_index"] ==
                    f"recovered-text-pages/{EXPECTED_SHA}/index.html"):
            raise AssertionError("missing_or_wrong_verified_story_promotion")
        private = output / "recovered-text" / EXPECTED_SHA
        checked = verifier.verify(source, private)
        if (checked["story_count"] != EXPECTED_STORIES or not checked["byte_verified"]
                or checked["native_pub_repair_claim"]):
            raise AssertionError("independent_byte_integrity_failed")
        gallery = output / "recovered" / EXPECTED_SHA / "index.html"
        text_page = output / "recovered-text-pages" / EXPECTED_SHA / "index.html"
        before_gallery, before_page = gallery.read_bytes(), text_page.read_bytes()
        if b"stories.txt" not in before_page or b"recovered/" not in before_page:
            raise AssertionError("private_text_index_missing_links")

        public_json = (output / "summary.json").read_text(encoding="utf-8")
        public_csv = (output / "entries.csv").read_text(encoding="utf-8")
        for forbidden in (str(source), source.name, "story_key", "stories_txt_sha256"):
            if forbidden in public_json + public_csv:
                raise AssertionError("private_text_leaked_to_source_free_report")
        if source.read_bytes() != original:
            raise AssertionError("original_PUB_modified")

        code, repeated = invoke(engine, ["run", "--output", str(output)], env)
        if code != 0 or repeated["entries"][0]["text_count"] != EXPECTED_STORIES:
            raise AssertionError("frozen_text_restart_not_idempotent")
        if gallery.read_bytes() != before_gallery or text_page.read_bytes() != before_page:
            raise AssertionError("immutable_presentation_rewritten_on_restart")

        (private / "stories.txt").write_bytes(b"deliberately_corrupted")
        code, failed = invoke(engine, ["run", "--output", str(output)], env)
        if (code != 1 or failed["counts"] != {"failed": 1}
                or failed["entries"][0]["reason"] != "text_integrity"):
            raise AssertionError("tampered_real_text_not_rejected")
        if gallery.read_bytes() != before_gallery or source.read_bytes() != original:
            raise AssertionError("tamper_check_mutated_other_evidence")

    print(json.dumps({
        "fixture": "Apache POI SampleNewsletter (pinned SHA)",
        "frozen_engine": True,
        "no_python_runtime_required": True,
        "verified_story_ranges": EXPECTED_STORIES,
        "source_free_reports": True,
        "restart_idempotent": True,
        "tamper_fail_closed": True,
        "native_repaired_pub_claim": False,
    }, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
