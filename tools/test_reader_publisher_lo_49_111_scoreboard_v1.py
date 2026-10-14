#!/usr/bin/env python3
"""Source-safe controls for frozen 49/111 Publisher/LibreOffice re-ranking."""
from __future__ import annotations

import copy
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import reader_publisher_lo_49_111_scoreboard_v1 as score

FROZEN=Path(__file__).resolve().parent / 'corpus' / 'receipts' / 'publisher-lo-t999-49-111-frozen-v1.json'


class FrozenPublisherLeaderboardTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.frozen=score.load_frozen(FROZEN)

    def make_registry_and_summary(self):
        # Pure synthetic 55/162 Reader fixture with the exact frozen 49/111
        # cohort. All acceptance values are synthetic; no Publisher claim.
        summary={
            'schema':'chaptera.publisher-visual-fingerprint-compare.v1',
            'comparator':{'changed_cell_channel_delta':12,'colorspace':'RGB',
                          'grid_height':64,'grid_width':64,'resampling':'Pillow.BOX'},
            'pair_count':55,'compared_page_count':162,'reference_page_count':162,
            'repository_commit_sha':'a'*40,
            'claims':{'pdf_used_as_visual_authority_only':True,'pixel_exact_visual_parity':False},
            'reference_surface_stage_counts':{'unknown':55},
            'pairs':[], 'pages':[],
        }
        pairs={}
        for fixture,source,pdf_sha,native,lo_count,old_reader_count,admitted in self.frozen['rows']:
            pairs[fixture]={'pub_sha256':source,'pdf_sha256':pdf_sha,'pdf_pages':str(native)}
            summary['pairs'].append({'fixture':fixture,'source_sha256':source,
                'reference_pages':native,'candidate_pages':old_reader_count})
            by_page={n:old_count for n,_,old_count in admitted}
            for i in range(1,native+1):
                summary['pages'].append({'fixture':fixture,'page':i,
                    'reference_surface_stage':'unknown',
                    'reference_media_extent_delta':{'width_pt':0.,'height_pt':0.},
                    'changed_cell_count':by_page.get(i,0)})
        # Exactly six unavailable public donor PUBs account for 9 additional
        # Publisher-reference pages. Never rank these into the 49 cohort.
        for i,extra_pages in enumerate([1,1,1,4,1,1],1):
            fixture=f'excluded-{i}'
            pub=f'{i:064x}'; pdf=f'{i+6:064x}'
            pairs[fixture]={'pub_sha256':pub,'pdf_sha256':pdf,'pdf_pages':str(extra_pages)}
            summary['pairs'].append({'fixture':fixture,'source_sha256':pub,
                'reference_pages':extra_pages,'candidate_pages':extra_pages})
            for page in range(1,extra_pages+1):
                summary['pages'].append({'fixture':fixture,'page':page,
                    'reference_surface_stage':'unknown',
                    'reference_media_extent_delta':{'width_pt':0.,'height_pt':0.},
                    'changed_cell_count':0})
        assert len(summary['pairs'])==55 and len(summary['pages'])==162
        return summary,pairs

    def test_frozen_full_scope_and_historical_baseline(self):
        self.assertEqual(len(self.frozen['rows']),49)
        self.assertEqual(sum(len(row[6]) for row in self.frozen['rows']),111)
        self.assertEqual(sum(bool(row[6]) for row in self.frozen['rows']),39)
        self.assertEqual(score.winner(.015), 'NEAR_TIE')
        self.assertEqual(score.winner(-.015),'NEAR_TIE')
        self.assertEqual(score.winner(.016),'LIBREOFFICE')
        self.assertEqual(score.winner(-.016),'READER')

    def test_synthetic_historical_score_is_not_a_current_main_verdict(self):
        summary,pairs=self.make_registry_and_summary()
        result=score.rerank(summary,self.frozen,pairs,summary['repository_commit_sha'])
        self.assertEqual(result['fixed_cohort']['ranked_pages'],111)
        self.assertEqual(result['current_pages']['libreoffice_wins'],85)
        self.assertEqual(result['current_pages']['reader_wins'],17)
        self.assertEqual(result['current_pages']['near_ties'],9)
        self.assertEqual(result['historic_documents'],{'documents':39,'libreoffice_wins':26,'reader_wins':8,'near_ties':5})
        self.assertEqual(result['reader_checkout_sha'],summary['repository_commit_sha'])
        self.assertFalse(result['claims']['native_publisher_pixel_exact_parity'])

    def test_wrong_reader_commit_and_wrong_reference_method_rejected(self):
        summary,pairs=self.make_registry_and_summary()
        with self.assertRaisesRegex(ValueError,'exact requested checkout SHA'):
            score.rerank(summary,self.frozen,pairs,'b'*40)
        summary['comparator']['changed_cell_channel_delta']=13
        with self.assertRaisesRegex(ValueError,'method differs'):
            score.rerank(summary,self.frozen,pairs,summary['repository_commit_sha'])

    def test_missing_page_or_duplicate_fixture_rejected(self):
        summary,pairs=self.make_registry_and_summary()
        summary['pages'].pop()
        with self.assertRaisesRegex(ValueError,'incomplete Batch01'):
            score.rerank(summary,self.frozen,pairs,summary['repository_commit_sha'])
        summary,pairs=self.make_registry_and_summary()
        summary['pairs'][1]=copy.deepcopy(summary['pairs'][0])
        with self.assertRaisesRegex(ValueError,'duplicate/missing'):
            score.rerank(summary,self.frozen,pairs,summary['repository_commit_sha'])

    def test_preselected_comparable_page_count_and_source_provenance_are_fixed(self):
        summary,pairs=self.make_registry_and_summary()
        fixed=next(x for x in summary['pairs'] if x['fixture'].startswith('022_'))
        fixed['candidate_pages']=2
        with self.assertRaisesRegex(ValueError,'predeclared comparable document page count changed'):
            score.rerank(summary,self.frozen,pairs,summary['repository_commit_sha'])
        summary,pairs=self.make_registry_and_summary()
        fixed=next(x for x in summary['pairs'] if x['fixture'].startswith('022_'))
        fixed['source_sha256']='f'*64
        with self.assertRaisesRegex(ValueError,'source/PDF SHA provenance drift'):
            score.rerank(summary,self.frozen,pairs,summary['repository_commit_sha'])

    def test_reference_extent_and_surface_stage_drift_fail_closed(self):
        summary,pairs=self.make_registry_and_summary()
        row=next(x for x in summary['pages'] if x['fixture'].startswith('022_'))
        row['reference_media_extent_delta']['width_pt']=0.051
        with self.assertRaisesRegex(ValueError,'physical page size drift'):
            score.rerank(summary,self.frozen,pairs,summary['repository_commit_sha'])
        summary,pairs=self.make_registry_and_summary()
        row=next(x for x in summary['pages'] if x['fixture'].startswith('022_'))
        row['reference_surface_stage']='logical_page'
        with self.assertRaisesRegex(ValueError,'surface-stage drift'):
            score.rerank(summary,self.frozen,pairs,summary['repository_commit_sha'])

    def test_frozen_manifest_sha_mutation_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            path=Path(directory)/'frozen.json'
            raw=FROZEN.read_bytes()
            path.write_bytes(raw[:-1]+bytes([raw[-1]^1]))
            with self.assertRaisesRegex(ValueError,'frozen SK1 science LO cohort SHA'):
                score.load_frozen(path)

    def test_score_recomputes_changed_candidate_without_modifying_lo_or_cohort(self):
        summary,pairs=self.make_registry_and_summary()
        source=next(x for x in summary['pages'] if x['fixture'].startswith('022_'))
        source['changed_cell_count']=466
        result=score.rerank(summary,self.frozen,pairs,summary['repository_commit_sha'])
        self.assertEqual(result['fixed_cohort']['ranked_pages'],111)
        self.assertEqual(len(result['predeclared_ineligible']),10)
        self.assertLess(result['current_pages']['mean_reader_changed_fraction'],result['historic_pages']['mean_reader_changed_fraction'])
        self.assertGreaterEqual(result['current_pages']['reader_wins'],result['historic_pages']['reader_wins'])
        self.assertTrue(result['claims']['frozen_lo_not_reexecuted'])


if __name__=='__main__':
    unittest.main()
