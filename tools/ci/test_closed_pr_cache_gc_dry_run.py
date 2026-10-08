#!/usr/bin/env python3
"""Fail-closed coverage for the inline, checkout-free closed-PR cache audit."""

from __future__ import annotations

import contextlib
import datetime as dt
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile


WORKFLOW = Path(".github/workflows/pr-obsolete-head-canceller.yml")
text = WORKFLOW.read_text(encoding="utf-8")
start_name = "      - name: Audit old closed-PR caches (DRY RUN; never delete)\n"
end_name = "      - name: Dispatch ChatGPT PR event controller\n"
assert text.count(start_name) == 1
assert text.count(end_name) == 1
assert text.count("      - name: Cancel obsolete pull-request head runs globally\n") == 1
section = text.split(start_name, 1)[1].split(end_name, 1)[0]
assert "github.event_name == 'pull_request' && github.event.action == 'closed'" in section
assert 'gh api -X GET "repos/$GH_REPO/actions/caches?per_page=100"' in section
assert "gh api -X DELETE" not in section
assert "gh api --method DELETE" not in section
assert "subprocess.run(" in section
assert "capture_output=True, text=True, timeout=20" in section

open_marker = '          python - "$cache_json" <<' + "'PY'\n"
close_marker = "\n          PY\n"
assert section.count(open_marker) == 1
source = section.split(open_marker, 1)[1].split(close_marker, 1)[0]
lines = source.splitlines()
assert lines and all(line.startswith("          ") or not line for line in lines)
python_source = "\n".join(line[10:] if line else "" for line in lines) + "\n"
compile(python_source, "<closed-pr-cache-gc-inline>", "exec")


def run_audit(caches: list[dict], pull_states: dict[int, dict], reported_count: int | None = None):
    with tempfile.TemporaryDirectory() as tmp:
        inventory = Path(tmp) / "cache.json"
        summary = Path(tmp) / "summary.md"
        inventory.write_text(
            json.dumps({
                "total_count": len(caches) if reported_count is None else reported_count,
                "actions_caches": caches,
            }),
            encoding="utf-8",
        )
        saved_argv = sys.argv
        saved_run = subprocess.run
        saved_repo = os.environ.get("GH_REPO")
        saved_summary = os.environ.get("GITHUB_STEP_SUMMARY")
        output = io.StringIO()
        calls = []

        def fake_run(args, **kwargs):
            calls.append(args)
            assert args[:4] == ["gh", "api", "-X", "GET"], args
            assert kwargs["capture_output"] is True
            assert kwargs["text"] is True
            assert kwargs["timeout"] == 20
            number = int(args[-1].split("/")[-1])
            obj = pull_states.get(number)
            if obj is None:
                return subprocess.CompletedProcess(args, 1, "", "no such PR")
            return subprocess.CompletedProcess(args, 0, json.dumps(obj), "")

        try:
            subprocess.run = fake_run
            sys.argv = ["<inline>", str(inventory)]
            os.environ["GH_REPO"] = "Lalalalendia/rar2"
            os.environ["GITHUB_STEP_SUMMARY"] = str(summary)
            with contextlib.redirect_stdout(output):
                try:
                    exec(compile(python_source, "<closed-pr-cache-gc-inline>", "exec"),
                         {"__name__": "__main__"})
                except SystemExit as e:
                    assert e.code == 0, e.code
            return output.getvalue(), summary.read_text(encoding="utf-8"), calls
        finally:
            subprocess.run = saved_run
            sys.argv = saved_argv
            for key, old in (("GH_REPO", saved_repo), ("GITHUB_STEP_SUMMARY", saved_summary)):
                if old is None:
                    os.environ.pop(key, None)
                else:
                    os.environ[key] = old


now = dt.datetime.now(dt.timezone.utc)
old = (now - dt.timedelta(hours=2)).isoformat()
recent = (now - dt.timedelta(minutes=3)).isoformat()
caches = [
    {"id": 101, "ref": "refs/heads/main", "size_in_bytes": 1000},
    {"id": 102, "ref": "refs/pull/11/merge", "size_in_bytes": 2000},
    {"id": 103, "ref": "refs/pull/12/merge", "size_in_bytes": 3000},
    {"id": 104, "ref": "refs/pull/13/merge", "size_in_bytes": 4000},
    {"id": 105, "ref": "refs/heads/release", "size_in_bytes": 5000},
    {"id": 106, "ref": "refs/pull/12/head", "size_in_bytes": 6000},
    {"id": 107, "ref": "refs/pull/14/merge", "size_in_bytes": 7000},
    {"id": 0, "ref": "refs/pull/12/merge", "size_in_bytes": 8000},
]
states = {
    11: {"state": "open"},
    12: {"state": "closed", "closed_at": old},
    13: {"state": "closed", "closed_at": recent},
}
log, summary, calls = run_audit(caches, states)
assert "closed_pr_cache_gc_mode=DRY_RUN" in log
assert "closed_pr_cache_gc_cache_entries=8" in log
assert "closed_pr_cache_gc_candidate_count=1" in log
assert "closed_pr_cache_gc_candidate_bytes=3000" in log
assert "closed_pr_cache_gc_protected_main=1" in log
assert "closed_pr_cache_gc_protected_open=1" in log
assert "closed_pr_cache_gc_protected_other=2" in log
assert "closed_pr_cache_gc_protected_recent=1" in log
assert "closed_pr_cache_gc_unknown=2" in log
assert "DRY_RUN_CANDIDATE pr=12 cache_id=103 bytes=3000 ref=refs/pull/12/merge" in log
assert "cache_id=102" not in log and "cache_id=104" not in log
assert "cache_id=101" not in log
assert "DRY RUN (no deletion)" in summary
assert len(calls) == 4, calls  # one state lookup for each unique PR number

partial, partial_summary, calls = run_audit(caches[:1], {}, reported_count=99)
assert "closed_pr_cache_gc=skipped: cache inventory incomplete" in partial
assert "closed_pr_cache_gc_status=incomplete (no deletion)" in partial_summary
assert not calls

print("closed PR cache GC dry-run safety: PASS")
