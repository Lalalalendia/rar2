#!/usr/bin/env python3
"""Small source-safe, deterministic controls for the real-022 comparative census."""
import base64
import hashlib
import json
import sys
import tempfile
import unittest
import zlib
from pathlib import Path
from unittest.mock import patch
import fitz
from PIL import Image
sys.path.insert(0,str(Path(__file__).parent))
import reader_pdf_022_probe_v1 as mod

WHITE=bytes([255,255,255])*4096
BLACK=bytes([0,0,0])*4096

def synthetic_reference(pairs):
    base={'schema':'chaptera.publisher-visual-fingerprint.v1','pair_count':55,'pairs':[]}
    for name, digest in pairs.items():
        base['pairs'].append({'basename':name,'pub_sha256':digest,'reference_pages':1,
                             'reference_surface_stage':'unknown','pages':[{'rgb_zlib_base64':base64.b64encode(zlib.compress(WHITE)).decode('ascii'),
                                                 'grid_sha256':hashlib.sha256(WHITE).hexdigest(),
                                                 'media_width_pt':612,'media_height_pt':792}]})
    for n in range(55-len(base['pairs'])):
        base['pairs'].append({'basename':f'control{n:02}','pub_sha256':('a'*62+f'{n:02x}'), 'reference_pages':1})
    return base

class ProbeTests(unittest.TestCase):
    def test_same_pixels_zero_and_black_vs_white_all_cells(self):
        self.assertEqual(mod.compare_cells(WHITE,WHITE),(0,[0]*16))
        n,tiles=mod.compare_cells(WHITE,BLACK)
        self.assertEqual(n,4096)
        self.assertEqual(tiles,[256]*16)

    def test_pixel_profile_occupancy(self):
        self.assertEqual(mod.occupancy(WHITE)['near_white_cell_count'],4096)
        self.assertEqual(mod.occupancy(BLACK)['near_black_cell_count'],4096)
        self.assertEqual(mod.occupancy(BLACK)['near_white_cell_count'],0)

    def test_unsafe_codes_and_bool_counts_rejected(self):
        self.assertEqual(mod.source_safe_codes(['pdf.node.foo','pdf.node.foo']),['pdf.node.foo'])
        for invalid in ['story secret','/private/path','code\ntext']:
            with self.assertRaises(ValueError): mod.source_safe_codes([invalid])
        with self.assertRaises(ValueError): mod.source_safe_counts({'pdf.nodes':True})

    def test_pdf_one_page_cardinality_fence(self):
        with tempfile.TemporaryDirectory() as temp:
            path=Path(temp)/'a.pdf'
            doc=fitz.open()
            doc.new_page(width=612,height=792)
            doc.save(path)
            doc.close()
            rgb,count,media=mod.pdf_page_rgb(path)
            self.assertEqual(len(rgb),len(WHITE))
            self.assertEqual((count,media),(1,(612.,792.)))
            doc=fitz.open(path)
            doc.new_page(width=612,height=792)
            doc.save(path,incremental=True,encryption=fitz.PDF_ENCRYPT_KEEP)
            doc.close()
            with self.assertRaisesRegex(ValueError,'cardinality'):mod.pdf_page_rgb(path)

    def test_reference_identity_sha_contract(self):
        with tempfile.TemporaryDirectory() as temp:
            p=Path(temp)/'ref.json'
            p.write_text(json.dumps(synthetic_reference(mod.EXPECTED)))
            self.assertEqual(len(mod.require_reference(p)),55)
            data=json.loads(p.read_text());data['pairs'][0]['pages'][0]['grid_sha256']='0'*64
            p.write_text(json.dumps(data))
            with self.assertRaises(ValueError):mod.require_reference(p)

    def test_full_same_head_collect_and_bad_screenshot_hash(self):
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp)
            expected={f'control_{i}':hashlib.sha256(bytes([i])).hexdigest() for i in range(3)}
            with patch.object(mod,'EXPECTED',expected):
                ref=root/'ref.json';ref.write_text(json.dumps(synthetic_reference(expected)))
                sources=root/'sources';sources.mkdir()
                for i,(name,digest) in enumerate(expected.items()):(sources/f'{digest}.pub').write_bytes(bytes([i]))
                manifest=mod.prepare(sources,ref,root/'manifest.json')
                self.assertEqual(len(manifest['fixtures']),3)
                pdfs=root/'pdf';pdfs.mkdir()
                brows=root/'browser';brows.mkdir()
                results=[]
                for name,digest in expected.items():
                    image=Image.new('RGB',(64,64),'white')
                    shot=brows/f'{name}-page-1.png';image.save(shot)
                    pd=fitz.open();pd.new_page(width=612,height=792)
                    pd.save(pdfs/f'{name}.pdf');pd.close()
                    (pdfs/f'{name}.pdf.loss.json').write_text(json.dumps({
                        'conversion_profile':{'source':{'source_sha256':digest}},
                        'pdf':{'nodes':[{'code':'pdf.node.painted_exact_image','disposition':'painted'}],'diagnostics':[]}}))
                    results.append({'fixture':name,'source_sha256':digest,'rendered':True,'pages':1,
                        'screenshots':[{'page':1,'filename':shot.name,'sha256':mod.sha256(shot)}],
                        'page_geometry':[{'width_emu':round(612*12700),'height_emu':round(792*12700)}],
                        'nodes':1,'stories':0,'shared_lines':0,'fidelity_reasons':[],
                        'diagnostic_codes':[],'text_layout_fallback_counts':{}})
                sha='b'*40
                (brows/'receipt.json').write_text(json.dumps({'protocol':'chaptera.cloud-reader-real-scene-browser.v1',
                    'repository_commit_sha':sha,'results':results}))
                # Witness index hard fence makes synthetic set deliberately invalid
                with self.assertRaisesRegex(ValueError,'022 experimental witness'):
                    mod.collect(brows,pdfs,ref,sha)
                # Fail closed on mutated screenshot provenance before structural witness check.
                results[0]['screenshots'][0]['sha256']='0'*64
                (brows/'receipt.json').write_text(json.dumps({'protocol':'chaptera.cloud-reader-real-scene-browser.v1',
                    'repository_commit_sha':sha,'results':results}))
                with self.assertRaisesRegex(ValueError,'screenshot digest'):mod.collect(brows,pdfs,ref,sha)

if __name__=='__main__':
    unittest.main(verbosity=2)
