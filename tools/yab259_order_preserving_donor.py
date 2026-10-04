#!/usr/bin/env python3
"""Prepare the exact repaired Yab #259 donor while preserving product scene order."""

from __future__ import annotations

import hashlib
import pathlib

from run_yab259_fixed_pdf_closure import (
    EXPECTED_REPAIR_FILES,
    bind_yab_repository,
    changed_files,
    prepare_repaired_donor,
    replace_once,
    run_checked,
)

ORDERED_PDF_FILE = "crates/pub-pdf/src/lib.rs"
EXPECTED_ORDERED_REPAIR_FILES = EXPECTED_REPAIR_FILES | {ORDERED_PDF_FILE}


def prepare_order_preserving_pdf_donor(
    repository: pathlib.Path,
    checkout: pathlib.Path,
) -> tuple[str, str]:
    """Apply the proven #259 repair plus the bounded product-order preservation patch."""

    base_repair_sha256 = prepare_repaired_donor(repository, checkout)
    pdf = checkout / ORDERED_PDF_FILE

    replace_once(
        pdf,
        """    let mut surfaces = scene.surfaces.clone();
    surfaces.sort_by_key(|surface| surface.origin);
""",
        """    let surfaces = scene.surfaces.clone();
""",
        label="Yab #259 product page-order preservation",
    )
    replace_once(
        pdf,
        """    let mut nodes = scene.nodes.clone();
    nodes.sort_by_key(|node| node.origin);
""",
        """    let nodes = scene.nodes.clone();
""",
        label="Yab #259 product node-order preservation",
    )

    replace_once(
        pdf,
        """    #[test]
    fn deterministic_pdf_has_sorted_pages_exact_media_boxes_and_origin_report() {
""",
        """    #[test]
    fn deterministic_pdf_preserves_input_pages_exact_media_boxes_and_origin_report() {
""",
        label="Yab #259 order-preserving page test name",
    )
    replace_once(
        pdf,
        """        assert_eq!(left.report.pages.len(), 2);
        assert_eq!(left.report.pages[0].origin, page_id(1));
        assert_eq!(
            left.report.pages[0].media_box_points,
            ["0", "0", "600", "780"]
        );
        assert_eq!(left.report.pages[1].origin, page_id(2));
        assert_eq!(
            left.report.pages[1].media_box_points,
            ["0", "0", "595.275590551", "841.88976378"]
        );
""",
        """        assert_eq!(left.report.pages.len(), 2);
        assert_eq!(left.report.pages[0].origin, page_id(2));
        assert_eq!(
            left.report.pages[0].media_box_points,
            ["0", "0", "595.275590551", "841.88976378"]
        );
        assert_eq!(left.report.pages[1].origin, page_id(1));
        assert_eq!(
            left.report.pages[1].media_box_points,
            ["0", "0", "600", "780"]
        );
""",
        label="Yab #259 order-preserving page test expectations",
    )

    run_checked(
        ["git", "diff", "--check"],
        cwd=checkout,
        label="order-preserving Yab #259 donor has whitespace errors",
    )
    repairs = changed_files(checkout)
    if repairs != EXPECTED_ORDERED_REPAIR_FILES:
        raise RuntimeError(
            "unexpected order-preserving Yab #259 repair file set: "
            + ",".join(sorted(repairs))
        )

    text = pdf.read_text(encoding="utf-8")
    if "surfaces.sort_by_key(|surface| surface.origin)" in text:
        raise RuntimeError("Yab #259 page-order sort survived bounded repair")
    if "nodes.sort_by_key(|node| node.origin)" in text:
        raise RuntimeError("Yab #259 node-order sort survived bounded repair")
    if "reports.sort_by_key(|node| node.origin)" not in text:
        raise RuntimeError("Yab #259 deterministic report ordering was accidentally removed")

    patch = run_checked(
        ["git", "diff", "--binary"],
        cwd=checkout,
        label="cannot fingerprint order-preserving Yab #259 repair bundle",
    )
    order_repair_sha256 = hashlib.sha256(patch.encode("utf-8")).hexdigest()
    return base_repair_sha256, order_repair_sha256


__all__ = [
    "bind_yab_repository",
    "prepare_order_preserving_pdf_donor",
]
