#!/usr/bin/env python3
"""Offline integration and negative controls for opt-in local feature handoff."""
from __future__ import annotations

import argparse
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parent))
import local_dev_handoff as handoff


def git(repo: Path, *args: str) -> str:
    return subprocess.check_output(
        ["git", "-C", str(repo), *args], text=True, stderr=subprocess.DEVNULL,
    ).strip()


def make_dev_repo(root: Path, task: str = "CI-LOCAL-DEV-HANDOFF-01") -> tuple[Path, str]:
    root.mkdir(parents=True)
    git(root, "init", "-q")
    git(root, "config", "user.name", "Local handoff test")
    git(root, "config", "user.email", "test@example.invalid")
    (root / "apps/web").mkdir(parents=True)
    (root / "apps/web/example.mjs").write_text("export const version = 1;\n", encoding="utf-8")
    git(root, "add", "--all")
    git(root, "commit", "-qm", "test: base")
    base = git(root, "rev-parse", "HEAD")
    git(root, "update-ref", "refs/remotes/origin/main", base)
    git(root, "remote", "add", "origin", "https://github.com/Lalalalendia/rar2.git")
    git(root, "switch", "-qc", handoff.task_branch(task))
    (root / "apps/web/example.mjs").write_text("export const version = 2;\n", encoding="utf-8")
    git(root, "add", "--all")
    git(root, "commit", "-qm", "test: feature")
    return root, base


def arguments(repo: Path, temp: Path, *, task: str = "CI-LOCAL-DEV-HANDOFF-01",
              publish: bool = False, draft_pr: bool = False) -> argparse.Namespace:
    return argparse.Namespace(
        repo=repo, task=task, receipt=temp / "handoff.json",
        publish=publish, draft_pr=draft_pr,
    )


class LocalHandoffTest(unittest.TestCase):
    def test_task_and_origin_identity(self):
        self.assertEqual(handoff.task_branch("CI-LOCAL-DEV-HANDOFF-01"),
                         "localdev/ci-local-dev-handoff-01")
        for bad in ("main", "../../something", "ci-bad", "CI--BAD", "CI-BAD ", ""):
            with self.assertRaises(handoff.HandoffError):
                handoff.valid_task(bad)
        for remote in ("https://example.com/rar2.git",
                       "https://github.com/Lalalalendia/rar2.git?token=secret",
                       "/tmp/local/repo.git", "git@other-host:Lalalalendia/rar2.git"):
            with self.assertRaises(handoff.HandoffError):
                handoff.validate_origin(remote)

    def test_preview_is_offline_and_source_safe(self):
        with tempfile.TemporaryDirectory() as work:
            temp = Path(work)
            repo, base = make_dev_repo(temp / "repository")
            status, report = handoff.handoff(arguments(repo, temp))
            self.assertEqual(status, 0)
            self.assertEqual(report["status"], "READY_TO_TEST")
            self.assertEqual(report["changed_count"], 1)
            self.assertEqual(report["main_sha"], base)
            self.assertEqual(report["head_sha"], git(repo, "rev-parse", "HEAD"))
            self.assertFalse(report["pushed"])
            self.assertNotIn(str(repo), json.dumps(report))
            self.assertNotIn("example.mjs", json.dumps(report))
            handoff.write_report(temp / "receipt.json", repo, report)
            self.assertEqual(json.loads((temp / "receipt.json").read_text())["status"],
                             "READY_TO_TEST")

    def test_draft_pr_requires_publish(self):
        with tempfile.TemporaryDirectory() as work:
            temp = Path(work)
            repo, _ = make_dev_repo(temp / "repo")
            status, receipt = handoff.handoff(arguments(repo, temp, draft_pr=True))
            self.assertEqual(status, 2)
            self.assertEqual(receipt["error_code"], "draft_pr_requires_publish")

    def test_rejects_dirty_tree_and_wrong_branch(self):
        with tempfile.TemporaryDirectory() as work:
            temp = Path(work)
            repo, base = make_dev_repo(temp / "repo")
            (repo / "untracked.txt").write_text("private", encoding="utf-8")
            with self.assertRaises(handoff.HandoffError) as exc:
                handoff.preview(repo, "CI-LOCAL-DEV-HANDOFF-01", fetch_main=False)
            self.assertEqual(str(exc.exception), "worktree_not_clean")
            (repo / "untracked.txt").unlink()
            git(repo, "switch", "-q", "--detach", base)
            with self.assertRaises(handoff.HandoffError):
                handoff.preview(repo, "CI-LOCAL-DEV-HANDOFF-01", fetch_main=False)

    def test_rejects_private_and_binary_changed_files(self):
        with tempfile.TemporaryDirectory() as work:
            temp = Path(work)
            repo, _ = make_dev_repo(temp / "repo")
            allowed = repo / "apps/web/example.mjs"
            allowed.write_bytes(b"sk-proj-PRIVATE")
            with self.assertRaises(handoff.HandoffError) as exc:
                handoff.validate_public_delta(repo, ["apps/web/example.mjs"])
            self.assertEqual(str(exc.exception), "private_key_or_token_marker")
            allowed.write_bytes(b"\x00\x01")
            with self.assertRaises(handoff.HandoffError):
                handoff.validate_public_delta(repo, ["apps/web/example.mjs"])
            (repo / "apps/web/picture.png").write_bytes(b"\x89PNG")
            with self.assertRaises(handoff.HandoffError):
                handoff.validate_public_delta(repo, ["apps/web/picture.png"])
            with self.assertRaises(handoff.HandoffError):
                handoff.validate_public_delta(repo, ["deploy/prod.yml"])
            with self.assertRaises(handoff.HandoffError):
                handoff.validate_public_delta(repo, [".github/workflows/evil.yml"])

    def test_rejects_symlink_and_escaping_path(self):
        with tempfile.TemporaryDirectory() as work:
            temp = Path(work)
            repo, _ = make_dev_repo(temp / "repo")
            link = repo / "apps/web/escape.mjs"
            try:
                link.symlink_to(temp / "missing-secret.mjs")
            except (OSError, NotImplementedError):
                self.skipTest("symlink unavailable")
            with self.assertRaises(handoff.HandoffError):
                handoff.validate_public_delta(repo, ["apps/web/escape.mjs"])
            with self.assertRaises(handoff.HandoffError):
                handoff.validate_public_delta(repo, ["apps/web/../private.mjs"])

    def test_blocks_intersecting_upstream_drift(self):
        with tempfile.TemporaryDirectory() as work:
            temp = Path(work)
            repo, base = make_dev_repo(temp / "repo")
            git(repo, "switch", "-qc", "test-upstream", base)
            (repo / "apps/web/example.mjs").write_text("export const upstream = 3;\n", encoding="utf-8")
            git(repo, "add", "--all")
            git(repo, "commit", "-qm", "upstream version")
            upstream = git(repo, "rev-parse", "HEAD")
            git(repo, "update-ref", "refs/remotes/origin/main", upstream)
            git(repo, "switch", "-q", "localdev/ci-local-dev-handoff-01")
            with self.assertRaises(handoff.HandoffError) as exc:
                handoff.preview(repo, "CI-LOCAL-DEV-HANDOFF-01", fetch_main=False)
            self.assertEqual(str(exc.exception), "intersecting_upstream_drift")

    def test_normal_git_push_refuses_non_fast_forward(self):
        with tempfile.TemporaryDirectory() as work:
            temp = Path(work)
            bare = temp / "origin.git"
            git(temp, "init", "--bare", "-q", str(bare))
            writer = temp / "writer"
            another = temp / "another"
            git(temp, "clone", "-q", str(bare), str(writer))
            git(writer, "config", "user.email", "test@example.invalid")
            git(writer, "config", "user.name", "Writer")
            branch = "localdev/ci-local-dev-handoff-01"
            git(writer, "switch", "-qc", branch)
            (writer / "work.txt").write_text("one\n", encoding="utf-8")
            git(writer, "add", "--all")
            git(writer, "commit", "-qm", "one")
            handoff.push_branch(writer, branch)
            git(temp, "clone", "-q", "--branch", branch, str(bare), str(another))
            git(another, "config", "user.email", "test@example.invalid")
            git(another, "config", "user.name", "Another")
            (another / "work.txt").write_text("two\n", encoding="utf-8")
            git(another, "add", "--all")
            git(another, "commit", "-qm", "two")
            handoff.push_branch(another, branch)
            (writer / "work.txt").write_text("divergence\n", encoding="utf-8")
            git(writer, "add", "--all")
            git(writer, "commit", "-qm", "divergence")
            with self.assertRaises(handoff.HandoffError):
                handoff.push_branch(writer, branch)

    def test_empty_and_wrong_origin(self):
        with tempfile.TemporaryDirectory() as work:
            temp = Path(work)
            repo, base = make_dev_repo(temp / "repo")
            git(repo, "remote", "set-url", "origin", str(temp / "bad-remote"))
            status, report = handoff.handoff(arguments(repo, temp))
            self.assertEqual(status, 2)
            self.assertEqual(report["error_code"], "origin_not_canonical")
            git(repo, "remote", "set-url", "origin", "https://github.com/Lalalalendia/rar2.git")
            git(repo, "update-ref", "refs/remotes/origin/main", git(repo, "rev-parse", "HEAD"))
            status, report = handoff.handoff(arguments(repo, temp))
            self.assertEqual(status, 2)
            self.assertEqual(report["error_code"], "no_local_change")


if __name__ == "__main__":
    unittest.main()
