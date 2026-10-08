from __future__ import annotations

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import required_merge_gate as gate


class RequiredGateTests(unittest.TestCase):
    def test_merge_authority_does_not_retrigger_on_ready_for_review(self) -> None:
        workflow = Path(".github/workflows/required-merge-gate.yml").read_text(encoding="utf-8")
        self.assertIn("types: [opened, synchronize, reopened]", workflow)
        self.assertNotIn("ready_for_review", workflow.split("types:", 1)[1].split("\n", 1)[0])

    def test_unconditional_and_path_filter(self) -> None:
        unconditional = "name: always\non:\n  pull_request:\n"
        selective = """name: selective
on:
  pull_request:
    paths:
      - "src/**"
      - "!src/generated/**"
"""
        self.assertTrue(gate.workflow_selected(unconditional, ["README.md"], "opened", "main"))
        self.assertTrue(gate.workflow_selected(selective, ["src/lib.rs"], "synchronize", "main"))
        self.assertFalse(gate.workflow_selected(selective, ["src/generated/a.rs"], "opened", "main"))
        self.assertFalse(gate.workflow_selected(selective, ["docs/guide.md"], "opened", "main"))

    def test_closed_only_and_dispatch_only_are_not_expected(self) -> None:
        closed = """name: closed
on:
  pull_request:
    types: [closed]
"""
        dispatch = """name: dispatch
on:
  workflow_dispatch:
"""
        self.assertFalse(gate.workflow_selected(closed, ["src/lib.rs"], "opened", "main"))
        self.assertFalse(gate.workflow_selected(dispatch, ["src/lib.rs"], "opened", "main"))

    def test_branch_and_path_ignore(self) -> None:
        yaml = """name: test
on:
  pull_request:
    branches: [main]
    paths-ignore:
      - "docs/**"
"""
        self.assertFalse(gate.workflow_selected(yaml, ["docs/readme.md"], "opened", "main"))
        self.assertTrue(gate.workflow_selected(yaml, ["docs/readme.md", "src/lib.rs"], "opened", "main"))
        self.assertFalse(gate.workflow_selected(yaml, ["src/lib.rs"], "opened", "release"))

    def test_exact_head_never_passes_missing_or_pending_runs(self) -> None:
        expected = {".github/workflows/reader.yml", ".github/workflows/global.yml"}
        runs = {".github/workflows/global.yml": {"status": "completed", "conclusion": "success"}}
        failure, pending = gate.classify(expected, runs)
        self.assertEqual(failure, [])
        self.assertIn("reader.yml: not registered", "\n".join(pending))
        runs[".github/workflows/reader.yml"] = {"status": "in_progress", "conclusion": None}
        _, pending = gate.classify(expected, runs)
        self.assertIn("reader.yml: in_progress", "\n".join(pending))
        runs[".github/workflows/reader.yml"] = {"status": "completed", "conclusion": "failure"}
        failure, _ = gate.classify(expected, runs)
        self.assertIn("reader.yml: failure", "\n".join(failure))

    def test_unknown_failed_workflow_blocks_merge(self) -> None:
        failure, pending = gate.classify({".github/workflows/global.yml"}, {
            ".github/workflows/global.yml": {"status": "completed", "conclusion": "success"},
            ".github/workflows/unknown.yml": {"status": "completed", "conclusion": "failure"},
        })
        self.assertEqual(pending, [])
        self.assertEqual(len(failure), 1)

    def test_run_filter_rejects_old_sha_and_preserves_latest_attempt(self) -> None:
        calls = []
        original = gate.request_json

        def request(repo, route, token):
            calls.append(route)
            return {"workflow_runs": [
                {"path": ".github/workflows/reader.yml", "head_sha": "OLD",
                 "status": "completed", "conclusion": "success",
                 "created_at": "2026-10-01T00:00:00Z", "id": 1},
                {"path": ".github/workflows/reader.yml", "head_sha": "NEW",
                 "status": "completed", "conclusion": "failure",
                 "created_at": "2026-10-02T00:00:00Z", "id": 2},
                {"path": ".github/workflows/reader.yml", "head_sha": "NEW",
                 "status": "completed", "conclusion": "success",
                 "created_at": "2026-10-02T00:00:00Z", "id": 2, "run_attempt": 2},
            ]}
        try:
            gate.request_json = request
            runs = gate.exact_head_runs("a/b", "feature", "NEW", "token")
            self.assertEqual(runs[".github/workflows/reader.yml"]["conclusion"], "success")
        finally:
            gate.request_json = original
        self.assertIn("event=pull_request", calls[0])


    def test_branch_gc_preserves_open_stacked_pr_bases(self) -> None:
        workflow = Path(".github/workflows/branch-gc.yml").read_text(encoding="utf-8")
        self.assertIn("open-bases.txt", workflow)
        self.assertIn('--jq \'.[] | .base.ref\'', workflow)
        self.assertIn('grep -Fxq "$branch" target/branch-gc/open-bases.txt', workflow)
        self.assertIn('echo "SKIP open PR base: $branch"', workflow)

    def test_branch_gc_requires_pinned_atomic_branch_delete(self) -> None:
        workflow = Path(".github/workflows/branch-gc.yml").read_text(encoding="utf-8")
        self.assertIn('[[ -z "${expected_sha:-}" || ! "$expected_sha" =~ ^[0-9a-f]{40}$ ]]', workflow)
        self.assertIn('[[ "$remote_sha" != "$expected_sha" ]]', workflow)
        self.assertIn('git push --force-with-lease="refs/heads/$branch:$expected_sha" origin ":refs/heads/$branch"', workflow)
        self.assertNotIn('git push origin --delete "$branch"', workflow)

if __name__ == "__main__":
    unittest.main()
