#!/usr/bin/env python3
"""Read-only, evidence-gated daily rar2 report (New York calendar days).

Counts use all indexed merged PRs. CI quantiles refer ONLY to a deterministic
sample and must never be presented as repository-wide values.
"""
from __future__ import annotations

import argparse
from datetime import date, datetime, time, timedelta, timezone
import json
import math
import os
from pathlib import Path
import re
import statistics
import sys
from urllib.parse import urlencode
from urllib.request import Request, urlopen
from zoneinfo import ZoneInfo

from tools.ci.repo_latency_census import GitHub, analyze_pr, seconds, timestamp

TZ = ZoneInfo("America/New_York")
SCHEMA = "rar2.daily-evidence.v1"
BLOCKERS = {
    1727: "Crash-safe EditorProject durability: fresh current-main acceptance",
    2348: "Exact082: lawful physical-font A/B still required",
    2416: "PDF crop: 2 reported regressions need discriminating control",
}


def day_bounds(day: date):
    start = datetime.combine(day, time.min, TZ).astimezone(timezone.utc)
    end = datetime.combine(day + timedelta(days=1), time.min, TZ).astimezone(timezone.utc)
    return start, end


def percentile(values):
    nums = sorted(v for v in values if v is not None)
    return {
        "n": len(nums),
        "median_s": statistics.median(nums) if nums else None,
        "p90_s": nums[math.ceil(0.9 * len(nums)) - 1] if nums else None,
    }


def even_sample(rows, limit):
    """Spread the small descriptive sample across the whole day."""
    rows = sorted(rows, key=lambda r: (r["pull_request"]["merged_at"], r["number"]))
    count = min(limit, len(rows))
    return [rows[((2 * i + 1) * len(rows)) // (2 * count)] for i in range(count)] if count else []


class Api(GitHub):
    def global_get(self, path, params):
        url = "https://api.github.com/" + path + "?" + urlencode(params)
        headers = {
            "Accept": "application/vnd.github+json",
            "User-Agent": "rar2-daily-evidence-v1",
            "X-GitHub-Api-Version": "2022-11-28",
        }
        if self.token:
            headers["Authorization"] = "Bearer " + self.token
        with urlopen(Request(url, headers=headers), timeout=35) as response:
            return json.load(response)

    def merged_search(self, first, last):
        # Search by whole UTC dates, then filter by *actual* merged_at
        # timestamps below. GitHub search date syntax is not a time-range API.
        q = (
            f"repo:{self.repository} is:pr is:merged "
            f"merged:{first.date().isoformat()}..{(last - timedelta(seconds=1)).date().isoformat()}"
        )
        rows = []
        total = None
        for page in range(1, 11):
            data = self.global_get("search/issues", {
                "q": q, "per_page": 100, "page": page,
            })
            if data.get("incomplete_results"):
                raise RuntimeError("GitHub merged search is incomplete")
            if total is None:
                total = data["total_count"]
                if total > 900:
                    raise RuntimeError("GitHub 1000-item search cap risks silent undercount")
            rows.extend(data["items"])
            if len(rows) >= total:
                break
        if len(rows) != total or len({x["number"] for x in rows}) != len(rows):
            raise RuntimeError("Merged search pagination was incomplete or unstable")
        for row in rows:
            if not timestamp((row.get("pull_request") or {}).get("merged_at")):
                raise RuntimeError(f"Missing merged_at for PR #{row.get('number')}")
        return rows


def split_days(rows, day):
    previous_start, today_start = day_bounds(day - timedelta(days=1))[0], day_bounds(day)[0]
    today_end = day_bounds(day)[1]
    previous, today = [], []
    for row in rows:
        at = timestamp(row["pull_request"]["merged_at"])
        if previous_start <= at < today_start:
            previous.append(row)
        elif today_start <= at < today_end:
            today.append(row)
    return previous, today


def measure_sample(client, selected):
    results, all_queues, reruns = [], [], 0
    for item in selected:
        number = item["number"]
        pr = client.get(f"pulls/{number}")
        paths = [f["filename"] for f in client.paged(f"pulls/{number}/files", max_pages=8)]
        runs = client.paged(
            f"actions/runs?head_sha={pr['head']['sha']}&event=pull_request",
            key="workflow_runs", max_pages=8,
        )
        runs = [
            run for run in runs
            if run.get("head_sha") == pr["head"]["sha"]
            and run.get("event") == "pull_request"
            and timestamp(run.get("created_at"))
            and timestamp(run["created_at"]) <= timestamp(pr["merged_at"])
        ]
        by_run = {}
        for run in runs:
            jobs = client.paged(f"actions/runs/{run['id']}/jobs", key="jobs", max_pages=8)
            by_run[str(run["id"])] = jobs
            reruns += max(0, int(run.get("run_attempt") or 1) - 1)
            for job in jobs:
                q = seconds(job.get("created_at"), job.get("started_at"))
                if job.get("status") == "completed" and job.get("conclusion") not in (None, "skipped") and q is not None:
                    all_queues.append(q)
        results.append(analyze_pr(pr, runs, by_run, paths))
    return {
        "sample_count": len(results),
        "sample_method": "time-spaced deterministic PR selection, not random",
        "sample_pr_numbers": [r["pr_number"] for r in results],
        "first_feedback": percentile([r["first_relevant_feedback_s"] for r in results]),
        "required_ci": percentile([r["merge_ready_s"] for r in results]),
        "queue_per_job": percentile(all_queues),
        "workflow_fanout_per_pr": percentile([r["workflow_count"] for r in results]),
        "jobs_per_pr": percentile([r["job_count"] for r in results]),
        "observed_extra_run_attempts_lower_bound": reruns,
        "prs": results,
    }


def verify_ledger(client, items, merged_numbers):
    accepted, excluded = [], []
    for item in items:
        reason = None
        number, run_id = item.get("pr"), item.get("run_id")
        if number not in merged_numbers:
            continue
        if item.get("kind") not in ("scenario", "conversion"):
            # An Actions success alone does not verify numerical raster cells.
            reason = "visual A/B needs machine-readable paired receipt; no automatic claim"
        else:
            pr = client.get(f"pulls/{number}")
            run = client.get(f"actions/runs/{run_id}")
            if (
                not pr.get("merged_at")
                or run.get("head_sha") != pr["head"]["sha"]
                or run.get("event") != "pull_request"
                or run.get("conclusion") != "success"
                or not timestamp(run.get("created_at"))
                or timestamp(run["created_at"]) > timestamp(pr["merged_at"])
            ):
                reason = "not successful exact merged-head pre-merge PR run"
            else:
                jobs = client.paged(f"actions/runs/{run_id}/jobs", key="jobs", max_pages=8)
                good = {
                    step["name"]
                    for job in jobs if job.get("conclusion") == "success"
                    for step in job.get("steps", [])
                    if step.get("conclusion") == "success"
                }
                if not item.get("required_steps") or not set(item["required_steps"]).issubset(good):
                    reason = "required real-user steps are missing or not successful"
        if reason:
            excluded.append({"pr": number, "run_id": run_id, "reason": reason})
        else:
            accepted.append({k: v for k, v in item.items() if k != "required_steps"})
    return accepted, excluded


def blockers_now(client):
    rows = []
    for number, reason in BLOCKERS.items():
        pr = client.get(f"pulls/{number}")
        if pr.get("state") == "open":
            rows.append({"pr": number, "title": pr.get("title"), "issue": reason, "state": "open"})
    return rows


def fmt(stats):
    if not stats or stats["n"] == 0:
        return "н/д (n=0)"
    def sec(v):
        return f"{v:g} с"
    return f"median {sec(stats['median_s'])}; p90 {sec(stats['p90_s'])}; n={stats['n']}"


def render(data):
    day = data["day"]
    prev = data["previous_day"]
    t = data["today"]
    p = data["previous"]
    lines = [
        f"# rar2 — ежедневный отчёт за {day} (America/New_York)",
        "",
        f"Окно UTC: {t['start_utc']} — {t['end_utc']} (конец исключён).",
        f"**Merged PR:** {t['merged_count']} против {p['merged_count']} за {prev} "
        f"({t['merged_count'] - p['merged_count']:+d}). Это активность, не продуктовый эффект.",
        "",
        "## Пользовательские сценарии и конвертация",
    ]
    found = [x for x in data["verified"] if x["kind"] in ("scenario", "conversion")]
    if found:
        for x in found:
            lines.append(
                f"- [#{x['pr']}](https://github.com/{data['repo']}/pull/{x['pr']}): "
                f"{x['claim']} Проверка: [run {x['run_id']}](https://github.com/"
                f"{data['repo']}/actions/runs/{x['run_id']}). Ограничение: {x['limit']}"
            )
    else:
        lines.append("- Подтверждённых по exact-head реальным пользовательским шагам новых сценариев нет в реестре; это **не** доказательство отсутствия изменений.")
    lines += ["", "## Визуальное A/B"]
    lines.append(
        "- Нет автоматически верифицированного парного машинного receipt с одинаковыми входными SHA, "
        "эталоном и растеризацией. Метрики из заголовков PR сюда **не** попадают."
    )
    if data["excluded"]:
        lines.append(
            f"- Исключено из списка доказательств: {len(data['excluded'])} записей; причины — в receipt.json."
        )
    lines += [
        "", "## Время обратной связи CI (описательная выборка, не repo-wide)",
        f"За {day}: {t['ci']['sample_count']} из {t['merged_count']} merged PR, распределённых по суткам.",
        f"- Очередь job created→started: {fmt(t['ci']['queue_per_job'])}.",
        f"- Первая полезная проверка от первого run creation: {fmt(t['ci']['first_feedback'])}.",
        f"- До required-ci SUCCESS от первого run creation: {fmt(t['ci']['required_ci'])}.",
        f"- Fanout workflow runs на PR: median {t['ci']['workflow_fanout_per_pr']['median_s']} / p90 {t['ci']['workflow_fanout_per_pr']['p90_s']} зап.; n={t['ci']['workflow_fanout_per_pr']['n']}.",
        f"- Дополнительные попытки (нижняя оценка по run_attempt): {t['ci']['observed_extra_run_attempts_lower_bound']}.",
        f"За {prev}: {p['ci']['sample_count']} из {p['merged_count']} merged PR.",
        f"- Очередь: {fmt(p['ci']['queue_per_job'])}; первая полезная проверка: "
        f"{fmt(p['ci']['first_feedback'])}; required-ci: {fmt(p['ci']['required_ci'])}.",
        "- Это выборки разных PR, **не** контролируемый A/B CI. Нельзя приписывать изменения оптимизациям. "
        "Ранние/отменённые SHA и старые попытки могут отсутствовать.",
        "", "## Незавершённые блокеры",
    ]
    if data["blockers"]:
        for b in data["blockers"]:
            lines.append(f"- [#{b['pr']}](https://github.com/{data['repo']}/pull/{b['pr']}): {b['issue']}.")
    else:
        lines.append("- Заранее отслеживаемые PR уже не открыты; отсутствие других блокеров не доказано.")
    lines += [
        "", "## Три приоритета (рекомендации, не измеренные результаты)",
        "1. Получить второй **независимый** native Publisher-accepted PUB через реальный UI, затем свежий reopen; не расширять сохранение без этой пары.",
        "2. Провести законный physical-font exact082 A/B и отдельно объяснить обе crop-регрессии до merge.",
        "3. Расширить сравнимые CI-когорты и отследить полное push→feedback; не путать run creation с push.",
        "", "Регрессии без подтверждённого парного A/B не оцениваются как ноль.",
        "Машинные данные, полные merged PR и выборки — в JSON-артефакте workflow.",
        "",
    ]
    return "\n".join(lines)


def build(client, day, sample_limit, ledger):
    prev_start = day_bounds(day - timedelta(days=1))[0]
    end = day_bounds(day)[1]
    merged = client.merged_search(prev_start, end)
    older, newer = split_days(merged, day)
    verified, excluded = verify_ledger(client, ledger, {x["number"] for x in newer})
    today_start, today_end = day_bounds(day)
    prior_start, prior_end = day_bounds(day - timedelta(days=1))
    return {
        "schema": SCHEMA, "generated_at_utc": datetime.now(timezone.utc).isoformat(),
        "repo": client.repository, "day": day.isoformat(),
        "previous_day": (day - timedelta(days=1)).isoformat(),
        "today": {"start_utc": today_start.isoformat(), "end_utc": today_end.isoformat(),
                  "merged_count": len(newer), "merged_prs": [
                      {"number": r["number"], "title": r["title"], "merged_at": r["pull_request"]["merged_at"]}
                      for r in sorted(newer, key=lambda x: x["pull_request"]["merged_at"])
                  ], "ci": measure_sample(client, even_sample(newer, sample_limit))},
        "previous": {"start_utc": prior_start.isoformat(), "end_utc": prior_end.isoformat(),
                     "merged_count": len(older), "merged_prs": [
                         {"number": r["number"], "title": r["title"], "merged_at": r["pull_request"]["merged_at"]}
                         for r in sorted(older, key=lambda x: x["pull_request"]["merged_at"])
                     ], "ci": measure_sample(client, even_sample(older, sample_limit))},
        "verified": verified, "excluded": excluded, "blockers": blockers_now(client),
        "caveats": [
            "Merged PR counts are from complete GitHub search, post-filtered by actual merged_at in NY days.",
            "CI distributions are only a deterministic time-spaced sample, NOT repository-wide median/p90.",
            "Run creation is not push time. Job queue is created_at-to-started_at for non-skipped latest jobs.",
            "run_attempt is an observed lower bound; previous heads, old rerun jobs and external CI are excluded.",
            "Visual A/B requires paired machine-readable input/reference receipts and is not inferred from PR titles.",
            "Verified real UI scenarios are restricted to exact merged SHA, successful jobs and named successful steps.",
            "The blockers list is a tracked subset; unlisted blockers are unknown.",
        ],
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", default="Lalalalendia/rar2")
    parser.add_argument("--day", default=None, help="YYYY-MM-DD New York date; default yesterday")
    parser.add_argument("--sample", type=int, default=8, help="PR sample per day, 0..12")
    parser.add_argument("--outdir", type=Path, default=Path("target/ci-daily-evidence"))
    parser.add_argument("--ledger", type=Path, default=Path("tools/ci/daily_verified_evidence.json"))
    args = parser.parse_args()
    if not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", args.repo):
        parser.error("repo must be owner/name")
    if not 0 <= args.sample <= 12:
        parser.error("sample must be 0..12")
    day = date.fromisoformat(args.day) if args.day else datetime.now(TZ).date() - timedelta(days=1)
    client = Api(args.repo, os.environ.get("GH_TOKEN") or os.environ.get("GITHUB_TOKEN") or "")
    ledger = json.loads(args.ledger.read_text(encoding="utf-8"))["evidence"]
    data = build(client, day, args.sample, ledger)
    args.outdir.mkdir(parents=True, exist_ok=True)
    (args.outdir / "receipt.json").write_text(json.dumps(data, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    text = render(data)
    (args.outdir / "report.md").write_text(text, encoding="utf-8")
    print(text)


if __name__ == "__main__":
    try:
        main()
    except Exception as exc:
        print(f"daily evidence failed (no report emitted): {exc}", file=sys.stderr)
        raise
