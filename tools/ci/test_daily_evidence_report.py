"""Offline tests: NY boundaries, quantiles, sampling and trustworthy claims."""
import unittest
from datetime import date

from tools.ci.daily_evidence_report import (
    day_bounds, percentile, even_sample, split_days, verify_ledger, render,
)


def row(n, at):
    return {"number": n, "title": f"PR {n}", "pull_request": {"merged_at": at}}


class DailyTests(unittest.TestCase):
    def test_ny_summer_winter_and_dst(self):
        self.assertEqual(day_bounds(date(2026, 10, 9))[0].isoformat(), "2026-10-09T04:00:00+00:00")
        self.assertEqual(day_bounds(date(2026, 1, 9))[0].isoformat(), "2026-01-09T05:00:00+00:00")
        a, b = day_bounds(date(2026, 11, 1))
        self.assertEqual(int((b - a).total_seconds()), 25 * 3600)

    def test_pr_window_exact_inclusive_exclusive(self):
        old, new = split_days([
            row(1, "2026-10-09T03:59:59Z"),
            row(2, "2026-10-09T04:00:00Z"),
            row(3, "2026-10-10T03:59:59Z"),
            row(4, "2026-10-10T04:00:00Z"),
        ], date(2026, 10, 9))
        self.assertEqual([r["number"] for r in old], [1])
        self.assertEqual([r["number"] for r in new], [2, 3])

    def test_quantiles_missing_and_nearest_rank(self):
        self.assertEqual(percentile([None])["n"], 0)
        self.assertIsNone(percentile([None])["p90_s"])
        self.assertEqual(percentile([5, 10, 15, 20, None])["median_s"], 12.5)
        self.assertEqual(percentile([5, 10, 15, 20])["p90_s"], 20)

    def test_time_spaced_sampling_never_overstates_full_census(self):
        rows = [row(i, f"2026-10-09T{(i // 60):02d}:{(i % 60):02d}:00Z") for i in range(40)]
        picked = even_sample(rows, 8)
        self.assertEqual(len(picked), 8)
        self.assertEqual(len({x["number"] for x in picked}), 8)
        self.assertEqual(len(even_sample(rows, 0)), 0)

    def test_visual_title_is_not_pixel_proof(self):
        class Unused: pass
        got, excluded = verify_ledger(Unused(), [
            {"kind": "visual", "pr": 2433, "run_id": 11, "claim": "100%"},
        ], {2433})
        self.assertEqual(got, [])
        self.assertEqual(excluded[0]["pr"], 2433)

    def test_wrong_head_and_step_omission_fail_closed(self):
        class Fake:
            def get(self, path):
                if path == "pulls/3":
                    return {"merged_at": "2026-10-09T23:00:00Z", "head": {"sha": "a" * 40}}
                return {"head_sha": "b" * 40, "event": "pull_request",
                        "conclusion": "success", "created_at": "2026-10-09T22:00:00Z"}
        got, excluded = verify_ledger(Fake(), [{
            "kind": "scenario", "pr": 3, "run_id": 22,
            "required_steps": ["Real Chromium edit"],
        }], {3})
        self.assertEqual(got, [])
        self.assertIn("exact", excluded[0]["reason"])


if __name__ == "__main__":
    unittest.main()
