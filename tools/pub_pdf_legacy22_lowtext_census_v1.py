#!/usr/bin/env python3
"""Publisher-oracle acceptance for the bounded Legacy22LowText PDF route."""
from __future__ import annotations

import argparse
from collections import Counter
import json
from pathlib import Path
import subprocess
import tempfile

from pub_pdf_cli_publisher_oracle_v1 import compare_pdf, load_references, sha256

SCHEMA = "chaptera.pub-pdf-legacy22-lowtext-acceptance.v1"
POSITIVE = (
    "002_4ab9b74e2f15b6f1",
    "004_e74a3c7ed167ee9b",
    "008_79210d84eb0558a1",
    "016_72a72400c1d9d0c0",
    "018_48384326430f61ec",
    "058_9307e2a826c2ed29",
    "072_7860acc670667c45",
    "076_09fca9b767f3a3f8",
    "079_5103362f268f977d",
    "080_72774de113195633",
)
NEGATIVE_QUILL = (
    "059_1b3d00a67c370d99",
    "068_2b1a2c5183d2fa83",
)


def run_cli(cli: Path, source: Path, font: Path, pdf: Path, timeout: int) -> tuple[int, str]:
    try:
        proc = subprocess.run(
            [str(cli), "convert", str(source), "--to", "pdf", "--output", str(pdf),
             "--fallback-font", str(font)],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.PIPE,
            text=True,
            errors="replace",
            check=False,
            timeout=timeout,
        )
    except subprocess.TimeoutExpired:
        return 124, "timeout"
    return proc.returncode, proc.stderr or ""


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--batch01", type=Path, required=True)
    parser.add_argument("--supplemental", type=Path, required=True)
    parser.add_argument("--source-root", type=Path, required=True)
    parser.add_argument("--cli", type=Path, required=True)
    parser.add_argument("--fallback-font", type=Path, required=True)
    parser.add_argument("--timeout", type=int, default=120)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()

    refs = {row["basename"]: row for row in load_references(args.batch01, args.supplemental)}
    if set(POSITIVE) - refs.keys() or set(NEGATIVE_QUILL) - refs.keys():
        raise SystemExit("registered legacy witness set drift")

    by_sha = {}
    for source in args.source_root.glob("*.pub"):
        by_sha[source.stem.casefold()] = source

    rows = []
    page_metrics = []
    status = Counter()
    with tempfile.TemporaryDirectory(prefix="legacy22-pdf-") as td:
        temp = Path(td)
        for name in POSITIVE:
            pair = refs[name]
            source = by_sha.get(pair["pub_sha256"])
            if source is None or source.stat().st_size != int(pair["pub_bytes"]) or sha256(source) != pair["pub_sha256"]:
                raise SystemExit(f"exact source unavailable for {name}")
            pdf = temp / f"{name}.pdf"
            code, stderr = run_cli(args.cli.resolve(), source, args.fallback_font.resolve(), pdf, args.timeout)
            row = {"fixture": name, "source_sha256": pair["pub_sha256"], "reference_pages": pair["reference_pages"]}
            if code != 0:
                row["status"] = "cli_failed"
                status["cli_failed"] += 1
            elif not pdf.is_file() or not Path(str(pdf) + ".loss.json").is_file():
                row["status"] = "output_missing"
                status["output_missing"] += 1
            else:
                loss = json.loads(Path(str(pdf) + ".loss.json").read_text(encoding="utf-8"))
                if loss["conversion_profile"]["source"]["profile_id"] != "pub-legacy-0x22-low-text-v0.1":
                    raise SystemExit(f"legacy conversion fence mismatch for {name}")
                if loss["conversion_profile"]["source"]["source_sha256"] != pair["pub_sha256"]:
                    raise SystemExit(f"source identity mismatch for {name}")
                comparison = compare_pdf(pdf, pair)
                row.update(comparison)
                status[comparison["status"]] += 1
                for page in comparison.get("pages", []):
                    if page.get("status") == "compared":
                        page_metrics.append({
                            "fixture": name,
                            "page": page["page"],
                            "changed_cell_fraction": page["changed_cell_fraction"],
                        })
            rows.append(row)

        negative_rows = []
        for name in NEGATIVE_QUILL:
            pair = refs[name]
            source = by_sha.get(pair["pub_sha256"])
            if source is None or sha256(source) != pair["pub_sha256"]:
                raise SystemExit(f"exact negative source unavailable for {name}")
            pdf = temp / f"{name}.pdf"
            code, stderr = run_cli(args.cli.resolve(), source, args.fallback_font.resolve(), pdf, args.timeout)
            blocked = code != 0 and (
                "mature 0x2C or legacy 0x22 low-text" in stderr
                or "requires mature 0x2C PUB input" in stderr
            )
            negative_rows.append({"fixture": name, "legacy22_quill_remains_blocked": blocked})
            if not blocked or pdf.exists():
                raise SystemExit(f"Legacy22Quill negative control escaped route fence: {name}")

    page_metrics.sort(key=lambda row: (-row["changed_cell_fraction"], row["fixture"], row["page"]))
    report = {
        "schema": SCHEMA,
        "positive_pair_count": len(POSITIVE),
        "negative_quill_pair_count": len(NEGATIVE_QUILL),
        "converted_pair_count": sum("candidate_pdf_sha256" in row for row in rows),
        "fully_compared_pair_count": sum(row.get("status") in ("raster_compared", "raster_compared_stage_unknown") for row in rows),
        "compared_page_count": len(page_metrics),
        "status_counts": dict(sorted(status.items())),
        "mean_changed_cell_fraction": (
            sum(row["changed_cell_fraction"] for row in page_metrics) / len(page_metrics)
            if page_metrics else None
        ),
        "worst_pages": page_metrics[:20],
        "pairs": rows,
        "negative_controls": negative_rows,
        "claims": {
            "legacy22_low_text_route_admitted": True,
            "legacy22_quill_route_admitted": False,
            "publisher_visual_parity_proven": False,
            "source_or_candidate_pdf_bytes_uploaded": False,
        },
    }
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps({k: report[k] for k in (
        "positive_pair_count", "converted_pair_count", "fully_compared_pair_count",
        "compared_page_count", "status_counts", "mean_changed_cell_fraction", "worst_pages",
    )}, indent=2, sort_keys=True))

    if report["converted_pair_count"] != len(POSITIVE):
        raise SystemExit("not all Legacy22LowText Publisher witnesses converted")
    if any(not row["legacy22_quill_remains_blocked"] for row in negative_rows):
        raise SystemExit("Legacy22Quill route widened unexpectedly")


if __name__ == "__main__":
    main()
