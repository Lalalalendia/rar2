#!/usr/bin/env python3
"""Strict Product-host-only Publisher/LibreOffice 49/111 visual receipt scorer.

This is a measurement contract, not a Product runner. The only accepted input is
source-neutral physical evidence of authenticated Product/host/Chromium capture.
It never upgrades DIRECT/Guest measurements to Product or publishes partial wins.
"""
from __future__ import annotations

import argparse
import json
import re
from pathlib import Path

from reader_publisher_lo_49_111_scoreboard_v1 import (
    load_frozen, read_publisher_registry, strict_load_json,
    winner, win_summary, document_summary,
)

SCHEMA = 'chaptera.product-publisher-lo-49-111-scoreboard.v1'
PHYSICAL_SCHEMA = 'chaptera.product-49-111-physical-pages.v1'
CAPTURE_ROUTE = 'oidc_pkce_https_source_ingress_host_admitted_product_chromium'
REQUIRED_CHECKS = frozenset({
    'oidc_pkce_authenticated', 'https_source_ingress', 'source_sha_rechecked',
    'durable_document_revision', 'isolated_product_worker',
    'host_independently_admitted_resources', 'product_scene_http_authenticated',
    'actual_chromium_capture', 'no_external_requests',
    'fidelity_disposition_exposed', 'privacy_scan_passed',
})
METHOD = {
    'changed_cell_channel_delta': 12, 'colorspace': 'RGB',
    'grid_width': 64, 'grid_height': 64, 'resampling': 'Pillow.BOX',
}
SHA40 = re.compile(r'^[a-f0-9]{40}$')
SHA256 = re.compile(r'^[a-f0-9]{64}$')
DOCUMENT_KEYS = frozenset({
    'fixture', 'pub_sha256', 'publisher_pdf_sha256', 'status',
    'product_page_count', 'revision_identity_sha256', 'checks', 'pages',
})
PAGE_KEYS = frozenset({
    'page', 'publisher_grid_sha256', 'product_page_identity_sha256',
    'chromium_png_sha256', 'changed_cell_count', 'physical_extent_delta_pt',
    'host_derived_resource_count', 'fidelity_state',
})


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ValueError(message)


def is_sha256(value: object) -> bool:
    return isinstance(value, str) and SHA256.fullmatch(value) is not None


def score(receipt: dict, frozen: dict, refs: dict, pairs: dict,
          expected_head: str) -> dict:
    """Reject the entire scoreboard unless all 39/111 Product pages are proven.

    The frozen, refs, and pairs inputs must have passed the existing pinned
    DIRECT benchmark's cryptographic source/Publisher-reference validators.
    """
    require(isinstance(expected_head, str) and SHA40.fullmatch(expected_head) is not None,
            'expected Product checkout SHA is invalid')
    require(set(receipt) == {'schema', 'repository_commit_sha', 'capture_route', 'method', 'documents'},
            'Product physical receipt schema/fields drift')
    require(receipt['schema'] == PHYSICAL_SCHEMA, 'missing full Product physical receipt')
    require(receipt['repository_commit_sha'] == expected_head,
            'physical Product receipt SHA does not match exact checkout')
    require(receipt['capture_route'] == CAPTURE_ROUTE,
            'DIRECT/Guest rendering is not Product evidence')
    require(receipt['method'] == METHOD, 'Publisher comparison raster method drift')
    documents = receipt['documents']
    require(isinstance(documents, list) and len(documents) == 49,
            'incomplete Product source cohort: must include all 49 preselected PUBs')
    by_name: dict[str, dict] = {}
    for doc in documents:
        require(isinstance(doc, dict) and set(doc) == DOCUMENT_KEYS,
                'Product document receipt contains unexpected/missing fields')
        name = doc['fixture']
        require(isinstance(name, str) and name not in by_name,
                'duplicate/invalid Product source fixture')
        by_name[name] = doc
    require(set(by_name) == {row[0] for row in frozen['rows']},
            'Product fixed source cohort identity mismatch')

    ranked_pages: list[dict] = []
    excluded: list[dict] = []
    for fixture, pub_sha, pdf_sha, pdf_pages, _lo_pages, _old_reader_pages, lo_records in frozen['rows']:
        doc = by_name[fixture]
        require(doc['pub_sha256'] == pub_sha and doc['publisher_pdf_sha256'] == pdf_sha,
                f'Product source/Publisher PDF identity mismatch: {fixture}')
        ref = refs[fixture]
        require(pairs[fixture]['pub_sha256'] == pub_sha and ref['pdf_sha256'] == pdf_sha,
                f'Publisher pinned pair identity mismatch: {fixture}')
        require(ref['reference_pages'] == pdf_pages and len(ref['pages']) == pdf_pages,
                f'Publisher pinned page count mismatch: {fixture}')
        if not lo_records:
            require(doc['status'] == 'predeclared_ineligible' and doc['pages'] == []
                    and doc['checks'] == {} and doc['revision_identity_sha256'] is None
                    and doc['product_page_count'] is None,
                    f'unsafe exclusion or post-hoc visual selection: {fixture}')
            excluded.append(fixture)
            continue
        require(doc['status'] == 'accepted_product',
                f'eligible Product import/render failed: {fixture}')
        require(doc['product_page_count'] == pdf_pages,
                f'Product PAGE count/surface mismatch: {fixture}')
        require(is_sha256(doc['revision_identity_sha256']),
                f'missing immutable Product revision identity: {fixture}')
        checks = doc['checks']
        require(isinstance(checks, dict) and set(checks) == REQUIRED_CHECKS
                and all(value is True for value in checks.values()),
                f'Product auth/host/privacy proof absent: {fixture}')
        pages = doc['pages']
        require(isinstance(pages, list) and len(pages) == pdf_pages,
                f'incomplete Product page capture: {fixture}')
        by_page: dict[int, dict] = {}
        for page in pages:
            require(isinstance(page, dict) and set(page) == PAGE_KEYS,
                    f'Product page proof schema drift: {fixture}')
            no = page['page']
            require(type(no) is int and 1 <= no <= pdf_pages and no not in by_page,
                    f'Product ordinal PAGE duplicate/mismatch: {fixture}')
            by_page[no] = page
        require(set(by_page) == {item[0] for item in lo_records},
                f'Product ordinal PAGE cohort mismatch: {fixture}')
        publisher_by_page = {p['page']: p for p in ref['pages']}
        for no, lo_cells, _historical_direct in lo_records:
            page = by_page[no]
            publisher = publisher_by_page[no]
            require(page['publisher_grid_sha256'] == publisher['grid_sha256'],
                    f'Product PAGE has wrong Publisher PDF oracle: {fixture}/p{no}')
            require(is_sha256(page['product_page_identity_sha256'])
                    and is_sha256(page['chromium_png_sha256']),
                    f'missing actual Product Chromium/page capture proof: {fixture}/p{no}')
            count = page['changed_cell_count']
            require(type(count) is int and 0 <= count <= 4096,
                    f'Product changed-cell count invalid: {fixture}/p{no}')
            require(type(page['host_derived_resource_count']) is int
                    and 0 <= page['host_derived_resource_count'] <= 128,
                    f'Product host resource count invalid: {fixture}/p{no}')
            require(page['fidelity_state'] in ('partial', 'full'),
                    f'Product fidelity disclosure missing: {fixture}/p{no}')
            ext = page['physical_extent_delta_pt']
            require(isinstance(ext, dict) and set(ext) == {'width_pt', 'height_pt'}
                    and all(type(ext[k]) in (float, int) and -0.05 <= ext[k] <= 0.05
                            for k in ('width_pt', 'height_pt')),
                    f'Product vs Publisher extent mismatch: {fixture}/p{no}')
            ranked_pages.append({
                'fixture': fixture, 'page': no,
                'reader_cells': count, 'libreoffice_cells': lo_cells,
                'winner': winner((count - lo_cells) / 4096),
            })

    require(len(excluded) == 10 and len(ranked_pages) == 111,
            'Product 49/39/111 fixed denominator violated')
    pages_summary = win_summary(ranked_pages)
    docs_summary = document_summary(ranked_pages)
    require(pages_summary['pages'] == 111 and docs_summary['documents'] == 39,
            'Product summary denominator corrupted')
    largest = sorted(ranked_pages, key=lambda x:
                     (x['reader_cells'] - x['libreoffice_cells'], x['fixture'], x['page']),
                     reverse=True)
    return {
        'schema': SCHEMA,
        'status': 'complete_product_physical',
        'repository_commit_sha': expected_head,
        'capture_route': CAPTURE_ROUTE,
        'frozen_cohort': {'pubs': 49, 'ranked_documents': 39,
                          'ranked_pages': 111, 'predeclared_ineligible': 10},
        'product_pages': pages_summary,
        'product_documents': docs_summary,
        'top_10_remaining_lo_advantages': [
            {'fixture': x['fixture'], 'page': x['page'],
             'product_cells': x['reader_cells'], 'lo_cells': x['libreoffice_cells']}
            for x in largest[:10]
        ],
        'source_free_and_private_pixels_excluded': True,
        'claims': {'product_route_physically_observed': True,
                   'publisher_pixel_exact_parity': False,
                   'frozen_lo_not_reexecuted': True,
                   'no_prod_s3_or_deployment_evidence': True,
                   'publisher_reference_surface_stage_unknown': True},
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--physical-product', type=Path, required=True)
    parser.add_argument('--frozen-lo', type=Path, required=True)
    parser.add_argument('--publisher-fingerprint', type=Path, required=True)
    parser.add_argument('--publisher-pairs', type=Path, required=True)
    parser.add_argument('--expected-head', required=True)
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    frozen = load_frozen(args.frozen_lo)
    refs, pairs = read_publisher_registry(args.publisher_fingerprint, args.publisher_pairs)
    physical = strict_load_json(args.physical_product)
    out = score(physical, frozen, refs, pairs, args.expected_head)
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(out, indent=2, sort_keys=True) + '\n', encoding='utf-8')
    print(json.dumps({'status': out['status'], 'physical_sha': args.expected_head,
                      'product_pages': out['product_pages'],
                      'product_documents': out['product_documents']}, sort_keys=True))
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
