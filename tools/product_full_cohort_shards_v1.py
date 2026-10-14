#!/usr/bin/env python3
"""Pin the Product 49/39/111 physical capture roster to the Publisher oracle.

This source-neutral preflight only selects shards. It does NOT produce a
Product physical receipt, declare a rendered page, or claim release readiness.
Inputs are already hash-pinned by the canonical 49/111 science helpers.
"""
from __future__ import annotations

import argparse
import json
import re
from pathlib import Path

from reader_publisher_lo_49_111_scoreboard_v1 import load_frozen, read_publisher_registry

ROOT = Path(__file__).resolve().parent
FROZEN = ROOT / 'corpus/receipts/publisher-lo-t999-49-111-frozen-v1.json'
FINGERPRINT = ROOT / 'corpus/receipts/publisher-visual-golden-batch-01-fingerprint-v1.json'
PAIRS = ROOT / 'corpus/receipts/publisher-visual-golden-batch-01-pairs.csv'
SCHEMA = 'chaptera.product-49-111-physical-shard-plan.v1'

# Fixed, independently reviewed 39-document / 111-page partition. Never
# rebalance in response to runtime failures or failed Publisher comparisons.
SHARDS = (
    ('075', '071'),
    ('025', '073', '048', '043', '057', '049', '051', '023', '034', '072', '066', '060'),
    ('027', '045', '041', '053', '046', '038', '061', '026', '044', '063', '070', '076'),
    ('028', '021', '024', '032', '037', '047', '050', '020', '022', '077', '059', '040', '001'),
)
EXPECTED_SHARD_PAGES = (28, 28, 28, 27)
EXCLUDED = frozenset(('029', '031', '033', '054', '062', '064', '065', '068', '079', '080'))
HEX_SHA = re.compile(r'^[0-9a-f]{64}$')
ID_NAME = re.compile(r'^([0-9]{3})_([0-9a-f]{16})$')


def require(value: bool, message: str) -> None:
    if not value:
        raise ValueError(message)


def select(frozen: dict, references: dict, pairs: dict) -> dict:
    """Require source/PDF SHA, page membership, and full fixed partition."""
    rows = frozen.get('rows')
    require(isinstance(rows, list) and len(rows) == 49,
            'frozen cohort must have exactly 49 PUBs')
    eligible, excluded = {}, set()
    for row in rows:
        require(isinstance(row, list) and len(row) == 7,
                'unexpected frozen cohort row')
        basename, source, pdf, pages, _lo_pages, _reader_pages, ranked = row
        match = ID_NAME.fullmatch(basename) if isinstance(basename, str) else None
        require(match is not None and match.group(2) == source[:16],
                'malformed frozen fixture identity')
        case = match.group(1)
        require(case not in eligible and case not in excluded,
                'duplicate 3-digit Publisher PUB identity')
        require(isinstance(source, str) and HEX_SHA.fullmatch(source) is not None
                and isinstance(pdf, str) and HEX_SHA.fullmatch(pdf) is not None,
                'invalid source or PDF hash')
        require(type(pages) is int and 0 < pages <= 100,
                'invalid signed Publisher page count')
        require(basename in references and basename in pairs,
                'Publisher pair missing from immutable reference registry')
        ref, pair = references[basename], pairs[basename]
        require(ref['pub_sha256'] == source and ref['pdf_sha256'] == pdf
                and pair['pub_sha256'] == source and pair['pdf_sha256'] == pdf,
                'source/PDF SHA differs from immutable Publisher reference')
        require(int(pair['pdf_pages']) == pages
                and len(ref['pages']) == pages,
                'Publisher PAGE membership/geometry reference drift')
        require(isinstance(ranked, list), 'frozen eligibility must be a list')
        if not ranked:
            excluded.add(case)
            continue
        require(len(ranked) == pages
                and {p[0] for p in ranked} == set(range(1, pages + 1)),
                'eligible ranked PAGE ordinals do not cover Publisher PDF')
        size = int(pair['pub_bytes'])
        require(size > 0 and size < 100_000_000,
                'out-of-range immutable native PUB size')
        eligible[case] = {
            'id': case, 'fixture': basename, 'source_sha256': source,
            'publisher_pdf_sha256': pdf, 'source_bytes': size,
            'pages': pages,
        }
    require(excluded == EXCLUDED,
            'historical ineligible sources changed or became post-hoc exclusions')
    require(len(eligible) == 39 and sum(e['pages'] for e in eligible.values()) == 111,
            '39 eligible / 111 PAGE fixed denominator changed')
    flat = [case for group in SHARDS for case in group]
    require(len(flat) == 39 and len(set(flat)) == 39
            and set(flat) == set(eligible),
            'four shards do not partition the complete eligible cohort')
    shards = []
    for index, group in enumerate(SHARDS):
        fixtures = [eligible[x] for x in group]
        require(sum(f['pages'] for f in fixtures) == EXPECTED_SHARD_PAGES[index],
                'fixed 28/28/28/27 shard PAGE balance drift')
        shards.append({'id': index, 'pages': EXPECTED_SHARD_PAGES[index],
                       'fixtures': fixtures})
    return {'schema': SCHEMA, 'fixed_total_pubs': 49,
            'eligible_documents': 39, 'eligible_pages': 111,
            'predeclared_ineligible': sorted(excluded), 'shards': shards,
            'physical_capture_completed': False, 'product_score_accepted': False}


def build() -> dict:
    frozen = load_frozen(FROZEN)
    refs, pairs = read_publisher_registry(FINGERPRINT, PAIRS)
    return select(frozen, refs, pairs)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    group = parser.add_mutually_exclusive_group()
    group.add_argument('--shard', type=int, choices=range(4))
    group.add_argument('--case', type=str)
    group.add_argument('--matrix', action='store_true')
    parser.add_argument('--out', type=Path)
    args = parser.parse_args()
    plan = build()
    if args.shard is not None:
        output = plan['shards'][args.shard]
    elif args.case is not None:
        require(re.fullmatch(r'[0-9]{3}', args.case) is not None,
                'case must be a canonical 3-digit Publisher fixture')
        hits = [f for s in plan['shards'] for f in s['fixtures']
                if f['id'] == args.case]
        require(len(hits) == 1, 'case is ineligible or missing')
        output = hits[0]
    elif args.matrix:
        output = {'include': [
            {'shard': str(s['id']), 'pages': s['pages'],
             'case_ids': ' '.join(f['id'] for f in s['fixtures'])}
            for s in plan['shards']]}
    else:
        output = plan
    payload = json.dumps(output, sort_keys=True, separators=(',', ':')) + '\n'
    if args.out:
        args.out.parent.mkdir(parents=True, exist_ok=True)
        args.out.write_text(payload, encoding='utf-8')
    else:
        print(payload, end='')


if __name__ == '__main__':
    main()
