#!/usr/bin/env python3
"""Fail-closed source-neutral controls for physical 49/39/111 Product evidence."""
from __future__ import annotations

import copy
import unittest

from product_full_cohort_shards_v1 import select
from test_product_full_cohort_shards_v1 import sample
from product_full_physical_receipts_v1 import (
    case_receipt, shard_receipt, combine,
)
from reader_product_49_111_scoreboard_v1 import REQUIRED_CHECKS


HEAD = 'a' * 40
GRID = 'b' * 64


def reports(case: dict):
    ident, src, pdf, n = (
        case['id'], case['source_sha256'], case['publisher_pdf_sha256'], case['pages'])
    owner = {
        'schema': 'chaptera.product-oidc-worst-census-browser.v1',
        'fixture': ident, 'repository_commit_sha': HEAD,
        'revision_identity_sha256': 'c' * 64,
        'real_oidc_pkce_session_reused': True,
        'tls_product_source_ingress': True,
        'exact_source_and_revision': True,
        'actual_product_http_status': 200,
        'isolated_worker_and_host_owned_image': True,
        'chromium_pixels_decoded': True,
        'anonymous_scene_status': 401,
        'external_image_requests': 0,
        'browser_errors': 0,
        'source_container_forwarded': False,
        'host_image_count': 0,
        'browser_painted_resources': 0,
        'document_page_count': n,
        'fidelity_state': 'supported',
    }
    summary = {
        'schema': 'chaptera.product-oidc-worst-census-physical.v1',
        'fixture': ident, 'repository_head_sha': HEAD,
        'original_pub_sha256': src,
        'expected_publisher_pdf_pages': n,
        'product_scene_pages': n,
        'real_oidc_https_source_ingress': True,
        'real_isolated_product_worker_host_derived_images': True,
        'authenticated_product_http_browser_paint': True,
        'real_publisher_visual_a_b': True,
        'publisher_comparison_page_count': n,
        'browser_painted_unique_resources': 0,
        'host_owned_image_count': 0,
        'source_or_pixel_artifact_uploaded': False,
        'deployed_production_claim': False,
        'external_s3_claim': False,
    }
    comparison = {
        'schema': 'chaptera.product-worst-publisher-ab-score.v1',
        'fixture': ident, 'source_sha256': src,
        'publisher_pdf_sha256': pdf,
        'repository_commit_sha': HEAD,
        'ranked_pages': n, 'publisher_reference_surface_stage': 'unknown',
        'route': 'oidc-https-source-ingress-isolated-worker-host-preview-chromium',
        'no_source_scene_or_pixels_in_output': True,
        'coarse_rgb_box64_only': True,
        'publisher_pixel_exact_parity': False,
        'product_full_49_111_score': False,
        'production_go': False,
        'host_admitted_image_count': 0,
        'pages': [{
            'page': i,
            'publisher_grid_sha256': GRID,
            'product_page_identity_sha256': 'd' * 64,
            'chromium_png_sha256': 'e' * 64,
            'product_changed_cells': 100,
            'physical_extent_delta_pt': {'width_pt': 0., 'height_pt': 0.},
            'host_derived_resource_count': 0,
            'fidelity_state': 'full',
        } for i in range(1, n + 1)],
    }
    privacy = {
        'protocol_version': 'chaptera.security-leak-scan.v1',
        'passed': True, 'finding_count': 0, 'findings': [],
        'files_scanned': 8,
    }
    return owner, summary, comparison, privacy


class PhysicalProductReceiptTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.frozen, cls.refs, cls.pairs = sample()
        for item in cls.refs.values():
            item['reference_pages'] = len(item['pages'])
            for page in item['pages']:
                page['grid_sha256'] = GRID
        cls.roster = select(cls.frozen, cls.refs, cls.pairs)
        cls.case = next(
            case for shard in cls.roster['shards'] for case in shard['fixtures']
            if case['id'] == '020'
        )

    def test_one_real_shaped_source_neutral_document(self):
        doc = case_receipt(self.case, HEAD, *reports(self.case))
        self.assertEqual(doc['status'], 'accepted_product')
        self.assertEqual(len(doc['pages']), self.case['pages'])
        self.assertEqual(set(doc['checks']), REQUIRED_CHECKS)
        self.assertTrue(all(doc['checks'].values()))

    def test_wrong_physical_head_rejected(self):
        with self.assertRaisesRegex(ValueError, 'owner/checkout'):
            case_receipt(self.case, 'f' * 40, *reports(self.case))

    def test_missing_oidc_rejected(self):
        evidence = list(reports(self.case))
        evidence[0]['real_oidc_pkce_session_reused'] = False
        with self.assertRaisesRegex(ValueError, 'browser/security'):
            case_receipt(self.case, HEAD, *evidence)

    def test_cross_tenant_anonymous_leak_rejected(self):
        evidence = list(reports(self.case))
        evidence[0]['anonymous_scene_status'] = 200
        with self.assertRaisesRegex(ValueError, 'browser/security'):
            case_receipt(self.case, HEAD, *evidence)

    def test_unbound_revision_rejected(self):
        evidence = list(reports(self.case))
        evidence[0].pop('revision_identity_sha256')
        with self.assertRaisesRegex(ValueError, 'durable revision'):
            case_receipt(self.case, HEAD, *evidence)

    def test_wrong_original_source_rejected(self):
        evidence = list(reports(self.case))
        evidence[1]['original_pub_sha256'] = '0' * 64
        with self.assertRaisesRegex(ValueError, 'summary identity'):
            case_receipt(self.case, HEAD, *evidence)

    def test_privacy_findings_rejected(self):
        evidence = list(reports(self.case))
        evidence[3]['finding_count'] = 1
        evidence[3]['passed'] = False
        with self.assertRaisesRegex(ValueError, 'privacy leak'):
            case_receipt(self.case, HEAD, *evidence)

    def test_missing_page_rejected(self):
        evidence = list(reports(self.case))
        evidence[2]['pages'].clear()
        with self.assertRaisesRegex(ValueError, 'ordinal pages'):
            case_receipt(self.case, HEAD, *evidence)

    def test_false_chromium_sha_rejected(self):
        evidence = list(reports(self.case))
        evidence[2]['pages'][0]['chromium_png_sha256'] = 'not-a-hash'
        with self.assertRaisesRegex(ValueError, 'SHA'):
            case_receipt(self.case, HEAD, *evidence)

    def test_bad_host_fidelity_rejected(self):
        evidence = list(reports(self.case))
        evidence[2]['pages'][0]['fidelity_state'] = 'partial'
        with self.assertRaisesRegex(ValueError, 'fidelity'):
            case_receipt(self.case, HEAD, *evidence)

    def test_out_of_extent_rejected(self):
        evidence = list(reports(self.case))
        evidence[2]['pages'][0]['physical_extent_delta_pt']['width_pt'] = 0.5
        with self.assertRaisesRegex(ValueError, 'extent'):
            case_receipt(self.case, HEAD, *evidence)

    def fragments(self):
        chunks = []
        for shard in self.roster['shards']:
            docs = [case_receipt(case, HEAD, *reports(case))
                    for case in shard['fixtures']]
            chunks.append(shard_receipt(self.roster, shard['id'], HEAD, docs))
        return chunks

    def test_complete_4_shards_can_enter_canonical_scorer(self):
        payload, score = combine(self.roster, self.frozen, self.refs,
                                 self.pairs, HEAD, self.fragments())
        self.assertEqual(len(payload['documents']), 49)
        self.assertEqual(sum(d['status'] == 'accepted_product'
                             for d in payload['documents']), 39)
        self.assertEqual(sum(d['status'] == 'predeclared_ineligible'
                             for d in payload['documents']), 10)
        self.assertEqual(score['product_pages']['pages'], 111)
        self.assertEqual(score['product_documents']['documents'], 39)
        self.assertEqual(score['status'], 'complete_product_physical')

    def test_one_missing_shard_rejected(self):
        with self.assertRaisesRegex(ValueError, 'four physically'):
            combine(self.roster, self.frozen, self.refs, self.pairs,
                    HEAD, self.fragments()[:-1])

    def test_one_duplicate_shard_rejected(self):
        rows = self.fragments()
        rows[-1] = copy.deepcopy(rows[0])
        with self.assertRaisesRegex(ValueError, 'duplicate'):
            combine(self.roster, self.frozen, self.refs, self.pairs, HEAD, rows)

    def test_one_stale_shard_rejected(self):
        rows = self.fragments()
        rows[1]['repository_commit_sha'] = '0' * 40
        with self.assertRaisesRegex(ValueError, 'stale'):
            combine(self.roster, self.frozen, self.refs, self.pairs, HEAD, rows)

    def test_omitted_case_rejected(self):
        rows = self.fragments()
        rows[1]['documents'].pop()
        with self.assertRaisesRegex(ValueError, 'incomplete shard'):
            combine(self.roster, self.frozen, self.refs, self.pairs, HEAD, rows)

    def test_invented_ineligible_receipt_rejected(self):
        rows = self.fragments()
        rows[0]['documents'][0]['status'] = 'predeclared_ineligible'
        with self.assertRaisesRegex(ValueError, 'unproven Product'):
            combine(self.roster, self.frozen, self.refs, self.pairs, HEAD, rows)


if __name__ == '__main__':
    unittest.main()
