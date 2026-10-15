#!/usr/bin/env python3
"""Offline positive/negative controls for the release event dispatch guard."""
from __future__ import annotations

import copy
import unittest
from pathlib import Path

from cloud_reader_release_dispatch_guard import (
    ReleaseDispatchRejected, validate,
)

SHA = "1" * 40
NEW = "2" * 40
ASSET = "chaptera-cloud-reader-linux-amd64.tar.gz"


def packet(sha: str, published: str = "2026-10-15T12:00:00Z") -> dict:
    return {
        "tag_name": "cloud-reader-" + sha,
        "target_commitish": sha,
        "draft": False,
        "published_at": published,
        "assets": [{"name": ASSET, "size": 20139600,
                    "digest": "sha256:" + "a" * 64}],
    }


def event() -> dict:
    return {
        "action": "completed",
        "workflow_run": {
            "name": "Cloud Reader Linux release packet",
            "conclusion": "success",
            "event": "push",
            "head_branch": "main",
            "head_sha": SHA,
            "head_repository": {"full_name": "Lalalalendia/rar2"},
        },
    }


def good() -> tuple[dict, dict, list, dict]:
    release = packet(SHA)
    return event(), release, [release], {"status": "identical", "behind_by": 0}


class TestCloudReaderReleaseDispatch(unittest.TestCase):
    def test_trusted_exact_released_main(self) -> None:
        self.assertEqual(validate(*good()), SHA)

    def test_trusted_ancestor_of_newer_main(self) -> None:
        e, r, rs, _ = good()
        self.assertEqual(validate(e, r, rs, {"status": "ahead", "behind_by": 0}), SHA)

    def test_upstream_only_successful_same_repo_main_push(self) -> None:
        mutations = [
            ("action", "requested"),
            ("workflow_run.conclusion", "failure"),
            ("workflow_run.event", "workflow_dispatch"),
            ("workflow_run.head_branch", "feature/unsafe"),
            ("workflow_run.name", "Untrusted workflow"),
            ("workflow_run.head_sha", "x" * 40),
            ("workflow_run.head_repository.full_name", "foreign/fork"),
        ]
        for key, bad_value in mutations:
            with self.subTest(key=key):
                e, r, rs, c = copy.deepcopy(good())
                node = e
                steps = key.split(".")
                for step in steps[:-1]:
                    node = node[step]
                node[steps[-1]] = bad_value
                with self.assertRaises(ReleaseDispatchRejected):
                    validate(e, r, rs, c)

    def test_reject_wrong_tag_target_missing_digest_or_draft(self) -> None:
        cases = [
            ("tag_name", "other"),
            ("target_commitish", NEW),
            ("draft", True),
            ("published_at", None),
            ("assets", [{"name": ASSET, "size": 1, "digest": "unknown"}]),
            ("assets", [{"name": ASSET, "size": 0,
                         "digest": "sha256:" + "a" * 64}]),
        ]
        for key, value in cases:
            with self.subTest(key=key):
                e, r, rs, c = copy.deepcopy(good())
                r[key] = value
                with self.assertRaises(ReleaseDispatchRejected):
                    validate(e, r, rs, c)

    def test_stale_reordered_releases_cannot_dispatch(self) -> None:
        e, r, rs, c = good()
        rs.append(packet(NEW, "2026-10-15T12:01:00Z"))
        with self.assertRaisesRegex(ReleaseDispatchRejected, "Stale"):
            validate(e, r, rs, c)

    def test_nonreader_or_undigested_releases_do_not_steal_latest(self) -> None:
        e, r, rs, c = good()
        undigested = packet(NEW, "2026-10-15T12:01:00Z")
        undigested["assets"][0]["digest"] = None
        rs += [
            undigested,
            {"tag_name": "not-a-reader", "draft": False,
             "published_at": "2026-10-15T12:02:00Z",
             "assets": [{"name": ASSET, "size": 1,
                         "digest": "sha256:" + "a" * 64}]},
        ]
        self.assertEqual(validate(e, r, rs, c), SHA)

    def test_refuse_unrelated_or_behind_base(self) -> None:
        for relation in ("behind", "diverged"):
            with self.subTest(relation=relation):
                e, r, rs, c = good()
                c["status"] = relation
                with self.assertRaises(ReleaseDispatchRejected):
                    validate(e, r, rs, c)
        e, r, rs, c = good()
        c["behind_by"] = 1
        with self.assertRaises(ReleaseDispatchRejected):
            validate(e, r, rs, c)

    def test_workflow_statically_protects_app_dispatch(self) -> None:
        source = (Path(__file__).resolve().parents[2] / ".github" / "workflows"
                  / "cloud-reader-published-deploy-dispatch.yml").read_text()
        for snippet in (
            "workflow_run:", "types: [completed]",
            "github.event.workflow_run.conclusion == 'success'",
            "github.event.workflow_run.event == 'push'",
            "github.event.workflow_run.head_branch == 'main'",
            "permission-actions: write",
            "repositories: chaptera-site",
            "CHAPTERA_DEPLOY_APP_PRIVATE_KEY",
            "inputs[release_mode]=published",
            "cloud_reader_release_dispatch_guard.py",
        ):
            self.assertIn(snippet, source)
        for forbidden in ("pull_request_target:", "secrets: inherit", "contents: write"):
            self.assertNotIn(forbidden, source)


if __name__ == "__main__":
    unittest.main()
