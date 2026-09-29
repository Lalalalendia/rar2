#!/usr/bin/env python3
from __future__ import annotations

import argparse
import hashlib
import json
import subprocess
import tempfile
import urllib.request
from pathlib import Path


NEWSLETTER_URL = "https://raw.githubusercontent.com/apache/poi/942d95d85b15d0dfdb3bc9ba1b4f273f277757c8/test-data/publisher/SampleNewsletter.pub"
NEWSLETTER_SHA256 = "6a825ba26ba35d6e885acdc62e859591ed37cb0ff7480b554b9cb362b644dfcf"
BROCHURE_URL = "https://raw.githubusercontent.com/apache/poi/942d95d85b15d0dfdb3bc9ba1b4f273f277757c8/test-data/publisher/SampleBrochure.pub"
BROCHURE_SHA256 = "ffed034ac87e679f0bd08ff9cf74ad11c0e0e510a42b1bc1a7502415f6c29c87"


def acquire(url: str, expected_sha256: str, target: Path) -> None:
    with urllib.request.urlopen(url, timeout=60) as response:
        payload = response.read()
    actual = hashlib.sha256(payload).hexdigest()
    if actual != expected_sha256:
        raise SystemExit(
            f"{target.name} SHA-256 mismatch: {actual} != {expected_sha256}"
        )
    target.write_bytes(payload)


def cargo_receipt(manifest: str, binary: str, source: Path, output: Path) -> None:
    subprocess.run(
        [
            "cargo",
            "run",
            "--quiet",
            "--manifest-path",
            manifest,
            "--bin",
            binary,
            "--",
            str(source),
            str(output),
        ],
        check=True,
    )


def load_json(path: Path):
    return json.loads(path.read_text(encoding="utf-8"))


def write_json(path: Path, payload) -> None:
    path.write_text(
        json.dumps(payload, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )


def validate_page_observation(out: Path) -> None:
    physical = load_json(out / "physical-page-role.json")
    actual = [page["contents_seq_num"] for page in physical["pages"]]
    expected_raw = [263, 266, 323, 352, 381, 269, 273, 277]
    if actual != expected_raw:
        raise SystemExit(f"raw PAGE order drifted: {actual!r}")

    customer_reference = [266, 323, 352, 381]
    publisher_reference_customer_oid_order = [
        [page["oid_dword0"], page["oid_dword1"]]
        for page in physical["pages"]
        if page["contents_seq_num"] in customer_reference
    ]
    decoded_scenario_page_lists = [
        field["pgids"]
        for controlling in physical.get("controlling", [])
        for field in controlling.get("fields", [])
        if field["id"] == 6
    ]
    if not decoded_scenario_page_lists:
        raise SystemExit("SampleNewsletter has no decoded OplControlling PageList")
    for index, pgids in enumerate(decoded_scenario_page_lists):
        if pgids != publisher_reference_customer_oid_order:
            raise SystemExit(
                f"OplControlling PageList[{index}] disagrees with Publisher customer Oid order: "
                f"{pgids!r} != {publisher_reference_customer_oid_order!r}"
            )

    comparison = {
        "schema": "chaptera.reader-page-role-reference.v1",
        "source_sha256": NEWSLETTER_SHA256,
        "raw_page_seq_nums": actual,
        "publisher_reference_customer_seq_nums": customer_reference,
        "publisher_reference_basis": (
            "recorded same-source side-by-side Publisher page crosswalk"
        ),
        "publisher_reference_customer_oid_order": (
            publisher_reference_customer_oid_order
        ),
        "decoded_oplcontrolling_page_lists": decoded_scenario_page_lists,
        "pgid_matches_publisher_reference_customer_subset": True,
        "classification_promoted": False,
        "pages": [
            {
                **page,
                "publisher_reference_customer": (
                    page["contents_seq_num"] in customer_reference
                ),
            }
            for page in physical["pages"]
        ],
    }
    write_json(out / "samplenewsletter-page-role-reference.json", comparison)

    newsletter = load_json(out / "viewer-page-projection-samplenewsletter.json")
    brochure = load_json(out / "viewer-page-projection-samplebrochure.json")

    if newsletter["viewer_page_count"] != 4:
        raise SystemExit(
            "SampleNewsletter exact Viewer profile must expose 4 Publisher pages, "
            f"got {newsletter['viewer_page_count']}"
        )
    if newsletter["scene_surface_count"] != 4:
        raise SystemExit(
            "SampleNewsletter exact scene must expose 4 Publisher surfaces, "
            f"got {newsletter['scene_surface_count']}"
        )
    if newsletter["viewer_page_indices"] != list(range(1, 5)):
        raise SystemExit(
            "SampleNewsletter Viewer page indices drifted: "
            f"{newsletter['viewer_page_indices']!r}"
        )
    newsletter_codes = set(newsletter["diagnostic_codes"])
    if "viewer.page_projection.family_profile_applied" not in newsletter_codes:
        raise SystemExit("SampleNewsletter exact Publisher-backed profile was not applied")
    if "viewer.page_projection.roles_unresolved" in newsletter_codes:
        raise SystemExit(
            "SampleNewsletter exact admitted profile must resolve product page presentation"
        )
    if "viewer.page_projection.scenario_order_observed" not in newsletter_codes:
        raise SystemExit(
            "SampleNewsletter should retain the independent Pgid scenario-order observation"
        )

    if brochure["viewer_page_count"] != 2:
        raise SystemExit(
            "SampleBrochure exact Viewer profile must expose 2 Publisher pages, "
            f"got {brochure['viewer_page_count']}"
        )
    if brochure["scene_surface_count"] != 2:
        raise SystemExit(
            "SampleBrochure exact scene must expose 2 Publisher surfaces, "
            f"got {brochure['scene_surface_count']}"
        )
    if brochure["viewer_page_indices"] != [1, 2]:
        raise SystemExit(
            f"SampleBrochure Viewer page indices drifted: {brochure['viewer_page_indices']!r}"
        )
    brochure_codes = set(brochure["diagnostic_codes"])
    if "viewer.page_projection.family_profile_applied" not in brochure_codes:
        raise SystemExit("SampleBrochure exact Publisher-backed profile was not applied")
    if "viewer.page_projection.roles_unresolved" in brochure_codes:
        raise SystemExit(
            "SampleBrochure exact admitted profile must resolve product page presentation"
        )
    if "viewer.page_projection.scenario_order_observed" in brochure_codes:
        raise SystemExit(
            "SampleBrochure has no field0x06 PageList authority and must not invent one"
        )

    write_json(
        out / "reader-page-role-observation.json",
        {
            "schema": "chaptera.reader-page-role-observation.v1",
            "sample_newsletter_sha256": NEWSLETTER_SHA256,
            "sample_brochure_sha256": BROCHURE_SHA256,
            "newsletter_raw_page_count": len(physical["pages"]),
            "newsletter_viewer_page_count": newsletter["viewer_page_count"],
            "brochure_viewer_page_count": brochure["viewer_page_count"],
            "publisher_backed_page_projection_verified": True,
            "source_pub_bytes_emitted": False,
        },
    )


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--out", required=True)
    args = parser.parse_args()

    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)

    with tempfile.TemporaryDirectory(prefix="chaptera-page-role-") as tmp:
        tmp_root = Path(tmp)
        newsletter = tmp_root / "SampleNewsletter.pub"
        brochure = tmp_root / "SampleBrochure.pub"
        acquire(NEWSLETTER_URL, NEWSLETTER_SHA256, newsletter)
        acquire(BROCHURE_URL, BROCHURE_SHA256, brochure)

        cargo_receipt(
            "vendor/producer-a/crates/pub-reader/Cargo.toml",
            "page-role-receipt",
            newsletter,
            out / "physical-page-role.json",
        )
        cargo_receipt(
            "vendor/producer-a/crates/pub-reader/Cargo.toml",
            "page-role-receipt",
            brochure,
            out / "physical-page-role-samplebrochure.json",
        )
        cargo_receipt(
            "vendor/producer-a/crates/pub-viewer/Cargo.toml",
            "page-projection-receipt",
            newsletter,
            out / "viewer-page-projection-samplenewsletter.json",
        )
        cargo_receipt(
            "vendor/producer-a/crates/pub-viewer/Cargo.toml",
            "page-projection-receipt",
            brochure,
            out / "viewer-page-projection-samplebrochure.json",
        )

    validate_page_observation(out)
    print((out / "reader-page-role-observation.json").read_text(encoding="utf-8"))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
