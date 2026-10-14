#!/usr/bin/env python3
from artifact_family import (
    annotate_row,
    artifact_families,
    artifact_family_evidence,
    template_family,
)


def row(
    source,
    category,
    filename,
    parent_archive_filename="",
    source_page="",
):
    return {
        "source": source,
        "category": category,
        "candidate_filename": filename,
        "parent_archive_filename": parent_archive_filename,
        "source_page": source_page,
    }


def main():
    brochure = row(
        "Tennessee State University",
        "university approved Publisher template",
        "pubtemplatebrochure2.pub",
    )
    newsletter = row(
        "Tennessee State University",
        "university approved Publisher template",
        "pubtemplatenewsletter3.pub",
    )
    assert template_family(brochure)[0] == (
        "tennessee-state-university:publisher-brochure-template"
    )
    assert template_family(newsletter)[0] == (
        "tennessee-state-university:publisher-newsletter-template"
    )
    assert template_family(brochure)[0] != template_family(newsletter)[0]
    assert artifact_families(brochure) == ("brochure",)
    assert artifact_families(newsletter) == ("newsletter",)

    unrelated_numeric = row(
        "Tennessee State University",
        "university approved Publisher template",
        "random-template-2.pub",
    )
    assert template_family(unrelated_numeric) is None

    ivy = row(
        "Ivy Road Primary School",
        "school KIRF maths family",
        "Year 6 - Summer 1.pub",
    )
    family, evidence = template_family(ivy)
    assert family == "ivy-road-primary-school:kirf-maths-family"
    assert "curated-category" in evidence

    westga_a = row(
        "University of West Georgia",
        "official Publisher graduation template",
        "Template1_horz_sized.pub",
    )
    westga_b = row(
        "University of West Georgia",
        "official Publisher graduation template",
        "Template2_vertical.pub",
    )
    assert template_family(westga_a)[0] == template_family(westga_b)[0]
    assert "announcement" in artifact_families(westga_a)

    archive_child = row(
        "Effective Church Communications",
        "public Publisher ZIP template archive",
        "Design-A.pub",
        "Senior-Newsletter-Revised-Design.zip",
    )
    assert "newsletter" in artifact_families(archive_child)
    assert template_family(archive_child) is None

    # Exact curated categories may name a workflow even when the filename does
    # not carry useful semantics.
    assert artifact_families(
        row(
            "St John the Baptist RC Primary School",
            "school topic webs",
            "A1 TOPIC WEB 25-6..pub",
        )
    ) == ("topic-web",)
    assert artifact_families(
        row(
            "Christ Church C of E VC Infant School",
            "school phonics cards",
            "S.pub",
        )
    ) == ("phonics-card",)
    assert artifact_families(
        row(
            "Wimborne First School & Nursery",
            "school art learning journeys",
            "Spring 1 Painting - Learning Journey.pub",
        )
    ) == ("learning-journey",)
    assert artifact_families(
        row(
            "Hampton Dene Primary School",
            "school curriculum statements",
            "Impact statement - Music final.pub",
        )
    ) == ("curriculum-document",)

    assert artifact_families(
        row(
            "Ribbleton Avenue Methodist Junior School",
            "school home-learning resource",
            "Dragons activities.pub",
        )
    ) == ("home-learning-resource",)
    assert artifact_families(
        row(
            "Quadring Cowley & Brown's Primary School",
            "school safeguarding guide",
            "Safeguarding Parent Pocket Guide.pub",
        )
    ) == ("parent-information",)

    # Visible template-form names are bounded artifact evidence, not template
    # family evidence.
    assert "brochure" in artifact_families(
        row(
            "Adelphi University",
            "university approved Publisher template",
            "Adelphi-TriFold.pub",
        )
    )
    assert "one-page-collateral" in artifact_families(
        row(
            "Adelphi University",
            "university approved Publisher template",
            "Adelphi-OnePage.pub",
        )
    )

    # Effective Church context is used only where the source page or visible
    # archive/member name explicitly identifies the artifact form.
    church_invitation = row(
        "Effective Church Communications",
        "public Publisher ZIP template archive",
        "Women's need cards.pub",
        "Womens-need-cards.zip",
        "https://www.effectivechurchcom.com/templates/business-invitation-card-templates/",
    )
    assert set(artifact_families(church_invitation)) == {"card", "invitation-card"}

    bulletin = row(
        "Effective Church Communications",
        "public Publisher ZIP template archive",
        "MS Publ file Hark the Herald Angels Sing Bulletin Insert.pub",
        "Hark-Bulletin-Inserts.zip",
        "https://www.effectivechurchcom.com/bulletin-insert-size-created-for-hark-the-herald-angels-sing/",
    )
    assert artifact_families(bulletin) == ("bulletin-insert",)

    still_questions = row(
        "Effective Church Communications",
        "public Publisher ZIP template archive",
        "MS Pub Still have questions newer.pub",
        "Still-have-questions.zip",
        "https://www.effectivechurchcom.com/templates/easter-templates/",
    )
    assert artifact_families(still_questions) == ("handout",)
    assert any(
        "still-have-questions-bulletin-insert-or-flyer" in evidence
        for evidence in artifact_family_evidence(still_questions)
    )

    explanations = row(
        "Effective Church Communications",
        "public Publisher ZIP template archive",
        "MS Pub Church Explanations for Easter Sunday.pub",
        "Church-Explanations-for-Easter-Sunday.zip",
        "https://www.effectivechurchcom.com/templates/easter-templates/",
    )
    assert artifact_families(explanations) == ("handout",)
    assert any(
        "bookmarks-and-bulletin-inserts" in evidence
        for evidence in artifact_family_evidence(explanations)
    )

    jelly_bean = row(
        "Effective Church Communications",
        "public Publisher ZIP template archive",
        "MS Pub Jelly Bean Prayer.pub",
        "Jelly-Bean-Prayer.zip",
        "https://www.effectivechurchcom.com/templates/easter-templates/",
    )
    assert artifact_families(jelly_bean) == ("children-ministry-resource",)
    assert any(
        "easter-jelly-bean-prayer-for-childrens-ministry" in evidence
        for evidence in artifact_family_evidence(jelly_bean)
    )

    annotated = annotate_row(newsletter)
    assert annotated["artifact_families"] == "newsletter"
    assert "newsletter|" in annotated["artifact_family_evidence"]
    assert annotated["template_family"].endswith("publisher-newsletter-template")
    assert annotated["template_family_evidence"]

    print("artifact family tests: ok")


if __name__ == "__main__":
    main()
