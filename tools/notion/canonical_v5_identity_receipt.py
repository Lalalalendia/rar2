#!/usr/bin/env python3
"""Read a real Notion V5 claim identity; never create a V5 snapshot or merge.

All live requests are GET. Requires an existing NOTION_TOKEN and a private
output path outside the repository. Does not print tokens, claim text or identity.
"""
from __future__ import annotations

import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import sys
import urllib.error
import urllib.parse
import urllib.request
from uuid import UUID

API_ROOT = "https://api.notion.com/v1"
API_VERSION = "2026-03-11"
CANONICAL_CLAIMS_SOURCE_ID = "d1155b4e-ad39-4e97-98fe-cea3b0fa9774"
SEMANTIC_IDENTITY_PROPERTY_ID = "Q2FITg"
IDENTITY_PROPERTY_NAME = "Semantic Identity v1"
SEMANTIC_FIELDS = ("Semantic Revision", "Status", "Current Wording", "Scope / Limits", "Do Not Infer")
SCHEMA = "pub.canonical-v5-identity-observation.v1"


class ObservationError(Exception):
    """Unsafe, absent or inconsistent remote value. Do not make a snapshot."""


class _RejectRedirect(urllib.request.HTTPRedirectHandler):
    """Do not forward a private Notion token to an unexpected host."""

    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None


SAFE_OPENER = urllib.request.build_opener(_RejectRedirect())


def normalize_uuid(value: str) -> str:
    try:
        return str(UUID(value))
    except (ValueError, TypeError, AttributeError) as exc:
        raise ObservationError("Invalid UUID; no request sent") from exc


def notion_get_json(path: str, token: str, *, opener=SAFE_OPENER.open) -> dict:
    if not token.strip():
        raise ObservationError("NOTION_TOKEN not available")
    req = urllib.request.Request(
        API_ROOT + path,
        headers={
            "Authorization": "Bearer " + token,
            "Notion-Version": API_VERSION,
            "Accept": "application/json",
            "User-Agent": "pub-v5-identity-readonly/1.0",
        },
        method="GET",
    )
    try:
        with opener(req, timeout=20) as resp:
            obj = json.load(resp)
    except urllib.error.HTTPError as exc:
        raise ObservationError(f"Notion API HTTP {exc.code}; no receipt created") from None
    except (urllib.error.URLError, TimeoutError, OSError, ValueError) as exc:
        raise ObservationError("Notion API transport/JSON failure; no receipt created") from None
    if not isinstance(obj, dict):
        raise ObservationError("Unexpected Notion response shape")
    return obj


def read_field(properties: dict, field: str):
    prop = properties.get(field)
    if not isinstance(prop, dict):
        raise ObservationError(f"Missing {field} in claim")
    typ = prop.get("type")
    if field == "Semantic Revision":
        v = prop.get("number")
        if typ != "number" or type(v) not in (int, float) or v != int(v) or v < 1:
            raise ObservationError("Invalid Semantic Revision")
        return int(v)
    if field == "Status":
        sub = prop.get("select")
        name = sub.get("name") if isinstance(sub, dict) else None
        if typ != "select" or not isinstance(name, str) or not name:
            raise ObservationError("Invalid claim Status")
        return name
    if typ != "rich_text" or not isinstance(prop.get("rich_text"), list):
        raise ObservationError(f"Invalid claim text property: {field}")
    if prop.get("has_more") is True:
        raise ObservationError(f"Truncated claim property: {field}")
    parts = prop["rich_text"]
    if not parts or any(not isinstance(p, dict) or not isinstance(p.get("plain_text"), str) for p in parts):
        raise ObservationError(f"Missing or unparseable claim body: {field}")
    value = "".join(p["plain_text"] for p in parts)
    if not value.strip():
        raise ObservationError(f"Empty claim body: {field}")
    return value


def claim_number_from_page(properties: dict) -> int:
    prop = properties.get("Claim ID")
    if not isinstance(prop, dict):
        raise ObservationError("Claim ID property missing")
    typ = prop.get("type")
    if typ == "unique_id" and isinstance(prop.get("unique_id"), dict):
        unique = prop["unique_id"]
        prefix = unique.get("prefix")
        if prefix not in (None, "", "PUB-C-"):
            raise ObservationError("Unexpected Claim ID prefix")
        value = unique.get("number")
    elif typ == "number":
        value = prop.get("number")
    else:
        raise ObservationError("Unexpected Claim ID type")
    if type(value) is not int or value < 1:
        raise ObservationError("Invalid Claim ID number")
    return value


def read_page(page: dict, *, page_id: str, expected_claim_id: str, revision: int, status: str) -> dict:
    if page.get("object") != "page" or normalize_uuid(page.get("id")) != page_id:
        raise ObservationError("Incorrect Notion claim page")
    if page.get("archived") is True or page.get("in_trash") is True:
        raise ObservationError("Claim is archived or trashed")
    parent = page.get("parent", {})
    if not isinstance(parent, dict) or parent.get("type") != "data_source_id" or normalize_uuid(parent.get("data_source_id")) != CANONICAL_CLAIMS_SOURCE_ID:
        raise ObservationError("Claim is not in the pinned Canonical Claims source")
    properties = page.get("properties")
    if not isinstance(properties, dict):
        raise ObservationError("Missing claim properties")
    identity = properties.get(IDENTITY_PROPERTY_NAME)
    if not isinstance(identity, dict) or identity.get("id") != SEMANTIC_IDENTITY_PROPERTY_ID or identity.get("type") != "formula":
        raise ObservationError("Semantic Identity property schema changed")
    values = {field: read_field(properties, field) for field in SEMANTIC_FIELDS}
    observed_claim_number = claim_number_from_page(properties)
    if expected_claim_id != f"PUB-C-{observed_claim_number}":
        raise ObservationError("Claim ID does not match requested claim")
    if values["Semantic Revision"] != revision or values["Status"] != status:
        raise ObservationError("Claim revision/status moved; no receipt created")
    edited = page.get("last_edited_time")
    if not isinstance(edited, str) or not edited:
        raise ObservationError("Claim has no last_edited_time")
    return {"semantic_fields": values, "last_edited_time": edited}


def read_identity(obj: dict) -> str:
    if obj.get("object") != "property_item" or obj.get("id") != SEMANTIC_IDENTITY_PROPERTY_ID or obj.get("type") != "formula":
        raise ObservationError("Expected single formula property_item")
    formula = obj.get("formula")
    if not isinstance(formula, dict) or formula.get("type") != "string":
        raise ObservationError("Formula is not a computed string (could be unsupported)")
    value = formula.get("string")
    if not isinstance(value, str) or not value or value.startswith("formulaResult://"):
        raise ObservationError("Formula value missing or unresolved")
    return value


def observe(fetch_json, *, page_id: str, expected_claim_id: str, revision: int, status: str) -> dict:
    """Four live GET reads, fail-closed on any change; no Notion mutation."""
    page_id = normalize_uuid(page_id)
    if not expected_claim_id.startswith("PUB-C-"):
        raise ObservationError("Expected claim ID must start with PUB-C-")
    path = f"/pages/{page_id}"
    identity_path = f"{path}/properties/{urllib.parse.quote(SEMANTIC_IDENTITY_PROPERTY_ID, safe='')}"
    before = read_page(fetch_json(path), page_id=page_id, expected_claim_id=expected_claim_id, revision=revision, status=status)
    identity_first = read_identity(fetch_json(identity_path))
    after = read_page(fetch_json(path), page_id=page_id, expected_claim_id=expected_claim_id, revision=revision, status=status)
    identity_last = read_identity(fetch_json(identity_path))
    if before != after or identity_first != identity_last:
        raise ObservationError("Notion claim changed during reads; no receipt created")
    return {
        "schema": SCHEMA,
        "kind": "READ_ONLY_OBSERVATION_NOT_VERIFICATION_OR_ADMISSION",
        "claim_page_id": page_id,
        "claim_id": expected_claim_id,
        "notion_api_version": API_VERSION,
        "formula_property_id": SEMANTIC_IDENTITY_PROPERTY_ID,
        "observed_semantic_identity_v1": identity_first,
        "semantic_identity_sha256": hashlib.sha256(identity_first.encode("utf-8")).hexdigest(),
        "semantic_revision": revision,
        "status": status,
        "last_edited_time": before["last_edited_time"],
        "semantic_fields": before["semantic_fields"],
        "observed_at_utc": datetime.now(timezone.utc).isoformat(),
        "requests": ["GET page", "GET property", "GET page", "GET property"],
        "raw_pub_verified": False,
        "independent_verifier_created": False,
        "v5_admission_ready": False,
    }


def save_private_receipt(receipt: dict, output_path: Path, *, repo_root: Path) -> None:
    output_path = output_path.expanduser().resolve()
    root = repo_root.resolve()
    if output_path == root or root in output_path.parents:
        raise ObservationError("Private receipt must be outside the Git checkout")
    if not output_path.parent.is_dir():
        raise ObservationError("Private receipt parent folder does not exist")
    data = (json.dumps(receipt, sort_keys=True, ensure_ascii=False, indent=2) + "\n").encode("utf-8")
    try:
        fd = os.open(output_path, os.O_CREAT | os.O_EXCL | os.O_WRONLY, 0o600)
    except FileExistsError:
        raise ObservationError("Receipt already exists; no overwrite") from None
    except OSError:
        raise ObservationError("Could not create private receipt") from None
    try:
        with os.fdopen(fd, "wb") as file:
            file.write(data)
    except BaseException:
        output_path.unlink(missing_ok=True)
        raise


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--claim-page-id", required=True, help="Notion Canonical Claim page UUID")
    parser.add_argument("--expect-claim-id", required=True, help="PUB-C-NNN")
    parser.add_argument("--expect-revision", type=int, required=True)
    parser.add_argument("--expect-status", default="OPEN")
    parser.add_argument("--output", type=Path, required=True, help="private JSON file outside Git checkout")
    args = parser.parse_args(argv)
    try:
        token = os.environ.get("NOTION_TOKEN", "")
        if not token.strip():
            raise ObservationError("NOTION_TOKEN not available")
        receipt = observe(
            lambda path: notion_get_json(path, token),
            page_id=args.claim_page_id,
            expected_claim_id=args.expect_claim_id,
            revision=args.expect_revision,
            status=args.expect_status,
        )
        save_private_receipt(receipt, args.output, repo_root=Path(__file__).resolve().parents[2])
    except ObservationError as exc:
        print(f"FAIL CLOSED: {exc}", file=sys.stderr)
        return 2
    print(f"READ-ONLY receipt saved privately: {receipt['claim_id']}@{receipt['semantic_revision']} "
          f"(identity SHA256={receipt['semantic_identity_sha256']}); no V5 admission")
    return 0


if __name__ == "__main__":
    sys.exit(main())
