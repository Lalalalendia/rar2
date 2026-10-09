#!/usr/bin/env python3
"""Offline positive/negative controls for the repo-wide Actions latency receipt."""
import unittest

from tools.ci.repo_latency_census import (
    GitHub, analyze_pr, census, distribution, domains, seconds, summarize, timestamp,
)


HEAD = "a" * 40


def pr(**overrides):
    row = {
        "number": 2285,
        "title": "Cloud/Web scene",
        "head": {"sha": HEAD},
        "created_at": "2026-10-08T22:57:44Z",
        "merged_at": "2026-10-08T23:18:57Z",
    }
    row.update(overrides)
    return row


def run(id, name, when="2026-10-08T22:57:49Z", **overrides):
    row = {
        "id": id, "name": name, "head_sha": HEAD,
        "created_at": when, "event": "pull_request",
    }
    row.update(overrides)
    return row


def job(id, name, created, started, completed, conclusion="success", **overrides):
    row = {
        "id": id, "name": name, "created_at": created,
        "started_at": started, "completed_at": completed,
        "conclusion": conclusion, "labels": ["ubuntu-latest"],
    }
    row.update(overrides)
    return row


class TimeTests(unittest.TestCase):
    def test_durations_ignore_missing_naive_negative_values(self):
        self.assertEqual(
            seconds("2026-10-08T23:09:22Z", "2026-10-08T23:18:14Z"), 532
        )
        self.assertIsNone(seconds(None, "2026-10-08T23:18:14Z"))
        self.assertIsNone(seconds("2026-10-08T23:18:14Z", "2026-10-08T23:09:22Z"))
        self.assertIsNone(timestamp("2026-10-08T12:00:00"))
        self.assertIsNone(timestamp("not a timestamp"))

    def test_nearest_rank_is_declared_not_interpolated(self):
        self.assertEqual(
            distribution([17, None, 20, 532, 600]),
            {"n": 4, "p50_s": 276.0, "p95_s": 600},
        )
        self.assertEqual(
            distribution([None]), {"n": 0, "p50_s": None, "p95_s": None}
        )

    def test_every_component_classified_without_editor_bias(self):
        self.assertEqual(domains(["apps/web/foo.mjs"]), ["web"])
        self.assertEqual(domains(["apps/chaptera-server/src/lib.rs"]), ["cloud-server"])
        self.assertEqual(domains(["vendor/producer-a/crates/pub-reader/src/lib.rs"]), ["reader"])
        self.assertEqual(domains(["vendor/producer-a/crates/pub-editor/src/lib.rs"]), ["editor"])
        self.assertEqual(domains(["apps/chaptera-desktop/src/lib.rs"]), ["desktop"])
        self.assertEqual(domains(["apps/android/src/lib.rs"]), ["android"])
        self.assertEqual(domains(["tools/ci/guard.py"]), ["ci-infra"])
        self.assertEqual(
            domains(["apps/web/foo.mjs", "apps/chaptera-server/src/lib.rs"]),
            ["cloud-server", "web"],
        )

    def test_actual_font_env_queue_tail_vs_nine_second_computation(self):
        workflows = [
            run(1, "WEB-FONT-ENV-01"),
            run(2, "PR merge authority"),
            run(3, "Cancel obsolete PR head runs"),
            run(4, "WEB-FONT-ENV-01", when="2026-10-08T23:19:12Z"),
            run(5, "WEB-FONT-ENV-01", head_sha="b" * 40),
        ]
        jobs = {
            "1": [
                job(
                    100, "real-browser (firefox)",
                    "2026-10-08T22:58:54Z", "2026-10-08T23:08:54Z",
                    "2026-10-08T23:09:22Z",
                ),
                job(
                    101, "cross-browser",
                    "2026-10-08T23:09:22Z", "2026-10-08T23:18:14Z",
                    "2026-10-08T23:18:23Z",
                ),
            ],
            "2": [
                job(
                    200, "required-ci",
                    "2026-10-08T22:57:50Z", "2026-10-08T22:58:52Z",
                    "2026-10-08T23:18:32Z",
                ),
            ],
            "3": [
                job(
                    300, "cancel",
                    "2026-10-08T22:57:50Z", "2026-10-08T22:57:54Z",
                    "2026-10-08T22:57:56Z",
                ),
            ],
            "4": [
                job(400, "late", "2026-10-08T23:19:12Z",
                    "2026-10-08T23:19:12Z", "2026-10-08T23:19:14Z"),
            ],
            "5": [
                job(500, "wrong head", "2026-10-08T22:58:01Z",
                    "2026-10-08T22:58:01Z", "2026-10-08T22:58:02Z"),
            ],
        }
        row = analyze_pr(pr(), workflows, jobs, ["apps/web/font-environment-v1.mjs"])
        self.assertEqual(row["workflow_count"], 3)
        self.assertEqual(row["job_count"], 4)
        self.assertEqual(row["class"], "web")
        self.assertEqual(row["longest_queue"]["queue_s"], 600)
        self.assertEqual(row["queue_outliers"][1]["queue_s"], 532)
        self.assertEqual(row["queue_outliers"][1]["execution_s"], 9)
        self.assertEqual(row["first_relevant_feedback_s"], 693)
        self.assertEqual(row["merge_ready_s"], 1243)
        self.assertEqual(row["post_gate_to_merge_s"], 25)
        self.assertEqual(row["critical_tail"]["job"], "cross-browser")
        self.assertEqual(row["critical_tail"]["gap_to_gate_s"], 9)
        self.assertEqual(row["runner_active_s"], 28 + 9 + 1180 + 2)
        self.assertEqual(summarize([row])["web"]["merge_ready"]["p50_s"], 1243)

    def test_missing_gate_and_no_useful_jobs_remain_unknown(self):
        row = analyze_pr(
            pr(merged_at=None), [run(1, "PR merge authority")], {"1": [
                job(3, "required-ci", "2026-10-08T22:57:49Z",
                    "2026-10-08T22:57:50Z", None, conclusion=None)
            ]}, [".github/workflows/public-boundary-guard.yml"]
        )
        self.assertIsNone(row["merge_ready_s"])
        self.assertIsNone(row["first_relevant_feedback_s"])
        self.assertIsNone(row["pr_created_to_merged_s"])
        self.assertEqual(row["runner_active_s"], 0)
        self.assertEqual(row["class"], "ci-infra")

    def test_single_product_failure_is_fast_negative_feedback(self):
        row = analyze_pr(
            pr(), [run(1, "Android core")], {"1": [
                job(9, "android", "2026-10-08T22:57:49Z",
                    "2026-10-08T22:57:51Z", "2026-10-08T22:58:03Z",
                    conclusion="failure")
            ]}, ["apps/android/core/src/lib.rs"]
        )
        self.assertEqual(row["first_relevant_feedback_s"], 14)
        self.assertEqual(row["first_failure_s"], 14)
        self.assertIsNone(row["merge_ready_s"])


class CensusTests(unittest.TestCase):
    def test_census_requires_a_bounded_real_merged_sample(self):
        class FakeGitHub:
            repository = "Lalalalendia/rar2"

            def get(self, path):
                if path.startswith("pulls?"):
                    return [pr()]
                raise AssertionError(path)

            def paged(self, path, key=None, max_pages=8):
                if path.startswith("pulls/2285/files"):
                    return [{"filename": "apps/web/render-v1.mjs"}]
                if path.startswith("actions/runs?"):
                    return [run(10, "PR merge authority")]
                if path.startswith("actions/runs/10/jobs"):
                    return [
                        job(10, "required-ci", "2026-10-08T22:57:49Z",
                            "2026-10-08T22:57:50Z", "2026-10-08T22:58:00Z")
                    ]
                raise AssertionError(path)

        receipt = census(FakeGitHub(), 1)
        self.assertEqual(receipt["sample_count"], 1)
        self.assertEqual(receipt["repository"], "Lalalalendia/rar2")
        self.assertEqual(receipt["prs"][0]["merge_ready_s"], 11)
        self.assertIsNone(receipt["prs"][0]["first_relevant_feedback_s"])
        self.assertEqual(receipt["by_class"]["web"]["pr_count"], 1)


if __name__ == "__main__":
    unittest.main()
