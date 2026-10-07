#!/usr/bin/env python3
from __future__ import annotations

import argparse
import csv
import hashlib
import json
import os
import re
import shutil
import stat
import subprocess
import tempfile
import zipfile
from pathlib import Path

import fitz

from cloud_reader_reference_pdf_raster_v1 import compare_cloud_rasters

SCHEMA = "chaptera.cloud-reader-manual-oracle-bundle.v1"
SUMMARY_SCHEMA = "chaptera.cloud-reader-manual-oracle-summary.v1"
READER_MANIFEST_SCHEMA = "chaptera.cloud-reader-real-fixtures.v1"
CFB_MAGIC = bytes.fromhex("d0cf11e0a1b11ae1")
SAFE_BASENAME = re.compile(r"^[A-Za-z0-9._-]+$")
SHA256 = re.compile(r"^[0-9a-f]{64}$")
REQUIRED_COLUMNS = {
    "oracle_id",
    "basename",
    "pub_filename",
    "pub_bytes",
    "pub_sha256",
    "pdf_filename",
    "pdf_bytes",
    "pdf_sha256",
    "pdf_pages",
}
MAX_BUNDLE_MEMBERS = 128
MAX_PAIR_COUNT = 64
MAX_PAIR_FILE_BYTES = 128 * 1024 * 1024
MAX_BUNDLE_UNCOMPRESSED_BYTES = 512 * 1024 * 1024


def file_sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _parse_positive_int(value: str, field: str, row: int) -> int:
    try:
        parsed = int(value)
    except (TypeError, ValueError) as error:
        raise ValueError(f"row {row}: {field} must be an integer") from error
    if parsed <= 0:
        raise ValueError(f"row {row}: {field} must be positive")
    return parsed


def _safe_zip_member(info: zipfile.ZipInfo) -> bool:
    path = Path(info.filename)
    if path.is_absolute() or ".." in path.parts:
        return False
    mode = (info.external_attr >> 16) & 0xFFFF
    return not stat.S_ISLNK(mode)


def materialize_bundle(bundle: Path, work_dir: Path) -> Path:
    bundle = bundle.resolve()
    work_dir = work_dir.resolve()
    work_dir.mkdir(parents=True, exist_ok=True)
    if bundle.is_dir():
        root = bundle
    elif zipfile.is_zipfile(bundle):
        extracted = work_dir / "bundle"
        if extracted.exists():
            shutil.rmtree(extracted)
        extracted.mkdir(parents=True)
        with zipfile.ZipFile(bundle) as archive:
            infos = archive.infolist()
            if not infos:
                raise ValueError("oracle bundle ZIP is empty")
            if len(infos) > MAX_BUNDLE_MEMBERS:
                raise ValueError("oracle bundle ZIP contains too many members")
            if any(not _safe_zip_member(info) for info in infos):
                raise ValueError("oracle bundle ZIP contains an unsafe path or symlink")
            declared_bytes = 0
            for info in infos:
                if info.is_dir():
                    continue
                if info.file_size > MAX_PAIR_FILE_BYTES:
                    raise ValueError("oracle bundle ZIP member exceeds bounded size")
                declared_bytes += info.file_size
                if declared_bytes > MAX_BUNDLE_UNCOMPRESSED_BYTES:
                    raise ValueError("oracle bundle ZIP exceeds bounded uncompressed size")
            archive.extractall(extracted)
        root = extracted
    else:
        raise ValueError("oracle bundle must be a directory or ZIP archive")

    if (root / "PAIRS.csv").is_file():
        return root
    candidates = sorted(path.parent for path in root.glob("*/PAIRS.csv") if path.is_file())
    if len(candidates) != 1:
        raise ValueError("oracle bundle must contain exactly one PAIRS.csv root")
    return candidates[0]


def load_pairs(root: Path) -> list[dict]:
    manifest = root / "PAIRS.csv"
    if not manifest.is_file():
        raise ValueError("PAIRS.csv is missing")
    with manifest.open(newline="", encoding="utf-8-sig") as handle:
        reader = csv.DictReader(handle)
        missing = REQUIRED_COLUMNS.difference(reader.fieldnames or [])
        if missing:
            raise ValueError(f"PAIRS.csv missing columns: {sorted(missing)}")
        rows = list(reader)
    if not rows:
        raise ValueError("PAIRS.csv contains no pairs")
    if len(rows) > MAX_PAIR_COUNT:
        raise ValueError("PAIRS.csv contains too many pairs")

    seen_ids: set[str] = set()
    declared_pair_bytes = 0
    seen_names: set[str] = set()
    normalized = []
    for number, raw in enumerate(rows, 2):
        oracle_id = (raw.get("oracle_id") or "").strip()
        basename = (raw.get("basename") or "").strip()
        pub_filename = (raw.get("pub_filename") or "").strip()
        pdf_filename = (raw.get("pdf_filename") or "").strip()
        pub_sha = (raw.get("pub_sha256") or "").strip().lower()
        pdf_sha = (raw.get("pdf_sha256") or "").strip().lower()

        if not oracle_id:
            raise ValueError(f"row {number}: oracle_id is required")
        if oracle_id in seen_ids:
            raise ValueError(f"row {number}: duplicate oracle_id {oracle_id!r}")
        seen_ids.add(oracle_id)

        if not SAFE_BASENAME.fullmatch(basename):
            raise ValueError(f"row {number}: basename is not path-safe: {basename!r}")
        if basename in seen_names:
            raise ValueError(f"row {number}: duplicate basename {basename!r}")
        seen_names.add(basename)
        if pub_filename != f"{basename}.pub" or pdf_filename != f"{basename}.pdf":
            raise ValueError(
                f"row {number}: PUB/PDF filenames must match basename exactly"
            )
        if not SHA256.fullmatch(pub_sha) or not SHA256.fullmatch(pdf_sha):
            raise ValueError(f"row {number}: SHA-256 fields must be canonical lowercase hex")

        pub_bytes = _parse_positive_int(raw.get("pub_bytes", ""), "pub_bytes", number)
        pdf_bytes = _parse_positive_int(raw.get("pdf_bytes", ""), "pdf_bytes", number)
        pdf_pages = _parse_positive_int(raw.get("pdf_pages", ""), "pdf_pages", number)
        if pub_bytes > MAX_PAIR_FILE_BYTES or pdf_bytes > MAX_PAIR_FILE_BYTES:
            raise ValueError(f"row {number}: pair file exceeds bounded size")
        declared_pair_bytes += pub_bytes + pdf_bytes
        if declared_pair_bytes > MAX_BUNDLE_UNCOMPRESSED_BYTES:
            raise ValueError("PAIRS.csv declares too many input bytes")
        pub_path = (root / pub_filename).resolve()
        pdf_path = (root / pdf_filename).resolve()
        if pub_path.parent != root.resolve() or pdf_path.parent != root.resolve():
            raise ValueError(f"row {number}: pair filenames must be direct bundle children")
        if not pub_path.is_file() or not pdf_path.is_file():
            raise ValueError(f"row {number}: pair file is missing")
        if pub_path.stat().st_size != pub_bytes or pdf_path.stat().st_size != pdf_bytes:
            raise ValueError(f"row {number}: byte length does not match PAIRS.csv")
        if file_sha256(pub_path) != pub_sha or file_sha256(pdf_path) != pdf_sha:
            raise ValueError(f"row {number}: file SHA-256 does not match PAIRS.csv")
        with pub_path.open("rb") as handle:
            if handle.read(len(CFB_MAGIC)) != CFB_MAGIC:
                raise ValueError(f"row {number}: PUB file does not have CFB magic")

        with fitz.open(pdf_path) as reference:
            if reference.page_count != pdf_pages:
                raise ValueError(f"row {number}: PDF page count does not match PAIRS.csv")
            metadata = reference.metadata or {}
            first_page = reference.load_page(0)
            page_rect = first_page.rect
            pdf_metadata = {
                "creator": metadata.get("creator") or "",
                "producer": metadata.get("producer") or "",
                "first_page_width_pt": round(page_rect.width, 6),
                "first_page_height_pt": round(page_rect.height, 6),
            }

        normalized.append(
            {
                "oracle_id": oracle_id,
                "basename": basename,
                "pub_filename": pub_filename,
                "pub_bytes": pub_bytes,
                "pub_sha256": pub_sha,
                "pub_path": str(pub_path),
                "pdf_filename": pdf_filename,
                "pdf_bytes": pdf_bytes,
                "pdf_sha256": pdf_sha,
                "pdf_pages": pdf_pages,
                "pdf_path": str(pdf_path),
                "pdf_metadata": pdf_metadata,
            }
        )
    return normalized


def prepare_bundle(
    bundle: Path,
    work_dir: Path,
    reader_manifest_path: Path,
    registry_path: Path,
) -> dict:
    root = materialize_bundle(bundle, work_dir)
    pairs = load_pairs(root)
    reader_manifest = {
        "schema": READER_MANIFEST_SCHEMA,
        "fixtures": [
            {
                "name": pair["basename"],
                "sha256": pair["pub_sha256"],
                "bytes": pair["pub_bytes"],
                "require_render": False,
                "require_shared_text": False,
                "source_path": pair["pub_path"],
            }
            for pair in pairs
        ],
    }
    registry = {
        "schema": SCHEMA,
        "bundle_root": str(root),
        "pair_count": len(pairs),
        "reference_page_count": sum(pair["pdf_pages"] for pair in pairs),
        "pairs": pairs,
        "claims": {
            "exact_pair_identity_checked": True,
            "raw_pub_bytes_emitted": False,
            "raw_story_text_emitted": False,
            "pdf_used_as_visual_authority_only": True,
        },
    }
    reader_manifest_path.parent.mkdir(parents=True, exist_ok=True)
    registry_path.parent.mkdir(parents=True, exist_ok=True)
    reader_manifest_path.write_text(
        json.dumps(reader_manifest, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    registry_path.write_text(
        json.dumps(registry, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    return registry


def _summary_pair(receipt: dict) -> dict:
    fractions = [
        page["diff"]["significant_fraction"]
        for page in receipt.get("pages", [])
        if page["diff"]["significant_fraction"] is not None
    ]
    return {
        "fixture": receipt["fixture"],
        "source_pub_sha256": receipt["source_pub_sha256"],
        "reference_sha256": receipt["reference_sha256"],
        "classification": receipt["classification"],
        "terminal_code": receipt.get("terminal_code"),
        "comparison_state": receipt["comparison_state"],
        "candidate_pages": receipt["page_count"]["candidate"],
        "reference_pages": receipt["page_count"]["reference"],
        "page_count_match": receipt["page_count"]["match"],
        "max_significant_fraction": max(fractions) if fractions else None,
        "mean_significant_fraction": (
            sum(fractions) / len(fractions) if fractions else None
        ),
        "reference_media_extent_mismatch_pages": sum(
            not page.get(
                "reference_media_extent_matches_candidate_page",
                page["physical_page_size_matches_reference"],
            )
            for page in receipt.get("pages", [])
        ),
        "fidelity_reasons": receipt.get("fidelity_reasons", []),
        "diagnostic_codes": receipt.get("diagnostic_codes", []),
    }


def compare_bundle(
    cloud_receipt_path: Path,
    registry_path: Path,
    output_dir: Path,
    summary_path: Path,
) -> dict:
    registry = json.loads(registry_path.read_text(encoding="utf-8"))
    if registry.get("schema") != SCHEMA:
        raise ValueError(f"unsupported oracle registry schema: {registry.get('schema')!r}")
    pairs = registry.get("pairs")
    if not isinstance(pairs, list) or not pairs:
        raise ValueError("oracle registry has no pairs")

    output_dir.mkdir(parents=True, exist_ok=True)
    pair_summaries = []
    ranked_pages = []
    reference_media_extent_mismatch_pages = 0
    page_count_mismatch_pairs = 0
    unsupported_pairs = []

    for pair in pairs:
        receipt = compare_cloud_rasters(
            cloud_receipt_path,
            pair["basename"],
            Path(pair["pdf_path"]),
        )
        if receipt["source_pub_sha256"] != pair["pub_sha256"]:
            raise ValueError(f"Reader source identity drift for {pair['basename']}")
        if receipt["reference_sha256"] != pair["pdf_sha256"]:
            raise ValueError(f"reference PDF identity drift for {pair['basename']}")
        pair_path = output_dir / f"{pair['basename']}.json"
        pair_path.write_text(
            json.dumps(receipt, indent=2, sort_keys=True) + "\n",
            encoding="utf-8",
        )
        pair_summaries.append(_summary_pair(receipt))
        if receipt["page_count"]["match"] is False:
            page_count_mismatch_pairs += 1
        if receipt["comparison_state"] != "compared":
            unsupported_pairs.append(
                {
                    "fixture": pair["basename"],
                    "classification": receipt["classification"],
                    "terminal_code": receipt.get("terminal_code"),
                    "comparison_state": receipt["comparison_state"],
                }
            )
            continue
        for page in receipt["pages"]:
            if not page.get(
                "reference_media_extent_matches_candidate_page",
                page["physical_page_size_matches_reference"],
            ):
                reference_media_extent_mismatch_pages += 1
            diff = page["diff"]
            ranked_pages.append(
                {
                    "fixture": pair["basename"],
                    "page": page["page_index"] + 1,
                    "significant_fraction": diff["significant_fraction"],
                    "mean_abs_channel_delta": diff["mean_abs_channel_delta"],
                    "max_channel_delta": diff["max_channel_delta"],
                    "significant_bbox": diff["significant_bbox"],
                    "largest_regions": diff.get("regions", [])[:5],
                    "reference_media_extent_matches_candidate_page": page.get(
                        "reference_media_extent_matches_candidate_page",
                        page["physical_page_size_matches_reference"],
                    ),
                    "comparison_scope": page.get(
                        "comparison_scope",
                        (
                            "publication_page_raster"
                            if page["physical_page_size_matches_reference"]
                            else "reference_media_extent_differs_from_candidate_page"
                        ),
                    ),
                    "physical_page_size_matches_reference": page[
                        "physical_page_size_matches_reference"
                    ],
                }
            )

    ranked_pages.sort(
        key=lambda row: (
            row["significant_fraction"] is None,
            -(row["significant_fraction"] or 0.0),
            row["fixture"],
            row["page"],
        )
    )
    compared_fractions = [
        row["significant_fraction"]
        for row in ranked_pages
        if row["significant_fraction"] is not None
    ]
    cloud = json.loads(cloud_receipt_path.read_text(encoding="utf-8"))
    summary = {
        "schema": SUMMARY_SCHEMA,
        "repository_commit_sha": cloud.get("repository_commit_sha"),
        "browser": cloud.get("browser"),
        "pair_count": len(pairs),
        "reference_page_count": registry.get("reference_page_count"),
        "compared_page_count": len(ranked_pages),
        "compared_pair_count": sum(
            row["comparison_state"] == "compared" for row in pair_summaries
        ),
        "unsupported_pair_count": len(unsupported_pairs),
        "page_count_mismatch_pair_count": page_count_mismatch_pairs,
        "reference_media_extent_mismatch_page_count": reference_media_extent_mismatch_pages,
        # Backward-compatible count only. It does not assert that the source
        # publication Page.size is wrong; PDF media can be printer/sheet state.
        "physical_size_mismatch_page_count": reference_media_extent_mismatch_pages,
        "corpus_mean_significant_fraction": (
            sum(compared_fractions) / len(compared_fractions)
            if compared_fractions
            else None
        ),
        "worst_page": ranked_pages[0] if ranked_pages else None,
        "ranked_pages": ranked_pages,
        "unsupported_pairs": unsupported_pairs,
        "pairs": pair_summaries,
        "claims": {
            "exact_external_pair_identity_checked": True,
            "publisher_visual_parity": False,
            "pdf_used_as_visual_authority_only": True,
            "reference_pdf_media_extent_used_as_page_size_authority": False,
            "source_page_size_inferred_from_reference_pdf": False,
            "raw_pub_bytes_emitted": False,
            "raw_story_text_emitted": False,
        },
    }
    summary_path.parent.mkdir(parents=True, exist_ok=True)
    summary_path.write_text(
        json.dumps(summary, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    return summary


def run_bundle(args: argparse.Namespace) -> dict:
    repo_root = args.repo_root.resolve()
    output_dir = args.output_dir.resolve()
    output_dir.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="chaptera-manual-oracle-") as temporary:
        temporary_path = Path(temporary)
        manifest = temporary_path / "reader-fixtures.json"
        registry = temporary_path / "pair-registry.json"
        prepare_bundle(args.bundle, temporary_path / "input", manifest, registry)
        browser_output = output_dir / "browser"
        env = os.environ.copy()
        env.update(
            {
                "READER_WORKER_BINARY": str(args.worker_binary.resolve()),
                "READER_REAL_MANIFEST": str(manifest),
                "READER_REAL_OUTPUT": str(browser_output),
                "READER_REFERENCE_RASTER_DPI": "144",
            }
        )
        subprocess.run(
            [args.node, "apps/cloud-reader/real-browser.test.mjs"],
            cwd=repo_root,
            env=env,
            check=True,
        )
        return compare_bundle(
            browser_output / "receipt.json",
            registry,
            output_dir / "pairs",
            output_dir / "summary.json",
        )


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Validate and compare an external Microsoft Publisher PUB/PDF visual-oracle bundle."
    )
    subparsers = parser.add_subparsers(dest="command", required=True)

    prepare = subparsers.add_parser("prepare")
    prepare.add_argument("bundle", type=Path)
    prepare.add_argument("work_dir", type=Path)
    prepare.add_argument("reader_manifest", type=Path)
    prepare.add_argument("pair_registry", type=Path)

    compare = subparsers.add_parser("compare")
    compare.add_argument("cloud_receipt", type=Path)
    compare.add_argument("pair_registry", type=Path)
    compare.add_argument("output_dir", type=Path)
    compare.add_argument("summary", type=Path)

    run = subparsers.add_parser("run")
    run.add_argument("bundle", type=Path)
    run.add_argument("worker_binary", type=Path)
    run.add_argument("output_dir", type=Path)
    run.add_argument("--repo-root", type=Path, default=Path(__file__).resolve().parents[1])
    run.add_argument("--node", default="node")
    return parser


def main() -> None:
    parser = build_parser()
    args = parser.parse_args()
    if args.command == "prepare":
        result = prepare_bundle(
            args.bundle,
            args.work_dir,
            args.reader_manifest,
            args.pair_registry,
        )
        print(
            f"MANUAL ORACLE PREPARED pairs={result['pair_count']} "
            f"reference_pages={result['reference_page_count']}"
        )
    elif args.command == "compare":
        result = compare_bundle(
            args.cloud_receipt,
            args.pair_registry,
            args.output_dir,
            args.summary,
        )
        worst = result["worst_page"]
        if worst:
            print(
                "MANUAL ORACLE COMPARED "
                f"pages={result['compared_page_count']} "
                f"worst={worst['fixture']}:p{worst['page']}="
                f"{worst['significant_fraction']:.6f}"
            )
        else:
            print("MANUAL ORACLE COMPARED pages=0")
    else:
        result = run_bundle(args)
        worst = result["worst_page"]
        if worst:
            print(
                "MANUAL ORACLE RUN COMPLETE "
                f"pages={result['compared_page_count']} "
                f"worst={worst['fixture']}:p{worst['page']}="
                f"{worst['significant_fraction']:.6f}"
            )
        else:
            print("MANUAL ORACLE RUN COMPLETE pages=0")


if __name__ == "__main__":
    main()
# CI baseline control: current-main Virginia text-layout census; no semantic change.
# CI baseline synchronize trigger: still no semantic change.
