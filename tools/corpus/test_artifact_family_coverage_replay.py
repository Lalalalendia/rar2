#!/usr/bin/env python3
from artifact_family_coverage_replay import build_report


def main():
    manifest = [
        {
            "sha256": "a",
            "source": "Tennessee State University",
            "category": "university approved Publisher template",
            "candidate_filename": "pubtemplatenewsletter1.pub",
        },
        {
            "sha256": "a",
            "source": "Tennessee State University",
            "category": "university approved Publisher template",
            "candidate_filename": "pubtemplatenewsletter1.pub",
        },
        {
            "sha256": "b",
            "source": "St John the Baptist RC Primary School",
            "category": "school topic webs",
            "candidate_filename": "opaque.pub",
        },
        {
            "sha256": "c",
            "source": "Unknown",
            "category": "",
            "candidate_filename": "opaque.pub",
        },
    ]
    inventory = {
        "files": [
            {"sha256": "a", "relative_path": "a.pub", "capabilities": {"story_count": 1}},
            {"sha256": "a", "relative_path": "duplicate-a.pub", "capabilities": {"story_count": 1}},
            {"sha256": "b", "relative_path": "b.pub", "capabilities": {"story_count": 1}},
            {"sha256": "c", "relative_path": "c.pub", "capabilities": {"story_count": 1}},
            {"sha256": "d", "relative_path": "unsupported.pub", "capabilities": None},
        ]
    }

    report = build_report(manifest, inventory)
    assert report["denominator_capability_files"] == 3
    assert report["labeled_capability_files"] == 2
    assert report["unlabeled_capability_files"] == 1
    assert report["family_file_counts"] == {"newsletter": 1, "topic-web": 1}
    assert report["unlabeled_identities"] == [
        {"sha256": "c", "relative_path": "c.pub"}
    ]
    print("artifact family coverage replay tests: ok")


if __name__ == "__main__":
    main()
