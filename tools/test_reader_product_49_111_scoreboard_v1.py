#!/usr/bin/env python3
"""Synthetic only; fail-closed Product full-cohort score contract controls."""
from __future__ import annotations

from copy import deepcopy
import hashlib
import unittest

import reader_product_49_111_scoreboard_v1 as s

HEAD = 'a' * 40


def sha(string):
    return hashlib.sha256(string.encode('ascii')).hexdigest()


def synthetic():
    rows, documents, refs, pairs = [], [], {}, {}
    for i in range(49):
        fixture = f'{i:03d}_{sha(f"case{i}")[:16]}'
        pub = sha(f'pub{i}')
        pdf = sha(f'pdf{i}')
        count = 73 if i == 0 else 1
        comparable = i < 39
        refpages = [{'page': k, 'grid_sha256': sha(f'{fixture}/{k}'),
                     'media_width_pt': 594.0, 'media_height_pt': 842.0}
                    for k in range(1, count + 1)]
        refs[fixture] = {'pub_sha256': pub, 'pdf_sha256': pdf,
                         'reference_pages': count, 'pages': refpages}
        pairs[fixture] = {'pub_sha256': pub, 'pdf_sha256': pdf,
                          'pdf_pages': str(count)}
        lo = [[k, 100, 500] for k in range(1, count + 1)] if comparable else []
        rows.append([fixture, pub, pdf, count, count, count, lo])
        if comparable:
            pages = [{
                'page': k, 'publisher_grid_sha256': sha(f'{fixture}/{k}'),
                'product_page_identity_sha256': sha(f'page{k}/{fixture}'),
                'chromium_png_sha256': sha(f'png{k}/{fixture}'),
                'changed_cell_count': 500,
                'physical_extent_delta_pt': {'width_pt': 0.0, 'height_pt': 0.0},
                'host_derived_resource_count': 0, 'fidelity_state': 'partial',
            } for k in range(1, count + 1)]
        else:
            pages = []
        documents.append({
            'fixture': fixture, 'pub_sha256': pub,
            'publisher_pdf_sha256': pdf,
            'status': 'accepted_product' if comparable else 'predeclared_ineligible',
            'product_page_count': count if comparable else None,
            'revision_identity_sha256': sha(f'revision{i}') if comparable else None,
            'checks': {key: True for key in s.REQUIRED_CHECKS} if comparable else {},
            'pages': pages,
        })
    receipt = {'schema': s.PHYSICAL_SCHEMA, 'repository_commit_sha': HEAD,
               'capture_route': s.CAPTURE_ROUTE, 'method': deepcopy(s.METHOD),
               'documents': documents}
    return receipt, {'rows': rows}, refs, pairs


class ProductFullCohortContractTests(unittest.TestCase):
    def setUp(self):
        self.receipt, self.frozen, self.refs, self.pairs = synthetic()

    def score(self):
        return s.score(self.receipt, self.frozen, self.refs, self.pairs, HEAD)

    def fail(self, phrase):
        with self.assertRaisesRegex(ValueError, phrase):
            self.score()

    def test_complete_synthetic_admitted_product_111_pages(self):
        result = self.score()
        self.assertEqual(result['product_pages']['pages'], 111)
        self.assertEqual(result['product_documents']['documents'], 39)
        self.assertEqual(result['product_pages']['libreoffice_wins'], 111)
        self.assertEqual(result['frozen_cohort']['predeclared_ineligible'], 10)
        self.assertTrue(result['claims']['frozen_lo_not_reexecuted'])

    def test_reject_direct_guest_measurement_route(self):
        self.receipt['capture_route'] = 'isolated_worker_direct'
        self.fail('DIRECT/Guest')

    def test_reject_missing_eligible_fixture(self):
        self.receipt['documents'].pop(3)
        self.fail('all 49')

    def test_reject_duplicate_fixture(self):
        self.receipt['documents'][1] = deepcopy(self.receipt['documents'][0])
        self.fail('duplicate/invalid')

    def test_reject_product_import_failure_not_exclusion(self):
        self.receipt['documents'][1]['status'] = 'source_ingress_failed'
        self.fail('import/render failed')

    def test_reject_missing_real_chromium_page(self):
        self.receipt['documents'][0]['pages'].pop()
        self.fail('incomplete Product page capture')

    def test_reject_duplicate_page(self):
        self.receipt['documents'][0]['pages'][1]['page'] = 1
        self.fail('PAGE duplicate')

    def test_reject_missing_oidc_privacy_proof(self):
        self.receipt['documents'][1]['checks']['privacy_scan_passed'] = False
        self.fail('auth/host/privacy')

    def test_reject_unknown_private_scene_field(self):
        self.receipt['documents'][0]['private_story_text'] = 'must not be here'
        self.fail('unexpected/missing fields')

    def test_reject_wrong_source_oracle_pdf(self):
        self.receipt['documents'][0]['publisher_pdf_sha256'] = sha('wrong')
        self.fail('source/Publisher PDF')

    def test_reject_wrong_publisher_grid(self):
        self.receipt['documents'][0]['pages'][0]['publisher_grid_sha256'] = sha('wrong')
        self.fail('wrong Publisher PDF oracle')

    def test_reject_wrong_exact_head(self):
        self.receipt['repository_commit_sha'] = 'b' * 40
        self.fail('receipt SHA')

    def test_reject_wrong_raster_method(self):
        self.receipt['method']['grid_width'] = 65
        self.fail('raster method drift')

    def test_reject_paint_without_png_capture_hash(self):
        self.receipt['documents'][0]['pages'][0]['chromium_png_sha256'] = None
        self.fail('Chromium/page capture')

    def test_reject_out_of_bounds_changed_cells(self):
        self.receipt['documents'][0]['pages'][0]['changed_cell_count'] = 4097
        self.fail('changed-cell count invalid')

    def test_reject_publisher_extent_mismatch(self):
        self.receipt['documents'][0]['pages'][0]['physical_extent_delta_pt']['width_pt'] = .2
        self.fail('extent mismatch')

    def test_reject_posthoc_exclusion_of_eligible(self):
        self.receipt['documents'][0]['status'] = 'predeclared_ineligible'
        self.fail('import/render failed')

    def test_reject_ineligible_contaminated_with_unscored_pages(self):
        self.receipt['documents'][-1]['pages'] = [{'page': 1}]
        self.fail('unsafe exclusion')

    def test_reject_fabricated_authorization_check(self):
        self.receipt['documents'][0]['checks']['oidc_pkce_authenticated'] = 1
        self.fail('auth/host/privacy')


if __name__ == '__main__':
    unittest.main()
