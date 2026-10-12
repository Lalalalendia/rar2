#!/usr/bin/env python3
"""Publisher PDF + frozen LO vs authenticated Product worst-case visual A/B.

Same physical OIDC Product source/Scene/head with and without only host image
bytes in independent Chromium captures. Only source-neutral numbers escape.
"""
from __future__ import annotations
import argparse
import json
import re
from pathlib import Path
from cloud_reader_visual_fingerprint_v1 import compare_grid, image_grid, reference_grid, sha256
from reader_publisher_lo_49_111_scoreboard_v1 import load_frozen, read_publisher_registry, winner

from product_full_cohort_shards_v1 import build as load_exact_product_cohort

CASES = {item['id']: (item['source_sha256'], item['pages'])
         for shard in load_exact_product_cohort()['shards']
         for item in shard['fixtures']}
BROWSER = 'chaptera.product-worst-publisher-ab-browser.v1'
OUTPUT = 'chaptera.product-worst-publisher-ab-score.v1'

def check_receipt(browser: dict, case: str, head: str) -> tuple[dict,dict]:
    source, pages = CASES[case]
    if not re.fullmatch('[0-9a-f]{40}',head):
        raise ValueError('expected exact checked-out repository SHA')
    if (browser.get('schema') != BROWSER or browser.get('fixture') != case or
        browser.get('source_sha256') != source or
        browser.get('repository_commit_sha') != head or
        browser.get('same_source_scene_head_and_browser') is not True or
        browser.get('image_only_ablation') is not True or
        browser.get('real_chromium') is not True or
        browser.get('external_url_request_count') != 0 or
        browser.get('page_error_count') != 0):
        raise ValueError('not exact-head real Product Chromium/host scene evidence')
    before, after = browser.get('before'),browser.get('after')
    admitted = browser.get('host_resource_count')
    if (not isinstance(before,dict) or not isinstance(after,dict) or
        type(admitted) is not int or not 0<=admitted<=96 or
        before.get('image_count') != 0 or
        type(after.get('image_count')) is not int or
        after['image_count'] < admitted or
        after.get('unique_painted_resource_ids') != admitted or
        before.get('picture_frame_count') != after.get('picture_frame_count')):
        raise ValueError('host admission/paint or ablation source mismatch')
    for state in (before,after):
        rows=state.get('pages')
        if not isinstance(rows,list) or len(rows)!=pages or \
                {r.get('page') for r in rows} != set(range(1,pages+1)):
            raise ValueError('PAGE membership does not match signed Publisher reference')
        for row in rows:
            for k in ('page_id_sha256','screenshot_sha256'):
                if not isinstance(row.get(k),str) or not re.fullmatch('[0-9a-f]{64}',row[k]):
                    raise ValueError('private raster / canonical PAGE identity missing')
            if any(type(row.get(k)) is not int or row[k]<=0 for k in ('width_emu','height_emu')):
                raise ValueError('source PAGE geometry missing')
            dims=row.get('png_dimensions')
            if not isinstance(dims,dict) or any(type(dims.get(k)) is not int or not 0<dims[k]<50000 for k in ('width','height')):
                raise ValueError('bad Chromium raster geometry')
    return before,after

def compare(case: str, ref_path: Path, pair_path: Path, frozen_path: Path,
            browser_dir: Path, output_path: Path, head: str) -> dict:
    if case not in CASES: raise ValueError('not pinned worst-case Product input')
    source,n=CASES[case]
    frozen=load_frozen(frozen_path)
    refs,pairs=read_publisher_registry(ref_path,pair_path)
    candidates=[r for r in frozen['rows'] if r[1]==source]
    if len(candidates)!=1: raise ValueError('fixture missing from signed frozen 49 cohort')
    fixture,src_sha,pdf_sha,pdf_pages,_,_,lo_rows=candidates[0]
    if len(lo_rows)!=n or pdf_pages!=n:
        raise ValueError('not an originally eligible Publisher/LO ordinal cohort')
    ref,pair=refs[fixture],pairs[fixture]
    if (ref['pub_sha256']!=src_sha or ref['pdf_sha256']!=pdf_sha or
        pair['pub_sha256']!=src_sha or pair['pdf_sha256']!=pdf_sha):
        raise ValueError('source Publisher/PUB PDF SHA mismatch')
    browser=json.loads((browser_dir/'browser-receipt.json').read_text(encoding='utf-8'))
    before,after=check_receipt(browser,case,head)
    if browser.get('fidelity_state') not in ('supported','partial'):
        raise ValueError('Product host fidelity disposition was not proven')
    rows=[]
    for i,(ordinal,lo_cells,_historic) in enumerate(lo_rows):
        page_ref=ref['pages'][ordinal-1]
        oracle=reference_grid(page_ref)
        amounts={}
        for state_name,state in (('before',before),('after',after)):
            p=state['pages'][i]
            b=before['pages'][i]
            if (p['page']!=ordinal or p['page_id_sha256']!=b['page_id_sha256'] or
                p['width_emu']!=b['width_emu'] or p['height_emu']!=b['height_emu']):
                raise ValueError('image-only ablation changed source PAGE')
            if (abs(p['width_emu']/12700-page_ref['media_width_pt']) > .05 or
                abs(p['height_emu']/12700-page_ref['media_height_pt']) > .05):
                raise ValueError('Product page physical geometry is not comparable')
            png=browser_dir/(state_name+'-page-'+str(ordinal)+'.png')
            if sha256(png)!=p['screenshot_sha256']:
                raise ValueError('ephemeral real Chromium screenshot identity mismatch')
            amounts[state_name]=compare_grid(image_grid(png),oracle)['changed_cell_count']
        product=amounts['after']
        p=after['pages'][i]
        extent={'width_pt': round(p['width_emu']/12700-page_ref['media_width_pt'],8),
                'height_pt': round(p['height_emu']/12700-page_ref['media_height_pt'],8)}
        rows.append({'page':ordinal,'product_changed_cells':product,
                     'product_page_identity_sha256':p['page_id_sha256'],
                     'chromium_png_sha256':p['screenshot_sha256'],
                     'physical_extent_delta_pt':extent,
                     'host_derived_resource_count':browser['host_resource_count'],
                     'fidelity_state':('partial' if browser['fidelity_state']=='partial' else 'full'),
                     'same_scene_image_off_changed_cells':amounts['before'],
                     'host_image_effect_cells':amounts['before']-product,
                     'frozen_lo_changed_cells':lo_cells,
                     'winner':winner((product-lo_cells)/4096),
                     'publisher_grid_sha256':page_ref['grid_sha256']})
    report={'schema':OUTPUT,'fixture':case,'source_sha256':source,
            'publisher_pdf_sha256':pdf_sha,'repository_commit_sha':head,
            'route':'oidc-https-source-ingress-isolated-worker-host-preview-chromium',
            'publisher_reference_surface_stage':'unknown','ranked_pages':n,
            'host_admitted_image_count':browser['host_resource_count'],
            'pages':rows,'product_page_wins':sum(p['winner']=='READER' for p in rows),
            'libreoffice_page_wins':sum(p['winner']=='LIBREOFFICE' for p in rows),
            'near_ties':sum(p['winner']=='NEAR_TIE' for p in rows),
            'coarse_rgb_box64_only':True,'publisher_pixel_exact_parity':False,
            'product_full_49_111_score':False,'production_go':False,
            'no_source_scene_or_pixels_in_output':True}
    output_path.parent.mkdir(parents=True,exist_ok=True)
    output_path.write_text(json.dumps(report,indent=2,sort_keys=True)+'\n',encoding='utf-8')
    print('PRODUCT_WORST_VISUAL64 '+json.dumps(report,sort_keys=True))
    return report

if __name__=='__main__':
    a=argparse.ArgumentParser(description=__doc__)
    for name in ('case','fingerprint','pairs','frozen','browser-dir','out','head'):
        a.add_argument('--'+name,required=True)
    x=vars(a.parse_args())
    compare(x['case'],Path(x['fingerprint']),Path(x['pairs']),
            Path(x['frozen']),Path(x['browser_dir']),Path(x['out']),x['head'])
