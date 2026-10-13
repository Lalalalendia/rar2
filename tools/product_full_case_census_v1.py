#!/usr/bin/env python3
"""Fail-closed, source-neutral census; failures never enter the Product score."""
from __future__ import annotations
import argparse
import json
import re
from pathlib import Path
from product_full_cohort_shards_v1 import build as roster

SCHEMA = 'chaptera.product-49-111-physical-failure-census.v1'
ROW_SCHEMA = 'chaptera.product-physical-case-outcome.v1'
FAILURES = frozenset({
    'product_http_denial','ingress_navigation_timeout',
    'product_http_response_timeout','chromium_paint_timeout',
    'source_or_revision_contract','image_count_contract','image_id_contract',
    'image_mime_contract','image_decode_cap','image_node_binding','scene_size_cap',
    'fidelity_disclosure','chromium_image_paint','external_image_denied',
    'anonymous_denial','oidc_session_contract','source_identity_contract',
    'unclassified_browser_failure','bounded_stage_failure','unclassified',
    # A failure at one of these fixed phases is NOT an accepted Product image.
    'visual_scene_input','visual_image_ablation','visual_chromium_init',
    'visual_before_navigation','visual_before_render',
    'visual_before_page_identity','visual_before_raster',
    'visual_after_navigation','visual_after_render',
    'visual_after_page_identity','visual_after_raster',
    'visual_final_checks','visual_finished','visual_unclassified',
    'http_both_unanswered','http_both_unrequested','http_both_mixed',
    'http_current_unanswered','http_current_unrequested',
    'http_scene_unanswered','http_scene_unrequested',
})
PUBLIC = frozenset({
    'redacted','grant_missing','grant_expired','capability_denied',
    'product_replay_worker_failed','product_replay_worker_timeout',
    'product_reader_safe_preview_invalid','product_replay_receipt_invalid',
    'product_replay_receipt_identity_mismatch','reader_scene_projection_failed',
    'revision_materialization_failed','stale_revision',
})
COLUMNS = frozenset({'schema','case_id','repository_commit_sha','state',
    'failure_code','public_error_code','current_http_status',
    'scene_http_status','elapsed_seconds'})


def require(cond: bool, msg: str) -> None:
    if not cond:
        raise ValueError(msg)


def read(path: Path) -> dict:
    require(path.is_file() and 0 < path.stat().st_size < 500_000,
            'missing or oversized source-neutral outcome')
    value = json.loads(path.read_text(encoding='utf8'))
    require(type(value) is dict, 'expected a JSON object')
    return value


def record(case: dict, head: str, status: int, elapsed: int,
           owner: dict | None, document: dict | None) -> dict:
    require(type(head) is str and re.fullmatch(r'[a-f0-9]{40}',head) is not None,
            'invalid exact checkout SHA')
    require(type(elapsed) is int and 0 <= elapsed <= 14400,
            'invalid bounded case duration')
    require(type(status) is int and 0 <= status <= 255,
            'invalid runner status')
    if status == 0:
        require(type(document) is dict
                and document.get('status') == 'accepted_product'
                and document.get('fixture') == case['fixture']
                and document.get('pub_sha256') == case['source_sha256']
                and document.get('publisher_pdf_sha256') == case['publisher_pdf_sha256']
                and document.get('product_page_count') == case['pages']
                and type(document.get('pages')) is list
                and len(document['pages']) == case['pages'],
                'successful case lacks actual admitted pages')
        state, failure, public, current, scene = 'accepted',None,None,None,None
    else:
        require(document is None, 'failed Product source cannot be accepted')
        state, failure, public, current, scene = 'rejected','unclassified','redacted',None,None
        if (type(owner) is dict
                and owner.get('schema') == 'chaptera.product-oidc-worst-census-failure.v1'
                and owner.get('fixture') == case['id']
                and owner.get('repository_commit_sha') == head):
            raw = owner.get('failure_code')
            failure = raw if type(raw) is str and raw in FAILURES else 'unclassified'
            raw = owner.get('public_error_code')
            public = raw if type(raw) is str and raw in PUBLIC else 'redacted'
            for attr in ('current_http_status','scene_http_status'):
                raw = owner.get(attr)
                val = raw if type(raw) is int and 100 <= raw <= 599 else None
                if attr == 'current_http_status':
                    current = val
                else:
                    scene = val
    return {'schema':ROW_SCHEMA,'case_id':case['id'],
            'repository_commit_sha':head,'state':state,'failure_code':failure,
            'public_error_code':public,'current_http_status':current,
            'scene_http_status':scene,'elapsed_seconds':elapsed}


def combine(plan: dict, shard: int, head: str, rows: list[dict]) -> dict:
    require(type(shard) is int and 0 <= shard < 4,'invalid shard')
    require(type(head) is str and re.fullmatch(r'[a-f0-9]{40}',head) is not None,
            'invalid exact head')
    selected = plan['shards'][shard]['fixtures']
    require(type(rows) is list and len(rows) == len(selected),
            'incomplete physical case census')
    by_id = {}
    for row in rows:
        require(type(row) is dict and set(row) == COLUMNS
                and row['schema'] == ROW_SCHEMA
                and row['repository_commit_sha'] == head
                and type(row['case_id']) is str
                and row['case_id'] not in by_id,
                'duplicate/stale/malformed census record')
        by_id[row['case_id']] = row
    require(set(by_id) == {f['id'] for f in selected},
            'signed physical roster not fully attempted')
    safe = []
    for case in selected:
        row = by_id[case['id']]
        require(type(row['elapsed_seconds']) is int
                and 0 <= row['elapsed_seconds'] <= 14400,
                'invalid duration')
        if row['state'] == 'accepted':
            require(all(row[k] is None for k in ('failure_code','public_error_code',
                         'current_http_status','scene_http_status')),
                    'accepted case cannot carry a failure')
        else:
            require(row['state'] == 'rejected'
                    and row['failure_code'] in FAILURES
                    and row['public_error_code'] in PUBLIC,
                    'uncontrolled failure text')
            for key in ('current_http_status','scene_http_status'):
                value = row[key]
                require(value is None or
                        (type(value) is int and 100 <= value <= 599),
                        'invalid public HTTP status')
        safe.append(row)
    failed = sum(row['state'] == 'rejected' for row in safe)
    return {'schema':SCHEMA,'repository_commit_sha':head,'shard':shard,
            'expected_cases':len(selected),
            'expected_pages':plan['shards'][shard]['pages'],
            'attempted_cases':len(safe),'accepted_cases':len(safe)-failed,
            'rejected_cases':failed,'full_shard_admitted':failed == 0,
            'global_product_score_published':False,'case_outcomes':safe}


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    sub=ap.add_subparsers(dest='mode',required=True)
    r=sub.add_parser('record')
    for key in ('case','head','exit-code','elapsed','owner','document','out'):
        r.add_argument('--'+key,required=True)
    s=sub.add_parser('summarize')
    for key in ('shard','head','case-dir','out'):
        s.add_argument('--'+key,required=True)
    args=ap.parse_args()
    plan=roster()
    if args.mode=='record':
        found=[f for x in plan['shards'] for f in x['fixtures'] if f['id']==args.case]
        require(len(found)==1,'not a selected Product case')
        owner=read(Path(args.owner)) if Path(args.owner).is_file() else None
        doc=read(Path(args.document)) if Path(args.document).is_file() else None
        result=record(found[0],args.head,int(args.exit_code),int(args.elapsed),owner,doc)
    else:
        shard=int(args.shard)
        require(0 <= shard < 4, 'invalid shard')
        expected=plan['shards'][shard]['fixtures']
        folder=Path(args.case_dir)
        require({p.name for p in folder.glob('*.json')} ==
                {c['id']+'.json' for c in expected},
                'unexpected/missing source-neutral case census file')
        result=combine(plan,shard,args.head,
                       [read(folder/(c['id']+'.json')) for c in expected])
    dst=Path(args.out)
    dst.parent.mkdir(parents=True,exist_ok=True)
    dst.write_text(json.dumps(result,sort_keys=True)+'\n',encoding='utf8')
    if args.mode=='summarize':
        print(json.dumps({'shard':result['shard'],
                          'accepted':result['accepted_cases'],
                          'rejected':result['rejected_cases'],
                          'rankable':result['full_shard_admitted']},sort_keys=True))


if __name__=='__main__':
    main()
