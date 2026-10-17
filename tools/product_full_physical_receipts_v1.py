#!/usr/bin/env python3
"""Admit only exact physical Product browser receipts into the existing 49/111 scorer.

This is a source-neutral bridge from the physical OIDC/HTTPS pilot to the
canonical scorer, NOT an alternative renderer, scorer, or source authority.
No importer failure is ever silently converted into a skipped document.
"""
from __future__ import annotations

import argparse
import json
import re
from pathlib import Path

from product_full_cohort_shards_v1 import build as load_roster
from reader_product_49_111_scoreboard_v1 import (
    CAPTURE_ROUTE, METHOD, PHYSICAL_SCHEMA, REQUIRED_CHECKS,
    score, DOCUMENT_KEYS, PAGE_KEYS,
)
from reader_publisher_lo_49_111_scoreboard_v1 import (
    load_frozen, read_publisher_registry,
)

SCHEMA_SHARD = 'chaptera.product-49-111-physical-shard.v1'
HEX40 = re.compile(r'^[a-f0-9]{40}$')
HEX64 = re.compile(r'^[a-f0-9]{64}$')
ROOT = Path(__file__).resolve().parent
FROZEN = ROOT / 'corpus/receipts/publisher-lo-t999-49-111-frozen-v1.json'
FINGERPRINT = ROOT / 'corpus/receipts/publisher-visual-golden-batch-01-fingerprint-v1.json'
PAIRS = ROOT / 'corpus/receipts/publisher-visual-golden-batch-01-pairs.csv'


def check(predicate: bool, message: str) -> None:
    if not predicate:
        raise ValueError(message)


def sha(value: object) -> bool:
    return isinstance(value, str) and HEX64.fullmatch(value) is not None


def load(path: Path, ceiling: int = 2_000_000) -> dict:
    check(path.is_file() and 0 < path.stat().st_size <= ceiling,
          'missing or oversized source-neutral physical evidence')
    result = json.loads(path.read_text(encoding='utf-8'))
    check(type(result) is dict, 'physical evidence must be a JSON object')
    return result


def case_receipt(case: dict, head: str, owner: dict, summary: dict,
                 comparison: dict, privacy: dict) -> dict:
    """Derive each authorization/privacy claim only from an observed receipt."""
    ident, fixture, src, pdf, pages = (
        case['id'], case['fixture'], case['source_sha256'],
        case['publisher_pdf_sha256'], case['pages'],
    )
    check(isinstance(head, str) and HEX40.fullmatch(head) is not None,
          'exact captured revision SHA is missing')
    check(owner.get('schema') == 'chaptera.product-oidc-worst-census-browser.v1'
          and owner.get('fixture') == ident
          and owner.get('repository_commit_sha') == head,
          'real OIDC browser owner/checkout mismatch')
    check(owner.get('real_oidc_pkce_session_reused') is True
          and owner.get('tls_product_source_ingress') is True
          and owner.get('exact_source_and_revision') is True
          and owner.get('actual_product_http_status') == 200
          and owner.get('isolated_worker_and_host_owned_image') is True
          and owner.get('chromium_pixels_decoded') is True
          and owner.get('anonymous_scene_status') == 401
          and owner.get('external_image_requests') == 0
          and owner.get('browser_errors') == 0
          and owner.get('source_container_forwarded') is False,
          'missing actual authenticated Product browser/security evidence')
    check(sha(owner.get('revision_identity_sha256')),
          'missing durable revision bound to authenticated Product owner')
    check(type(owner.get('host_image_count')) is int
          and 0 <= owner['host_image_count'] <= 96
          and owner.get('browser_painted_resources') == owner['host_image_count']
          and owner.get('document_page_count') == pages,
          'browser admitted image/page membership mismatch')
    check(owner.get('fidelity_state') in ('partial', 'supported'),
          'browser did not expose actual Product fidelity')
    check(summary.get('schema') == 'chaptera.product-oidc-worst-census-physical.v1'
          and summary.get('fixture') == ident
          and summary.get('repository_head_sha') == head
          and summary.get('original_pub_sha256') == src
          and summary.get('expected_publisher_pdf_pages') == pages
          and summary.get('product_scene_pages') == pages,
          'physical OIDC SourceIngress/worker summary identity mismatch')
    check(summary.get('real_oidc_https_source_ingress') is True
          and summary.get('real_isolated_product_worker_host_derived_images') is True
          and summary.get('authenticated_product_http_browser_paint') is True
          and summary.get('real_publisher_visual_a_b') is True
          and summary.get('publisher_comparison_page_count') == pages
          and summary.get('browser_painted_unique_resources') == owner['host_image_count']
          and summary.get('host_owned_image_count') == owner['host_image_count']
          and summary.get('source_or_pixel_artifact_uploaded') is False
          and summary.get('deployed_production_claim') is False
          and summary.get('external_s3_claim') is False,
          'physical Product worker/host/PDF proofs incomplete')
    check(privacy.get('protocol_version') == 'chaptera.security-leak-scan.v1'
          and privacy.get('passed') is True
          and privacy.get('finding_count') == 0
          and privacy.get('findings') == []
          and type(privacy.get('files_scanned')) is int
          and privacy['files_scanned'] >= 3,
          'privacy leak scan absent, incomplete or failed')
    check(comparison.get('schema') == 'chaptera.product-worst-publisher-ab-score.v1'
          and comparison.get('fixture') == ident
          and comparison.get('source_sha256') == src
          and comparison.get('publisher_pdf_sha256') == pdf
          and comparison.get('repository_commit_sha') == head
          and comparison.get('ranked_pages') == pages
          and comparison.get('publisher_reference_surface_stage') == 'unknown'
          and comparison.get('route') == 'oidc-https-source-ingress-isolated-worker-host-preview-chromium'
          and comparison.get('no_source_scene_or_pixels_in_output') is True
          and comparison.get('coarse_rgb_box64_only') is True
          and comparison.get('publisher_pixel_exact_parity') is False
          and comparison.get('product_full_49_111_score') is False
          and comparison.get('production_go') is False
          and comparison.get('host_admitted_image_count') == owner['host_image_count'],
          'Publisher/browser visual report is not exact and source-neutral')
    raw = comparison.get('pages')
    check(isinstance(raw, list) and len(raw) == pages,
          'not all Publisher ordinal pages were physically captured')
    document_pages = []
    seen = set()
    for item in raw:
        check(type(item) is dict and type(item.get('page')) is int
              and 1 <= item['page'] <= pages and item['page'] not in seen,
              'duplicate/out-of-bounds Product ordinal page')
        seen.add(item['page'])
        check(sha(item.get('publisher_grid_sha256'))
              and sha(item.get('product_page_identity_sha256'))
              and sha(item.get('chromium_png_sha256')),
              'Publisher/page/raster SHA of real Chromium capture missing')
        check(type(item.get('product_changed_cells')) is int
              and 0 <= item['product_changed_cells'] <= 4096,
              'nonphysical Product changed-cell measurement')
        check(item.get('host_derived_resource_count') == owner['host_image_count']
              and item.get('fidelity_state') ==
                  ('partial' if owner['fidelity_state'] == 'partial' else 'full'),
              'per-page Product host or fidelity claim differs from OIDC owner')
        extent = item.get('physical_extent_delta_pt')
        check(type(extent) is dict
              and set(extent) == {'width_pt', 'height_pt'}
              and all(type(extent[k]) in (float, int)
                      and -0.05 <= extent[k] <= 0.05
                      for k in ('width_pt', 'height_pt')),
              'physical page extent is not Publisher-comparable')
        page = {
            'page': item['page'],
            'publisher_grid_sha256': item['publisher_grid_sha256'],
            'product_page_identity_sha256': item['product_page_identity_sha256'],
            'chromium_png_sha256': item['chromium_png_sha256'],
            'changed_cell_count': item['product_changed_cells'],
            'physical_extent_delta_pt': extent,
            'host_derived_resource_count': owner['host_image_count'],
            'fidelity_state': item['fidelity_state'],
        }
        check(set(page) == PAGE_KEYS, 'canonical physical page contract drift')
        document_pages.append(page)
    check(seen == set(range(1, pages + 1)),
          'incomplete original ordinal page membership')
    result = {
        'fixture': fixture,
        'pub_sha256': src,
        'publisher_pdf_sha256': pdf,
        'status': 'accepted_product',
        'product_page_count': pages,
        'revision_identity_sha256': owner['revision_identity_sha256'],
        'checks': {key: True for key in REQUIRED_CHECKS},
        'pages': sorted(document_pages, key=lambda item: item['page']),
    }
    check(set(result) == DOCUMENT_KEYS, 'canonical Product document schema drift')
    return result


def shard_receipt(roster: dict, shard_index: int, head: str,
                  documents: list[dict]) -> dict:
    check(type(shard_index) is int and 0 <= shard_index < 4,
          'invalid fixed physical shard')
    check(isinstance(head, str) and HEX40.fullmatch(head) is not None,
          'shard must bind one exact repository head')
    expected = roster['shards'][shard_index]['fixtures']
    check(isinstance(documents, list) and len(documents) == len(expected),
          'incomplete shard physical documents (no silent source exclusion)')
    by_name = {}
    for document in documents:
        check(type(document) is dict and set(document) == DOCUMENT_KEYS,
              'unexpected source-neutral Product document keys')
        name = document['fixture']
        check(isinstance(name, str) and name not in by_name,
              'duplicate Product shard source')
        by_name[name] = document
    check(set(by_name) == {case['fixture'] for case in expected},
          'physical shard contents do not match immutable selection')
    for case in expected:
        doc = by_name[case['fixture']]
        check(doc['status'] == 'accepted_product'
              and doc['pub_sha256'] == case['source_sha256']
              and doc['publisher_pdf_sha256'] == case['publisher_pdf_sha256']
              and doc['product_page_count'] == case['pages']
              and sha(doc['revision_identity_sha256']),
              'shard includes unproven Product source/revision')
        check(type(doc['pages']) is list
              and len(doc['pages']) == case['pages']
              and {x['page'] for x in doc['pages']} == set(range(1,case['pages'] + 1)),
              'Product shard missing ordinal physical pages')
        check(type(doc['checks']) is dict
              and set(doc['checks']) == REQUIRED_CHECKS
              and all(value is True for value in doc['checks'].values()),
              'Product shard missing physical/auth/privacy evidence')
    return {'schema': SCHEMA_SHARD, 'repository_commit_sha': head,
            'shard_index': shard_index,
            'documents': [by_name[case['fixture']] for case in expected]}


def combine(roster: dict, frozen: dict, refs: dict, pairs: dict, head: str,
            fragments: list[dict]) -> tuple[dict, dict]:
    check(len(fragments) == 4, 'exactly four physically executed shards required')
    indexed = {}
    for fragment in fragments:
        check(type(fragment) is dict
              and set(fragment) == {
                  'schema', 'repository_commit_sha', 'shard_index', 'documents'}
              and fragment['schema'] == SCHEMA_SHARD
              and fragment['repository_commit_sha'] == head
              and type(fragment['shard_index']) is int
              and 0 <= fragment['shard_index'] < 4
              and fragment['shard_index'] not in indexed,
              'shard receipt is duplicate, stale or malformed')
        index = fragment['shard_index']
        indexed[index] = shard_receipt(
            roster, index, head, fragment['documents'])['documents']
    check(set(indexed) == set(range(4)),
          'missing physical shard (no reduced scoreboard denominator)')
    by_name = {doc['fixture']: doc
               for index in range(4) for doc in indexed[index]}
    check(len(by_name) == 39, 'eligible Product shard source is missing/duplicated')
    for row in frozen['rows']:
        fixture, source, pdf, _pages, _lo, _reader, eligible = row
        if not eligible:
            check(fixture not in by_name, 'excluded Publisher source appeared in Product score')
            by_name[fixture] = {
                'fixture': fixture, 'pub_sha256': source,
                'publisher_pdf_sha256': pdf,
                'status': 'predeclared_ineligible',
                'product_page_count': None,
                'revision_identity_sha256': None,
                'checks': {}, 'pages': [],
            }
    payload = {'schema': PHYSICAL_SCHEMA, 'repository_commit_sha': head,
               'capture_route': CAPTURE_ROUTE, 'method': METHOD,
               'documents': [by_name[row[0]] for row in frozen['rows']]}
    ranking = score(payload, frozen, refs, pairs, head)
    return payload, ranking


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    subs = parser.add_subparsers(dest='action', required=True)
    p_case = subs.add_parser('case')
    for field in ('case','head','owner','summary','comparison','privacy','out'):
        p_case.add_argument('--'+field, required=True)
    p_shard = subs.add_parser('shard')
    for field in ('shard','head','case-dir','out'):
        p_shard.add_argument('--'+field, required=True)
    p_combine = subs.add_parser('combine')
    for field in ('head','shard-dir','physical-out','score-out'):
        p_combine.add_argument('--'+field, required=True)
    args = parser.parse_args()
    roster = load_roster()
    if args.action == 'case':
        candidates = [c for s in roster['shards'] for c in s['fixtures']
                      if c['id'] == args.case]
        check(len(candidates) == 1, 'case is not in fixed eligible physical roster')
        document = case_receipt(
            candidates[0], args.head,
            load(Path(args.owner)), load(Path(args.summary)),
            load(Path(args.comparison)), load(Path(args.privacy)),
        )
        output = Path(args.out)
        payload = document
    elif args.action == 'shard':
        index = int(args.shard)
        check(0 <= index < 4, 'invalid shard')
        docs = [load(Path(args.case_dir) / case['id'] / 'document.json')
                for case in roster['shards'][index]['fixtures']]
        payload = shard_receipt(roster, index, args.head, docs)
        output = Path(args.out)
    else:
        frozen = load_frozen(FROZEN)
        refs, pairs = read_publisher_registry(FINGERPRINT, PAIRS)
        base = Path(args.shard_dir)
        fragments = [load(base / f'shard-{index}.json')
                     for index in range(4)]
        payload, ranking = combine(roster, frozen, refs, pairs, args.head,
                                   fragments)
        output = Path(args.physical_out)
        scored = Path(args.score_out)
        scored.parent.mkdir(parents=True, exist_ok=True)
        scored.write_text(json.dumps(ranking, sort_keys=True) + '\n',
                          encoding='utf-8')
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(payload, sort_keys=True) + '\n',
                      encoding='utf-8')
    print(json.dumps({'stage': args.action,
                      'complete': args.action != 'case',
                      'actual_product_pages_scored': (
                          ranking['product_pages']['pages']
                          if args.action == 'combine' else 0)},
                     sort_keys=True))


if __name__ == '__main__':
    main()
