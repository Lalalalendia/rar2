#!/usr/bin/env python3
"""Re-rank the fixed PUB-T-999 49/111 cohort against an exact Reader Batch01.

Reference remains previously exported Microsoft Publisher 2019 64x64 RGB
fingerprints; frozen LO 24.2.7.2 is a comparison implementation, never an
independent PUB or Publisher semantic authority. No PUB/PDF/PNG are emitted.
"""
from __future__ import annotations

import argparse
from collections import Counter
import csv
import hashlib
import json
from pathlib import Path
import re

SCHEMA = 'chaptera.reader-publisher-lo-fixed-49-111-scoreboard.v1'
FROZEN_SCHEMA = 'chaptera.reader-publisher-lo-t999-frozen-v1'
FROZEN_SHA256 = 'ffde596be77ec38850ed93c3a653adbeada844c55c9764bc2f4e53fd3b091194'
REFERENCE_BLOB = '1e87b52d268f87dcd4896f3d1af351a0b8cb9d96'
PAIRS_BLOB = '2c2b03652e79390befe55f2889dbc248eb2abe01'
SCIENCE_REPORT_SHA256 = '2f3f1bcddaa6aa094b46893825593d9aef4789b6960620dfdaf8d01a6af5b5fd'
TIE = 0.015
MAX_FROZEN_BYTES = 32768
HEX_SHA = re.compile(r'^[0-9a-f]{40}$')
HEX_SHA256 = re.compile(r'^[0-9a-f]{64}$')


def sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def git_blob_sha(value: bytes) -> str:
    return hashlib.sha1(b'blob ' + str(len(value)).encode() + b'\x00' + value).hexdigest()


def strict_load_json(path: Path) -> dict:
    # Source-free, but bounded against CI archive mistakes and bomb inputs.
    if not path.is_file() or path.stat().st_size > 3_000_000:
        raise ValueError(f'missing or oversized JSON input: {path.name}')
    result = json.loads(path.read_text(encoding='utf-8'))
    if not isinstance(result, dict):
        raise ValueError('expected object-form JSON receipt')
    return result


def load_frozen(path: Path) -> dict:
    source = path.read_bytes()
    if sha256_bytes(source) != FROZEN_SHA256:
        raise ValueError('frozen SK1 science LO cohort SHA-256 mismatch')
    if len(source) > MAX_FROZEN_BYTES:
        raise ValueError('over-budget frozen LO record')
    data = json.loads(source)
    if data.get('schema') != FROZEN_SCHEMA:
        raise ValueError('unexpected frozen LO manifest schema')
    if data.get('source_science_report_sha256') != SCIENCE_REPORT_SHA256:
        raise ValueError('SCIENCE_REPORT_SHA256 was silently repinned')
    if data.get('publisher_ref_git_blob') != REFERENCE_BLOB or data.get('pair_manifest_git_blob') != PAIRS_BLOB:
        raise ValueError('source Publisher reference authority drift')
    if data.get('method') != {
        'channel_significance': 12, 'grid': [64,64], 'colorspace': 'RGB',
        'resampling': 'Pillow.BOX', 'tie_fraction': TIE,
        'physical_page_tolerance_pt': 0.05,
    }:
        raise ValueError('historical benchmark method drift')
    cohort = data.get('cohort', {})
    if cohort != {
        'pub_count': 49, 'eligible_pair_count': 39, 'eligible_page_count': 111,
        'reference_page_count': 153, 'native_reference_surface_stage': 'unknown',
    }:
        raise ValueError('frozen 49/39/111 benchmark denominator drift')
    rows = data.get('rows')
    if not isinstance(rows, list) or len(rows) != 49 or len({r[0] for r in rows}) != 49:
        raise ValueError('frozen LO expected exactly 49 unique PUB source cases')
    if sum(bool(r[6]) for r in rows) != 39 or sum(len(r[6]) for r in rows) != 111:
        raise ValueError('historical ranking eligibility drift')
    if sum(r[3] for r in rows) != 153:
        raise ValueError('frozen Publisher reference page count drift')
    if any(not HEX_SHA256.fullmatch(r[1]) or not HEX_SHA256.fullmatch(r[2]) for r in rows):
        raise ValueError('invalid frozen PUB/PDF SHA')
    return data


def winner(gap: float) -> str:
    return 'LIBREOFFICE' if gap > TIE else 'READER' if gap < -TIE else 'NEAR_TIE'


def win_summary(pages: list[dict]) -> dict:
    scores = Counter(row['winner'] for row in pages)
    total = len(pages)
    return {
        'pages': total,
        'libreoffice_wins': scores['LIBREOFFICE'],
        'reader_wins': scores['READER'],
        'near_ties': scores['NEAR_TIE'],
        'mean_reader_changed_fraction': round(sum(p['reader_cells'] for p in pages) / (4096 * total), 12),
        'mean_libreoffice_changed_fraction': round(sum(p['libreoffice_cells'] for p in pages) / (4096 * total), 12),
    }


def document_summary(pages: list[dict]) -> dict:
    documents = {}
    for p in pages:
        documents.setdefault(p['fixture'], []).append(p)
    scores = Counter(winner(sum(p['reader_cells'] - p['libreoffice_cells'] for p in group) / (4096 * len(group))) for group in documents.values())
    return {'documents': len(documents), 'libreoffice_wins': scores['LIBREOFFICE'],
            'reader_wins': scores['READER'], 'near_ties': scores['NEAR_TIE']}


def read_publisher_registry(reference_path: Path, pairs_path: Path) -> tuple[dict, dict]:
    reference_bytes, pair_bytes = reference_path.read_bytes(), pairs_path.read_bytes()
    if git_blob_sha(reference_bytes) != REFERENCE_BLOB:
        raise ValueError('Publisher fingerprint Git blob does not match science oracle')
    if git_blob_sha(pair_bytes) != PAIRS_BLOB:
        raise ValueError('Publisher pairs registry Git blob does not match science oracle')
    reference = json.loads(reference_bytes)
    if reference.get('pair_count') != 55 or reference.get('reference_page_count') != 162:
        raise ValueError('Publisher fingerprint 55/162 scope drift')
    refpairs = reference.get('pairs')
    if not isinstance(refpairs, list) or len(refpairs) != 55:
        raise ValueError('Publisher fingerprint registry malformed')
    refs = {x['basename']:x for x in refpairs}
    if len(refs) != 55:
        raise ValueError('duplicate Publisher reference fixture')
    rows = list(csv.DictReader(pair_bytes.decode('utf-8').splitlines()))
    if len(rows) != 55:
        raise ValueError('Publisher SHA pairs registry no longer has 55 pairs')
    pairs = {x['basename']: x for x in rows}
    if len(pairs) != 55 or set(pairs) != set(refs):
        raise ValueError('Publisher registry identity discrepancy')
    for fixture, entry in pairs.items():
        ref = refs[fixture]
        if entry['pub_sha256'] != ref['pub_sha256'] or entry['pdf_sha256'] != ref['pdf_sha256'] or int(entry['pdf_pages']) != len(ref['pages']):
            raise ValueError(f'Publisher source/PDF pair identity drift: {fixture}')
    return refs,pairs


def rerank(reader: dict, frozen: dict, pairs: dict, expected_reader_sha: str) -> dict:
    if not HEX_SHA.fullmatch(expected_reader_sha) or reader.get('repository_commit_sha') != expected_reader_sha:
        raise ValueError('actual Reader build SHA is not exact requested checkout SHA')
    if reader.get('schema') != 'chaptera.publisher-visual-fingerprint-compare.v1':
        raise ValueError('Reader Batch01 result schema drift')
    if reader.get('comparator') != {
        'changed_cell_channel_delta':12,'colorspace':'RGB', 'grid_height':64, 'grid_width':64,'resampling':'Pillow.BOX'
    }:
        raise ValueError('Reader 64x64 method differs from frozen Publisher/LO method')
    if reader.get('pair_count') != 55 or reader.get('reference_page_count') != 162 or reader.get('compared_page_count') != 162:
        raise ValueError('current Reader did not measure full 55/162 reference')
    claims=reader.get('claims',{})
    if claims.get('pdf_used_as_visual_authority_only') is not True or claims.get('pixel_exact_visual_parity') is not False:
        raise ValueError('false Publisher pixel-parity or reference authority claim')
    if reader.get('reference_surface_stage_counts',{}).get('unknown') != 55:
        raise ValueError('Publisher stage authority changed; do not reuse historical ordinal cohort')
    pair_rows = reader.get('pairs', [])
    page_rows = reader.get('pages', [])
    if len(pair_rows) != 55 or len(page_rows) != 162:
        raise ValueError('incomplete Batch01 Reader pair/page records')
    current_pairs = {p['fixture']:p for p in pair_rows}
    current_pages = {(p['fixture'],p['page']):p for p in page_rows}
    if len(current_pairs)!=55 or len(current_pages)!=162 or set(current_pairs)!=set(pairs):
        raise ValueError('duplicate/missing Reader fixture or page rows')
    for fixture,pair in current_pairs.items():
        authority=pairs[fixture]
        if pair.get('source_sha256') != authority['pub_sha256'] or pair.get('reference_pages')!=int(authority['pdf_pages']):
            raise ValueError(f'Reader source/PDF SHA provenance drift: {fixture}')
    historic, current = [], []
    ineligible=[]
    for fixture,source_sha,pdf_sha,pdf_pages,lo_pages,old_reader_pages,lo_records in frozen['rows']:
        authority=pairs.get(fixture)
        if authority is None or authority['pub_sha256'] != source_sha or authority['pdf_sha256'] != pdf_sha or int(authority['pdf_pages']) != pdf_pages:
            raise ValueError(f'frozen 49 source/PDF pairing drift: {fixture}')
        curr=current_pairs[fixture]
        if curr.get('source_sha256') != source_sha:
            raise ValueError(f'candidate Reader source SHA mismatch: {fixture}')
        if not lo_records:
            ineligible.append({'fixture':fixture,'historical_native_pdf_pages':pdf_pages,
                               'historical_reader_pages':old_reader_pages,
                               'current_reader_pages':curr['candidate_pages'],
                               'historical_libreoffice_pages':lo_pages,
                               'reason':'predeclared_page_count_or_extent_mismatch'})
            continue
        if len(lo_records)!=pdf_pages or curr['candidate_pages'] != pdf_pages:
            raise ValueError(f'predeclared comparable document page count changed: {fixture}')
        seen_ordinals=set()
        for page_no,lo_count,old_count in lo_records:
            if not (isinstance(page_no,int) and 1 <= page_no <= pdf_pages and page_no not in seen_ordinals):
                raise ValueError(f'frozen LO page identity duplicate/drift: {fixture}')
            seen_ordinals.add(page_no)
            if not all(isinstance(n,int) and 0 <= n <= 4096 for n in (lo_count,old_count)):
                raise ValueError('frozen LO/Reader changed-cells corrupt')
            current_row=current_pages.get((fixture,page_no))
            if current_row is None or current_row.get('reference_surface_stage')!='unknown':
                raise ValueError(f'missing comparable Reader page or surface-stage drift: {fixture}/p{page_no}')
            sizes=current_row.get('reference_media_extent_delta',{})
            if any(not isinstance(sizes.get(k),(int,float)) or abs(sizes[k]) > 0.05 for k in ('width_pt','height_pt')):
                raise ValueError(f'comparable Publisher physical page size drift: {fixture}/p{page_no}')
            new_count = current_row.get('changed_cell_count')
            if not isinstance(new_count,int) or not 0 <= new_count <= 4096:
                raise ValueError(f'current Reader changed-cell invalid: {fixture}/p{page_no}')
            historic.append({'fixture':fixture,'page':page_no,'reader_cells':old_count,'libreoffice_cells':lo_count,
                             'winner':winner((old_count-lo_count)/4096)})
            current.append({'fixture':fixture,'page':page_no,'reader_cells':new_count,'libreoffice_cells':lo_count,
                            'old_reader_cells':old_count,'winner':winner((new_count-lo_count)/4096),
                            'reader_change_cells_vs_historical':new_count-old_count})
    baseline_pages, now_pages = win_summary(historic), win_summary(current)
    baseline_docs, now_docs = document_summary(historic), document_summary(current)
    if baseline_pages['pages'] != 111 or baseline_docs['documents'] != 39:
        raise ValueError('historical 39/111 cohort count mismatch')
    if (baseline_pages['libreoffice_wins'],baseline_pages['reader_wins'],baseline_pages['near_ties']) != (85,17,9):
        raise ValueError('old 85:17:9 page scores cannot be reproduced from frozen science')
    if (baseline_docs['libreoffice_wins'],baseline_docs['reader_wins'],baseline_docs['near_ties']) != (26,8,5):
        raise ValueError('old 26:8:5 document scores cannot be reproduced')
    if len(ineligible)!=10:
        raise ValueError('historical exclusion denominator drift')
    ranking = sorted(current,key=lambda p:(p['reader_cells']-p['libreoffice_cells'],p['fixture'],p['page']),reverse=True)
    improvements = sorted(current,key=lambda p:(p['reader_change_cells_vs_historical'],p['fixture'],p['page']))
    return {
        'schema':SCHEMA,
        'reader_checkout_sha':expected_reader_sha,
        'benchmark':'PUB-T-999 (Publisher reference / frozen LO / current Reader)',
        'source_free_reference':{'publisher_reference_blob':REFERENCE_BLOB,'pair_manifest_blob':PAIRS_BLOB,
                                 'science_report_sha256':SCIENCE_REPORT_SHA256,'lo_version':frozen['lo_version'],
                                 'reference_surface_stage':'unknown','pixel_exact_publisher_parity_proven':False},
        'fixed_cohort':{'preselected_pubs':49,'ranked_documents':39,'ranked_pages':111,
                        'historically_ineligible_documents':10,'extra_6_pub_donor_unavailable':6},
        'historic_pages':baseline_pages,'current_pages':now_pages,
        'historic_documents':baseline_docs,'current_documents':now_docs,
        'top_10_current_lo_advantages':[{'fixture':p['fixture'],'page':p['page'],
               'lo_changed_cells':p['libreoffice_cells'],'reader_changed_cells':p['reader_cells']}
            for p in ranking[:10]],
        'top_10_reader_improvements':[{'fixture':p['fixture'],'page':p['page'],
             'improvement_cells':-p['reader_change_cells_vs_historical'],
             'current_reader_changed_cells':p['reader_cells']} for p in improvements[:10]],
        'predeclared_ineligible':ineligible,
        'claims':{'publisher_pdf_visual_oracle_only':True,'frozen_lo_not_reexecuted':True,
                  'exact_current_reader_batch01_reexecuted':True,'same_39_111_comparable_cohort':True,
                  'no_pub_pdf_odg_png_or_story_text_in_report':True,
                  'native_publisher_pixel_exact_parity':False,
                  'ordinal_stage_unknown':True},
    }


def main() -> None:
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--reader-summary',type=Path,required=True)
    parser.add_argument('--frozen-lo',type=Path,required=True)
    parser.add_argument('--publisher-fingerprint',type=Path,required=True)
    parser.add_argument('--publisher-pairs',type=Path,required=True)
    parser.add_argument('--expected-reader-sha',required=True)
    parser.add_argument('--out',type=Path,required=True)
    args=parser.parse_args()
    frozen=load_frozen(args.frozen_lo)
    _,pairs=read_publisher_registry(args.publisher_fingerprint,args.publisher_pairs)
    candidate=strict_load_json(args.reader_summary)
    report=rerank(candidate,frozen,pairs,args.expected_reader_sha)
    args.out.parent.mkdir(parents=True,exist_ok=True)
    args.out.write_text(json.dumps(report,sort_keys=True,indent=2)+'\n',encoding='utf-8')
    scores=report['current_pages']; docs=report['current_documents']
    print(f"PUBLISHER_LO_FIXED49_111 reader_sha={report['reader_checkout_sha']}")
    print(f"pages: LO {scores['libreoffice_wins']} Reader {scores['reader_wins']} near {scores['near_ties']}; documents: LO {docs['libreoffice_wins']} Reader {docs['reader_wins']} near {docs['near_ties']}")
    print('NATIVE_PUBLISHER_144DPI_EXACT_PARITY=UNVERIFIED; 64x64 coarse ordinal ranking only')


if __name__=='__main__':
    main()
