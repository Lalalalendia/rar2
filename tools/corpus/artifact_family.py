#!/usr/bin/env python3
"""Conservative artifact/template-family labels for the public PUB corpus.

Labels are deliberately provenance/title driven. They are research strata, not
claims about Publisher-market share or hidden document semantics.
"""
from __future__ import annotations

import re
from typing import Iterable


def _norm(value: object) -> str:
    text = str(value or "").casefold()
    text = re.sub(r"[_\-]+", " ", text)
    return re.sub(r"\s+", " ", text).strip()


# Exact mappings from already human-curated acquisition categories. These are
# stronger than filename guesses: the category itself names the publication
# workflow represented by the source row.
_CATEGORY_ARTIFACT_FAMILIES: dict[str, tuple[str, ...]] = {
    "school topic webs": ("topic-web",),
    "school project web": ("topic-web",),
    "school phonics cards": ("phonics-card",),
    "school art learning journeys": ("learning-journey",),
    "school home learning resource": ("home-learning-resource",),
    "school homework": ("home-learning-resource",),
    "school curriculum statements": ("curriculum-document",),
    "school curriculum plan": ("curriculum-document",),
    "school curriculum overview": ("curriculum-document",),
    "school health leaflet": ("leaflet",),
    "school parent leaflet": ("leaflet",),
    "school safeguarding guide": ("parent-information",),
    "school assessment guide": ("parent-information",),
    "school parent overview": ("parent-information",),
    "school parent handout": ("parent-information",),
    "school prayer/resource": ("prayer-resource",),
    "official publisher print template": ("print-template",),
}


# Broad artifact labels from explicit words in a curated category, visible file
# name, or parent archive name. A row may legitimately receive more than one.
_ARTIFACT_RULES: tuple[tuple[tuple[str, ...], str], ...] = (
    (("newsletter",), "newsletter"),
    (("brochure",), "brochure"),
    (("business card", "visiting card"), "business-card"),
    (("invitation", "invite card"), "invitation-card"),
    (("postcard",), "postcard"),
    (("flyer",), "flyer"),
    (("booklet",), "booklet"),
    (("knowledge organiser", "knowledge organizer"), "knowledge-organiser"),
    (("worksheet",), "worksheet"),
    (("curriculum leaflet", "topic leaflet"), "leaflet"),
    (("label",), "label"),
    (("placard",), "poster-sign"),
    (("envelope",), "envelope"),
    (("graduation", "announcement"), "announcement"),
    (("calendar",), "calendar"),
)


def _artifact_family_pairs(row: dict) -> tuple[tuple[str, str], ...]:
    pairs: set[tuple[str, str]] = set()

    category = _norm(row.get("category"))
    for family in _CATEGORY_ARTIFACT_FAMILIES.get(category, ()):
        pairs.add((family, f"curated-category:{category}"))

    evidence_fields = (
        ("category", row.get("category")),
        ("filename", row.get("candidate_filename")),
        ("parent-archive", row.get("parent_archive_filename")),
    )
    for field_name, raw_value in evidence_fields:
        evidence = _norm(raw_value)
        if not evidence:
            continue
        for needles, family in _ARTIFACT_RULES:
            matching = next((needle for needle in needles if needle in evidence), None)
            if matching is not None:
                pairs.add((family, f"{field_name}-keyword:{matching}"))

    source = _norm(row.get("source"))
    filename = _norm(row.get("candidate_filename"))
    parent_archive = _norm(row.get("parent_archive_filename"))
    source_page = _norm(row.get("source_page"))
    combined_name = f"{filename} {parent_archive}".strip()

    # Explicit template names: keep the label no more specific than the visible
    # artifact form unless independent provenance proves a richer workflow.
    if "trifold" in filename.replace(" ", "") or "tri fold" in filename:
        pairs.add(("brochure", "filename-keyword:trifold"))
    if "onepage" in filename.replace(" ", "") or "one page" in filename:
        pairs.add(("one-page-collateral", "filename-keyword:onepage"))

    # Effective Church exposes some ZIPs from an explicit invitation-card
    # template page. Other archive children are labeled only when the visible
    # member/container name itself says card or bulletin insert. The five
    # remaining Easter assets intentionally stay unresolved.
    if source == "effective church communications":
        if "easter templates" in source_page:
            if "still have questions" in combined_name:
                pairs.add(
                    (
                        "handout",
                        "source-page-section:still-have-questions-bulletin-insert-or-flyer",
                    )
                )
            if "church explanations for easter sunday" in combined_name:
                pairs.add(
                    (
                        "handout",
                        "source-page-section:bookmarks-and-bulletin-inserts",
                    )
                )
            if "jelly bean prayer" in combined_name:
                pairs.add(
                    (
                        "children-ministry-resource",
                        "source-page-section:easter-jelly-bean-prayer-for-childrens-ministry",
                    )
                )
        if "business invitation card templates" in source_page:
            pairs.add(
                (
                    "invitation-card",
                    "source-page:business-invitation-card-templates",
                )
            )
        if "bulletin insert" in combined_name:
            pairs.add(("bulletin-insert", "filename-or-archive:bulletin-insert"))
        if re.search(r"\bcards?\b", combined_name):
            pairs.add(("card", "filename-or-archive:card"))

    return tuple(sorted(pairs))


def artifact_families(row: dict) -> tuple[str, ...]:
    """Return conservative non-exclusive artifact-family labels."""
    return tuple(sorted({family for family, _ in _artifact_family_pairs(row)}))


def artifact_family_evidence(row: dict) -> tuple[str, ...]:
    """Return audit evidence for each emitted broad artifact-family label."""
    return tuple(
        f"{family}|{evidence}" for family, evidence in _artifact_family_pairs(row)
    )


def template_family(row: dict) -> tuple[str, str] | None:
    """Return one explicit template-family label plus evidence, or None.

    These rules intentionally cover only sibling families whose provenance and
    filenames make the relationship explicit. Do not generalize numeric suffix
    stripping to arbitrary PUB names.
    """
    source = _norm(row.get("source"))
    category = _norm(row.get("category"))
    filename = _norm(row.get("candidate_filename"))

    if source == "tennessee state university":
        if re.fullmatch(r"pubtemplatebrochure[123]\.pub", filename.replace(" ", "")):
            return (
                "tennessee-state-university:publisher-brochure-template",
                "exact-source+explicit-brochure-sibling-filenames",
            )
        if re.fullmatch(r"pubtemplatenewsletter[123]\.pub", filename.replace(" ", "")):
            return (
                "tennessee-state-university:publisher-newsletter-template",
                "exact-source+explicit-newsletter-sibling-filenames",
            )

    if (
        source == "ivy road primary school"
        and category == "school kirf maths family"
        and filename.endswith(".pub")
    ):
        return (
            "ivy-road-primary-school:kirf-maths-family",
            "exact-source+curated-category-explicitly-names-family",
        )

    if source == "university of west georgia" and filename in {
        "template1 horz sized.pub",
        "template2 vertical.pub",
    }:
        return (
            "university-of-west-georgia:graduation-announcement-template",
            "exact-source+same-graduation-template-page+orientation-siblings",
        )

    return None


def annotate_row(row: dict) -> dict:
    """Return a copy with audit-friendly family labels."""
    output = dict(row)
    output["artifact_families"] = ";".join(artifact_families(row))
    output["artifact_family_evidence"] = ";".join(artifact_family_evidence(row))

    template = template_family(row)
    if template is None:
        output["template_family"] = ""
        output["template_family_evidence"] = ""
    else:
        output["template_family"], output["template_family_evidence"] = template

    return output


def annotate_rows(rows: Iterable[dict]) -> list[dict]:
    return [annotate_row(row) for row in rows]
