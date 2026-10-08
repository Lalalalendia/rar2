#!/usr/bin/env python3
from __future__ import annotations

import importlib.util
from pathlib import Path

HERE = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location(
    "artifact_family_provenance_join",
    HERE / "artifact_family_provenance_join.py",
)
MOD = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(MOD)


def test_exact_sha_and_url_enrichment() -> None:
    sha = "a" * 64
    materialized = [{"sha256": sha, "sources": ["lalamu_625"], "source_paths": ["/x/foo.pub"]}]
    lalamu = {
        sha: [{
            "sha256": sha,
            "candidate_id": "x",
            "source_filename": "Template1_horz_sized.pub",
            "source_type": "institutional-web",
            "source_url": "https://example.test/a%20b.pub",
        }]
    }
    legacy = {
        "https://example.test/a b.pub": [{
            "source": "University of West Georgia",
            "source_page": "https://example.test/templates",
            "category": "official Publisher graduation template",
            "candidate_filename": "Template1_horz_sized.pub",
            "direct_url": "https://example.test/a b.pub",
        }]
    }
    rows = MOD.build_join(materialized, lalamu, legacy)
    assert len(rows) == 1
    row = rows[0]
    assert row["provenance_join"] == "sha:lalamu+url:legacy-exact"
    assert row["source"] == "University of West Georgia"
    assert row["category"] == "official Publisher graduation template"


def test_filename_never_creates_join() -> None:
    sha = "b" * 64
    materialized = [{"sha256": sha, "sources": [], "source_paths": []}]
    lalamu = {
        sha: [{
            "sha256": sha,
            "source_filename": "same.pub",
            "source_type": "github",
            "source_repo": "owner/repo",
            "source_url": "https://example.test/actual.pub",
        }]
    }
    # Same filename, different URL: MUST NOT promote curated metadata.
    legacy = {
        "https://example.test/other.pub": [{
            "source": "Curated Source",
            "category": "newsletter",
            "candidate_filename": "same.pub",
            "direct_url": "https://example.test/other.pub",
        }]
    }
    row = MOD.build_join(materialized, lalamu, legacy)[0]
    assert row["provenance_join"] == "sha:lalamu"
    assert row["source"] == "owner/repo"
    assert row["category"] == ""


def test_missing_provenance_stays_explicit() -> None:
    sha = "c" * 64
    materialized = [{"sha256": sha, "sources": ["historical"], "source_paths": ["/hash.pub"]}]
    row = MOD.build_join(materialized, {}, {})[0]
    assert row["provenance_join"] == "materialized-sha-only"
    assert row["candidate_filename"] == ""
    summary = MOD.build_summary(materialized, [row])
    assert summary["sha_without_lalamu_provenance"] == 1


def test_ambiguous_exact_url_refuses_curated_promotion() -> None:
    sha = "d" * 64
    materialized = [{"sha256": sha}]
    lalamu = {
        sha: [{
            "sha256": sha,
            "source_filename": "x.pub",
            "source_type": "web",
            "source_url": "https://example.test/x.pub",
        }]
    }
    legacy = {
        "https://example.test/x.pub": [
            {"source": "A", "category": "newsletter"},
            {"source": "B", "category": "brochure"},
        ]
    }
    row = MOD.build_join(materialized, lalamu, legacy)[0]
    assert row["provenance_join"] == "sha:lalamu+url:legacy-ambiguous"
    assert row["category"] == ""


if __name__ == "__main__":
    test_exact_sha_and_url_enrichment()
    test_filename_never_creates_join()
    test_missing_provenance_stays_explicit()
    test_ambiguous_exact_url_refuses_curated_promotion()
    print("ok")
