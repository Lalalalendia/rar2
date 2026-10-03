#!/usr/bin/env python3
import argparse
import base64
import hashlib
import json
import pathlib
import re
import zipfile

CONTENTS_RE = re.compile(
    r"<Contents><!\\[CDATA\\[([A-Za-z0-9+/=\\s]+)\\]\\]></Contents>"
)

def load_json(path):
    return json.loads(pathlib.Path(path).read_text(encoding="utf-8"))

def sha256_bytes(data):
    return hashlib.sha256(data).hexdigest()

def report_item(report, feature, origin):
    matches = [
        item
        for item in report.get("items", [])
        if item.get("feature") == feature and item.get("origin") == origin
    ]
    assert len(matches) == 1, (feature, origin, len(matches))
    return matches[0]

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--reader-probe", required=True)
    parser.add_argument("--route-probe", required=True)
    parser.add_argument("--idml-report", required=True)
    parser.add_argument("--odg-report", required=True)
    parser.add_argument("--idml", required=True)
    parser.add_argument("--odg", required=True)
    parser.add_argument("--output", required=True)
    args = parser.parse_args()

    reader = load_json(args.reader_probe)
    route = load_json(args.route_probe)
    idml_report = load_json(args.idml_report)
    odg_report = load_json(args.odg_report)

    assert reader["schema"] == "chaptera.editable-source-image-reader-probe.v1"
    assert route["schema"] == "chaptera.migration-editable-route-probe.v1"
    assert reader["source_sha256"] == route["source_sha256"]
    assert reader["resource_count"] > 0, "pinned fixture exposes no exact PNG/JPEG resources"
    assert reader["use_count"] > 0, "pinned fixture exposes no exact PNG/JPEG placements"
    assert route["source_image_count"] == reader["use_count"]
    assert route["targets"]["idml"]["state"] == "available_with_declared_losses"
    assert route["targets"]["odg"]["state"] == "available_with_declared_losses"
    assert route["targets"]["idml"]["materialized"] is True
    assert route["targets"]["odg"]["materialized"] is True
    assert idml_report["can_serialize"] is True
    assert odg_report["can_serialize"] is True

    expected_hashes = set()
    expected_odg_paths = {}
    expected_nodes = set()

    for resource in reader["resources"]:
        resource_id = resource["resource_id"]
        mime = resource["mime"]
        expected_hashes.add(resource["sha256"])

        for report in (idml_report, odg_report):
            item = report_item(report, "image.bytes", resource_id)
            assert item["disposition"] == "preserved", item

        extension = {"image/png": "png", "image/jpeg": "jpg"}[mime]
        expected_odg_paths[f"Pictures/{resource_id}.{extension}"] = resource["sha256"]

        for use in resource["uses"]:
            node_id = use["node_id"]
            expected_nodes.add(node_id)
            for report in (idml_report, odg_report):
                geometry = report_item(report, "image.frame_geometry", node_id)
                transform = report_item(report, "image.content_transform", node_id)
                assert geometry["disposition"] == "preserved", geometry
                assert transform["disposition"] == "approximated", transform
                unsupported = [
                    item
                    for item in report.get("items", [])
                    if item.get("feature") == "node.unsupported"
                    and item.get("origin") == node_id
                ]
                assert not unsupported, (node_id, unsupported)

    idml_payload_hashes = set()
    with zipfile.ZipFile(args.idml, "r") as archive:
        spread_names = [
            name for name in archive.namelist()
            if name.startswith("Spreads/") and name.endswith(".xml")
        ]
        assert spread_names, "IDML has no spread XML"
        for name in spread_names:
            text = archive.read(name).decode("utf-8")
            for encoded in CONTENTS_RE.findall(text):
                raw = base64.b64decode("".join(encoded.split()), validate=True)
                idml_payload_hashes.add(sha256_bytes(raw))

    missing_idml = sorted(expected_hashes - idml_payload_hashes)
    assert not missing_idml, {"missing_idml_payload_sha256": missing_idml}

    with zipfile.ZipFile(args.odg, "r") as archive:
        names = set(archive.namelist())
        for path, expected_sha in expected_odg_paths.items():
            assert path in names, path
            actual_sha = sha256_bytes(archive.read(path))
            assert actual_sha == expected_sha, (path, expected_sha, actual_sha)

    receipt = {
        "schema": "chaptera.editable-source-image-export-acceptance.v1",
        "source_sha256": reader["source_sha256"],
        "source_image_resource_count": reader["resource_count"],
        "source_image_placement_count": reader["use_count"],
        "source_image_nodes": sorted(expected_nodes),
        "idml": {
            "package_sha256": sha256_bytes(pathlib.Path(args.idml).read_bytes()),
            "exact_source_payloads_observed": len(expected_hashes),
            "report_blocking": idml_report["counts"]["blocking"],
        },
        "odg": {
            "package_sha256": sha256_bytes(pathlib.Path(args.odg).read_bytes()),
            "exact_source_payloads_observed": len(expected_odg_paths),
            "report_blocking": odg_report["counts"]["blocking"],
        },
        "invariants": {
            "source_images_present_without_replace_image": True,
            "exact_payload_bytes_verified": True,
            "frame_geometry_preserved": True,
            "content_transform_loss_explicit": True,
            "source_image_nodes_not_generic_unsupported": True,
        },
    }
    pathlib.Path(args.output).write_text(
        json.dumps(receipt, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    print(json.dumps(receipt, indent=2, sort_keys=True))

if __name__ == "__main__":
    main()
